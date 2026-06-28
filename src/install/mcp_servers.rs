//! Universal MCP-server registration (the "works with everyone" path). Registers the
//! `recanta mcp` bridge into whatever MCP-capable harnesses are present on this machine,
//! so any of them can use Recanta with no manual config.
//!
//! Reuses the non-destructive installer machinery: JSON configs get a single `recanta`
//! server key merged in (the user's other servers are preserved); Codex's TOML gets a
//! managed block. The server key name *is* the marker, so removal strips exactly our entry.
//!
//! All entries launch `recanta mcp` with no `--project`, so the harness's working directory
//! (the open workspace) selects the project — portable across repos and machines.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value};

use super::managed;
use super::{Action, Plan, Verb};

/// Config dialects across harnesses.
enum Fmt {
    /// `{ "mcpServers": { "recanta": { command, args } } }` — Claude Code, Cursor,
    /// Antigravity, Windsurf, and most others.
    McpServers,
    /// OpenCode's `{ "mcp": { "recanta": { type:"local", command:[…], enabled:true } } }`.
    OpenCode,
    /// Codex's `~/.codex/config.toml` `[mcp_servers.recanta]` (appended as a managed block).
    CodexToml,
}

struct Target {
    /// Recorded harness id (`mcp-*`) — also the uninstall key.
    harness: &'static str,
    label: &'static str,
    path: PathBuf,
    fmt: Fmt,
    /// Whether the harness appears installed (we don't create configs for absent tools,
    /// except the project-local Claude Code `.mcp.json`).
    detected: bool,
}

pub fn plan(root: &Path) -> Result<Plan> {
    let mut plan = Plan::default();
    let home = home_dir();
    let mut any_detected = false;

    for t in targets(root, home.as_deref()) {
        if !t.detected {
            plan.notes.push(format!("{}: not detected — `recanta install` it once present", t.label));
            continue;
        }
        any_detected = true;
        match merge(&t)? {
            None => plan.notes.push(format!("{}: recanta already registered", t.label)),
            Some(content) => {
                let verb = if t.path.exists() { Verb::Update } else { Verb::Create };
                plan.actions.push(Action {
                    harness: t.harness,
                    mechanism: "mcp".to_string(),
                    path: t.path,
                    verb,
                    new_content: content,
                    executable: false,
                    block_id: t.harness.to_string(),
                    preview: format!("{}: register the recanta MCP server", t.label),
                });
            }
        }
    }
    if !any_detected {
        plan.notes.push(
            "No MCP harness detected beyond Claude Code. Any MCP client can still use Recanta — \
             see the README 'works with any MCP harness' config."
                .into(),
        );
    }
    Ok(plan)
}

/// The candidate harness configs, with detection.
fn targets(root: &Path, home: Option<&Path>) -> Vec<Target> {
    let mut v = vec![
        // Project-local; Claude Code (and others) read `.mcp.json`. Always offered.
        Target {
            harness: "mcp-claude-code",
            label: "Claude Code (.mcp.json)",
            path: root.join(".mcp.json"),
            fmt: Fmt::McpServers,
            detected: true,
        },
        Target {
            harness: "mcp-cursor",
            label: "Cursor (.cursor/mcp.json)",
            path: root.join(".cursor").join("mcp.json"),
            fmt: Fmt::McpServers,
            detected: root.join(".cursor").exists(),
        },
        Target {
            harness: "mcp-opencode",
            label: "OpenCode (opencode.json)",
            path: root.join("opencode.json"),
            fmt: Fmt::OpenCode,
            detected: root.join("opencode.json").exists()
                || home.is_some_and(|h| h.join(".config").join("opencode").exists()),
        },
    ];
    if let Some(h) = home {
        v.push(Target {
            harness: "mcp-antigravity",
            label: "Antigravity (~/.gemini/config/mcp_config.json)",
            path: h.join(".gemini").join("config").join("mcp_config.json"),
            fmt: Fmt::McpServers,
            detected: h.join(".gemini").exists(),
        });
        v.push(Target {
            harness: "mcp-windsurf",
            label: "Windsurf (~/.codeium/windsurf/mcp_config.json)",
            path: h.join(".codeium").join("windsurf").join("mcp_config.json"),
            fmt: Fmt::McpServers,
            detected: h.join(".codeium").exists(),
        });
        v.push(Target {
            harness: "mcp-codex",
            label: "Codex (~/.codex/config.toml)",
            path: h.join(".codex").join("config.toml"),
            fmt: Fmt::CodexToml,
            detected: h.join(".codex").exists(),
        });
    }
    v
}

