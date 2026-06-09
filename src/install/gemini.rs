//! Gemini CLI adapter (PRD §8.6). Gemini uses the same JSON-over-stdin command-hook
//! contract and reads project hooks from `.gemini/settings.json`. Merge + removal reuse
//! `json_hooks`.
//!
//! Note: Gemini requires hook stdout to be JSON only. The `record-edit`/`record-event`
//! hooks consume stdin and don't rely on stdout, so they're robust; the SessionStart
//! `brief` injection prints plain text and may need per-version output shaping — flagged
//! to the user.

use std::path::Path;

use anyhow::Result;

use super::json_hooks::{self, Hook};
use super::{Action, Plan, Verb};

const HOOKS: &[Hook] = &[
    ("SessionStart", None, "recanta brief --budget 1200 # recanta:session-start"),
    ("PostToolUse", None, "recanta record-edit --stdin --source gemini # recanta:post-edit"),
    ("Stop", None, "recanta record-event --type harness.stop --stdin --source gemini # recanta:stop"),
];

pub fn plan(root: &Path) -> Result<Plan> {
    let mut plan = Plan::default();
    let path = root.join(".gemini").join("settings.json");
    match json_hooks::plan_merge(&path, HOOKS)? {
        None => plan.notes.push("Gemini hooks already installed".into()),
        Some(new_content) => {
            let verb = if path.exists() { Verb::Update } else { Verb::Create };
            plan.actions.push(Action {
                harness: "gemini",
                mechanism: "settings".to_string(),
                path,
                verb,
                new_content,
                executable: false,
                block_id: "gemini-hooks".to_string(),
                preview: "Gemini SessionStart→brief, PostToolUse→record-edit, Stop→record-event".to_string(),
            });
            plan.notes.push(
                "Gemini runs project hooks only in a trusted folder, and expects hook \
                 stdout to be JSON — the brief hook may need output shaping on your version."
                    .into(),
            );
        }
    }
    Ok(plan)
}
