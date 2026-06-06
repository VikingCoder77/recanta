//! Output budgeting (PRD §14). Every query command enforces a strict, deterministic
//! limit: for any `--budget n`, the emitted text is `<= n` characters, footer included.
//!
//! The algorithm: pack rendered blocks greedily; if anything is dropped, repack while
//! reserving a small fixed allowance for the "N more omitted" footer, then append it.
//! Reserving up front (rather than trimming after) keeps the `<= budget` guarantee
//! trivially correct.

use clap::ValueEnum;

/// Output profiles (PRD §14). Budget shaping applies to the text profiles; `json` and
/// `ids-only` emit complete output (machine-readable formats are not byte-truncated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// Compact, source-id-bearing text for agents (default).
    Compact,
    /// One id per line.
    IdsOnly,
    /// Versioned, stable JSON schema.
    Json,
}

/// Characters reserved for the omitted-count footer when truncation occurs.
const FOOTER_RESERVE: usize = 96;

/// Default budgets (characters) drawn from the PRD examples.
pub const BRIEF_DEFAULT: usize = 1200;
pub const SEARCH_DEFAULT: usize = 800;

/// Greedily concatenate blocks (newline-separated) without exceeding `budget`.
/// Returns the text and how many blocks were included.
fn greedy(blocks: &[String], budget: usize) -> (String, usize) {
    let mut out = String::new();
    let mut included = 0;
    for block in blocks {
        let sep = usize::from(!out.is_empty());
        if out.len() + sep + block.len() > budget {
            break;
        }
        if sep == 1 {
            out.push('\n');
        }
        out.push_str(block);
        included += 1;
    }
    (out, included)
}

/// Pack `blocks` to fit `budget` characters, appending an omitted-count footer when
/// some are dropped. Guarantees `result.len() <= budget`.
pub fn pack(blocks: Vec<String>, budget: usize) -> String {
    let (full, included) = greedy(&blocks, budget);
    if included == blocks.len() {
        return full;
    }

    // Something will be dropped: repack with room reserved for the footer.
    let effective = budget.saturating_sub(FOOTER_RESERVE);
    let (mut out, included) = greedy(&blocks, effective);
    let omitted = blocks.len() - included;
    let footer =
        format!("… {omitted} more item(s) omitted (raise --budget or inspect by id)");

    let sep = usize::from(!out.is_empty());
    if out.len() + sep + footer.len() <= budget {
        if sep == 1 {
            out.push('\n');
        }
        out.push_str(&footer);
    } else {
        // Degenerate tiny budget: emit a clamped footer only.
        out.clear();
        out.push_str(&footer[..footer.len().min(budget)]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("[{i}] item number {i} with some descriptive text")).collect()
    }

    #[test]
    fn never_exceeds_budget() {
        for budget in [0, 10, 25, 50, 96, 120, 200, 500, 5000] {
            let out = pack(blocks(20), budget);
            assert!(out.len() <= budget, "budget {budget}: got len {}", out.len());
        }
    }

    #[test]
    fn includes_everything_when_it_fits() {
        let out = pack(blocks(3), 10_000);
        assert!(out.contains("[0]") && out.contains("[2]"));
        assert!(!out.contains("omitted"));
    }

    #[test]
    fn reports_omission_when_truncated() {
        let out = pack(blocks(20), 200);
        assert!(out.contains("omitted"), "expected footer, got: {out}");
        assert!(out.len() <= 200);
    }
}
