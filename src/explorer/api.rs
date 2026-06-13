//! Read-only JSON endpoints for the Explorer, over the existing store + retrieval.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::{json, Value};

use crate::memory;
use crate::repo::{self, Freshness};
use crate::{documents, git, sessions};

/// `/api/status` — identity, git state, counts, index freshness.
pub fn status(conn: &Connection, cfg: &crate::project::Config, root: &Path, repo_id: Option<i64>) -> Result<String> {
    let count = |t: &str| -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap_or(0)
    };
    let active_symbols: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM code_symbols WHERE status='active' AND symbol_type NOT IN ('import','module')",
            [], |r| r.get(0),
        )
        .unwrap_or(0);
    let freshness = repo_id
        .and_then(|id| repo::freshness(conn, id, root).ok())
        .map(|f| match f {
            Freshness::Fresh => "fresh".to_string(),
            Freshness::NotIndexed => "not built".to_string(),
            Freshness::Unknown => "n/a".to_string(),
            Freshness::Stale { .. } => "stale".to_string(),
        })
        .unwrap_or_else(|| "n/a".into());

    Ok(json!({
        "project": cfg.name,
        "branch": git::current_branch(root),
        "head": git::head_sha(root).map(|s| s[..s.len().min(10)].to_string()),
        "dirty": git::is_dirty(root),
        "index": freshness,
        "counts": {
            "memory": count("memory_items"),
            "documents": count("documents"),
            "sessions": count("sessions"),
            "symbols": active_symbols,
            "events": count("events"),
        }
    })
    .to_string())
}

/// `/api/search?q=` — unified results across memory, documents, transcripts, and symbols.
pub fn search(conn: &Connection, root: &Path, repo_id: Option<i64>, q: &str) -> Result<String> {
    let Some(expr) = memory::fts_query(q) else {
        return Ok(json!({"query": q, "memory": [], "documents": [], "sessions": [], "symbols": []}).to_string());
    };
    let branch = git::current_branch(root);

    let mem: Vec<Value> = memory::search(conn, &expr, None, branch.as_deref(), 25)?
        .into_iter()
        .map(|h| json!({"id": h.row.id, "scope": h.row.scope, "type": h.row.mem_type, "title": h.row.title, "snippet": snippet(&h.row.content, 160)}))
        .collect();
    let docs: Vec<Value> = documents::search(conn, &expr, 25)?
        .into_iter()
        .map(|d| json!({"id": d.id, "title": d.title, "path": d.path, "snippet": flatten(&d.snippet)}))
        .collect();
    let sess: Vec<Value> = sessions::search_transcripts(conn, &expr, 25)?
        .into_iter()
        .map(|s| json!({"id": s.session_id, "started_at": s.started_at, "snippet": flatten(&s.snippet)}))
        .collect();
    let syms = match repo_id {
        Some(id) => symbol_matches(conn, id, q)?,
        None => vec![],
    };

    Ok(json!({"query": q, "memory": mem, "documents": docs, "sessions": sess, "symbols": syms}).to_string())
}

