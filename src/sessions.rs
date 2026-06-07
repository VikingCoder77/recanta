//! Session import (general AIOS memory). Reads agent transcripts for a project and
//! extracts structured memories with the raw conversation kept as searchable evidence —
//! never a verbatim dump (PRD §13). Everything is redacted first (§8.12a); raw transcript
//! storage is governed by the capture policy (§8.12b).
//!
//! Harness transcript formats differ, so each lives in a `sessions::<harness>` submodule
//! that produces a uniform [`ParsedSession`]; the import + search logic here is shared.
//! Harness storage layouts drift between versions — parsers are tolerant and every
//! command accepts `--from` to point at the files directly.

mod claude;
mod codex;
mod gemini;
mod opencode;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::memory::{self, Importance, MemType, NewMemory, Scope};
use crate::redact;

/// Who produced a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// One conversation turn, normalized across harnesses.
#[derive(Debug, Clone)]
pub struct RawTurn {
    pub role: Role,
    pub text: String,
    pub timestamp: Option<String>,
    /// File paths the turn touched (from tool calls), if any.
    pub files: Vec<String>,
}

/// One session's worth of turns, harness-agnostic.
#[derive(Debug)]
pub struct ParsedSession {
    pub session_uid: String,
    pub source_path: String,
    pub turns: Vec<RawTurn>,
}

#[derive(Debug, Default)]
pub struct ImportStats {
    pub imported: usize,
    pub skipped: usize,
    pub memories: usize,
    pub redactions: usize,
    /// Per-harness informational notes (e.g. "no sessions found at …").
    pub notes: Vec<String>,
}

/// The harnesses Recanta can import from.
pub const HARNESSES: &[&str] = &["claude-code", "codex", "gemini", "opencode"];

/// Import sessions for one harness (or `all`). `from` overrides the harness's default
/// session location for the single-harness case.
pub fn import(
    conn: &Connection,
    project_id: &str,
    home: &Path,
    root: &Path,
    capture_raw: bool,
    harness: &str,
    from: Option<&Path>,
) -> Result<ImportStats> {
    let mut stats = ImportStats::default();
    let targets: Vec<&str> = if harness == "all" {
        HARNESSES.to_vec()
    } else {
        vec![harness]
    };

    for name in targets {
        // `--from` only applies when a single harness was requested.
        let from = if harness == "all" { None } else { from };
        let (sessions, notes) = match name {
            "claude-code" => claude::collect(home, root, from),
            "codex" => codex::collect(home, root, from),
            "gemini" => gemini::collect(home, root, from),
            "opencode" => opencode::collect(home, root, from),
            other => {
                stats.notes.push(format!("unknown harness `{other}`"));
                continue;
            }
        };
        stats.notes.extend(notes);
        for session in sessions {
            import_one(conn, project_id, name, &session, capture_raw, &mut stats)?;
        }
    }
    Ok(stats)
}

fn import_one(
    conn: &Connection,
    project_id: &str,
    harness: &str,
    parsed: &ParsedSession,
    capture_raw: bool,
    stats: &mut ImportStats,
) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM sessions WHERE project_id = ?1 AND harness = ?2 AND session_uid = ?3",
            rusqlite::params![project_id, harness, parsed.session_uid],
            |_| Ok(()),
        )
        .is_ok();
    if exists {
        stats.skipped += 1;
        return Ok(());
    }

    // Build the derived view from the normalized turns.
    let mut user_messages = 0u32;
    let mut assistant_messages = 0u32;
    let mut intent: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut transcript = String::new();
    let mut started_at: Option<String> = None;
    let mut ended_at: Option<String> = None;

    for turn in &parsed.turns {
        if let Some(ts) = &turn.timestamp {
            if started_at.is_none() {
                started_at = Some(ts.clone());
            }
            ended_at = Some(ts.clone());
        }
        for f in &turn.files {
            if !files.contains(f) {
                files.push(f.clone());
            }
        }
        let text = turn.text.trim();
        if text.is_empty() {
            continue;
        }
        match turn.role {
            Role::User => {
                user_messages += 1;
                if intent.is_none() {
                    intent = Some(text.to_string());
                }
                transcript.push_str("user: ");
            }
            Role::Assistant => {
                assistant_messages += 1;
                transcript.push_str("assistant: ");
            }
        }
        transcript.push_str(text);
        transcript.push('\n');
    }

    let files_json = serde_json::to_string(&files)?;
    let raw = if capture_raw {
        let red = redact::redact(&transcript);
        stats.redactions += red.hits.len();
        Some(red.text)
    } else {
        None
    };

    conn.execute(
        "INSERT INTO sessions
            (project_id, harness, session_uid, source_path, started_at, ended_at,
             user_messages, assistant_messages, files_touched, raw_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            project_id, harness, parsed.session_uid, parsed.source_path, started_at,
            ended_at, user_messages, assistant_messages, files_json, raw
        ],
    )
    .context("recording session")?;

    let intent = intent.unwrap_or_else(|| "(no initial request captured)".into());
    let red_intent = redact::redact(&intent);
    stats.redactions += red_intent.hits.len();
    let when = started_at.as_deref().unwrap_or("unknown date");
    let files_line = if files.is_empty() {
        "no files recorded".to_string()
    } else {
        format!("files: {}", files.join(", "))
    };
    memory::insert(
        conn,
        &NewMemory {
            mem_type: MemType::Episodic,
            scope: Scope::Project,
            title: format!("{harness} session {}: {}", short_date(when), truncate(&red_intent.text, 60)),
            content: format!(
                "{harness} session on {when}. Intent: {}. {files_line}. \
                 {user_messages} user / {assistant_messages} assistant messages.",
                red_intent.text
            ),
            importance: Importance::Low,
            confidence: 1.0,
            branch: None,
        },
    )?;
    stats.memories += 1;
    stats.imported += 1;
    Ok(())
}

