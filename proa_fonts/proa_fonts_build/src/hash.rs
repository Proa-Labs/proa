// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Content hashing and identifier derivation.
//!
//! Vended files are fingerprinted by the first 8 hex characters of the SHA-256
//! of their bytes, matching the `<name>_<8hex>` convention the rsjs asset
//! manifest already uses.

use sha2::{Digest, Sha256};

/// Full lowercase-hex SHA-256 of `bytes`. Stored in the lockfile for integrity
/// checks.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    to_hex(&digest)
}

/// The first 8 hex characters of the SHA-256 of `bytes`: the content
/// fingerprint embedded in vended filenames.
pub fn content_hash8(bytes: &[u8]) -> String {
    let mut hex = sha256_hex(bytes);
    hex.truncate(8);
    hex
}

/// A stable 8-hex identifier derived from an arbitrary string. Used to make
/// generated class names collision-resistant across fonts.
pub fn short_hash(seed: &str) -> String {
    content_hash8(seed.as_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        // SHA-256("") well-known digest.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn content_hash_is_first_eight_hex() {
        assert_eq!(content_hash8(b""), "e3b0c442");
        assert_eq!(content_hash8(b"").len(), 8);
    }

    #[test]
    fn short_hash_is_deterministic_and_seed_sensitive() {
        assert_eq!(short_hash("Geist"), short_hash("Geist"));
        assert_ne!(short_hash("Geist"), short_hash("Geist Mono"));
    }
}
