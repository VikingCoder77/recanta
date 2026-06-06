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

/// Read a git config value (`git config --get <key>`), if set.
pub fn config_get(dir: &Path, key: &str) -> Option<String> {
    git(dir, &["config", "--get", key])
}

/// Resolve a path inside the git dir (`git rev-parse --git-path <name>`), e.g. the
/// real `hooks` directory honoring linked worktrees. Does NOT honor `core.hooksPath`
/// (callers check that separately). Returned path may be relative to `dir`.
pub fn git_path(dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    git(dir, &["rev-parse", "--git-path", name]).map(std::path::PathBuf::from)
}

/// Metadata for a single commit (PRD §11.1 `commits`).
#[derive(Debug, Clone)]
pub struct CommitMeta {
    pub sha: String,
    pub parents: Vec<String>,
    pub author_email: String,
    pub subject: String,
    /// Committer date, strict ISO 8601.
    pub timestamp: String,
}

/// Resolve a revision (e.g. `HEAD`, a branch, a short SHA) to a full commit SHA.
pub fn resolve_commit(dir: &Path, rev: &str) -> Option<String> {
    git(dir, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")])
}

/// Read metadata for a commit. `None` if it doesn't exist.
pub fn commit_meta(dir: &Path, sha: &str) -> Option<CommitMeta> {
    // One record, fields separated by newlines: hash, parents, author email, date, subject.
    let raw = git(dir, &["show", "-s", "--format=%H%n%P%n%ae%n%cI%n%s", sha])?;
    let mut lines = raw.lines();
    let sha = lines.next()?.trim().to_string();
    let parents = lines
        .next()
        .map(|p| p.split_whitespace().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    let author_email = lines.next().unwrap_or("").trim().to_string();
    let timestamp = lines.next().unwrap_or("").trim().to_string();
    let subject = lines.collect::<Vec<_>>().join("\n");
    Some(CommitMeta { sha, parents, author_email, subject, timestamp })
}

/// Files changed by a commit (vs its first parent; all files for a root commit, via
/// `--root`).
pub fn changed_files(dir: &Path, sha: &str) -> Vec<String> {
    match git(dir, &["diff-tree", "--root", "--no-commit-id", "--name-only", "-r", sha]) {
        Some(out) => out.lines().map(|l| l.to_string()).collect(),
        None => Vec::new(),
    }
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
