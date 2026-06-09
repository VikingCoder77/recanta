//! Incremental code graph (PRD §8.10). Tree-sitter extracts, per file: a `module`
//! symbol, the functions/classes/methods it `DEFINES`, and its file-level `IMPORTS`.
//! Symbol identity is `qualified_name` + `body_hash`. `CALLS` is deferred (§8.10).
//!
//! A full reindex rebuilds the repo's edges and upserts symbols by
//! `(file_path, qualified_name)`; symbols no longer seen are marked `deleted`, never
//! silently removed.

pub mod lang;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;
use tree_sitter::{Node, Parser};

use crate::hash;
use lang::Lang;

/// Skip files larger than this when indexing (mirrors the diff cap spirit, §8.4).
const MAX_FILE_BYTES: u64 = 1_000_000;

/// One extracted definition.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub qualified_name: String,
    pub symbol_type: String,
    pub signature: String,
    pub start_line: i64,
    pub end_line: i64,
    pub body_hash: String,
}

/// Result of parsing one file.
#[derive(Debug, Default)]
pub struct FileParse {
    pub symbols: Vec<Symbol>,
    pub imports: Vec<String>,
}

/// Parse source text into symbols + imports. Returns `None` if the grammar can't load.
pub fn parse_source(src: &str, lang: Lang) -> Option<FileParse> {
    let mut parser = Parser::new();
    parser.set_language(&lang.tree_sitter()).ok()?;
    let tree = parser.parse(src, None)?;
    let mut parse = FileParse::default();
    let mut scope: Vec<(String, bool)> = Vec::new();
    walk(tree.root_node(), src.as_bytes(), lang, &mut scope, &mut parse);
    Some(parse)
}

/// Recursive descent: collect definitions (tracking enclosing scope to build qualified
/// names) and imports, driven by each language's `classify`.
fn walk(node: Node, src: &[u8], lang: Lang, scope: &mut Vec<(String, bool)>, out: &mut FileParse) {
    use lang::Classified;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let in_methods = scope.last().map(|(_, m)| *m).unwrap_or(false);
        match lang.classify(child.kind(), in_methods) {
            Classified::Import => {
                if let Some(t) = lang::import_target(child, src, lang) {
                    out.imports.push(t);
                }
            }
            Classified::Def { symbol_type, emit, scopes, methods } => {
                let name = lang::def_name(child, src).unwrap_or_else(|| "<anonymous>".into());
                if emit {
                    let text = child.utf8_text(src).unwrap_or("");
                    out.symbols.push(Symbol {
                        qualified_name: qualify(scope, &name),
                        symbol_type: symbol_type.to_string(),
                        signature: first_line(text),
                        start_line: child.start_position().row as i64 + 1,
                        end_line: child.end_position().row as i64 + 1,
                        body_hash: hash::sha256_hex(text),
                        name: name.clone(),
                    });
                }
                if scopes {
                    scope.push((name, methods));
                    walk(child, src, lang, scope, out);
                    scope.pop();
                } else {
                    walk(child, src, lang, scope, out);
                }
            }
            Classified::Recurse => walk(child, src, lang, scope, out),
        }
    }
}

fn qualify(scope: &[(String, bool)], name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        let mut q: String = scope.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(".");
        q.push('.');
        q.push_str(name);
        q
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().chars().take(200).collect()
}

/// Parent qualified name (everything before the last `.`), if nested.
fn parent_qualified(q: &str) -> Option<&str> {
    q.rfind('.').map(|i| &q[..i])
}

/// Counts produced by an index run.
#[derive(Debug, Default)]
pub struct IndexStats {
    pub files: usize,
    pub symbols: usize,
    pub imports: usize,
    pub deleted: usize,
}

