//! Shared JSON command-hook merging for harnesses that converge on the same model
//! (Claude Code, Codex, Gemini — PRD §8.6, §10.3). Each Recanta-owned hook command
//! carries a `# recanta:` marker comment (inert in shell) so uninstall can remove exactly
//! our entries from a settings/hooks JSON document without disturbing the user's.

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// Marker substring tagging Recanta-owned hook commands.
pub const MARKER: &str = "# recanta:";

/// One hook to install: (event, optional matcher, command — which must contain MARKER).
pub type Hook = (&'static str, Option<&'static str>, &'static str);

/// Merge `hooks` into a settings document under `settings.hooks.<event>`, preserving any
/// existing entries.
pub fn inject(settings: &mut Value, hooks: &[Hook]) {
    if !settings.is_object() {
        *settings = json!({});
    }
    let obj = settings.as_object_mut().unwrap();
    let root = obj.entry("hooks").or_insert_with(|| json!({}));
    if !root.is_object() {
        *root = json!({});
    }
    let root = root.as_object_mut().unwrap();
    for (event, matcher, command) in hooks {
        let group = match matcher {
            Some(m) => json!({ "matcher": m, "hooks": [ { "type": "command", "command": command } ] }),
            None => json!({ "hooks": [ { "type": "command", "command": command } ] }),
        };
        let arr = root.entry((*event).to_string()).or_insert_with(|| json!([]));
        if !arr.is_array() {
            *arr = json!([]);
        }
        arr.as_array_mut().unwrap().push(group);
    }
}

/// True if any Recanta-owned hook is already present.
pub fn already_installed(settings: &Value) -> bool {
    let Some(hooks) = settings.get("hooks").and_then(|h| h.as_object()) else {
        return false;
    };
    hooks
        .values()
        .filter_map(|g| g.as_array())
        .any(|groups| groups.iter().any(group_is_ours))
}

/// Remove Recanta-owned hook groups. Returns the new pretty JSON if anything changed.
pub fn strip(text: &str) -> Result<Option<String>> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let mut settings: Value = serde_json::from_str(text).context("parsing hooks JSON")?;
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

fn group_is_ours(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .is_some_and(|cmds| {
            cmds.iter().any(|c| {
                c.get("command").and_then(|v| v.as_str()).is_some_and(|s| s.contains(MARKER))
            })
        })
}

/// Plan a JSON-hooks merge for `path`, parsing existing content. Returns the new pretty
/// content, or `None` if our hooks are already present. Caller builds the Action.
pub fn plan_merge(path: &std::path::Path, hooks: &[Hook]) -> Result<Option<String>> {
    let existing = std::fs::read_to_string(path).ok();
    let mut settings: Value = match &existing {
        Some(text) if !text.trim().is_empty() => serde_json::from_str(text)
            .with_context(|| format!("parsing {} (fix or move it aside)", path.display()))?,
        _ => json!({}),
    };
    if already_installed(&settings) {
        return Ok(None);
    }
    inject(&mut settings, hooks);
    Ok(Some(serde_json::to_string_pretty(&settings)? + "\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOOKS: &[Hook] = &[
        ("SessionStart", None, "recanta brief # recanta:session-start"),
        ("PostToolUse", Some("Edit"), "recanta record-edit --stdin # recanta:post-edit"),
    ];

    #[test]
    fn inject_then_strip_preserves_user_entries() {
        let mut s = json!({ "model": "x", "hooks": { "Stop": [{"hooks":[{"type":"command","command":"echo mine"}]}] } });
        assert!(!already_installed(&s));
        inject(&mut s, HOOKS);
        assert!(already_installed(&s));
        assert!(s["hooks"]["PostToolUse"][0]["matcher"] == "Edit");

        let text = serde_json::to_string(&s).unwrap();
        let back: Value = serde_json::from_str(&strip(&text).unwrap().unwrap()).unwrap();
        assert!(!already_installed(&back));
        assert_eq!(back["model"], "x");
        assert_eq!(back["hooks"]["Stop"][0]["hooks"][0]["command"], "echo mine");
    }
}
