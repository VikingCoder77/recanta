//! The cross-project **workspace registry** (`~/.recanta/workspace.json`).
//!
//! Recanta keeps one single-file store per project — that portability is the moat, so the
//! registry never merges data. It is just a list of project roots that the Explorer unions
//! *at read time* into one overview, with a per-project filter. `recanta init` registers a
//! project automatically; `recanta workspace …` manages the list; and `recanta serve`
//! defaults to **workspace mode** (show every registered project), falling back to a single
//! project when the registry is empty, disabled, or a specific `--project` is given.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::project;

/// Registry file name inside `~/.recanta/`.
const FILE: &str = "workspace.json";

/// One registered project: where it lives and a human label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRef {
    pub path: PathBuf,
    pub name: String,
}

/// The persisted workspace: a global toggle plus the registered projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    /// When false, `serve` ignores the registry and shows only the current project.
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub projects: Vec<ProjectRef>,
}

fn default_true() -> bool {
    true
}

impl Default for Workspace {
    fn default() -> Self {
        Workspace { enabled: true, projects: Vec::new() }
    }
}

impl Workspace {
    /// Load the registry, or a default (enabled, empty) one when the file is absent.
    /// A malformed file is surfaced as an error rather than silently discarded.
    pub fn load() -> Result<Workspace> {
        let path = Self::path()?;
        if !path.is_file() {
            return Ok(Workspace::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Persist the registry, creating `~/.recanta/` if needed.
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)? + "\n";
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    }

    pub fn path() -> Result<PathBuf> {
        Ok(project::global_dir()?.join(FILE))
    }

    /// Register a project root (idempotent: matching by canonical path). Returns true if it
    /// was newly added. The name defaults to the directory basename.
    pub fn register(&mut self, root: &Path) -> bool {
        let canon = canonical(root);
        if self.projects.iter().any(|p| canonical(&p.path) == canon) {
            return false;
        }
        let name = root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("project")
            .to_string();
        self.projects.push(ProjectRef { path: canon, name });
        true
    }

    /// Remove a project root (matching by canonical path). Returns true if one was removed.
    pub fn remove(&mut self, root: &Path) -> bool {
        let canon = canonical(root);
        let before = self.projects.len();
        self.projects.retain(|p| canonical(&p.path) != canon);
        self.projects.len() != before
    }

    /// Registered projects that still have an initialized `.recanta/` store, in order.
    pub fn live_projects(&self) -> Vec<ProjectRef> {
        self.projects
            .iter()
            .filter(|p| project::Paths::for_root(&p.path).is_initialized())
            .cloned()
            .collect()
    }
}

/// Best-effort canonicalization so the same project isn't registered twice under different
/// spellings. Falls back to the path as-given when it can't be resolved (e.g. not yet on
/// disk), which still de-dupes exact repeats.
fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_is_idempotent_and_names_from_basename() {
        let mut ws = Workspace::default();
        let root = std::env::temp_dir();
        assert!(ws.register(&root), "first add");
        assert!(!ws.register(&root), "second add is a no-op");
        assert_eq!(ws.projects.len(), 1);
        assert!(!ws.projects[0].name.is_empty());
    }

    #[test]
    fn remove_matches_by_path() {
        let mut ws = Workspace::default();
        let root = std::env::temp_dir();
        ws.register(&root);
        assert!(ws.remove(&root));
        assert!(!ws.remove(&root));
        assert!(ws.projects.is_empty());
    }

    #[test]
    fn defaults_to_enabled() {
        assert!(Workspace::default().enabled);
        // Missing `enabled` in older files deserializes to true.
        let ws: Workspace = serde_json::from_str(r#"{"projects":[]}"#).unwrap();
        assert!(ws.enabled);
    }
}
