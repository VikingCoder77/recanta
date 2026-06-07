//! OpenCode transcript parser. Storage (under `$OPENCODE_DATA_DIR` or
//! `~/.local/share/opencode`) splits a conversation across files:
//! `storage/session/<projectID>/<sessionID>.json` (metadata, incl. the working
//! `directory`), `storage/message/<sessionID>/*.json` (role), and
//! `storage/part/<messageID>/*.json` (text). We join them per session.
//!
//! Field names vary between versions, so this is best-effort and tolerant; `--from`
//! (pointing at the `storage` directory) overrides discovery.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{ParsedSession, RawTurn, Role};

pub(super) fn collect(home: &Path, root: &Path, from: Option<&Path>) -> (Vec<ParsedSession>, Vec<String>) {
    let storage = from.map(Path::to_path_buf).unwrap_or_else(|| data_dir(home).join("storage"));
    let mut notes = Vec::new();
    let session_root = storage.join("session");
    if !session_root.is_dir() {
        notes.push(format!("opencode: no sessions at {}", session_root.display()));
        return (Vec::new(), notes);
    }

    let want = root.to_string_lossy().to_string();
    let mut sessions = Vec::new();
    // Session files are nested one level under a project-id directory.
    for session_file in json_files_recursive(&session_root) {
        let Some(meta) = read_json(&session_file) else { continue };
        let dir = meta.get("directory").or_else(|| meta.get("cwd")).and_then(|d| d.as_str());
        // Auto mode: keep only sessions for this project's directory.
        if from.is_none() {
            if let Some(d) = dir {
                if d != want {
                    continue;
                }
            } else {
                continue; // can't associate without a directory
            }
        }
        let sid = meta
            .get("id")
            .and_then(|i| i.as_str())
            .map(str::to_string)
            .or_else(|| session_file.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();
        let turns = read_messages(&storage, &sid);
        if !turns.is_empty() {
            sessions.push(ParsedSession {
                session_uid: sid,
                source_path: session_file.to_string_lossy().to_string(),
                turns,
            });
        }
    }
    (sessions, notes)
}

fn data_dir(home: &Path) -> PathBuf {
    if let Some(v) = std::env::var_os("OPENCODE_DATA_DIR") {
        // May be a comma-separated list; take the first entry.
        let s = v.to_string_lossy().to_string();
        if let Some(first) = s.split(',').next() {
            return PathBuf::from(first);
        }
    }
    home.join(".local").join("share").join("opencode")
}

fn read_messages(storage: &Path, sid: &str) -> Vec<RawTurn> {
    let msg_dir = storage.join("message").join(sid);
    let mut turns = Vec::new();
    for msg_file in json_files(&msg_dir) {
        let Some(msg) = read_json(&msg_file) else { continue };
        let role = match msg.get("role").and_then(|r| r.as_str()) {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            _ => continue,
        };
        let ts = msg
            .get("time")
            .and_then(|t| t.get("created"))
            .or_else(|| msg.get("timestamp"))
            .and_then(|t| t.as_str())
            .map(str::to_string);
        let mid = msg
            .get("id")
            .and_then(|i| i.as_str())
            .map(str::to_string)
            .or_else(|| msg_file.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();

        let text = inline_text(&msg).unwrap_or_else(|| read_parts(storage, &mid));
        if !text.trim().is_empty() {
            turns.push(RawTurn { role, text, timestamp: ts, files: vec![] });
        }
    }
    turns
}

/// Some versions embed text directly on the message (string `content` or `parts`).
fn inline_text(msg: &Value) -> Option<String> {
    if let Some(s) = msg.get("content").and_then(|c| c.as_str()) {
        return Some(s.to_string());
    }
    let parts = msg.get("parts").and_then(|p| p.as_array())?;
    let text = collect_part_text(parts);
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Read `storage/part/<messageID>/*.json` text parts and join them.
fn read_parts(storage: &Path, mid: &str) -> String {
    let part_dir = storage.join("part").join(mid);
    let mut out = String::new();
    for part_file in json_files(&part_dir) {
        if let Some(p) = read_json(&part_file) {
            if p.get("type").and_then(|t| t.as_str()).unwrap_or("text") == "text" {
                if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
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

fn collect_part_text(parts: &[Value]) -> String {
    let mut out = String::new();
    for part in parts {
        if part.get("type").and_then(|t| t.as_str()).unwrap_or("text") == "text" {
            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(t);
            }
        }
    }
    out
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    out.sort();
    out
}

/// Like `json_files` but descends subdirectories (session files live under
/// `session/<projectID>/`).
fn json_files_recursive(dir: &Path) -> Vec<PathBuf> {
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
