//! Project layout, identity, and the on-disk `.recanta/project.json` config.
//!
//! Recanta stores everything for a project under a single `.recanta/` directory at the
//! project root (PRD §1, §11). Identity is the Git root-commit SHA when available,
//! falling back to a generated UUID for repos with no commits yet (PRD §11.2).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::git;

/// Name of the per-project directory.
pub const DIR: &str = ".recanta";
/// Config file inside `.recanta/`.
pub const CONFIG_FILE: &str = "project.json";
/// SQLite store inside `.recanta/`.
pub const DB_FILE: &str = "recanta.db";

/// Resolved on-disk locations for one project.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Project root (the directory that contains `.recanta/`).
    pub root: PathBuf,
    /// The `.recanta/` directory.
    pub dir: PathBuf,
    /// `.recanta/project.json`.
    pub config: PathBuf,
    /// `.recanta/recanta.db`.
    pub db: PathBuf,
    /// `.recanta/backups/` (installer backups, PRD §8.5).
    pub backups: PathBuf,
}

impl Paths {
    /// Compute paths for a given project root, without touching the filesystem.
    pub fn for_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let dir = root.join(DIR);
        Paths {
            config: dir.join(CONFIG_FILE),
            db: dir.join(DB_FILE),
            backups: dir.join("backups"),
            dir,
            root,
        }
    }

    /// True if this project is initialized (config present).
    pub fn is_initialized(&self) -> bool {
        self.config.is_file()
    }

    /// Locate an initialized project. If `explicit` is given it is used directly;
    /// otherwise we walk up from the current directory to the nearest `.recanta/`.
    pub fn discover(explicit: Option<&Path>) -> Result<Self> {
        if let Some(p) = explicit {
            let paths = Paths::for_root(p);
            if !paths.is_initialized() {
                bail!(
                    "no Recanta project at {} (run `recanta init` there first)",
                    paths.root.display()
                );
            }
            return Ok(paths);
        }

        let start = std::env::current_dir().context("cannot read current directory")?;
        let mut cur = start.as_path();
        loop {
            let candidate = Paths::for_root(cur);
            if candidate.is_initialized() {
                return Ok(candidate);
            }
            match cur.parent() {
                Some(parent) => cur = parent,
                None => bail!(
                    "no Recanta project found in {} or any parent (run `recanta init`)",
                    start.display()
                ),
            }
        }
    }
}

/// Contents of `.recanta/project.json`. Kept small and human-readable; it is the
/// stable identity record, not a cache. The SQLite store holds everything else.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Schema/format version of this file.
    pub version: u32,
    /// Generated project UUID (stable handle, also used when no root commit exists).
    pub id: String,
    /// Human-friendly name (defaults to the root directory name).
    pub name: String,
    /// Git root-commit SHA — primary identity when present (PRD §11.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_commit_sha: Option<String>,
    /// RFC 3339 creation time.
    pub created_at: String,
    /// Default ignore rules applied before parsing/redaction (PRD §8.12).
    pub ignore: Vec<String>,
}

impl Config {
    pub const VERSION: u32 = 1;

    /// Default ignore rules — secrets and generated/vendor artifacts (PRD §8.12a).
    pub fn default_ignore() -> Vec<String> {
        [
            ".env*", "*.pem", "*.key", "id_rsa*", "node_modules/", ".venv/", "dist/",
            "build/", ".git/", "target/", "*.lock",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    pub fn load(path: &Path) -> Result<Config> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(path, raw + "\n")
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

/// Build the initial config for a freshly-initialized project root.
pub fn new_config(root: &Path) -> Result<Config> {
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .to_string();
    Ok(Config {
        version: Config::VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        name,
        root_commit_sha: git::root_commit_sha(root),
        created_at: now_rfc3339(),
        ignore: Config::default_ignore(),
    })
}

/// Current time as an RFC 3339 string (UTC). Kept here so callers don't depend on a
/// time crate directly.
pub fn now_rfc3339() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrips_through_disk() {
        let dir = std::env::temp_dir().join(format!("recanta-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = new_config(&dir).unwrap();
        let path = dir.join("project.json");
        cfg.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.id, cfg.id);
        assert_eq!(loaded.version, Config::VERSION);
        assert!(!loaded.ignore.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn paths_are_under_dot_recanta() {
        let p = Paths::for_root("/tmp/example");
        assert!(p.dir.ends_with(".recanta"));
        assert!(p.db.ends_with(".recanta/recanta.db"));
        assert!(p.config.ends_with(".recanta/project.json"));
    }
}
