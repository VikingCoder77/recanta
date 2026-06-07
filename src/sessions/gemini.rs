//! Gemini CLI transcript parser. Conversation history lives under
//! `~/.gemini/tmp/<projectHash>/` as JSON: checkpoint/chat files hold an array of
//! `{role: "user"|"model", parts: [{text}]}`, and `logs.json` holds user prompts.
//!
//! The project-hash scheme varies by version, so auto mode tries `sha256(root)`; point
//! at the directory with `--from` if that misses.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{ParsedSession, RawTurn, Role};
use crate::hash;

pub(super) fn collect(home: &Path, root: &Path, from: Option<&Path>) -> (Vec<ParsedSession>, Vec<String>) {
    let dir = from.map(Path::to_path_buf).unwrap_or_else(|| {
        home.join(".gemini").join("tmp").join(hash::sha256_hex(&root.to_string_lossy()))
    });
    let mut notes = Vec::new();
    if !dir.is_dir() {
        notes.push(format!(
            "gemini: no sessions at {} (Gemini's project-hash scheme varies; try --from)",
            dir.display()
        ));
        return (Vec::new(), notes);
    }

    let mut sessions = Vec::new();
    for file in json_files(&dir) {
        if let Some(s) = parse(&file) {
            if !s.turns.is_empty() {
                sessions.push(s);
            }
        }
    }
    (sessions, notes)
}

fn parse(file: &Path) -> Option<ParsedSession> {
    let text = std::fs::read_to_string(file).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    // The conversation array may be the top level or under a `messages`/`history` key.
    let arr = v
        .as_array()
        .or_else(|| v.get("messages").and_then(|m| m.as_array()))
        .or_else(|| v.get("history").and_then(|m| m.as_array()))?;

    let mut turns = Vec::new();
    for msg in arr {
        let ts = msg.get("timestamp").and_then(|t| t.as_str()).map(str::to_string);
        // `logs.json` shape: {type:"user", message:"…"}.
        if let Some(m) = msg.get("message").and_then(|m| m.as_str()) {
            if !m.trim().is_empty() {
                turns.push(RawTurn { role: Role::User, text: m.to_string(), timestamp: ts, files: vec![] });
            }
            continue;
        }
        // Checkpoint/chat shape: {role:"user"|"model", parts:[{text}]}.
        let role = match msg.get("role").and_then(|r| r.as_str()) {
            Some("user") => Role::User,
            Some("model") | Some("assistant") => Role::Assistant,
            _ => continue,
        };
        let text = parts_text(msg);
        if !text.trim().is_empty() {
            turns.push(RawTurn { role, text, timestamp: ts, files: vec![] });
        }
    }
    Some(ParsedSession {
        session_uid: file.file_stem()?.to_string_lossy().to_string(),
        source_path: file.to_string_lossy().to_string(),
        turns,
    })
}

fn parts_text(msg: &Value) -> String {
    let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) else {
        return String::new();
    };
    let mut out = String::new();
    for part in parts {
        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(t);
        }
    }
    out
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(path),
                Ok(ft) if ft.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") => {
                    out.push(path)
                }
                _ => {}
            }
        }
    }
    out.sort();
    out
}
