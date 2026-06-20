//! Read-only JSON endpoints for the Explorer, over the existing store + retrieval.

use anyhow::Result;
use rusqlite::Connection;
use serde_json::{json, Value};

use super::ProjectStore;
use crate::memory;
use crate::repo::{self, Freshness};
use crate::{documents, git, sessions};

/// `/api/status` — per-project identity/state/counts plus combined totals for the whole
/// workspace. The UI shows totals and, in multi-project mode, the project list.
pub fn status_all(stores: &[ProjectStore]) -> Result<String> {
    let mut projects: Vec<Value> = Vec::new();
    let mut totals = Counts::default();
    for s in stores {
        let (meta, counts) = status_value(s);
        totals.add(&counts);
        let mut obj = meta;
        obj["key"] = json!(s.key);
        obj["counts"] = counts.to_json();
        projects.push(obj);
    }
    Ok(json!({
        "workspace": stores.len() > 1,
        "projects": projects,
        "counts": totals.to_json(),
    })
    .to_string())
}

/// Per-project status: (identity/git/index metadata, counts).
fn status_value(store: &ProjectStore) -> (Value, Counts) {
    let conn = &store.conn;
    let root = &store.root;
    let count = |t: &str| -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap_or(0)
    };
    let active_symbols: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM code_symbols WHERE status='active' AND symbol_type NOT IN ('import','module')",
            [], |r| r.get(0),
        )
        .unwrap_or(0);
    let freshness = store
        .repo_id
        .and_then(|id| repo::freshness(conn, id, root).ok())
        .map(|f| match f {
            Freshness::Fresh => "fresh".to_string(),
            Freshness::NotIndexed => "not built".to_string(),
            Freshness::Unknown => "n/a".to_string(),
            Freshness::Stale { .. } => "stale".to_string(),
        })
        .unwrap_or_else(|| "n/a".into());

    let counts = Counts {
        memory: count("memory_items"),
        documents: count("documents"),
        sessions: count("sessions"),
        symbols: active_symbols,
        events: count("events"),
    };
    let meta = json!({
        "project": store.name,
        "branch": git::current_branch(root),
        "head": git::head_sha(root).map(|s| s[..s.len().min(10)].to_string()),
        "dirty": git::is_dirty(root),
        "index": freshness,
    });
    (meta, counts)
}

#[derive(Default)]
struct Counts {
    memory: i64,
    documents: i64,
    sessions: i64,
    symbols: i64,
    events: i64,
}

impl Counts {
    fn add(&mut self, o: &Counts) {
        self.memory += o.memory;
        self.documents += o.documents;
        self.sessions += o.sessions;
        self.symbols += o.symbols;
        self.events += o.events;
    }
    fn to_json(&self) -> Value {
        json!({
            "memory": self.memory,
            "documents": self.documents,
            "sessions": self.sessions,
            "symbols": self.symbols,
            "events": self.events,
        })
    }
}

/// `/api/search?q=` — unified results across memory, documents, transcripts, and symbols,
/// merged across every open project (each result tagged with its project).
pub fn search_all(stores: &[ProjectStore], q: &str) -> Result<String> {
    let Some(expr) = memory::fts_query(q) else {
        return Ok(json!({"query": q, "memory": [], "documents": [], "sessions": [], "symbols": []}).to_string());
    };
    let (mut mem, mut docs, mut sess, mut syms) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for s in stores {
        search_store(s, q, &expr, &mut mem, &mut docs, &mut sess, &mut syms)?;
    }
    Ok(json!({"query": q, "memory": mem, "documents": docs, "sessions": sess, "symbols": syms}).to_string())
}

