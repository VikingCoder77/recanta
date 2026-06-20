//! `recanta mcp` — a thin Model Context Protocol bridge (PRD §9.2: optional MCP, capped
//! at ≤5 tools). stdio transport, JSON-RPC 2.0, newline-delimited messages.
//!
//! The bridge deliberately stays tiny: it re-enters the *same* in-process command logic
//! the CLI uses (`brief`/`search`/`remember`/`inspect` via their `render` functions), so
//! redaction, token-budgeting, and ranking are byte-for-byte identical to the CLI. There
//! is no network access and no telemetry — the whole point is to give MCP-native agents a
//! read/write path into the local store without the tool-surface bloat the PRD warns
//! against (we expose four named tools, well under the ≤5 cap).
//!
//! stdout carries *only* protocol frames; everything diagnostic goes to stderr, so a
//! command's `render` output can never corrupt the JSON-RPC stream.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use clap::{Args, ValueEnum};
use serde_json::{json, Value};

use crate::commands::{brief, inspect, remember, search};
use crate::memory::{Importance, MemType, Scope};
use crate::output::{self, Format};

/// Protocol revision we speak. We echo back the client's version when we recognize it,
/// otherwise we offer this one (per the MCP version-negotiation rule).
const SUPPORTED_PROTOCOL: &str = "2024-11-05";

#[derive(Debug, Args)]
pub struct McpArgs {}

pub fn run(_args: McpArgs, project_override: Option<&Path>) -> Result<()> {
    serve(
        project_override.map(Path::to_path_buf),
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )
}

/// The JSON-RPC read/dispatch/write loop. Generic over reader/writer so tests can drive
/// it with in-memory buffers.
pub fn serve(project: Option<PathBuf>, input: impl BufRead, mut out: impl Write) -> Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            // Parse errors get a spec error frame with a null id (we have no id to echo).
            Err(e) => {
                write_frame(&mut out, rpc_error(Value::Null, -32700, &e.to_string()))?;
                continue;
            }
        };
        // JSON-RPC batches arrive as arrays; respond with an array of the non-empty replies.
        if let Some(arr) = msg.as_array() {
            let replies: Vec<Value> =
                arr.iter().filter_map(|m| handle(m, project.as_deref())).collect();
            if !replies.is_empty() {
                write_frame(&mut out, Value::Array(replies))?;
            }
        } else if let Some(reply) = handle(&msg, project.as_deref()) {
            write_frame(&mut out, reply)?;
        }
    }
    Ok(())
}

/// Dispatch one request. Returns `None` for notifications (which take no response).
fn handle(req: &Value, project: Option<&Path>) -> Option<Value> {
    let method = req.get("method").and_then(Value::as_str)?;
    let id = req.get("id").cloned();
    let is_notification = id.is_none();
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => Some(rpc_ok(id, initialize_result(&params))),
        "ping" => Some(rpc_ok(id, json!({}))),
        "tools/list" => Some(rpc_ok(id, json!({ "tools": tool_defs() }))),
        "tools/call" => Some(tools_call(id, &params, project)),
        // Notifications (initialized, cancelled, …) and unknown notifications: stay silent.
        _ if is_notification => None,
        other => Some(rpc_error(
            id.unwrap_or(Value::Null),
            -32601,
            &format!("method not found: {other}"),
        )),
    }
}

fn initialize_result(params: &Value) -> Value {
    // Honor the client's requested protocol version when we recognize it; else offer ours.
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = match requested {
        Some(v) if v == SUPPORTED_PROTOCOL => v,
        _ => SUPPORTED_PROTOCOL,
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "recanta", "version": env!("CARGO_PKG_VERSION") },
        "instructions": "Recanta is a local, Git-aware memory substrate. Call recanta_brief \
            at the start of a session to load current work, decisions, and risks; recanta_search \
            to recall specifics from memory, documents, and past chats; recanta_remember to \
            persist a durable note/decision/task; recanta_inspect to look up a code symbol or file."
    })
}

