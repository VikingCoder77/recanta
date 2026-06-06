//! Git `post-commit` hook planning (PRD §8.5, §10.2). Detects the *real* hook mechanism
//! — `core.hooksPath`, Husky, the pre-commit framework, lefthook — rather than blindly
//! writing `.git/hooks`, which silently no-ops when hooks are redirected.

use std::path::{Path, PathBuf};

use crate::git;

use super::{Action, Plan, Verb};

/// The body of the managed block (inner lines only). Fail-open: never blocks a commit.
const HOOK_BODY: &str = "if command -v recanta >/dev/null 2>&1; then\n  \
    recanta record-commit --commit HEAD --project \"$(git rev-parse --show-toplevel)\" \
    >/dev/null 2>&1 || true\nfi";

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
                "git hooks are managed by `{name}`; add this line to your {name} \
                 post-commit step instead of letting Recanta write .git/hooks:\n      \
                 recanta record-commit --commit HEAD"
            ));
        }
        Mechanism::Writable { dir, label } => {
            let path = dir.join("post-commit");
            let existing = std::fs::read_to_string(&path).unwrap_or_default();
            if super::managed::has_block(&existing) {
                plan.notes
                    .push(format!("git post-commit already installed ({label})"));
                return plan;
            }
            let new_content = if existing.trim().is_empty() {
                format!("#!/usr/bin/env bash\n{}\n", block(&existing))
            } else {
                block(&existing)
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
                block_id: "git-post-commit".to_string(),
                preview,
            });
        }
    }
    plan
}

fn block(existing: &str) -> String {
    super::managed::upsert_block(existing, HOOK_BODY)
}
