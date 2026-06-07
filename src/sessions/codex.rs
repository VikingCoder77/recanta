//! Codex CLI transcript parser. Sessions live at
//! `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. Each line is `{type, payload, …}`;
//! `session_meta` carries the working directory (`cwd`), and `response_item` lines with
//! `payload.type == "message"` carry user/assistant turns. Sessions are global (not
//! per-project), so in auto mode we keep only those whose `cwd` matches this project.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{ParsedSession, RawTurn, Role};

pub(super) fn collect(home: &Path, root: &Path, from: Option<&Path>) -> (Vec<ParsedSession>, Vec<String>) {
    let dir = from.map(Path::to_path_buf).unwrap_or_else(|| home.join(".codex").join("sessions"));
    let mut notes = Vec::new();
    if !dir.is_dir() {
        notes.push(format!("codex: no sessions at {}", dir.display()));
        return (Vec::new(), notes);
    }
    let want_cwd = root.to_string_lossy().to_string();
    let mut sessions = Vec::new();
    for file in jsonl_files(&dir) {
        if let Some((parsed, cwd)) = parse(&file) {
            // In auto mode, only keep sessions whose recorded cwd is this project.
            // With --from, the user pointed us here, so keep everything readable.
            let keep = from.is_some() || cwd.as_deref() == Some(want_cwd.as_str());
            if keep && !parsed.turns.is_empty() {
                sessions.push(parsed);
            }
        }
    }
    if sessions.is_empty() && from.is_none() {
        notes.push("codex: no sessions matched this project's directory".into());
    }
    (sessions, notes)
}

fn parse(file: &Path) -> Option<(ParsedSession, Option<String>)> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut uid = file.file_stem()?.to_string_lossy().to_string();
    let mut cwd = None;
    let mut turns = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        // The meaningful record is either the line itself or its `payload`.
        let item = v.get("payload").unwrap_or(&v);
        let ts = v.get("timestamp").and_then(|x| x.as_str()).map(str::to_string);

        // Session metadata: working directory + a stable id.
        if cwd.is_none() {
            if let Some(c) = item.get("cwd").or_else(|| v.get("cwd")).and_then(|x| x.as_str()) {
                cwd = Some(c.to_string());
            }
        }
        if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
            uid = id.to_string();
        }

        if item.get("type").and_then(|t| t.as_str()) != Some("message") {
            continue;
        }
        let role = match item.get("role").and_then(|r| r.as_str()) {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            _ => continue, // skip developer/system/tool
        };
        let text = content_text(item);
        if !text.trim().is_empty() {
            turns.push(RawTurn { role, text, timestamp: ts, files: vec![] });
        }
    }
    Some((ParsedSession { session_uid: uid, source_path: file.to_string_lossy().to_string(), turns }, cwd))
}

/// Concatenate text from a message's `content` array (`input_text`/`output_text`/`text`).
fn content_text(item: &Value) -> String {
    let Some(arr) = item.get("content").and_then(|c| c.as_array()) else {
        return item.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
    };
    let mut out = String::new();
    for block in arr {
        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_payload_messages_and_cwd() {
        let dir = std::env::temp_dir().join(format!("recanta-codex-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("rollout-x.jsonl");
        let lines = [
            r#"{"type":"session_meta","payload":{"id":"x9","cwd":"/work/proj"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"add caching"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell"}}"#,
        ];
        std::fs::write(&f, lines.join("\n")).unwrap();

        let (parsed, cwd) = parse(&f).unwrap();
        assert_eq!(cwd.as_deref(), Some("/work/proj"));
        assert_eq!(parsed.session_uid, "x9");
        assert_eq!(parsed.turns.len(), 2, "only the two messages, not the function_call");
        assert_eq!(parsed.turns[0].role, Role::User);
        assert_eq!(parsed.turns[0].text, "add caching");
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// Recursively collect `*.jsonl` files under a directory.
fn jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(path),
                Ok(ft) if ft.is_file() && path.extension().and_then(|e| e.to_str()) == Some("jsonl") => {
                    out.push(path)
                }
                _ => {}
            }
        }
    }
    out.sort();
    out
}