/// Append one store's matches (tagged with `project`/`projectName`) to the shared buckets.
fn search_store(
    store: &ProjectStore,
    q: &str,
    expr: &str,
    mem: &mut Vec<Value>,
    docs: &mut Vec<Value>,
    sess: &mut Vec<Value>,
    syms: &mut Vec<Value>,
) -> Result<()> {
    let conn = &store.conn;
    let tag = |mut v: Value| -> Value {
        v["project"] = json!(store.key);
        v["projectName"] = json!(store.name);
        v
    };
    let branch = git::current_branch(&store.root);
    mem.extend(
        memory::search(conn, expr, None, branch.as_deref(), 25)?
            .into_iter()
            .map(|h| tag(json!({"id": h.row.id, "scope": h.row.scope, "type": h.row.mem_type, "title": h.row.title, "snippet": snippet(&h.row.content, 160)}))),
    );
    docs.extend(
        documents::search(conn, expr, 25)?
            .into_iter()
            .map(|d| tag(json!({"id": d.id, "title": d.title, "path": d.path, "snippet": flatten(&d.snippet)}))),
    );
    sess.extend(
        sessions::search_transcripts(conn, expr, 25)?
            .into_iter()
            .map(|s| tag(json!({"id": s.session_id, "started_at": s.started_at, "snippet": flatten(&s.snippet)}))),
    );
    if let Some(id) = store.repo_id {
        syms.extend(symbol_matches(conn, id, q)?.into_iter().map(tag));
    }
    Ok(())
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

/// `/api/graph/full` — the whole knowledge graph: code symbols + modules, documents,
/// memories, sessions, commits, and the relationships between them (DEFINES, resolved
/// internal IMPORTS, CHANGED_BY, memory→evidence, document→code, document↔document).
/// Aggregate the full graph across every open project. Each project's node ids are
/// namespaced with its `key` (and every node tagged with `project`/`projectName`), so
/// stores can be unioned into one view without id collisions, and the UI can filter or
/// color by project. A `projects` list rides along for the project filter chips.
pub fn full_graph_all(stores: &[ProjectStore]) -> Result<String> {
    let mut nodes: Vec<Value> = Vec::new();
    let mut edges: Vec<Value> = Vec::new();
    let mut projects: Vec<Value> = Vec::new();
    for s in stores {
        let (local_nodes, local_edges) = collect_graph(&s.conn, s.repo_id)?;
        for mut n in local_nodes {
            if let Some(raw) = n["id"].as_str().map(str::to_string) {
                n["id"] = json!(ns(&s.key, &raw));
            }
            n["project"] = json!(s.key);
            n["projectName"] = json!(s.name);
            nodes.push(n);
        }
        for mut e in local_edges {
            if let Some(src) = e["source"].as_str().map(str::to_string) {
                e["source"] = json!(ns(&s.key, &src));
            }
            if let Some(tgt) = e["target"].as_str().map(str::to_string) {
                e["target"] = json!(ns(&s.key, &tgt));
            }
            edges.push(e);
        }
        projects.push(json!({"key": s.key, "name": s.name}));
    }
    Ok(json!({"nodes": nodes, "edges": edges, "projects": projects}).to_string())
}

/// Namespace a per-store node id with the project key (`p0~sym:42`).
fn ns(key: &str, id: &str) -> String {
    format!("{key}~{id}")
}

/// Build one project's nodes + edges with store-local ids (caller namespaces them).
fn collect_graph(conn: &Connection, repo_id: Option<i64>) -> Result<(Vec<Value>, Vec<Value>)> {
    let mut nodes: Vec<Value> = Vec::new();
    let mut edges: Vec<Value> = Vec::new();
    // Node ids are type-prefixed so id-spaces never collide.
    let sid = |id: i64| format!("sym:{id}");
    let cid = |sha: &str| format!("com:{sha}");

    // --- Code symbols (modules + defs; import nodes are excluded, resolved below) ---
    let repo = repo_id.unwrap_or(-1);
    let mut module_stem: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut symbol_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, qualified_name, symbol_type, file_path FROM code_symbols
             WHERE repo_id=?1 AND status='active' AND symbol_type != 'import' LIMIT 3000",
        )?;
        let rows = stmt.query_map([repo], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        })?;
        for row in rows {
            let (id, qn, ty, file) = row?;
            symbol_ids.insert(id);
            if ty == "module" {
                if let Some(stem) = std::path::Path::new(&file).file_stem().and_then(|s| s.to_str()) {
                    module_stem.insert(stem.to_string(), id);
                }
            }
            let label = if ty == "module" { file.rsplit('/').next().unwrap_or(&file).to_string() } else { qn.rsplit('.').next().unwrap_or(&qn).to_string() };
            nodes.push(json!({"id": sid(id), "label": label, "kind": ty, "group": file, "ref": id}));
        }
    }

    // --- DEFINES (and any non-IMPORTS) edges between included symbols ---
    {
        let mut stmt = conn.prepare(
            "SELECT from_symbol_id, to_symbol_id, relationship_type FROM code_relationships
             WHERE relationship_type != 'IMPORTS'",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (from, to, rel) = row?;
            if let Some(to) = to {
                if symbol_ids.contains(&from) && symbol_ids.contains(&to) {
                    edges.push(json!({"source": sid(from), "target": sid(to), "rel": rel}));
                }
            }
        }
    }

    // --- IMPORTS resolved to internal modules (file dependency graph) ---
    {
        let mut stmt = conn.prepare(
            "SELECT r.from_symbol_id, imp.qualified_name FROM code_relationships r
             JOIN code_symbols imp ON imp.id = r.to_symbol_id
             WHERE r.relationship_type='IMPORTS' AND imp.symbol_type='import'",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut seen = std::collections::HashSet::new();
        for row in rows {
            let (module_id, target) = row?;
            // Try each path segment of the import target against internal module stems.
            for seg in target.split([':', '/', '.']).filter(|s| !s.is_empty()) {
                if let Some(&dst) = module_stem.get(seg) {
                    if dst != module_id && seen.insert((module_id, dst)) {
                        edges.push(json!({"source": sid(module_id), "target": sid(dst), "rel": "IMPORTS"}));
                    }
                }
            }
        }
    }

    // --- Commits (recent) ---
    let mut commit_shas: std::collections::HashSet<String> = std::collections::HashSet::new();
    {
        let mut stmt = conn.prepare("SELECT sha, message FROM commits ORDER BY timestamp DESC LIMIT 150")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
        for row in rows {
            let (sha, msg) = row?;
            commit_shas.insert(sha.clone());
            let label = msg.unwrap_or_default().lines().next().unwrap_or("").chars().take(32).collect::<String>();
            nodes.push(json!({"id": cid(&sha), "label": format!("{} {}", &sha[..sha.len().min(7)], label), "kind": "commit"}));
        }
    }
    // CHANGED_BY: symbol → commit
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT symbol_id, commit_sha FROM symbol_changes WHERE commit_sha IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (sym, sha) = row?;
            if symbol_ids.contains(&sym) && commit_shas.contains(&sha) {
                edges.push(json!({"source": sid(sym), "target": cid(&sha), "rel": "CHANGED_BY"}));
            }
        }
    }

    // --- Memories (+ evidence → commit) ---
    {
        let mut stmt = conn.prepare(
            "SELECT id, title, type, scope, source_commit_shas FROM memory_items WHERE status='active' LIMIT 2000",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?))
        })?;
        for row in rows {
            let (id, title, ty, scope, commits) = row?;
            nodes.push(json!({"id": format!("mem:{id}"), "label": title.chars().take(40).collect::<String>(), "kind": "memory", "memtype": ty, "scope": scope, "ref": id}));
            if let Some(js) = commits {
                if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&js) {
                    for sha in arr.iter().filter_map(|v| v.as_str()) {
                        if commit_shas.contains(sha) {
                            edges.push(json!({"source": format!("mem:{id}"), "target": cid(sha), "rel": "EVIDENCE"}));
                        }
                    }
                }
            }
        }
    }

    // --- Documents + sessions, with document→code mentions and document↔document links ---
    add_documents(conn, repo, &module_stem, &mut nodes, &mut edges)?;
    {
        let mut stmt = conn.prepare("SELECT id, started_at FROM sessions LIMIT 500")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)))?;
        for row in rows {
            let (id, started) = row?;
            nodes.push(json!({"id": format!("ses:{id}"), "label": format!("session {}", started.as_deref().map(|s| s.split('T').next().unwrap_or(s)).unwrap_or("")), "kind": "session", "ref": id}));
        }
    }

    Ok((nodes, edges))
}

