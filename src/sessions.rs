//! Session import (general AIOS memory). Reads agent transcripts for a project and
//! extracts structured memories with the raw transcript kept as evidence — never a
//! verbatim dump (PRD §13, "summaries are indexes").
//!
//! Governance (PRD §8.12b): running `import-sessions` is an explicit opt-in, so derived
//! metadata + an episodic memory are always extracted. The *raw transcript* is stored
//! only when the capture policy enables it. Everything is redacted first (§8.12a).
//!
//! v0.1 supports Claude Code, whose per-project transcripts live at
//! `~/.claude/projects/<encoded-root>/<session>.jsonl` (one JSON event per line).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::Value;

use crate::memory::{self, Importance, MemType, NewMemory, Scope};
use crate::redact;

/// Claude Code encodes a project's absolute path by replacing every non-alphanumeric
/// character with `-` (e.g. `/Users/tp/AI Projects/recanta` →
/// `-Users-tp-AI-Projects-recanta`).
pub fn claude_project_dir(home: &Path, root: &Path) -> PathBuf {
    let encoded: String = root
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    home.join(".claude").join("projects").join(encoded)
}

/// Derived view of one session transcript.
#[derive(Debug, Default)]
struct SessionSummary {
    session_uid: String,
    started_at: Option<String>,
    ended_at: Option<String>,
    user_messages: u32,
    assistant_messages: u32,
    intent: Option<String>,
    files_touched: Vec<String>,
    /// Concatenated human/assistant text (no tool noise), for optional raw evidence.
    transcript_text: String,
}

#[derive(Debug, Default)]
pub struct ImportStats {
    pub imported: usize,
    pub skipped: usize,
    pub memories: usize,
    pub redactions: usize,
    /// Set when the harness session directory doesn't exist.
    pub no_session_dir: Option<PathBuf>,
}

/// Import Claude Code sessions for `root` into the store.
pub fn import_claude(
    conn: &Connection,
    project_id: &str,
    home: &Path,
    root: &Path,
    capture_raw: bool,
    from_override: Option<&Path>,
) -> Result<ImportStats> {
    let dir = match from_override {
        Some(p) => p.to_path_buf(),
        None => claude_project_dir(home, root),
    };
    let mut stats = ImportStats::default();
    if !dir.is_dir() {
        stats.no_session_dir = Some(dir);
        return Ok(stats);
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect();
    files.sort();

    for file in &files {
        let summary = match parse_session(file) {
            Some(s) => s,
            None => continue,
        };
        import_one(conn, project_id, file, &summary, capture_raw, &mut stats)?;
    }
    Ok(stats)
}

fn import_one(
    conn: &Connection,
    project_id: &str,
    file: &Path,
    s: &SessionSummary,
    capture_raw: bool,
    stats: &mut ImportStats,
) -> Result<()> {
    // Idempotency: skip sessions already imported (PRD §12.2 spirit).
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM sessions WHERE project_id = ?1 AND harness = 'claude-code' AND session_uid = ?2",
            rusqlite::params![project_id, s.session_uid],
            |_| Ok(()),
        )
        .is_ok();
    if exists {
        stats.skipped += 1;
        return Ok(());
    }

    let files_json = serde_json::to_string(&s.files_touched)?;
    let raw = if capture_raw {
        let red = redact::redact(&s.transcript_text);
        stats.redactions += red.hits.len();
        Some(red.text)
    } else {
        None
    };

    conn.execute(
        "INSERT INTO sessions
            (project_id, harness, session_uid, source_path, started_at, ended_at,
             user_messages, assistant_messages, files_touched, raw_text)
         VALUES (?1, 'claude-code', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            project_id, s.session_uid, file.to_string_lossy(), s.started_at, s.ended_at,
            s.user_messages, s.assistant_messages, files_json, raw
        ],
    )
    .context("recording session")?;

    // Episodic memory summarizing the session, redacted, searchable.
    let intent = s.intent.clone().unwrap_or_else(|| "(no initial request captured)".into());
    let red_intent = redact::redact(&intent);
    stats.redactions += red_intent.hits.len();
    let when = s.started_at.as_deref().unwrap_or("unknown date");
    let title = format!("Session {}: {}", short_date(when), truncate(&red_intent.text, 60));
    let files_line = if s.files_touched.is_empty() {
        "no files recorded".to_string()
    } else {
        format!("files: {}", s.files_touched.join(", "))
    };
    let content = format!(
        "Claude Code session on {when}. Intent: {}. {files_line}. \
         {} user / {} assistant messages.",
        red_intent.text, s.user_messages, s.assistant_messages
    );
    memory::insert(
        conn,
        &NewMemory {
            mem_type: MemType::Episodic,
            scope: Scope::Project,
            title,
            content,
            importance: Importance::Low,
            confidence: 1.0,
            branch: None,
        },
    )?;
    stats.memories += 1;
    stats.imported += 1;
    Ok(())
}