/// Full reindex of a repository's code graph.
pub fn index_repo(
    conn: &Connection,
    repo_id: i64,
    root: &Path,
    ignore: &[String],
    head: Option<&str>,
) -> Result<IndexStats> {
    let files = collect_source_files(root, ignore);
    let tx = conn.unchecked_transaction()?;

    // Rebuild edges from scratch for this repo.
    tx.execute(
        "DELETE FROM code_relationships
         WHERE from_symbol_id IN (SELECT id FROM code_symbols WHERE repo_id = ?1)",
        [repo_id],
    )?;

    // Existing active symbols, so we can update in place and detect deletions.
    let mut existing: HashMap<(String, String), i64> = HashMap::new();
    {
        let mut stmt = tx.prepare(
            "SELECT id, file_path, qualified_name FROM code_symbols
             WHERE repo_id = ?1 AND status = 'active'",
        )?;
        let rows = stmt.query_map([repo_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (id, file, q) = row?;
            existing.insert((file, q), id);
        }
    }

    let mut seen: HashSet<i64> = HashSet::new();
    let mut stats = IndexStats::default();

    for abs in &files {
        let rel = abs.strip_prefix(root).unwrap_or(abs).to_string_lossy().to_string();
        let Some(lang) = abs.extension().and_then(|e| e.to_str()).and_then(Lang::from_extension)
        else {
            continue;
        };
        let Ok(src) = std::fs::read_to_string(abs) else { continue };
        let Some(parse) = parse_source(&src, lang) else { continue };
        reindex_file(&tx, repo_id, &rel, abs, &src, &parse, head, &mut existing, &mut seen, &mut stats)?;
        stats.files += 1;
    }

    // Anything previously active but not seen this run is now deleted (not removed).
    for ((_, _), id) in existing.iter() {
        if !seen.contains(id) {
            tx.execute("UPDATE code_symbols SET status = 'deleted' WHERE id = ?1", [id])?;
            stats.deleted += 1;
        }
    }

    if let Some(h) = head {
        tx.execute("UPDATE repositories SET indexed_commit = ?1 WHERE id = ?2", rusqlite::params![h, repo_id])?;
    }
    tx.commit()?;
    Ok(stats)
}

/// Incrementally reindex only the given files (relative paths), e.g. after a Git event.
/// Each file's symbols and edges are rebuilt in place; symbols no longer present in a
/// file are marked deleted; files that no longer exist have all their symbols deleted.
/// Falls back to nothing if `changed` is empty. Updates `indexed_commit`.
pub fn index_changed(
    conn: &Connection,
    repo_id: i64,
    root: &Path,
    changed: &[String],
    head: Option<&str>,
) -> Result<IndexStats> {
    let tx = conn.unchecked_transaction()?;
    let mut stats = IndexStats::default();

    for rel in changed {
        let abs = root.join(rel);
        // Existing active symbols for just this file.
        let mut existing: HashMap<(String, String), i64> = HashMap::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id, file_path, qualified_name FROM code_symbols
                 WHERE repo_id = ?1 AND file_path = ?2 AND status = 'active'",
            )?;
            let rows = stmt.query_map(rusqlite::params![repo_id, rel], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?;
            for row in rows {
                let (id, file, q) = row?;
                existing.insert((file, q), id);
            }
        }
        // Clear this file's outgoing edges; we rebuild them below.
        tx.execute(
            "DELETE FROM code_relationships WHERE from_symbol_id IN
                (SELECT id FROM code_symbols WHERE repo_id = ?1 AND file_path = ?2)",
            rusqlite::params![repo_id, rel],
        )?;

        let mut seen: HashSet<i64> = HashSet::new();
        let lang = abs.extension().and_then(|e| e.to_str()).and_then(Lang::from_extension);
        if let (Some(lang), Ok(src)) = (lang, std::fs::read_to_string(&abs)) {
            if let Some(parse) = parse_source(&src, lang) {
                reindex_file(&tx, repo_id, rel, &abs, &src, &parse, head, &mut existing, &mut seen, &mut stats)?;
                stats.files += 1;
            }
        }
        // Anything previously active in this file but not seen is now deleted.
        for ((_, _), id) in existing.iter() {
            if !seen.contains(id) {
                tx.execute("UPDATE code_symbols SET status = 'deleted' WHERE id = ?1", [id])?;
                stats.deleted += 1;
            }
        }
    }

    if let Some(h) = head {
        tx.execute("UPDATE repositories SET indexed_commit = ?1 WHERE id = ?2", rusqlite::params![h, repo_id])?;
    }
    tx.commit()?;
    Ok(stats)
}

