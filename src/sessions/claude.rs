//! Claude Code transcript parser. Per-project transcripts live at
//! `~/.claude/projects/<encoded-root>/<session>.jsonl`, one JSON event per line.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{ParsedSession, RawTurn, Role};

/// Claude Code encodes a project's absolute path by replacing every non-alphanumeric
/// character with `-` (e.g. `/Users/tp/AI Projects/recanta` →
/// `-Users-tp-AI-Projects-recanta`).
pub(crate) fn project_dir(home: &Path, root: &Path) -> PathBuf {
    let encoded: String = root
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    home.join(".claude").join("projects").join(encoded)
}

pub(super) fn collect(home: &Path, root: &Path, from: Option<&Path>) -> (Vec<ParsedSession>, Vec<String>) {
    let dir = from.map(Path::to_path_buf).unwrap_or_else(|| project_dir(home, root));
    let mut notes = Vec::new();
    if !dir.is_dir() {
        notes.push(format!("claude-code: no sessions at {}", dir.display()));
        return (Vec::new(), notes);
    }
    let mut files: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl")).collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    let sessions = files.iter().filter_map(|f| parse(f)).collect();
    (sessions, notes)
}

fn parse(file: &Path) -> Option<ParsedSession> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut uid = file.file_stem()?.to_string_lossy().to_string();
    let mut turns = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(s) = v.get("sessionId").and_then(|x| x.as_str()) {
            uid = s.to_string();
        }
        let ts = v.get("timestamp").and_then(|x| x.as_str()).map(str::to_string);
        match v.get("type").and_then(|x| x.as_str()) {
            Some("user") => {
                let text = message_text(&v);
                if !text.trim().is_empty() {
                    turns.push(RawTurn { role: Role::User, text, timestamp: ts, files: vec![] });
                }
            }
            Some("assistant") => {
                turns.push(RawTurn {
                    role: Role::Assistant,
                    text: message_text(&v),
                    timestamp: ts,
                    files: tool_files(&v),
                });
            }
            _ => {}
        }
    }
    Some(ParsedSession { session_uid: uid, source_path: file.to_string_lossy().to_string(), turns })
}

/// Text from a message's `content` (string, or array of `{type:"text",text}` blocks).
fn message_text(v: &Value) -> String {
    let Some(content) = v.get("message").and_then(|m| m.get("content")) else {
        return String::new();
    };
    if let Some(t) = content.as_str() {
        return t.to_string();
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

fn tool_files(v: &Value) -> Vec<String> {
    let mut files = Vec::new();
    if let Some(arr) = v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) {
        for block in arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                if let Some(p) = block.get("input").and_then(|i| i.get("file_path").or_else(|| i.get("path"))).and_then(|p| p.as_str()) {
                    if !files.contains(&p.to_string()) {
                        files.push(p.to_string());
                    }
                }
            }
        }
    }
    files
}
