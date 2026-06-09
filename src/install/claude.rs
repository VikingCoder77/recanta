//! Claude Code adapter (PRD §8.6, §10.3). Merges Recanta's command hooks into
//! `.claude/settings.json`: SessionStart injects `recanta brief`, PostToolUse records
//! edits, Stop records the session end. JSON-merge + marker-based removal live in
//! `json_hooks`.

use std::path::Path;

use anyhow::Result;

use super::json_hooks::{self, Hook};
use super::{Action, Plan, Verb};

const HOOKS: &[Hook] = &[
    ("SessionStart", None, "recanta brief --budget 1200 # recanta:session-start"),
    (
        "PostToolUse",
        Some("Edit|Write|MultiEdit|NotebookEdit"),
        "recanta record-edit --stdin --source claude-code # recanta:post-edit",
    ),
    ("Stop", None, "recanta record-event --type harness.stop --stdin --source claude-code # recanta:stop"),
];

pub fn plan(root: &Path) -> Result<Plan> {
    let mut plan = Plan::default();
    let path = root.join(".claude").join("settings.json");
    match json_hooks::plan_merge(&path, HOOKS)? {
        None => plan.notes.push("Claude Code hooks already installed".into()),
        Some(new_content) => {
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
                "Claude Code shows a folder-trust prompt; approve this project or the hooks won't run."
                    .into(),
            );
        }
    }
    Ok(plan)
}
