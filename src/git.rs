//! Thin wrappers over the `git` CLI. Recanta is Git-aware (PRD §8.11) but does not
//! embed a Git library — shelling out keeps the binary small and matches real user
//! state exactly (worktrees, hooksPath, etc.). Every helper is best-effort: a missing
//! `git`, a non-repo directory, or a repo with no commits returns `None`, never errors.

use std::path::Path;
use std::process::Command;

/// Run `git <args>` in `dir`, returning trimmed stdout on exit status 0.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// True if `dir` is inside a Git work tree.
pub fn is_repo(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
}

/// Absolute path of the work-tree root, if any.
pub fn toplevel(dir: &Path) -> Option<String> {
    git(dir, &["rev-parse", "--show-toplevel"])
}

/// The root (parent-less) commit SHA. This is Recanta's primary project identity
/// (PRD §11.2): stable across clones, remotes, and SSH/HTTPS. `None` until the first
/// commit exists. If history has multiple roots we take the first deterministically.
pub fn root_commit_sha(dir: &Path) -> Option<String> {
    let out = git(dir, &["rev-list", "--max-parents=0", "HEAD"])?;
    out.lines().next().map(|l| l.trim().to_string())
}

/// Current branch name, or `None` for detached HEAD / no commits.
pub fn current_branch(dir: &Path) -> Option<String> {
    git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])
}

/// Current HEAD commit SHA, if any.
pub fn head_sha(dir: &Path) -> Option<String> {
    git(dir, &["rev-parse", "HEAD"])
}

/// Whether the work tree has uncommitted changes. `None` if not a repo.
pub fn is_dirty(dir: &Path) -> Option<bool> {
    if !is_repo(dir) {
        return None;
    }
    // `--porcelain` is empty exactly when the tree is clean; an empty result from
    // `git()` therefore means clean, not failure, so call Command directly.
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(!out.stdout.is_empty())
}