/// Documents as nodes, linked to the code they mention (MENTIONS) and to other documents
/// they're topically similar to (RELATED, by shared significant terms — a lightweight
/// "related documents" without embeddings).
fn add_documents(
    conn: &Connection,
    repo: i64,
    _module_stem: &std::collections::HashMap<String, i64>,
    nodes: &mut Vec<Value>,
    edges: &mut Vec<Value>,
) -> Result<()> {
    use std::collections::{HashMap, HashSet};

    // Code tokens to look for in document text: file paths (specific), qualified method
    // names, and identifier names of length >= 5 (so prose like "redaction" matches the
    // `redact` symbol). Keyed token -> a representative symbol id.
    let mut tokens: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, name, qualified_name, symbol_type, file_path FROM code_symbols
             WHERE repo_id=?1 AND status='active' AND symbol_type != 'import' LIMIT 4000",
        )?;
        let rows = stmt.query_map([repo], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
        })?;
        for row in rows {
            let (id, name, qn, ty, file) = row?;
            if ty == "module" {
                tokens.entry(file).or_insert(id);
            } else {
                if qn.contains('.') {
                    tokens.entry(qn).or_insert(id);
                }
                if name.len() >= 5 {
                    tokens.entry(name).or_insert(id);
                }
            }
        }
    }

    let mut doc_terms: Vec<(i64, HashSet<String>)> = Vec::new();
    let mut stmt = conn.prepare("SELECT id, title, content FROM documents WHERE status='active' LIMIT 400")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;
    for row in rows {
        let (id, title, content) = row?;
        nodes.push(json!({"id": format!("doc:{id}"), "label": title, "kind": "document", "ref": id}));
        let mut mentioned = HashSet::new();
        for (tok, sym) in &tokens {
            if content.contains(tok.as_str()) {
                mentioned.insert(*sym);
            }
        }
        for sym in &mentioned {
            edges.push(json!({"source": format!("doc:{id}"), "target": format!("sym:{sym}"), "rel": "MENTIONS"}));
        }
        doc_terms.push((id, significant_terms(&content)));
    }

    // Document ↔ document: related if they share enough significant terms (Jaccard).
    for i in 0..doc_terms.len() {
        for j in (i + 1)..doc_terms.len() {
            let shared = doc_terms[i].1.intersection(&doc_terms[j].1).count();
            let union = doc_terms[i].1.union(&doc_terms[j].1).count().max(1);
            if shared >= 6 && (shared as f64 / union as f64) >= 0.08 {
                edges.push(json!({"source": format!("doc:{}", doc_terms[i].0), "target": format!("doc:{}", doc_terms[j].0), "rel": "RELATED"}));
            }
        }
    }
    Ok(())
}

/// Distinctive lowercase terms in a document (length 5..=24, alphabetic, minus common
/// stopwords), for lightweight document-similarity.
fn significant_terms(content: &str) -> std::collections::HashSet<String> {
    const STOP: &[&str] = &[
        "which", "there", "their", "would", "about", "these", "those", "where", "while",
        "could", "should", "every", "other", "after", "before", "being", "below", "above",
        "between", "because", "through", "without", "within", "across", "into", "with",
    ];
    content
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| (5..=24).contains(&w.len()) && w.chars().all(|c| c.is_alphabetic()) && !STOP.contains(&w.as_str()))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespacing_keeps_per_store_ids_distinct() {
        // Two stores with the same local id must map to different namespaced ids, and the
        // separator must survive the already-prefixed local id (`sym:`/`mem:` etc.).
        assert_eq!(ns("p0", "mem:1"), "p0~mem:1");
        assert_eq!(ns("p1", "mem:1"), "p1~mem:1");
        assert_ne!(ns("p0", "sym:42"), ns("p1", "sym:42"));
    }
}
