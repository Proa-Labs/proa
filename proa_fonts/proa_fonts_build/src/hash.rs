// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Content hashing and identifier derivation.
//!
//! Vended files are fingerprinted by the first 16 hex characters of the
//! SHA-256 of their bytes. That is the scheme the public asset catalog uses,
//! so it publishes a vendored file under the name it already has.

use sha2::{Digest, Sha256};

/// Full lowercase-hex SHA-256 of `bytes`. Stored in the lockfile for integrity
/// checks.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    to_hex(&digest)
}

/// Hex characters of the SHA-256 carried in a vended filename. Keep this
/// equal to `proa_assets::FINGERPRINT_LENGTH`: the catalog keeps a name only
/// when its suffix is that prefix of the file's digest.
pub const FINGERPRINT_LENGTH: usize = 16;

/// The content fingerprint embedded in vended filenames.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut hex = sha256_hex(bytes);
    hex.truncate(FINGERPRINT_LENGTH);
    hex
}

/// A stable 8-hex identifier derived from an arbitrary string. Used to make
/// generated class names collision-resistant across fonts.
pub fn short_hash(seed: &str) -> String {
    let mut hex = sha256_hex(seed.as_bytes());
    hex.truncate(8);
    hex
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
    fn content_hash_is_the_catalog_fingerprint() {
        assert_eq!(content_hash(b""), "e3b0c44298fc1c14");
        assert_eq!(short_hash(""), "e3b0c442");
    }

    #[test]
    fn short_hash_is_deterministic_and_seed_sensitive() {
        assert_eq!(short_hash("Geist"), short_hash("Geist"));
        assert_ne!(short_hash("Geist"), short_hash("Geist Mono"));
    }
}
