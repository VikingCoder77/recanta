//! Managed-block and backup primitives for non-destructive installation (PRD §8.5,
//! §10). Recanta only ever edits inside clearly marked `RECANTA:START`/`END` blocks and
//! backs up any file it touches, so existing setup is preserved and removal is exact.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Marker lines for shell/text files (the markers are `#` comments so they are inert in
/// shell hook scripts).
pub const START: &str = "# RECANTA:START (managed)";
pub const END: &str = "# RECANTA:END";

/// Insert or replace the Recanta managed block in `existing`. Content outside the
/// markers is preserved verbatim (chaining, PRD §8.5.6). `body` is the block's inner
/// lines (without markers).
pub fn upsert_block(existing: &str, body: &str) -> String {
    let block = format!("{START}\n{body}\n{END}");
    match find_block(existing) {
        Some((start, end)) => {
            let mut out = String::new();
            out.push_str(&existing[..start]);
            out.push_str(&block);
            out.push_str(&existing[end..]);
            out
        }
        None => {
            let mut out = existing.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&block);
            out.push('\n');
            out
        }
    }
}

/// Remove the managed block from `existing`. Returns the new content if a block was
/// present, else `None`.
pub fn remove_block(existing: &str) -> Option<String> {
    let (start, end) = find_block(existing)?;
    // Also swallow a single trailing newline and any blank line we inserted before it.
    let mut e = end;
    if existing[e..].starts_with('\n') {
        e += 1;
    }
    let mut s = start;
    let before = &existing[..s];
    if before.ends_with("\n\n") {
        s -= 1;
    }
    let mut out = String::new();
    out.push_str(&existing[..s]);
    out.push_str(&existing[e..]);
    Some(out)
}

/// True if the text already contains a managed block.
pub fn has_block(existing: &str) -> bool {
    find_block(existing).is_some()
}

/// Byte range `[start, end)` covering the `START..END` block (markers included).
fn find_block(s: &str) -> Option<(usize, usize)> {
    let start = s.find(START)?;
    let end_marker = s[start..].find(END)? + start;
    Some((start, end_marker + END.len()))
}

/// Copy `path` to a timestamped file under `backups_dir`, returning the backup path.
/// No-op (returns `None`) if the source doesn't exist.
pub fn backup(path: &Path, backups_dir: &Path, stamp: &str) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    fs::create_dir_all(backups_dir)
        .with_context(|| format!("creating {}", backups_dir.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let dest = backups_dir.join(format!("{name}.{stamp}.bak"));
    fs::copy(path, &dest)
        .with_context(|| format!("backing up {} -> {}", path.display(), dest.display()))?;
    Ok(Some(dest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_preserves_surrounding_content_and_is_idempotent() {
        let original = "#!/usr/bin/env bash\necho existing hook\n";
        let once = upsert_block(original, "recanta record-commit");
        assert!(once.contains("echo existing hook"));
        assert!(once.contains(START) && once.contains(END));
        // Re-applying replaces in place rather than stacking blocks.
        let twice = upsert_block(&once, "recanta record-commit");
        assert_eq!(once, twice);
        assert_eq!(twice.matches(START).count(), 1);
    }

    #[test]
    fn remove_restores_original() {
        let original = "#!/usr/bin/env bash\necho existing hook\n";
        let installed = upsert_block(original, "recanta record-commit");
        let removed = remove_block(&installed).unwrap();
        assert!(!removed.contains(START));
        assert!(removed.contains("echo existing hook"));
    }

    #[test]
    fn remove_returns_none_when_absent() {
        assert!(remove_block("no markers here").is_none());
    }
}