/// The four exposed tools. Names are `recanta_*` per the naming convention; schemas mirror
/// the CLI args so an agent gets the same knobs without learning the CLI.
fn tool_defs() -> Value {
    json!([
        {
            "name": "recanta_brief",
            "description": "Compact, task-aware briefing for a fresh session: project + branch \
                state, active task, known risks, active decisions, durable user preferences, and \
                recent changes. Token-budgeted. Call this first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "Optional focus; surfaces memories relevant to it." },
                    "budget": { "type": "integer", "description": "Max output characters.", "default": output::BRIEF_DEFAULT }
                }
            }
        },
        {
            "name": "recanta_search",
            "description": "Hybrid search over memory, ingested documents, and past chat \
                transcripts. Recalls specifics from earlier sessions. Ranked deterministically.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Free-text search query." },
                    "scope": { "type": "string", "enum": ["user", "project", "repo", "branch", "symbol"],
                        "description": "Restrict to one scope. Omit to search project + user memory (and documents/chats)." },
                    "budget": { "type": "integer", "description": "Max output characters.", "default": output::SEARCH_DEFAULT },
                    "with_evidence": { "type": "boolean", "description": "Return full memory content instead of a snippet." }
                },
                "required": ["query"]
            }
        },
        {
            "name": "recanta_remember",
            "description": "Persist a durable memory: a fact, decision, task, warning, or \
                preference. Redaction runs before storage.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "The memory content." },
                    "scope": { "type": "string", "enum": ["user", "project", "repo", "branch", "symbol"],
                        "description": "Default project. `user` is stored globally and reused across projects." },
                    "type": { "type": "string",
                        "enum": ["semantic", "procedural", "episodic", "decision", "task", "warning", "bug", "code-summary"],
                        "description": "Memory type (default: semantic)." },
                    "importance": { "type": "string", "enum": ["low", "normal", "high"],
                        "description": "Importance (default: normal)." },
                    "title": { "type": "string", "description": "Optional explicit title (otherwise derived from content)." }
                },
                "required": ["content"]
            }
        },
        {
            "name": "recanta_inspect",
            "description": "Look up a code symbol or file from the code graph: location, \
                signature, recent changes, and index freshness — without reading the whole file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["function", "class", "file"],
                        "description": "What to inspect." },
                    "target": { "type": "string", "description": "Symbol name / qualified name, or file path." },
                    "budget": { "type": "integer", "description": "Max output characters.", "default": 1000 }
                },
                "required": ["kind", "target"]
            }
        }
    ])
}

fn tools_call(id: Option<Value>, params: &Value, project: Option<&Path>) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    // Tool-execution failures are reported as a *successful* RPC result with isError=true
    // (per the MCP spec), so the model sees the error text and can recover.
    match dispatch_tool(name, &args, project) {
        Ok(text) => rpc_ok(id, tool_content(&text, false)),
        Err(e) => rpc_ok(id, tool_content(&format!("error: {e:#}"), true)),
    }
}

fn tool_content(text: &str, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.trim_end_matches('\n') }],
        "isError": is_error
    })
}

/// Map a tool name + arguments to the matching `render` call. This is the only place that
/// knows the tool↔command mapping; everything below it is shared CLI logic.
fn dispatch_tool(name: &str, args: &Value, project: Option<&Path>) -> Result<String> {
    match name {
        "recanta_brief" => brief::render(
            brief::BriefArgs {
                budget: arg_usize(args, "budget", output::BRIEF_DEFAULT),
                task: arg_string(args, "task"),
            },
            project,
        ),
        "recanta_search" => search::render(
            search::SearchArgs {
                query: required_string(args, "query")?,
                scope: arg_enum::<Scope>(args, "scope")?,
                budget: arg_usize(args, "budget", output::SEARCH_DEFAULT),
                with_evidence: arg_bool(args, "with_evidence"),
                // The bridge always returns text; JSON framing is the MCP layer's job.
                format: Format::Compact,
            },
            project,
        ),
        "recanta_remember" => remember::render(
            remember::RememberArgs {
                content: required_string(args, "content")?,
                scope: arg_enum::<Scope>(args, "scope")?.unwrap_or(Scope::Project),
                mem_type: arg_enum::<MemType>(args, "type")?.unwrap_or(MemType::Semantic),
                importance: arg_enum::<Importance>(args, "importance")?.unwrap_or(Importance::Normal),
                title: arg_string(args, "title"),
            },
            project,
        ),
        "recanta_inspect" => inspect::render(
            inspect::InspectArgs {
                kind: required_string(args, "kind")?,
                target: required_string(args, "target")?,
                budget: arg_usize(args, "budget", 1000),
            },
            project,
        ),
        other => bail!("unknown tool: {other}"),
    }
}

