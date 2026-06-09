//! Git `post-commit` hook planning (PRD §8.5, §10.2). Detects the *real* hook mechanism
//! — `core.hooksPath`, Husky, the pre-commit framework, lefthook — rather than blindly
//! writing `.git/hooks`, which silently no-ops when hooks are redirected.

use std::path::{Path, PathBuf};

use crate::git;

use super::{Action, Plan, Verb};

/// The git hooks Recanta installs: (hook file, managed-block body). Bodies are
/// fail-open (`|| true`) and never block the git operation.
/// - `post-commit`  → record the commit.
/// - `post-checkout`→ refresh the code graph after a branch switch (`$3==1`).
/// - `post-merge`   → refresh the code graph after a merge/pull.
const HOOKS: &[(&str, &str)] = &[
    (
        "post-commit",
        "if command -v recanta >/dev/null 2>&1; then\n  \
         ROOT=\"$(git rev-parse --show-toplevel)\"\n  \
         recanta index --changed-only --project \"$ROOT\" >/dev/null 2>&1 || true\n  \
         recanta record-commit --commit HEAD --project \"$ROOT\" >/dev/null 2>&1 || true\nfi",
    ),
    (
        "post-checkout",
        "if [ \"${3:-1}\" = \"1\" ] && command -v recanta >/dev/null 2>&1; then\n  \
         recanta index --changed-only --project \"$(git rev-parse --show-toplevel)\" \
         >/dev/null 2>&1 || true\nfi",
    ),
    (
        "post-merge",
        "if command -v recanta >/dev/null 2>&1; then\n  \
         recanta index --changed-only --project \"$(git rev-parse --show-toplevel)\" \
         >/dev/null 2>&1 || true\nfi",
    ),
];

/// Where (and whether) we can safely write a `post-commit` hook.
enum Mechanism {
    /// A writable hooks directory (standard `.git/hooks`, a `core.hooksPath`, or Husky).
    Writable { dir: PathBuf, label: &'static str },
    /// A framework owns hooks; writing a raw hook would be wrong. Warn instead.
    Framework(&'static str),
}

fn detect(root: &Path) -> Mechanism {
    if root.join(".pre-commit-config.yaml").exists() {
        return Mechanism::Framework("pre-commit");
    }
    if ["lefthook.yml", "lefthook.yaml", "lefthook.toml", ".lefthook.yml"]
        .iter()
        .any(|f| root.join(f).exists())
    {
        return Mechanism::Framework("lefthook");
    }
    if root.join(".husky").is_dir() {
        return Mechanism::Writable { dir: root.join(".husky"), label: "husky" };
    }
    if let Some(hp) = git::config_get(root, "core.hooksPath") {
        let dir = resolve(root, &hp);
        return Mechanism::Writable { dir, label: "core.hooksPath" };
    }
    // Default: the real hooks dir (honors linked worktrees).
    let dir = git::git_path(root, "hooks")
        .map(|p| resolve(root, &p.to_string_lossy()))
        .unwrap_or_else(|| root.join(".git").join("hooks"));
    Mechanism::Writable { dir, label: "git-hooks" }
}

/// Resolve a possibly-relative path against the project root.
fn resolve(root: &Path, p: &str) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

/// Plan the git post-commit hook installation.
pub fn plan(root: &Path) -> Plan {
    let mut plan = Plan::default();
    match detect(root) {
        Mechanism::Framework(name) => {
            plan.warnings.push(format!(
                "git hooks are managed by `{name}`; add Recanta to your {name} \
                 post-commit/post-checkout/post-merge steps instead of letting it write \
                 .git/hooks (e.g. `recanta record-commit --commit HEAD`)."
            ));
        }
        Mechanism::Writable { dir, label } => {
            for (hook, body) in HOOKS {
                let path = dir.join(hook);
                let existing = std::fs::read_to_string(&path).unwrap_or_default();
                if super::managed::has_block(&existing) {
                    plan.notes.push(format!("git {hook} already installed ({label})"));
                    continue;
                }
                let block = super::managed::upsert_block(&existing, body);
                let new_content = if existing.trim().is_empty() {
                    format!("#!/usr/bin/env bash\n{block}\n")
                } else {
                    block
                };
                let verb = if path.exists() { Verb::Update } else { Verb::Create };
                let preview = match verb {
                    Verb::Create => format!("create {} ({label})", path.display()),
                    Verb::Update => format!("add managed block to {} ({label}, chained)", path.display()),
                };
                plan.actions.push(Action {
                    harness: "git",
                    mechanism: label.to_string(),
                    path,
                    verb,
                    new_content,
                    executable: true,
                    block_id: format!("git-{hook}"),
                    preview,
                });
            }
        }
    }
    plan
}
