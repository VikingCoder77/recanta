//! Google Antigravity transcript parser. Antigravity is Gemini-based and stores each
//! conversation as JSON Lines under
//! `~/.gemini/{antigravity-ide,antigravity,antigravity-cli}/brain/<conversation-id>/
//! .system_generated/logs/transcript.jsonl` — one object per line with `source`
//! (`USER_EXPLICIT` / `MODEL` / `SYSTEM`), `type`, `content`, `thinking`, `tool_calls`
//! (`[{name,args}]`), and `created_at`.
//!
//! A conversation isn't keyed by project on disk, so — like the Codex parser keys on cwd —
//! we associate it with `root` when the conversation references the project's root path
//! (Antigravity's file actions carry absolute workspace paths throughout). `--from`
//! overrides the brain directory when the layout drifts.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{ParsedSession, RawTurn, Role};

pub(super) fn collect(
    home: &Path,
    root: &Path,
    from: Option<&Path>,
) -> (Vec<ParsedSession>, Vec<String>) {
    let mut notes = Vec::new();
    let brains: Vec<PathBuf> = match from {
        Some(p) => vec![p.to_path_buf()],
        None => ["antigravity-ide", "antigravity", "antigravity-cli"]
            .iter()
            .map(|v| home.join(".gemini").join(v).join("brain"))
            .filter(|p| p.is_dir())
            .collect(),
    };
    if brains.is_empty() {
        notes.push("antigravity: no brain directory under ~/.gemini (try --from)".into());
        return (Vec::new(), notes);
    }

    let root_str = root.to_string_lossy().to_string();
    let mut sessions = Vec::new();
    let mut seen_any = false;
    for brain in &brains {
        for transcript in transcripts(brain) {
            seen_any = true;
            if let Some(s) = parse(&transcript, &root_str) {
                if !s.turns.is_empty() {
                    sessions.push(s);
                }
            }
        }
    }
    if seen_any && sessions.is_empty() {
        notes.push("antigravity: conversations found, but none reference this project".into());
    }
    (sessions, notes)
}

/// Every `…/<conversation-id>/.system_generated/logs/transcript.jsonl` under a brain dir.
fn transcripts(brain: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(brain) else { return out };
    for entry in rd.flatten() {
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            let t = entry
                .path()
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            if t.is_file() {
                out.push(t);
            }
        }
    }
    out.sort();
    out
}

/// Parse one transcript, returning a session only if it belongs to `root_str`.
fn parse(file: &Path, root_str: &str) -> Option<ParsedSession> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut turns = Vec::new();
    let mut belongs = false;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        // Only user/model turns become conversation; SYSTEM/ephemeral steps are skipped.
        let role = match v.get("source").and_then(|s| s.as_str()) {
            Some("USER_EXPLICIT") => Role::User,
            Some("MODEL") => Role::Assistant,
            _ => continue,
        };
        let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("");
        let ts = v.get("created_at").and_then(|t| t.as_str()).map(str::to_string);

        let mut files = Vec::new();
        if let Some(calls) = v.get("tool_calls").and_then(|t| t.as_array()) {
            for call in calls {
                if let Some(args) = call.get("args") {
                    collect_paths(args, &mut files);
                }
            }
        }

        if content.contains(root_str) || files.iter().any(|f| f.contains(root_str)) {
            belongs = true;
        }

        let text = content.trim();
        if text.is_empty() && files.is_empty() {
            continue;
        }
        turns.push(RawTurn { role, text: text.to_string(), timestamp: ts, files });
    }

    if !belongs {
        return None;
    }
    Some(ParsedSession {
        session_uid: conversation_id(file),
        source_path: file.to_string_lossy().to_string(),
        turns,
    })
}

/// `…/brain/<id>/.system_generated/logs/transcript.jsonl` → `<id>`.
fn conversation_id(file: &Path) -> String {
    file.ancestors()
        .nth(3)
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file.to_string_lossy().to_string())
}

/// Recursively gather path-like strings from a tool-call `args` value.
fn collect_paths(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if s.contains('/') && s.len() > 1 => {
            if !out.contains(s) {
                out.push(s.clone());
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_paths(x, out)),
        Value::Object(o) => o.values().for_each(|x| collect_paths(x, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_roles_and_associates_by_root() {
        let dir = std::env::temp_dir()
            .join(format!("recanta-ag-{}", std::process::id()))
            .join("brain")
            .join("conv-1")
            .join(".system_generated")
            .join("logs");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("transcript.jsonl");
        // Mirrors the real schema: USER_EXPLICIT input, MODEL response with a tool call
        // referencing the project root, and a SYSTEM step that must be ignored.
        let jsonl = [
            r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","created_at":"2026-06-27T10:00:00Z","content":"add a feature"}"#,
            r#"{"step_index":1,"source":"SYSTEM","type":"EPHEMERAL_MESSAGE","created_at":"2026-06-27T10:00:01Z","content":"thinking..."}"#,
            r#"{"step_index":2,"source":"MODEL","type":"CODE_ACTION","created_at":"2026-06-27T10:00:02Z","content":"editing the file","tool_calls":[{"name":"edit","args":{"path":"/work/proj/src/main.rs"}}]}"#,
        ]
        .join("\n");
        std::fs::write(&file, jsonl).unwrap();

        // Belongs to /work/proj (referenced in the tool call).
        let s = parse(&file, "/work/proj").expect("should belong to root");
        assert_eq!(s.session_uid, "conv-1");
        assert_eq!(s.turns.len(), 2, "user + model, system skipped");
        assert_eq!(s.turns[0].role, Role::User);
        assert_eq!(s.turns[1].role, Role::Assistant);
        assert!(s.turns[1].files.iter().any(|f| f == "/work/proj/src/main.rs"));

        // A different project root => not this conversation.
        assert!(parse(&file, "/some/other/project").is_none());

        std::fs::remove_dir_all(std::env::temp_dir().join(format!("recanta-ag-{}", std::process::id()))).ok();
    }
}
