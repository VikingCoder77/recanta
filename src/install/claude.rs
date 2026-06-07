//! Claude Code adapter (PRD §8.6, §10.3). Merges a `SessionStart` hook into
//! `.claude/settings.json` that injects `recanta brief` as additional context on each
//! fresh session. Settings is JSON, so instead of a text managed-block we tag our hook
//! command with a `# recanta:<id>` marker comment (inert in shell) for exact removal.
//!
//! v0.1 installs only the `SessionStart → brief` hook because its command exists today.
//! The `PostToolUse → record-edit` and `Stop → record-event` hooks arrive with v0.2,
//! when those commands land — wiring them now would only emit errors.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use super::{Action, Plan, Verb};

/// Marker that identifies Recanta-owned hook commands for clean uninstall.
/// Marker comment (inert in shell) tagging every Recanta-owned hook command, for exact
/// removal on uninstall.
pub const MARKER: &str = "# recanta:";

/// The hooks Recanta installs: (event, optional matcher, command).
const HOOKS: &[(&str, Option<&str>, &str)] = &[
    ("SessionStart", None, "recanta brief --budget 1200 # recanta:session-start"),
    (
        "PostToolUse",
        Some("Edit|Write|MultiEdit|NotebookEdit"),
        "recanta record-edit --stdin --source claude-code # recanta:post-edit",
    ),
    ("Stop", None, "recanta record-event --type harness.stop --stdin # recanta:stop"),
];

/// Plan the Claude Code settings merge for `root/.claude/settings.json`.
pub fn plan(root: &Path) -> Result<Plan> {
    let mut plan = Plan::default();
    let path = root.join(".claude").join("settings.json");

    let existing = std::fs::read_to_string(&path).ok();
    let mut settings: Value = match &existing {
        Some(text) if !text.trim().is_empty() => serde_json::from_str(text)
            .with_context(|| format!("parsing {} (fix or move it aside)", path.display()))?,
        _ => json!({}),
    };

    if already_installed(&settings) {
        plan.notes.push("Claude Code hooks already installed".into());
        return Ok(plan);
    }

    inject_hooks(&mut settings);
    let new_content = serde_json::to_string_pretty(&settings)? + "\n";
    let verb = if path.exists() { Verb::Update } else { Verb::Create };

    plan.actions.push(Action {
        harness: "claude-code",
        mechanism: "settings".to_string(),
        path,
        verb,
        new_content,
        executable: false,
        block_id: "claude-hooks".to_string(),
        preview: "SessionStart→brief, PostToolUse→record-edit, Stop→record-event".to_string(),
    });
    plan.notes.push(
        "Claude Code shows a folder-trust prompt; approve this project or the hooks \
         won't run."
            .into(),
    );
    Ok(plan)
}

/// Remove Recanta-owned hook entries from a settings document. Returns the new pretty
/// JSON if anything changed.
pub fn strip(text: &str) -> Result<Option<String>> {
    let mut settings: Value = if text.trim().is_empty() {
        return Ok(None);
    } else {
        serde_json::from_str(text).context("parsing settings.json")?
    };
    let mut changed = false;
    if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for (_event, groups) in hooks.iter_mut() {
            if let Some(arr) = groups.as_array_mut() {
                let before = arr.len();
                arr.retain(|group| !group_is_ours(group));
                changed |= arr.len() != before;
            }
        }
        hooks.retain(|_, groups| !groups.as_array().is_some_and(|a| a.is_empty()));
    }
    if let Some(obj) = settings.as_object_mut() {
        if obj.get("hooks").is_some_and(|h| h.as_object().is_some_and(|m| m.is_empty())) {
            obj.remove("hooks");
            changed = true;
        }
    }
    if changed {
        Ok(Some(serde_json::to_string_pretty(&settings)? + "\n"))
    } else {
        Ok(None)
    }
}

fn already_installed(settings: &Value) -> bool {
    let Some(hooks) = settings.get("hooks").and_then(|h| h.as_object()) else {
        return false;
    };
    hooks
        .values()
        .filter_map(|g| g.as_array())
        .any(|groups| groups.iter().any(group_is_ours))
}

/// A hook group is Recanta's if any of its commands carries our marker.
fn group_is_ours(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .is_some_and(|cmds| {
            cmds.iter().any(|c| {
                c.get("command")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.contains(MARKER))
            })
        })
}

fn inject_hooks(settings: &mut Value) {
    if !settings.is_object() {
        *settings = json!({});
    }
    let obj = settings.as_object_mut().unwrap();
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let hooks = hooks.as_object_mut().unwrap();

    for (event, matcher, command) in HOOKS {
        let group = match matcher {
            Some(m) => json!({ "matcher": m, "hooks": [ { "type": "command", "command": command } ] }),
            None => json!({ "hooks": [ { "type": "command", "command": command } ] }),
        };
        let arr = hooks.entry((*event).to_string()).or_insert_with(|| json!([]));
        if !arr.is_array() {
            *arr = json!([]);
        }
        arr.as_array_mut().unwrap().push(group);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_then_strip_roundtrips() {
        let mut s = json!({ "model": "opus", "hooks": { "Stop": [{"hooks":[{"type":"command","command":"echo mine"}]}] } });
        assert!(!already_installed(&s));
        inject_hooks(&mut s);
        assert!(already_installed(&s));
        // All three events are installed.
        assert!(s["hooks"]["SessionStart"].is_array());
        assert!(s["hooks"]["PostToolUse"].is_array());
        // Existing keys and the user's own Stop hook are preserved.
        assert_eq!(s["model"], "opus");

        let text = serde_json::to_string_pretty(&s).unwrap();
        let stripped = strip(&text).unwrap().unwrap();
        let back: Value = serde_json::from_str(&stripped).unwrap();
        assert!(!already_installed(&back));
        assert_eq!(back["model"], "opus");
        // The user's pre-existing Stop hook survives removal of Recanta's.
        assert_eq!(back["hooks"]["Stop"][0]["hooks"][0]["command"], "echo mine");
    }

    #[test]
    fn strip_is_none_when_nothing_ours() {
        let text = serde_json::to_string(&json!({ "model": "x" })).unwrap();
        assert!(strip(&text).unwrap().is_none());
    }
}
