//! Document ingestion (general AIOS memory). Brings non-code documents — Markdown,
//! plain text, PDF, Word — into the store as **redacted evidence** with their own FTS
//! index, so an agent can recall project/customer knowledge, not just code.
//!
//! Governance: an explicit `recanta ingest <path>` is itself an opt-in action, so it is
//! always allowed; redaction (PRD §8.12a) runs on every document before storage. Bulk
//! automatic capture (sessions) is what the capture policy gates separately (§8.12b).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use crate::{hash, redact};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    Markdown,
    Text,
    Pdf,
    Docx,
    Doc,
}

impl DocFormat {
    pub fn from_extension(ext: &str) -> Option<DocFormat> {
        match ext.to_ascii_lowercase().as_str() {
            "md" | "markdown" | "mdown" | "mkd" => Some(DocFormat::Markdown),
            "txt" | "text" | "rst" | "org" | "adoc" | "log" => Some(DocFormat::Text),
            "pdf" => Some(DocFormat::Pdf),
            "docx" => Some(DocFormat::Docx),
            "doc" => Some(DocFormat::Doc),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            DocFormat::Markdown => "markdown",
            DocFormat::Text => "text",
            DocFormat::Pdf => "pdf",
            DocFormat::Docx => "docx",
            DocFormat::Doc => "doc",
        }
    }
}

/// Extract plain text from a document file.
pub fn extract_text(path: &Path, format: DocFormat) -> Result<String> {
    match format {
        DocFormat::Markdown | DocFormat::Text => std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display())),
        DocFormat::Pdf => crate::extract::pdf(path),
        DocFormat::Docx => crate::extract::docx(path),
        DocFormat::Doc => crate::extract::doc(path),
    }
}

/// Best-effort title: the first Markdown `# ` heading, else the file stem.
fn derive_title(path: &Path, format: DocFormat, text: &str) -> String {
    if format == DocFormat::Markdown {
        for line in text.lines().take(40) {
            let t = line.trim_start();
            if let Some(h) = t.strip_prefix("# ") {
                return h.trim().to_string();
            }
        }
    }
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "document".into())
}

#[derive(Debug, Default)]
pub struct IngestStats {
    pub ingested: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub redactions: usize,
}

/// Ingest one or more files/directories. Directories are walked (recursively when
/// `recursive`), picking up supported document files.
pub fn ingest_paths(
    conn: &Connection,
    project_id: &str,
    paths: &[PathBuf],
    recursive: bool,
) -> Result<IngestStats> {
    let mut stats = IngestStats::default();
    for path in paths {
        if path.is_dir() {
            for file in collect_docs(path, recursive) {
                ingest_one(conn, project_id, &file, &mut stats)?;
            }
        } else if path.is_file() {
            match path.extension().and_then(|e| e.to_str()).and_then(DocFormat::from_extension) {
                Some(_) => ingest_one(conn, project_id, path, &mut stats)?,
                None => bail!("unsupported document type: {} (try .md/.txt/.pdf/.docx/.doc)", path.display()),
            }
        } else {
            bail!("no such file or directory: {}", path.display());
        }
    }
    Ok(stats)
}

fn ingest_one(
    conn: &Connection,
    project_id: &str,
    path: &Path,
    stats: &mut IngestStats,
) -> Result<()> {
    let Some(format) = path.extension().and_then(|e| e.to_str()).and_then(DocFormat::from_extension)
    else {
        return Ok(());
    };
    let raw = match extract_text(path, format) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("recanta: skipping {} ({e:#})", path.display());
            stats.skipped += 1;
            return Ok(());
        }
    };
    // Collapse layout whitespace (PDFs especially pad lines to page width).
    let raw = normalize_whitespace(&raw);

    // Redact before storage (PRD §8.12a).
    let red = redact::redact(&raw);
    let content_hash = hash::sha256_hex(&red.text);
    let path_str = path.to_string_lossy().to_string();
    let title = derive_title(path, format, &red.text);

    // Skip if unchanged since last ingest.
    let prior: Option<String> = conn
        .query_row(
            "SELECT content_hash FROM documents WHERE project_id = ?1 AND path = ?2",
            rusqlite::params![project_id, path_str],
            |r| r.get(0),
        )
        .ok();
    let is_update = prior.is_some();
    if prior.as_deref() == Some(content_hash.as_str()) {
        stats.unchanged += 1;
        return Ok(());
    }

    conn.execute(
        "INSERT INTO documents
            (project_id, path, format, title, content, content_hash, char_count, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active')
         ON CONFLICT(project_id, path) DO UPDATE SET
            format = excluded.format, title = excluded.title, content = excluded.content,
            content_hash = excluded.content_hash, char_count = excluded.char_count,
            status = 'active', ingested_at = datetime('now')",
        rusqlite::params![
            project_id, path_str, format.label(), title, red.text, content_hash,
            red.text.chars().count() as i64
        ],
    )
    .with_context(|| format!("storing {}", path.display()))?;

    for hit in &red.hits {
        conn.execute(
            "INSERT INTO redaction_audit (pattern_id, path, span) VALUES (?1, ?2, ?3)",
            rusqlite::params![hit.pattern_id, path_str, format!("{}-{}", hit.start, hit.end)],
        )?;
    }
    stats.redactions += red.hits.len();
    if is_update {
        stats.updated += 1;
    } else {
        stats.ingested += 1;
    }
    Ok(())
}

/// Collapse runs of spaces/tabs to one space, trim line ends, and squeeze blank-line
/// runs — turning padded/extracted text (PDF page padding, Word runs) into compact,
/// searchable prose without losing paragraph structure.
fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0u32;
    for line in s.lines() {
        let mut collapsed = String::with_capacity(line.len());
        let mut prev_space = false;
        for ch in line.chars() {
            if ch == ' ' || ch == '\t' {
                if !prev_space {
                    collapsed.push(' ');
                }
                prev_space = true;
            } else {
                collapsed.push(ch);
                prev_space = false;
            }
        }
        let trimmed = collapsed.trim_end();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// A document search hit.
#[derive(Debug, Clone)]
pub struct DocHit {
    pub id: i64,
    pub title: String,
    pub path: String,
    pub snippet: String,
    pub rank: f64,
}

/// FTS5/BM25 search over ingested documents (PRD §13).
pub fn search(conn: &Connection, match_expr: &str, limit: usize) -> Result<Vec<DocHit>> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.title, d.path,
                snippet(documents_fts, 1, '', '', '…', 12) AS snip,
                bm25(documents_fts) AS rank
         FROM documents_fts
         JOIN documents d ON d.id = documents_fts.rowid
         WHERE documents_fts MATCH ?1 AND d.status = 'active'
         ORDER BY rank, d.id
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![match_expr, limit as i64], |r| {
            Ok(DocHit {
                id: r.get(0)?,
                title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                path: r.get(2)?,
                snippet: r.get(3)?,
                rank: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Recursively (or shallowly) collect supported document files under a directory.
fn collect_docs(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if recursive && !name.starts_with('.') && name != "node_modules" {
                    stack.push(path);
                }
            } else if ft.is_file()
                && path.extension().and_then(|e| e.to_str()).and_then(DocFormat::from_extension).is_some()
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