/// Parse a Claude Code `.jsonl` transcript into a derived summary. Tolerant of unknown
/// line shapes — anything it can't read is skipped.
fn parse_session(file: &Path) -> Option<SessionSummary> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut s = SessionSummary {
        session_uid: file.file_stem()?.to_string_lossy().to_string(),
        ..Default::default()
    };

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };

        if let Some(uid) = v.get("sessionId").and_then(|x| x.as_str()) {
            s.session_uid = uid.to_string();
        }
        if let Some(ts) = v.get("timestamp").and_then(|x| x.as_str()) {
            if s.started_at.is_none() {
                s.started_at = Some(ts.to_string());
            }
            s.ended_at = Some(ts.to_string());
        }

        match v.get("type").and_then(|x| x.as_str()) {
            Some("user") => {
                let text = message_text(&v);
                // Skip tool-result-only turns (no human text).
                if !text.trim().is_empty() {
                    s.user_messages += 1;
                    if s.intent.is_none() {
                        s.intent = Some(text.trim().to_string());
                    }
                    push_transcript(&mut s.transcript_text, "user", &text);
                }
            }
            Some("assistant") => {
                s.assistant_messages += 1;
                let text = message_text(&v);
                if !text.trim().is_empty() {
                    push_transcript(&mut s.transcript_text, "assistant", &text);
                }
                collect_files(&v, &mut s.files_touched);
            }
            _ => {}
        }
    }
    Some(s)
}

/// Extract concatenated text from a message's `content` (string or array of blocks).
fn message_text(v: &Value) -> String {
    let content = match v.get("message").and_then(|m| m.get("content")) {
        Some(c) => c,
        None => return String::new(),
    };
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    let mut out = String::new();
    if let Some(arr) = content.as_array() {
        for block in arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(t);
                }
            }
        }
    }
    out
}

/// Collect file paths from an assistant turn's tool-use blocks.
fn collect_files(v: &Value, files: &mut Vec<String>) {
    let Some(arr) = v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) else {
        return;
    };
    for block in arr {
        if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
            continue;
        }
        let path = block
            .get("input")
            .and_then(|i| i.get("file_path").or_else(|| i.get("path")))
            .and_then(|p| p.as_str());
        if let Some(p) = path {
            let p = p.to_string();
            if !files.contains(&p) {
                files.push(p);
            }
        }
    }
}

fn push_transcript(buf: &mut String, role: &str, text: &str) {
    buf.push_str(role);
    buf.push_str(": ");
    buf.push_str(text);
    buf.push('\n');
}

fn short_date(ts: &str) -> &str {
    ts.split('T').next().unwrap_or(ts)
}

fn truncate(s: &str, max: usize) -> String {
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
    fn encodes_claude_project_path() {
        let dir = claude_project_dir(Path::new("/home/u"), Path::new("/Users/tp/AI Projects/recanta"));
        assert!(dir.ends_with("-Users-tp-AI-Projects-recanta"));
    }

    #[test]
    fn parses_a_minimal_transcript() {
        let dir = std::env::temp_dir().join(format!("recanta-sess-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("abc.jsonl");
        let lines = [
            r#"{"type":"user","timestamp":"2026-06-06T10:00:00Z","message":{"role":"user","content":"fix the trailing stop bug"}}"#,
            r#"{"type":"assistant","timestamp":"2026-06-06T10:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"On it"},{"type":"tool_use","name":"Edit","input":{"file_path":"src/trade.rs"}}]}}"#,
            r#"{"type":"user","timestamp":"2026-06-06T10:00:06Z","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#,
        ];
        std::fs::write(&f, lines.join("\n")).unwrap();

        let s = parse_session(&f).unwrap();
        assert_eq!(s.user_messages, 1, "tool-result-only user turn must not count");
        assert_eq!(s.assistant_messages, 1);
        assert_eq!(s.intent.as_deref(), Some("fix the trailing stop bug"));
        assert_eq!(s.files_touched, vec!["src/trade.rs".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
