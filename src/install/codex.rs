//! Codex CLI adapter (PRD §8.6). Codex shares the JSON-over-stdin command-hook model and
//! reads project hooks from `.codex/hooks/hooks.json` (in a trusted folder). Only
//! `command` hooks execute. Merge + removal reuse `json_hooks`.

use std::path::Path;

use anyhow::Result;

use super::json_hooks::{self, Hook};
use super::{Action, Plan, Verb};

const HOOKS: &[Hook] = &[
    ("SessionStart", None, "recanta brief --budget 1200 # recanta:session-start"),
    ("PostToolUse", None, "recanta record-edit --stdin --source codex # recanta:post-edit"),
    ("Stop", None, "recanta record-event --type harness.stop --stdin --source codex # recanta:stop"),
];

pub fn plan(root: &Path) -> Result<Plan> {
    let mut plan = Plan::default();
    let path = root.join(".codex").join("hooks").join("hooks.json");
    match json_hooks::plan_merge(&path, HOOKS)? {
        None => plan.notes.push("Codex hooks already installed".into()),
        Some(new_content) => {
            let verb = if path.exists() { Verb::Update } else { Verb::Create };
            plan.actions.push(Action {
                harness: "codex",
                mechanism: "hooks.json".to_string(),
                path,
                verb,
                new_content,
                executable: false,
                block_id: "codex-hooks".to_string(),
                preview: "Codex SessionStart→brief, PostToolUse→record-edit, Stop→record-event".to_string(),
            });
            plan.notes.push(
                "Codex only runs project hooks in a TRUSTED folder — trust this project, \
                 or the hooks won't fire."
                    .into(),
            );
        }
    }
    Ok(plan)
}