// --- argument coercion helpers ------------------------------------------------

fn required_string(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("missing required string argument `{key}`"))
}

fn arg_string(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn arg_usize(args: &Value, key: &str, default: usize) -> usize {
    args.get(key).and_then(Value::as_u64).map(|n| n as usize).unwrap_or(default)
}

/// Parse a clap `ValueEnum` from a string argument (case-insensitive). `None` if absent.
fn arg_enum<T: ValueEnum>(args: &Value, key: &str) -> Result<Option<T>> {
    match args.get(key).and_then(Value::as_str) {
        None => Ok(None),
        Some(s) => T::from_str(s, true)
            .map(Some)
            .map_err(|_| anyhow::anyhow!("invalid value for `{key}`: {s:?}")),
    }
}

// --- JSON-RPC framing ---------------------------------------------------------

fn rpc_ok(id: Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": result })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn write_frame(out: &mut impl Write, v: Value) -> Result<()> {
    let s = serde_json::to_string(&v)?;
    out.write_all(s.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Drive `serve` with a canned set of newline-delimited requests; return parsed replies.
    fn exchange(lines: &[&str]) -> Vec<Value> {
        exchange_in(None, lines)
    }

    fn exchange_in(project: Option<PathBuf>, lines: &[&str]) -> Vec<Value> {
        let input = Cursor::new(lines.join("\n").into_bytes());
        let mut out: Vec<u8> = Vec::new();
        serve(project, input, &mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// A temp directory with no `.recanta/` at or above it, so project discovery fails.
    fn dir_without_project() -> PathBuf {
        let p = std::env::temp_dir().join(format!("recanta-mcp-noproj-{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn initialize_handshake_reports_server_and_tools() {
        let replies = exchange(&[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ]);
        // initialize + tools/list reply; the notification produces nothing.
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["result"]["serverInfo"]["name"], "recanta");
        assert_eq!(replies[0]["result"]["protocolVersion"], "2024-11-05");

        let tools = replies[1]["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4, "≤5-tool cap; four named tools");
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"recanta_brief"));
        assert!(names.contains(&"recanta_search"));
        assert!(names.contains(&"recanta_remember"));
        assert!(names.contains(&"recanta_inspect"));
        assert!(names.iter().all(|n| n.starts_with("recanta_")));
    }

    #[test]
    fn ping_replies_empty_result() {
        let replies = exchange(&[r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#]);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["id"], 7);
        assert!(replies[0]["result"].is_object());
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let replies = exchange(&[r#"{"jsonrpc":"2.0","id":9,"method":"does/not/exist"}"#]);
        assert_eq!(replies[0]["error"]["code"], -32601);
    }

    #[test]
    fn unknown_notification_is_silent() {
        // No id ⇒ notification ⇒ no reply, even for an unknown method.
        let replies = exchange(&[r#"{"jsonrpc":"2.0","method":"notifications/whatever"}"#]);
        assert!(replies.is_empty());
    }

    #[test]
    fn malformed_json_yields_parse_error() {
        let replies = exchange(&["{ not json"]);
        assert_eq!(replies[0]["error"]["code"], -32700);
        assert!(replies[0]["id"].is_null());
    }

    #[test]
    fn tool_call_outside_a_project_reports_iserror_not_rpc_error() {
        // No `.recanta/` at/above the project dir, so the command fails — but the MCP layer
        // must surface it as a tool result with isError=true (a normal RPC result), never a
        // transport error.
        let replies = exchange_in(
            Some(dir_without_project()),
            &[r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"recanta_brief","arguments":{}}}"#],
        );
        assert!(replies[0].get("error").is_none(), "must not be an RPC error");
        assert_eq!(replies[0]["result"]["isError"], true);
        let text = replies[0]["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("error:"));
    }

    #[test]
    fn unknown_tool_name_is_reported() {
        let replies = exchange(&[
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"recanta_bogus","arguments":{}}}"#,
        ]);
        assert_eq!(replies[0]["result"]["isError"], true);
        let text = replies[0]["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("unknown tool"));
    }

    #[test]
    fn batch_request_returns_array_of_replies() {
        let batch = r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","id":2,"method":"ping"}]"#;
        let replies = exchange(&[batch]);
        // The whole batch comes back as a single array frame.
        assert_eq!(replies.len(), 1);
        let arr = replies[0].as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }
}
