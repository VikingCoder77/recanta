//! OpenCode adapter (PRD §8.6, §10.4). OpenCode integrates via JS/TS plugin modules, not
//! shell hooks, so Recanta writes a small plugin to `.opencode/plugin/recanta.ts` that
//! shells out to `recanta` on session/tool events. Best-effort: the OpenCode plugin API
//! varies, so the module is defensive (all calls are `nothrow`).

use std::path::Path;

use anyhow::Result;

use super::{Action, Plan, Verb};

/// Marker so uninstall can recognize the file as ours.
const MARKER: &str = "# recanta:plugin";

const PLUGIN_TS: &str = r#"// Recanta OpenCode plugin (managed by `recanta install`) -- # recanta:plugin
// Briefs new sessions and records edits/events into Recanta. Defensive by design:
// every call is best-effort and never throws into OpenCode.
export const recanta = async ({ $ }) => {
  const run = async (cmd) => { try { await cmd.quiet().nothrow() } catch (_) {} }
  return {
    "session.created": async () => {
      await run($`recanta brief --budget 1200`)
    },
    "tool.execute.after": async (input, output) => {
      try {
        const fp = output?.args?.filePath ?? output?.args?.file_path ?? input?.args?.filePath
        if (fp) {
          const payload = JSON.stringify({ file_path: fp })
          await run($`printf %s ${payload} | recanta record-edit --stdin --source opencode`)
        }
      } catch (_) {}
    },
    "session.idle": async () => {
      await run($`recanta record-event --type harness.idle --source opencode`)
    },
  }
}
"#;

pub fn plan(root: &Path) -> Result<Plan> {
    let mut plan = Plan::default();
    let path = root.join(".opencode").join("plugin").join("recanta.ts");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.contains(MARKER) {
        plan.notes.push("OpenCode plugin already installed".into());
        return Ok(plan);
    }
    let verb = if path.exists() { Verb::Update } else { Verb::Create };
    plan.actions.push(Action {
        harness: "opencode",
        mechanism: "plugin".to_string(),
        path,
        verb,
        new_content: PLUGIN_TS.to_string(),
        executable: false,
        block_id: "opencode-plugin".to_string(),
        preview: "OpenCode plugin: session.created→brief, tool.execute.after→record-edit".to_string(),
    });
    Ok(plan)
}

/// Whether a file is the Recanta OpenCode plugin (for uninstall).
pub fn is_ours(text: &str) -> bool {
    text.contains(MARKER)
}
