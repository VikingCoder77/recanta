//! Memory items: the typed vocabulary (scope/type/importance), writes, and the read
//! helpers shared by `search` and `brief`. Storage routing follows PRD §8.8: `user`
//! scope lives in the global store, everything else in the project store.

use anyhow::{Context, Result};
use clap::ValueEnum;
use rusqlite::Connection;

/// Memory scope taxonomy (PRD §9.1). No "global" — `user` *is* cross-project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Scope {
    User,
    Project,
    Repo,
    Branch,
    Symbol,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Project => "project",
            Scope::Repo => "repo",
            Scope::Branch => "branch",
            Scope::Symbol => "symbol",
        }
    }

    /// `user` scope is stored in the global `~/.recanta` store; all others are local.
    pub fn is_global(self) -> bool {
        matches!(self, Scope::User)
    }
}

/// Memory item types (PRD §11.1 `memory_items.type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MemType {
    Semantic,
    Procedural,
    Episodic,
    Decision,
    Task,
    Warning,
    Bug,
    CodeSummary,
}

impl MemType {
    pub fn as_str(self) -> &'static str {
        match self {
            MemType::Semantic => "semantic",
            MemType::Procedural => "procedural",
            MemType::Episodic => "episodic",
            MemType::Decision => "decision",
            MemType::Task => "task",
            MemType::Warning => "warning",
            MemType::Bug => "bug",
            MemType::CodeSummary => "code_summary",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Importance {
    Low,
    Normal,
    High,
}

impl Importance {
    pub fn as_str(self) -> &'static str {
        match self {
            Importance::Low => "low",
            Importance::Normal => "normal",
            Importance::High => "high",
        }
    }
}

/// A new memory to persist.
#[derive(Debug)]
pub struct NewMemory {
    pub mem_type: MemType,
    pub scope: Scope,
    pub title: String,
    pub content: String,
    pub importance: Importance,
    pub confidence: f64,
    /// Set when `scope == branch`.
    pub branch: Option<String>,
}

/// A row read back for display.
#[derive(Debug, Clone)]
pub struct MemoryRow {
    pub id: i64,
    pub mem_type: String,
    pub scope: String,
    pub title: String,
    pub content: String,
    pub importance: String,
    pub status: String,
    pub updated_at: String,
}

/// Insert a memory, returning its new id.
pub fn insert(conn: &Connection, m: &NewMemory) -> Result<i64> {
    conn.execute(
        "INSERT INTO memory_items
            (type, scope, title, content, importance, confidence, branch)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            m.mem_type.as_str(),
            m.scope.as_str(),
            m.title,
            m.content,
            m.importance.as_str(),
            m.confidence,
            m.branch,
        ],
    )
    .context("inserting memory item")?;
    Ok(conn.last_insert_rowid())
}

/// A search hit with its BM25 rank (lower is a better match).
#[derive(Debug, Clone)]
pub struct Hit {
    pub row: MemoryRow,
    pub rank: f64,
}

/// Turn arbitrary user text into a safe FTS5 MATCH expression: each alphanumeric token
/// is quoted as a literal phrase and OR-joined, so query punctuation can never be
/// interpreted as FTS operators (which would raise a syntax error). Returns `None`
/// when the query has no usable tokens.
pub fn fts_query(raw: &str) -> Option<String> {
    let terms: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.to_lowercase()))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// FTS5/BM25 search over active memories in one store. Results are ordered