fn symbol_matches(conn: &Connection, repo_id: i64, q: &str) -> Result<Vec<Value>> {
    // Match a symbol if its name contains ANY query token (so multi-word queries work).
    let tokens: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() >= 2)
        .map(|t| format!("%{t}%"))
        .collect();
    if tokens.is_empty() {
        return Ok(vec![]);
    }
    let ors = vec!["(qualified_name LIKE ? OR name LIKE ?)"; tokens.len()].join(" OR ");
    let sql = format!(
        "SELECT id, qualified_name, symbol_type, file_path, start_line FROM code_symbols
         WHERE repo_id=? AND status='active' AND symbol_type NOT IN ('import','module')
           AND ({ors})
         ORDER BY length(qualified_name) LIMIT 25"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&repo_id];
    for t in &tokens {
        params.push(t); // qualified_name LIKE
        params.push(t); // name LIKE
    }
    let rows = stmt
        .query_map(params.as_slice(), |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?, "qualified_name": r.get::<_, String>(1)?,
                "type": r.get::<_, String>(2)?, "file": r.get::<_, String>(3)?, "line": r.get::<_, i64>(4)?
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// `/api/graph?focus=<symbol id>` — the focus symbol and its directly-connected symbols.
pub fn graph(conn: &Connection, focus: &str) -> Result<String> {
    let Ok(focus_id) = focus.parse::<i64>() else {
        return Ok(json!({"error": "graph focus must be a symbol id"}).to_string());
    };
    let node = |conn: &Connection, id: i64| -> Option<Value> {
        conn.query_row(
            "SELECT id, qualified_name, symbol_type, file_path FROM code_symbols WHERE id=?1",
            [id],
            |r| Ok(json!({"id": r.get::<_,i64>(0)?, "label": r.get::<_,String>(1)?, "type": r.get::<_,String>(2)?, "file": r.get::<_,String>(3)?})),
        )
        .ok()
    };

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut ids = vec![focus_id];
    if let Some(n) = node(conn, focus_id) {
        nodes.push(n);
    }

    let mut stmt = conn.prepare(
        "SELECT from_symbol_id, to_symbol_id, relationship_type FROM code_relationships
         WHERE from_symbol_id=?1 OR to_symbol_id=?1",
    )?;
    let rels = stmt
        .query_map([focus_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, String>(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for (from, to, rel) in rels {
        let Some(to) = to else { continue };
        edges.push(json!({"from": from, "to": to, "rel": rel}));
        for id in [from, to] {
            if !ids.contains(&id) {
                ids.push(id);
                if let Some(n) = node(conn, id) {
                    nodes.push(n);
                }
            }
        }
    }
    Ok(json!({"focus": focus_id, "nodes": nodes, "edges": edges}).to_string())
}

/// `/api/symbol?id=` — symbol detail + recent changes.
pub fn symbol(conn: &Connection, id: &str) -> Result<String> {
    let Ok(id) = id.parse::<i64>() else {
        return Ok(json!({"error": "id required"}).to_string());
    };
    let detail = conn.query_row(
        "SELECT qualified_name, symbol_type, file_path, start_line, end_line, signature
         FROM code_symbols WHERE id=?1",
        [id],
        |r| Ok(json!({
            "id": id, "qualified_name": r.get::<_,String>(0)?, "type": r.get::<_,String>(1)?,
            "file": r.get::<_,String>(2)?, "start": r.get::<_,i64>(3)?, "end": r.get::<_,i64>(4)?,
            "signature": r.get::<_,Option<String>>(5)?
        })),
    );
    let mut detail = match detail {
        Ok(v) => v,
        Err(_) => return Ok(json!({"error": "symbol not found"}).to_string()),
    };
    let mut stmt = conn.prepare(
        "SELECT change_type, commit_sha FROM symbol_changes WHERE symbol_id=?1 ORDER BY id DESC LIMIT 10",
    )?;
    let changes: Vec<Value> = stmt
        .query_map([id], |r| {
            Ok(json!({"change": r.get::<_,String>(0)?, "commit": r.get::<_,Option<String>>(1)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    detail["changes"] = json!(changes);
    Ok(detail.to_string())
}

/// `/api/memory?id=` — full memory item (the evidence view).
pub fn memory(conn: &Connection, id: &str) -> Result<String> {
    let Ok(id) = id.parse::<i64>() else {
        return Ok(json!({"error": "id required"}).to_string());
    };
    let v = conn.query_row(
        "SELECT type, scope, title, content, status, importance, confidence, created_at,
                source_commit_shas FROM memory_items WHERE id=?1",
        [id],
        |r| Ok(json!({
            "id": id, "type": r.get::<_,String>(0)?, "scope": r.get::<_,String>(1)?,
            "title": r.get::<_,String>(2)?, "content": r.get::<_,String>(3)?, "status": r.get::<_,String>(4)?,
            "importance": r.get::<_,String>(5)?, "confidence": r.get::<_,f64>(6)?,
            "created_at": r.get::<_,String>(7)?, "source_commits": r.get::<_,Option<String>>(8)?
        })),
    );
    match v {
        Ok(v) => Ok(v.to_string()),
        Err(_) => Ok(json!({"error": "memory not found"}).to_string()),
    }
}

fn snippet(text: &str, max: usize) -> String {
    let flat = flatten(text);
    if flat.chars().count() <= max {
        flat
    } else {
        flat.chars().take(max).collect::<String>() + "…"
    }
}

fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