/// The merged new content for a target, or `None` if recanta is already registered.
fn merge(t: &Target) -> Result<Option<String>> {
    match t.fmt {
        Fmt::McpServers => merge_json(&t.path, "mcpServers", json!({"command":"recanta","args":["mcp"]})),
        Fmt::OpenCode => {
            merge_json(&t.path, "mcp", json!({"type":"local","command":["recanta","mcp"],"enabled":true}))
        }
        Fmt::CodexToml => merge_codex(&t.path),
    }
}

/// Merge `{ "<container>": { "recanta": <entry> } }` into a JSON config, preserving
/// everything else. `None` if `recanta` is already present (idempotent).
fn merge_json(path: &Path, container: &str, entry: Value) -> Result<Option<String>> {
    let existing = std::fs::read_to_string(path).ok();
    let mut root: Value = match &existing {
        Some(text) if !text.trim().is_empty() => serde_json::from_str(text)
            .with_context(|| format!("parsing {} (fix or move it aside)", path.display()))?,
        _ => json!({}),
    };
    if !root.is_object() {
        root = json!({});
    }
    let obj = root.as_object_mut().unwrap();
    let c = obj.entry(container).or_insert_with(|| json!({}));
    if !c.is_object() {
        *c = json!({});
    }
    let cmap = c.as_object_mut().unwrap();
    if cmap.contains_key("recanta") {
        return Ok(None);
    }
    cmap.insert("recanta".to_string(), entry);
    Ok(Some(serde_json::to_string_pretty(&root)? + "\n"))
}

fn merge_codex(path: &Path) -> Result<Option<String>> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if managed::has_block(&existing) && existing.contains("[mcp_servers.recanta]") {
        return Ok(None);
    }
    let body = "[mcp_servers.recanta]\ncommand = \"recanta\"\nargs = [\"mcp\"]";
    Ok(Some(managed::upsert_block(&existing, body)))
}

/// Remove the `recanta` entry from a JSON MCP config (any of the known container keys).
/// Returns the new content if anything changed. Used by uninstall.
pub fn strip_json(text: &str) -> Result<Option<String>> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let mut root: Value = serde_json::from_str(text).context("parsing MCP config JSON")?;
    let mut changed = false;
    if let Some(obj) = root.as_object_mut() {
        for container in ["mcpServers", "mcp", "servers"] {
            if let Some(c) = obj.get_mut(container).and_then(|v| v.as_object_mut()) {
                if c.remove("recanta").is_some() {
                    changed = true;
                }
            }
            if obj.get(container).is_some_and(|v| v.as_object().is_some_and(|m| m.is_empty())) {
                obj.remove(container);
                changed = true;
            }
        }
    }
    if changed {
        Ok(Some(serde_json::to_string_pretty(&root)? + "\n"))
    } else {
        Ok(None)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_json_is_idempotent_and_preserves_other_servers() {
        let dir = std::env::temp_dir().join(format!("recanta-mcp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mcp.json");
        std::fs::write(&path, r#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();

        let merged = merge_json(&path, "mcpServers", json!({"command":"recanta","args":["mcp"]}))
            .unwrap()
            .unwrap();
        std::fs::write(&path, &merged).unwrap();
        let v: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x", "user server preserved");
        assert_eq!(v["mcpServers"]["recanta"]["command"], "recanta");

        // Second merge is a no-op.
        assert!(merge_json(&path, "mcpServers", json!({"command":"recanta","args":["mcp"]}))
            .unwrap()
            .is_none());

        // Strip removes only our entry.
        let stripped = strip_json(&merged).unwrap().unwrap();
        let back: Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(back["mcpServers"]["other"]["command"], "x");
        assert!(back["mcpServers"].get("recanta").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn strip_drops_empty_container() {
        // If recanta was the only server, the container key is removed entirely.
        let text = r#"{"mcpServers":{"recanta":{"command":"recanta"}}}"#;
        let out = strip_json(text).unwrap().unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("mcpServers").is_none());
    }

    #[test]
    fn codex_block_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("recanta-codex-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "model = \"gpt\"\n").unwrap();
        let once = merge_codex(&path).unwrap().unwrap();
        assert!(once.contains("[mcp_servers.recanta]"));
        assert!(once.contains("model = \"gpt\""), "existing config preserved");
        std::fs::write(&path, &once).unwrap();
        assert!(merge_codex(&path).unwrap().is_none(), "second time is a no-op");
        std::fs::remove_dir_all(&dir).ok();
    }
}