/// Upsert one file's module symbol, definitions, DEFINES edges, and IMPORTS.
#[allow(clippy::too_many_arguments)]
fn reindex_file(
    tx: &Connection,
    repo_id: i64,
    rel: &str,
    abs: &Path,
    src: &str,
    parse: &FileParse,
    head: Option<&str>,
    existing: &mut HashMap<(String, String), i64>,
    seen: &mut HashSet<i64>,
    stats: &mut IndexStats,
) -> Result<()> {
    let filename = abs.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let line_count = src.lines().count() as i64;
    let module_id = upsert(
        tx, repo_id, rel, "module", &filename, rel, "", 1, line_count.max(1),
        &hash::sha256_hex(src), head, existing, seen,
    )?;

    let mut local: HashMap<String, i64> = HashMap::new();
    for s in &parse.symbols {
        let id = upsert(
            tx, repo_id, rel, &s.symbol_type, &s.name, &s.qualified_name, &s.signature,
            s.start_line, s.end_line, &s.body_hash, head, existing, seen,
        )?;
        local.insert(s.qualified_name.clone(), id);
        stats.symbols += 1;
    }
    for s in &parse.symbols {
        let parent = parent_qualified(&s.qualified_name)
            .and_then(|p| local.get(p).copied())
            .unwrap_or(module_id);
        insert_edge(tx, parent, Some(local[&s.qualified_name]), "DEFINES")?;
    }
    let mut seen_imports: HashSet<String> = HashSet::new();
    for target in &parse.imports {
        if !seen_imports.insert(target.clone()) {
            continue;
        }
        let imp_id = upsert(tx, repo_id, rel, "import", target, target, "", 0, 0, target, head, existing, seen)?;
        insert_edge(tx, module_id, Some(imp_id), "IMPORTS")?;
        stats.imports += 1;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn upsert(
    conn: &Connection,
    repo_id: i64,
    file: &str,
    sym_type: &str,
    name: &str,
    qualified: &str,
    signature: &str,
    start: i64,
    end: i64,
    body_hash: &str,
    head: Option<&str>,
    existing: &mut HashMap<(String, String), i64>,
    seen: &mut HashSet<i64>,
) -> Result<i64> {
    let key = (file.to_string(), qualified.to_string());
    if let Some(&id) = existing.get(&key) {
        conn.execute(
            "UPDATE code_symbols SET symbol_type=?1, name=?2, signature=?3, start_line=?4,
                end_line=?5, body_hash=?6, last_seen_commit=?7, status='active' WHERE id=?8",
            rusqlite::params![sym_type, name, signature, start, end, body_hash, head, id],
        )?;
        seen.insert(id);
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO code_symbols
            (repo_id, file_path, symbol_type, name, qualified_name, signature,
             start_line, end_line, body_hash, last_seen_commit, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'active')",
        rusqlite::params![repo_id, file, sym_type, name, qualified, signature, start, end, body_hash, head],
    )
    .context("inserting symbol")?;
    let id = conn.last_insert_rowid();
    existing.insert(key, id);
    seen.insert(id);
    Ok(id)
}

fn insert_edge(conn: &Connection, from: i64, to: Option<i64>, rel: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO code_relationships (from_symbol_id, to_symbol_id, relationship_type, confidence, source)
         VALUES (?1, ?2, ?3, 1.0, 'tree-sitter')",
        rusqlite::params![from, to, rel],
    )?;
    Ok(())
}

/// Recursively collect indexable source files under `root`, skipping ignored and
/// hidden directories and oversized files.
fn collect_source_files(root: &Path, ignore: &[String]) -> Vec<std::path::PathBuf> {
    let skip_dirs = ignored_dirs(ignore);
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if name.starts_with('.') || skip_dirs.contains(&name) {
                    continue;
                }
                stack.push(path);
            } else if ft.is_file() {
                let supported = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| Lang::from_extension(e).is_some());
                let small = entry.metadata().map(|m| m.len() <= MAX_FILE_BYTES).unwrap_or(false);
                if supported && small {
                    out.push(path);
                }
            }
        }
    }
    out.sort();
    out
}

