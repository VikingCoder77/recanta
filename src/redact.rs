//! Secret redaction (PRD §8.12a). Runs before anything is stored, indexed, summarized,
//! or embedded. v0.1 redacts **strong-signal** secrets — known token formats and
//! secret-named assignments — which deliberately never match a bare git SHA, UUID, or
//! hash (those are core data here, §8.12a). High-false-positive entropy scanning is
//! intentionally not enabled; an allowlist guards the assignment path as belt-and-braces.
//!
//! Every redaction reports which pattern fired, for the audit trail (PRD §11.1
//! `redaction_audit`) and false-positive debugging.

use std::sync::OnceLock;

use regex::Regex;

/// One redaction occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub pattern_id: String,
    pub start: usize,
    pub end: usize,
}

/// Result of redacting some text.
#[derive(Debug, Clone)]
pub struct Redaction {
    pub text: String,
    pub hits: Vec<Hit>,
}

impl Redaction {
    pub fn is_clean(&self) -> bool {
        self.hits.is_empty()
    }
}

struct Pattern {
    id: &'static str,
    re: Regex,
    /// Capture group to redact (0 = whole match).
    group: usize,
}

fn patterns() -> &'static [Pattern] {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let p = |id, re: &str, group| Pattern { id, re: Regex::new(re).unwrap(), group };
        vec![
            p("private-key",
              r"-----BEGIN[A-Z ]*PRIVATE KEY-----[\s\S]*?-----END[A-Z ]*PRIVATE KEY-----", 0),
            p("aws-access-key", r"\bAKIA[0-9A-Z]{16}\b", 0),
            p("github-token", r"\bgh[pousr]_[A-Za-z0-9]{36,}\b", 0),
            p("slack-token", r"\bxox[baprs]-[0-9A-Za-z-]{10,}\b", 0),
            p("google-api-key", r"\bAIza[0-9A-Za-z_\-]{35}\b", 0),
            p("ai-provider-key", r"\bsk-(?:ant-|proj-)?[A-Za-z0-9_\-]{20,}\b", 0),
            p("jwt", r"\beyJ[A-Za-z0-9_\-]+\.eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+", 0),
            p("bearer", r"(?i)\bbearer\s+([A-Za-z0-9._\-]{12,})", 1),
            p("secret-assignment",
              r#"(?i)\b(?:api[_-]?key|secret|token|password|passwd|pwd|access[_-]?key)\b\s*[:=]\s*['"]?([^\s'"]{6,})['"]?"#, 1),
        ]
    })
}

/// Redact secrets in `text`, replacing each with `[redacted:<pattern>]`.
pub fn redact(text: &str) -> Redaction {
    // Collect candidate spans across all patterns.
    let mut spans: Vec<(usize, usize, &'static str)> = Vec::new();
    for p in patterns() {
        for caps in p.re.captures_iter(text) {
            if let Some(m) = caps.get(p.group) {
                if is_allowlisted(m.as_str()) {
                    continue;
                }
                spans.push((m.start(), m.end(), p.id));
            }
        }
    }
    // Resolve overlaps: earliest start wins, longest at a tie.
    spans.sort_by_key(|(s, e, _)| (*s, std::cmp::Reverse(*e)));

    let mut out = String::with_capacity(text.len());
    let mut hits = Vec::new();
    let mut idx = 0;
    for (s, e, id) in spans {
        if s < idx {
            continue; // overlaps an already-redacted region
        }
        out.push_str(&text[idx..s]);
        out.push_str(&format!("[redacted:{id}]"));
        hits.push(Hit { pattern_id: id.to_string(), start: s, end: e });
        idx = e;
    }
    out.push_str(&text[idx..]);
    Redaction { text: out, hits }
}

/// Values that must never be treated as secrets even in a secret-named assignment:
/// git SHAs, UUIDs, and hashes (pure hex), which are core data (PRD §8.12a).
fn is_allowlisted(v: &str) -> bool {
    let is_hex = (7..=64).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_hexdigit());
    is_hex || uuid_re().is_match(v)
}

fn uuid_re() -> &'static Regex {
    static UUID: OnceLock<Regex> = OnceLock::new();
    UUID.get_or_init(|| {
        Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
            .unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(r: &Redaction) -> Vec<&str> {
        r.hits.iter().map(|h| h.pattern_id.as_str()).collect()
    }

    #[test]
    fn redacts_known_token_formats() {
        let r = redact("key=AKIAIOSFODNN7EXAMPLE and tok ghp_0123456789012345678901234567890123ab");
        assert!(r.text.contains("[redacted:aws-access-key]"));
        assert!(r.text.contains("[redacted:github-token]"));
        assert!(!r.text.contains("AKIA"));
    }

    #[test]
    fn redacts_secret_assignment_and_bearer() {
        let r = redact("password = 'hunter2supersecret'");
        assert_eq!(ids(&r), vec!["secret-assignment"]);
        assert!(r.text.contains("[redacted:secret-assignment]"));

        let b = redact("Authorization: Bearer abcdefghijklmnop");
        assert_eq!(ids(&b), vec!["bearer"]);
    }

    #[test]
    fn never_redacts_git_shas_uuids_or_hashes() {
        // The exact acceptance criterion in PRD §18.
        let r = redact("revert to 9f31a2c0b8e4d5f6a7b8c9d0e1f2a3b4c5d6e7f8 see uuid \
                        550e8400-e29b-41d4-a716-446655440000");
        assert!(r.is_clean(), "SHAs/UUIDs must not be redacted, got: {:?}", r.hits);

        // Even when a SHA is assigned to a secret-looking name, the hash allowlist wins.
        let a = redact("commit_token=9f31a2c0b8e4d5f6a7b8c9d0e1f2a3b4c5d6e7f8");
        assert!(a.is_clean());
    }

    #[test]
    fn clean_text_is_unchanged() {
        let r = redact("just a normal commit message about refactoring");
        assert!(r.is_clean());
        assert_eq!(r.text, "just a normal commit message about refactoring");
    }
}
