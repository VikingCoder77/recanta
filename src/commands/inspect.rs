//! `recanta inspect` — symbol/file inspection (PRD §8.3). Lets an agent learn a
//! symbol's location, signature, recent changes, and index freshness without reading
//! the whole file. v0.1 supports `function`, `class`, and `file`.

use std::path::Path;

use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::output;
use crate::project::Paths;
use crate::repo::{self, Freshness};
use crate::{db, git};

use clap::Args;

#[derive(Debug, Args)]
pub struct InspectArgs {
    /// What to inspect: `function`, `class`, or `file`.
    pub kind: String,

    /// Symbol name / qualified name, or file path.
    pub target: String,

    /// Output budget in characters.
    #[arg(long, default_value_t = 1000)]
    pub budget: usize,
}

pub fn run(args: InspectArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let conn = db::open_existing(&paths.db)?;
    let project_id = repo::current_project_id(&conn)?;
    let branch = git::current_branch(&paths.root);
    let repo_id = repo::ensure_repository(&conn, &project_id, &paths.root, branch.as_deref())?;
    let fresh = repo::freshness(&conn, repo_id, &paths.root)?;

    let mut blocks: Vec<String> = match args.kind.as_str() {
        "function" | "method" => inspect_symbols(&conn, repo_id, &args.target, &["function", "method"])?,
        "class" | "interface" => inspect_symbols(&conn, repo_id, &args.target, &["class", "interface"])?,
        "file" => inspect_file(&conn, repo_id, &args.target)?,
        other => bail!("don't know how to inspect `{other}` (try: function, class, file)"),
    };

    if blocks.is_empty() {
        let hint = match fresh {
            Freshness::NotIndexed => " (code graph not built — run `recanta index`)",
            _ => "",
        };
        println!("no {} matching {:?}{hint}", args.kind, args.target);
        return Ok(());
    }

    if let Some(note) = staleness_note(&fresh) {
        blocks.push(note);
    }
    println!("{}", output::pack(blocks, args.budget));
    Ok(())
}

/// Render matching symbols (with location, signature, recent changes).
fn inspect_symbols(
    conn: &Connection,
    repo_id: i64,
    target: &str,
    types: &[&str],
) -> Result<Vec<String>> {
    let placeholders = vec!["?"; types.len()].join(",");
    // All-anonymous placeholders bound strictly in left-to-right appearance order:
    // repo_id, the IN(...) types, then the target twice (name / qualified_name).
    let sql = format!(
        "SELECT id, qualified_name, symbol_type, file_path, signature, start_line, end_line
         FROM code_symbols
         WHERE repo_id = ? AND status = 'active'
           AND symbol_type IN ({placeholders})
           AND (name = ? OR qualified_name = ?)
         ORDER BY qualified_name"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(repo_id)];
    for t in types {
        params.push(Box::new(t.to_string()));
    }
    params.push(Box::new(target.to_string()));
    params.push(Box::new(target.to_string()));
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

    let rows = stmt
        .query_map(refs.as_slice(), |r| {
            Ok(SymbolRow {
                id: r.get(0)?,
                qualified_name: r.get(1)?,
                symbol_type: r.get(2)?,
                file_path: r.get(3)?,
                signature: r.get(4)?,
                start_line: r.get(5)?,
                end_line: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut blocks = Vec::new();
    for row in rows {
        let mut b = format!(
            "{} [{}]  {}:{}-{}\n   {}",
            row.qualified_name, row.symbol_type, row.file_path, row.start_line, row.end_line,
            if row.signature.is_empty() { "(no signature)" } else { &row.signature }
        );
        let changes = recent_changes(conn, row.id)?;
        if !changes.is_empty() {
            b.push_str("\n   changes: ");
            b.push_str(&changes.join("; "));
        }
        blocks.push(b);
    }
    Ok(blocks)
}

fn inspect_file(conn: &Connection, repo_id: i64, target: &str) -> Result<Vec<String>> {
    // Match an exact relative path or a path suffix (so `inspect file foo.py` works).
    let like = format!("%{target}");
    let mut stmt = conn.prepare(
        "SELECT qualified_name, symbol_type, start_line, end_line
         FROM code_symbols
         WHERE repo_id = ?1 AND status = 'active' AND symbol_type != 'import'
           AND (file_path = ?2 OR file_path LIKE ?3)
         ORDER BY start_line",
    )?;
    let defs = stmt
        .query_map(rusqlite::params![repo_id, target, like], |r| {
            Ok(format!(
                "{} [{}] L{}-{}",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut imports_stmt = conn.prepare(
        "SELECT qualified_name FROM code_symbols
         WHERE repo_id = ?1 AND status = 'active' AND symbol_type = 'import'
           AND (file_path = ?2 OR file_path LIKE ?3)
         ORDER BY qualified_name",
    )?;
    let imports = imports_stmt
        .query_map(rusqlite::params![repo_id, target, like], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut blocks = Vec::new();
    if !imports.is_empty() {
        blocks.push(format!("imports: {}", imports.join(", ")));
    }
    blocks.extend(defs);
    Ok(blocks)
}

fn recent_changes(conn: &Connection, symbol_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT change_type, commit_sha FROM symbol_changes
         WHERE symbol_id = ?1 ORDER BY id DESC LIMIT 3",
    )?;
    let rows = stmt
        .query_map([symbol_id], |r| {
            let ct: String = r.get(0)?;
            let sha: Option<String> = r.get(1)?;
            Ok(match sha {
                Some(s) => format!("{ct}@{}", &s[..s.len().min(8)]),
                None => ct,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn staleness_note(fresh: &Freshness) -> Option<String> {
    match fresh {
        Freshness::Stale { indexed, head } => Some(format!(
            "⚠ index stale: built at {}, HEAD is {} — run `recanta index`",
            &indexed[..indexed.len().min(8)],
            &head[..head.len().min(8)]
        )),
        Freshness::NotIndexed => Some("⚠ code graph not built — run `recanta index`".into()),
        _ => None,
    }
}

struct SymbolRow {
    id: i64,
    qualified_name: String,
    symbol_type: String,
    file_path: String,
    signature: String,
    start_line: i64,
    end_line: i64,
}