/// deterministically by `(rank, id)` (PRD §9.1, §13.1). `scope` optionally restricts;
/// `branch` is the current branch — `branch`-scoped memories are visible only on their
/// own branch (PRD §8.11). Pass `None` for `branch` to hide all branch-scoped memories.
pub fn search(
    conn: &Connection,
    match_expr: &str,
    scope: Option<Scope>,
    branch: Option<&str>,
    limit: usize,
) -> Result<Vec<Hit>> {
    let sql = "SELECT m.id, m.type, m.scope, m.title, m.content, m.importance,
                      m.status, m.updated_at, bm25(memory_fts) AS rank
               FROM memory_fts
               JOIN memory_items m ON m.id = memory_fts.rowid
               WHERE memory_fts MATCH ?1
                 AND m.status = 'active'
                 AND (?2 IS NULL OR m.scope = ?2)
                 AND (m.scope != 'branch' OR m.branch = ?3)
               ORDER BY rank, m.id
               LIMIT ?4";
    let mut stmt = conn.prepare(sql)?;
    let scope_str = scope.map(|s| s.as_str());
    let rows = stmt
        .query_map(
            rusqlite::params![match_expr, scope_str, branch.unwrap_or(""), limit as i64],
            map_hit,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Fetch active memories of the given types, most-recent first. Used by `brief` to pull
/// tasks, decisions, and risks (PRD §8.1).
pub fn by_types(
    conn: &Connection,
    types: &[MemType],
    scope: Option<Scope>,
    branch: Option<&str>,
    limit: usize,
) -> Result<Vec<MemoryRow>> {
    if types.is_empty() {
        return Ok(vec![]);
    }
    let placeholders = vec!["?"; types.len()].join(",");
    let scope_clause = match scope {
        Some(_) => "AND scope = ?",
        None => "",
    };
    // branch-scoped memories are visible only on their own branch (§8.11).
    let sql = format!(
        "SELECT id, type, scope, title, content, importance, status, updated_at
         FROM memory_items
         WHERE status = 'active' AND type IN ({placeholders}) {scope_clause}
           AND (scope != 'branch' OR branch = ?)
         ORDER BY updated_at DESC, id DESC
         LIMIT ?"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = types
        .iter()
        .map(|t| Box::new(t.as_str()) as Box<dyn rusqlite::ToSql>)
        .collect();
    if let Some(s) = scope {
        params.push(Box::new(s.as_str()));
    }
    params.push(Box::new(branch.unwrap_or("").to_string()));
    params.push(Box::new(limit as i64));
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(refs.as_slice(), map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Top user preferences (global store), ranked by importance then recency. Used by
/// `brief` to surface durable cross-project preferences (PRD §8.1, §8.8).
pub fn top_user_prefs(conn: &Connection, limit: usize) -> Result<Vec<MemoryRow>> {
    let sql = "SELECT id, type, scope, title, content, importance, status, updated_at
               FROM memory_items
               WHERE status = 'active' AND scope = 'user'
               ORDER BY CASE importance WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END,
                        updated_at DESC, id DESC
               LIMIT ?1";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([limit as i64], map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn map_row(r: &rusqlite::Row) -> rusqlite::Result<MemoryRow> {
    Ok(MemoryRow {
        id: r.get(0)?,
        mem_type: r.get(1)?,
        scope: r.get(2)?,
        title: r.get(3)?,
        content: r.get(4)?,
        importance: r.get(5)?,
        status: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

fn map_hit(r: &rusqlite::Row) -> rusqlite::Result<Hit> {
    Ok(Hit {
        row: map_row(r)?,
        rank: r.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;

    fn store() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn fts_query_is_punctuation_safe() {
        assert_eq!(fts_query("why was trailing-stop changed?").as_deref(),
            Some("\"why\" OR \"was\" OR \"trailing\" OR \"stop\" OR \"changed\""));
        assert_eq!(fts_query("   ?!  "), None);
    }

    #[test]
    fn insert_then_search_finds_it() {
        let conn = store();
        insert(&conn, &NewMemory {
            mem_type: MemType::Decision,
            scope: Scope::Project,
            title: "Storage is one SQLite file".into(),
            content: "Single portable file, zero mandatory daemon.".into(),
            importance: Importance::High,
            confidence: 1.0,
            branch: None,
        }).unwrap();
        let q = fts_query("portable daemon").unwrap();
        let hits = search(&conn, &q, None, None, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].row.title, "Storage is one SQLite file");
    }

    #[test]
    fn by_types_filters_and_orders() {
        let conn = store();
        for (t, title) in [
            (MemType::Task, "active task"),
            (MemType::Decision, "a decision"),
            (MemType::Warning, "a risk"),
        ] {
            insert(&conn, &NewMemory {
                mem_type: t, scope: Scope::Project, title: title.into(),
                content: "x".into(), importance: Importance::Normal,
                confidence: 1.0, branch: None,
            }).unwrap();
        }
        let risks = by_types(&conn, &[MemType::Warning, MemType::Bug], None, None, 10).unwrap();
        assert_eq!(risks.len(), 1);
        assert_eq!(risks[0].title, "a risk");
    }

    #[test]
    fn branch_scoped_memory_is_only_visible_on_its_branch() {
        let conn = store();
        insert(&conn, &NewMemory {
            mem_type: MemType::Decision, scope: Scope::Branch,
            title: "feature flag rollout plan".into(), content: "ship behind a flag".into(),
            importance: Importance::Normal, confidence: 1.0, branch: Some("feature".into()),
        }).unwrap();
        let q = fts_query("flag rollout").unwrap();

        // Visible on its own branch, hidden on another branch / detached.
        assert_eq!(search(&conn, &q, None, Some("feature"), 10).unwrap().len(), 1);
        assert_eq!(search(&conn, &q, None, Some("main"), 10).unwrap().len(), 0);
        assert_eq!(search(&conn, &q, None, None, 10).unwrap().len(), 0);
    }
}
