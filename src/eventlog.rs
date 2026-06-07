//! Shared ingestion for `record-edit` / `record-event` (PRD §8.4): read a payload from
//! stdin, redact it (§8.12a), and append an idempotent event (§12.2). Ingestion caps
//! apply — oversized payloads are noted, not stored in full.

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::Value;

use crate::redact;

/// Skip storing payloads larger than this (PRD §8.4 default cap).
pub const MAX_PAYLOAD: usize = 512 * 1024;

/// Read all of stdin to a string.
pub fn read_stdin() -> Result<String> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).context("reading stdin")?;
    Ok(buf)
}

/// Outcome of recording an event.
pub struct Recorded {
    pub event_id: i64,
    /// False if a duplicate (same idempotency key) was already present.
    pub inserted: bool,
    pub redactions: usize,
}

/// Append an event. `idempotency_key` makes duplicate hook fires no-ops. The payload is
/// redacted before storage; oversized payloads are replaced by a note.
#[allow(clippy::too_many_arguments)]
pub fn record(
    conn: &Connection,
    kind: &str,
    source: &str,
    project_id: &str,
    repo_id: Option<i64>,
    branch: Option<&str>,
    idempotency_key: &str,
    payload: &str,
) -> Result<Recorded> {
    let (stored, redactions) = if payload.len() > MAX_PAYLOAD {
        (format!("[skipped: payload {} bytes exceeds {MAX_PAYLOAD}]", payload.len()), 0)
    } else {
        let red = redact::redact(payload);
        (red.text, red.hits.len())
    };

    let inserted = conn.execute(
        "INSERT OR IGNORE INTO events
            (idempotency_key, type, source, project_id, repo_id, branch, payload_redacted, capture_mode)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'summary')",
        rusqlite::params![idempotency_key, kind, source, project_id, repo_id, branch, stored],
    )? == 1;

    if !inserted {
        let id: i64 = conn.query_row(
            "SELECT id FROM events WHERE idempotency_key = ?1",
            [idempotency_key],
            |r| r.get(0),
        )?;
        return Ok(Recorded { event_id: id, inserted: false, redactions: 0 });
    }
    let event_id = conn.last_insert_rowid();

    // Re-run redaction to record the audit rows (cheap; payload already in memory).
    if payload.len() <= MAX_PAYLOAD {
        for hit in redact::redact(payload).hits {
            conn.execute(
                "INSERT INTO redaction_audit (event_id, pattern_id, span) VALUES (?1, ?2, ?3)",
                rusqlite::params![event_id, hit.pattern_id, format!("{}-{}", hit.start, hit.end)],
            )?;
        }
    }
    Ok(Recorded { event_id, inserted: true, redactions })
}

/// Extract changed file paths from a payload — either a unified diff or a harness JSON
/// event (e.g. a Claude Code PostToolUse payload carrying `file_path`).
pub fn changed_paths(payload: &str) -> Vec<String> {
    if let Ok(v) = serde_json::from_str::<Value>(payload) {
        let mut out = Vec::new();
        collect_json_paths(&v, &mut out);
        if !out.is_empty() {
            return out;
        }
    }
    diff_paths(payload)
}

fn collect_json_paths(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if matches!(k.as_str(), "file_path" | "path" | "notebook_path") {
                    if let Some(s) = val.as_str() {
                        push_unique(out, s);
                    }
                }
                collect_json_paths(val, out);
            }
        }
        Value::Array(arr) => arr.iter().for_each(|x| collect_json_paths(x, out)),
        _ => {}
    }
}

/// Paths from `+++ b/<path>` / `diff --git a/x b/<path>` lines of a unified diff.
fn diff_paths(diff: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            push_unique(&mut out, rest.trim());
        } else if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(b) = rest.split(" b/").nth(1) {
                push_unique(&mut out, b.trim());
            }
        }
    }
    out
}

fn push_unique(out: &mut Vec<String>, s: &str) {
    let s = s.to_string();
    if !s.is_empty() && s != "/dev/null" && !out.contains(&s) {
        out.push(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_paths_from_diff() {
        let diff = "diff --git a/src/x.rs b/src/x.rs\n--- a/src/x.rs\n+++ b/src/x.rs\n@@\n+code\n";
        assert_eq!(changed_paths(diff), vec!["src/x.rs".to_string()]);
    }

    #[test]
    fn extracts_paths_from_harness_json() {
        let json = r#"{"tool_name":"Edit","tool_input":{"file_path":"/repo/a.py","old":"x"}}"#;
        assert_eq!(changed_paths(json), vec!["/repo/a.py".to_string()]);
    }
}