/// A full-text hit inside a captured session transcript.
#[derive(Debug, Clone)]
pub struct TranscriptHit {
    pub session_id: i64,
    pub started_at: Option<String>,
    pub snippet: String,
    pub rank: f64,
}

/// Search captured chat transcripts (only sessions imported with capture on). Lets
/// retrieval recall what was discussed many sessions ago (PRD §13).
pub fn search_transcripts(conn: &Connection, match_expr: &str, limit: usize) -> Result<Vec<TranscriptHit>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.started_at,
                snippet(session_fts, 0, '', '', '…', 14) AS snip,
                bm25(session_fts) AS rank
         FROM session_fts
         JOIN sessions s ON s.id = session_fts.rowid
         WHERE session_fts MATCH ?1 AND s.raw_text IS NOT NULL
         ORDER BY rank, s.id
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![match_expr, limit as i64], |r| {
            Ok(TranscriptHit {
                session_id: r.get(0)?,
                started_at: r.get(1)?,
                snippet: r.get(2)?,
                rank: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub(crate) fn short_date(ts: &str) -> &str {
    ts.split('T').next().unwrap_or(ts)
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("");
    if line.chars().count() <= max {
        line.to_string()
    } else {
        line.chars().take(max).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_transcript_is_searchable() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::migrate(&conn).unwrap();
        conn.execute("INSERT INTO projects (id, name, root_path) VALUES ('p','p','/tmp')", []).unwrap();
        conn.execute(
            "INSERT INTO sessions (project_id, harness, session_uid, raw_text)
             VALUES ('p', 'claude-code', 's1', 'user: q assistant: we shard by tenant_id past 500 customers')",
            [],
        ).unwrap();

        let q = crate::memory::fts_query("shard tenant customers").unwrap();
        let hits = search_transcripts(&conn, &q, 10).unwrap();
        assert_eq!(hits.len(), 1, "captured transcript must be full-text searchable");
        assert!(hits[0].snippet.contains("tenant_id"));
    }

    #[test]
    fn import_is_idempotent_and_extracts_intent() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::migrate(&conn).unwrap();
        conn.execute("INSERT INTO projects (id, name, root_path) VALUES ('p','p','/tmp')", []).unwrap();

        let parsed = ParsedSession {
            session_uid: "s1".into(),
            source_path: "/x".into(),
            turns: vec![
                RawTurn { role: Role::User, text: "design the api".into(), timestamp: Some("2026-06-01T10:00:00Z".into()), files: vec![] },
                RawTurn { role: Role::Assistant, text: "use REST".into(), timestamp: None, files: vec!["src/api.rs".into()] },
            ],
        };
        let mut stats = ImportStats::default();
        import_one(&conn, "p", "codex", &parsed, true, &mut stats).unwrap();
        assert_eq!(stats.imported, 1);
        assert_eq!(stats.memories, 1);

        // Re-import is a no-op.
        let mut stats2 = ImportStats::default();
        import_one(&conn, "p", "codex", &parsed, true, &mut stats2).unwrap();
        assert_eq!(stats2.imported, 0);
        assert_eq!(stats2.skipped, 1);

        let title: String = conn
            .query_row("SELECT title FROM memory_items WHERE type='episodic'", [], |r| r.get(0))
            .unwrap();
        assert!(title.contains("design the api"));
    }
}
