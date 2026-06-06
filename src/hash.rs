//! Hashing helpers. Used for privacy tokens (author email, remote URL — PRD §11.1)
//! and, later, content digests (symbol body-hash, §8.10). SHA-256 hex; callers pick a
//! prefix length where a short token suffices.

use sha2::{Digest, Sha256};

/// Full lowercase hex SHA-256 of the input.
pub fn sha256_hex(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    let mut s = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(s, "{byte:02x}");
    }
    s
}

/// First 16 hex chars of the SHA-256 — an opaque, non-reversible-at-a-glance token for
/// low-stakes identifiers like a hashed author email.
pub fn short_token(input: &str) -> String {
    sha256_hex(input)[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_and_distinct() {
        assert_eq!(sha256_hex("a"), sha256_hex("a"));
        assert_ne!(sha256_hex("a"), sha256_hex("b"));
        assert_eq!(short_token("a").len(), 16);
    }
}