/// Directory names to skip, derived from ignore rules plus always-skip defaults.
fn ignored_dirs(ignore: &[String]) -> HashSet<String> {
    let mut set: HashSet<String> = ["node_modules", "target", ".venv", "dist", "build", ".git", ".recanta"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for pat in ignore {
        if let Some(dir) = pat.strip_suffix('/') {
            if !dir.contains('*') {
                set.insert(dir.to_string());
            }
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_python_symbols_and_imports() {
        let src = "import os\nfrom a.b import c\n\nclass Foo:\n    def bar(self):\n        pass\n\ndef top():\n    return 1\n";
        let p = parse_source(src, Lang::Python).unwrap();
        let names: Vec<_> = p.symbols.iter().map(|s| (s.qualified_name.as_str(), s.symbol_type.as_str())).collect();
        assert!(names.contains(&("Foo", "class")));
        assert!(names.contains(&("Foo.bar", "method")));
        assert!(names.contains(&("top", "function")));
        assert!(p.imports.contains(&"os".to_string()));
        assert!(p.imports.iter().any(|i| i.contains("a.b")));
    }

    fn temp_repo(conn: &Connection) -> (std::path::PathBuf, i64) {
        crate::db::migrations::migrate(conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, root_path) VALUES ('p', 'p', '/tmp')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO repositories (project_id, root_path) VALUES ('p', '/tmp')",
            [],
        )
        .unwrap();
        let repo_id = conn.last_insert_rowid();
        let dir = std::env::temp_dir().join(format!("recanta-idx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        (dir, repo_id)
    }

    #[test]
    fn reindex_marks_vanished_symbols_deleted_not_removed() {
        let conn = Connection::open_in_memory().unwrap();
        let (dir, repo_id) = temp_repo(&conn);
        let file = dir.join("m.py");

        std::fs::write(&file, "def a():\n    pass\ndef b():\n    pass\n").unwrap();
        let s1 = index_repo(&conn, repo_id, &dir, &[], Some("c1")).unwrap();
        assert_eq!(s1.symbols, 2);

        // Drop `b`, reindex: `b` must be marked deleted, not deleted from the table.
        std::fs::write(&file, "def a():\n    pass\n").unwrap();
        let s2 = index_repo(&conn, repo_id, &dir, &[], Some("c2")).unwrap();
        assert_eq!(s2.deleted, 1);
        let b_status: String = conn
            .query_row("SELECT status FROM code_symbols WHERE name='b'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(b_status, "deleted");
        let active: i64 = conn
            .query_row("SELECT COUNT(*) FROM code_symbols WHERE name='a' AND status='active'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(active, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extracts_rust_symbols_with_impl_methods() {
        let src = "use std::collections::HashMap;\n\
                   pub struct Trader { balance: u32 }\n\
                   enum Side { Buy, Sell }\n\
                   impl Trader {\n    pub fn new() -> Self { Self { balance: 0 } }\n    fn buy(&self) {}\n}\n\
                   trait Exec { fn run(&self); }\n\
                   fn main() {}\n";
        let p = parse_source(src, Lang::Rust).unwrap();
        let got: Vec<_> = p.symbols.iter().map(|s| (s.qualified_name.as_str(), s.symbol_type.as_str())).collect();
        assert!(got.contains(&("Trader", "struct")));
        assert!(got.contains(&("Side", "enum")));
        assert!(got.contains(&("Trader.new", "method")), "impl methods qualify to the type: {got:?}");
        assert!(got.contains(&("Trader.buy", "method")));
        assert!(got.contains(&("Exec", "trait")));
        assert!(got.contains(&("main", "function")));
        // The impl block itself is not emitted as a separate symbol.
        assert!(!got.iter().any(|(_, t)| *t == "impl"));
        assert!(p.imports.iter().any(|i| i.contains("std::collections")));
    }

    #[test]
    fn extracts_typescript_symbols() {
        let src = "import { x } from './m';\nexport class A {\n  m() {}\n}\nfunction f() {}\n";
        let p = parse_source(src, Lang::TypeScript).unwrap();
        let q: Vec<_> = p.symbols.iter().map(|s| s.qualified_name.as_str()).collect();
        assert!(q.contains(&"A"));
        assert!(q.contains(&"A.m"));
        assert!(q.contains(&"f"));
        assert!(p.imports.iter().any(|i| i == "./m"));
    }
}
