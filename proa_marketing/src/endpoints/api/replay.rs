// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Bounded response replay for the synchronous, public JSON search operation.
//! Entries are local to one serving process; no account or mutation is involved.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{body::Bytes, http::HeaderMap};
use sha2::{Digest, Sha256};

use super::{ApiError, SearchInput, StatusCode};

const RETENTION: Duration = Duration::from_secs(15 * 60);
const MAX_ENTRIES: usize = 256;
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

type DigestBytes = [u8; 32];

struct Entry {
    fingerprint: DigestBytes,
    body: Bytes,
    expires: Instant,
}

#[derive(Default)]
struct Store {
    entries: HashMap<DigestBytes, Entry>,
    body_bytes: usize,
}

#[derive(Default)]
pub struct SearchReplayCache {
    store: Mutex<Store>,
}

#[derive(Debug)]
pub(super) struct ReplayResponse {
    pub body: Bytes,
    pub replayed: bool,
}

fn unavailable() -> ApiError {
    ApiError {
        status: StatusCode::SERVICE_UNAVAILABLE,
        code: "replay_unavailable",
        detail: "Response replay is unavailable or full. Retry later or omit the key for this safe, read-only search.",
    }
}

impl SearchReplayCache {
    pub(super) fn get_or_compute(
        &self,
        key: DigestBytes,
        fingerprint: DigestBytes,
        now: Instant,
        build: impl FnOnce() -> Result<Bytes, ApiError>,
    ) -> Result<ReplayResponse, ApiError> {
        let mut store = self.store.lock().map_err(|_| unavailable())?;
        let Store {
            entries,
            body_bytes,
        } = &mut *store;
        entries.retain(|_, entry| {
            if entry.expires > now {
                true
            } else {
                *body_bytes -= entry.body.len();
                false
            }
        });

        if let Some(entry) = entries.get(&key) {
            if entry.fingerprint != fingerprint {
                return Err(ApiError {
                    status: StatusCode::UNPROCESSABLE_ENTITY,
                    code: "idempotency_key_reused",
                    detail: "This Idempotency-Key was already used for a different request. Generate a new key when q, limit, or cursor changes.",
                });
            }
            return Ok(ReplayResponse {
                body: entry.body.clone(),
                replayed: true,
            });
        }
        if entries.len() >= MAX_ENTRIES {
            return Err(unavailable());
        }

        // Search and serialization are synchronous and bounded. Keep the lock
        // until insertion so concurrent retries compute once and then replay.
        // No I/O or await occurs here. Failed searches never reserve the key.
        let body = build()?;
        if body.len() > MAX_BODY_BYTES - *body_bytes {
            return Err(unavailable());
        }
        *body_bytes += body.len();
        entries.insert(
            key,
            Entry {
                fingerprint,
                body: body.clone(),
                expires: now + RETENTION,
            },
        );
        Ok(ReplayResponse {
            body,
            replayed: false,
        })
    }
}

pub(super) fn request_key(headers: &HeaderMap) -> Result<Option<DigestBytes>, ApiError> {
    let mut values = headers.get_all("idempotency-key").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    let invalid = || {
        ApiError::invalid("Send one Idempotency-Key: a quoted string of 1–128 ASCII letters, digits, dots, colons, underscores, or hyphens.")
    };
    if values.next().is_some() {
        return Err(invalid());
    }
    let value = value.to_str().map_err(|_| invalid())?;
    let key = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .filter(|key| (1..=128).contains(&key.len()))
        .filter(|key| {
            key.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        })
        .ok_or_else(invalid)?;
    // Store only a digest, never the supplied key or arbitrary header data.
    Ok(Some(Sha256::digest(key.as_bytes()).into()))
}

pub(super) fn fingerprint(input: &SearchInput) -> DigestBytes {
    // Typed serialization gives stable field order. JSON whitespace and object
    // field order do not change the identity of the decoded request.
    Sha256::digest(serde_json::to_vec(input).expect("serialize search input")).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Result<Bytes, ApiError> {
        Ok(Bytes::from_static(b"first response"))
    }

    #[test]
    fn retries_replay_without_extending_expiry_or_allowing_different_payloads() {
        let cache = SearchReplayCache::default();
        let now = Instant::now();
        let first = cache.get_or_compute([1; 32], [2; 32], now, body).unwrap();
        assert!(!first.replayed);
        let replay = cache
            .get_or_compute([1; 32], [2; 32], now + RETENTION / 2, || {
                panic!("a retry must not recompute")
            })
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.body, first.body);
        let conflict = cache
            .get_or_compute([1; 32], [3; 32], now, || panic!("mismatched key"))
            .unwrap_err();
        assert_eq!(conflict.status, StatusCode::UNPROCESSABLE_ENTITY);
        let expired = cache
            .get_or_compute([1; 32], [3; 32], now + RETENTION, || {
                Ok(Bytes::from_static(b"new response"))
            })
            .unwrap();
        assert!(!expired.replayed);
        assert_ne!(expired.body, first.body);
    }

    #[test]
    fn failed_requests_do_not_reserve_keys() {
        let cache = SearchReplayCache::default();
        let now = Instant::now();
        assert!(cache
            .get_or_compute([1; 32], [2; 32], now, || Err(ApiError::invalid(
                "bad query"
            )))
            .is_err());
        assert!(
            !cache
                .get_or_compute([1; 32], [3; 32], now, body)
                .unwrap()
                .replayed
        );
    }

    #[test]
    fn full_cache_preserves_active_entries_and_reclaims_expired_capacity() {
        let cache = SearchReplayCache::default();
        let now = Instant::now();
        for index in 0..MAX_ENTRIES {
            let key = Sha256::digest(index.to_le_bytes()).into();
            cache.get_or_compute(key, [0; 32], now, body).unwrap();
        }
        assert_eq!(
            cache
                .get_or_compute([1; 32], [0; 32], now, body)
                .unwrap_err()
                .status,
            StatusCode::SERVICE_UNAVAILABLE
        );
        let first_key = Sha256::digest(0_usize.to_le_bytes()).into();
        assert!(
            cache
                .get_or_compute(first_key, [0; 32], now, || panic!(
                    "must retain active keys"
                ))
                .unwrap()
                .replayed
        );
        assert!(
            !cache
                .get_or_compute([1; 32], [0; 32], now + RETENTION, body)
                .unwrap()
                .replayed
        );
    }

    #[test]
    fn byte_budget_rejects_new_keys_without_losing_retained_responses() {
        let cache = SearchReplayCache::default();
        let now = Instant::now();
        cache
            .get_or_compute([1; 32], [0; 32], now, || {
                Ok(Bytes::from(vec![0; MAX_BODY_BYTES]))
            })
            .unwrap();
        assert_eq!(
            cache
                .get_or_compute([2; 32], [0; 32], now, body)
                .unwrap_err()
                .status,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert!(
            cache
                .get_or_compute([1; 32], [0; 32], now, body)
                .unwrap()
                .replayed
        );
        assert!(cache
            .get_or_compute([2; 32], [0; 32], now + RETENTION, body)
            .is_ok());
    }

    #[test]
    fn concurrent_retries_compute_one_response() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Barrier,
        };
        let cache = SearchReplayCache::default();
        let started = Barrier::new(8);
        let computations = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    started.wait();
                    let result = cache
                        .get_or_compute([1; 32], [2; 32], Instant::now(), || {
                            computations.fetch_add(1, Ordering::Relaxed);
                            body()
                        })
                        .unwrap();
                    assert_eq!(result.body, body().unwrap());
                });
            }
        });
        assert_eq!(computations.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn key_parser_rejects_ambiguous_or_unbounded_headers() {
        let mut headers = HeaderMap::new();
        assert!(request_key(&headers).unwrap().is_none());
        for value in [
            "plain",
            "\"\"",
            "\"has space\"",
            "\"a\", \"b\"",
            "\"a\\\"b\"",
            "\"a\";param=1",
        ] {
            headers.insert("idempotency-key", value.parse().unwrap());
            assert_eq!(
                request_key(&headers).unwrap_err().status,
                StatusCode::BAD_REQUEST
            );
        }
        headers.insert(
            "idempotency-key",
            format!("\"{}\"", "a".repeat(129)).parse().unwrap(),
        );
        assert!(request_key(&headers).is_err());
        headers.insert(
            "idempotency-key",
            format!("\"{}\"", "a".repeat(128)).parse().unwrap(),
        );
        assert!(request_key(&headers).unwrap().is_some());
        headers.insert("idempotency-key", "\"abc-123._:XYZ\"".parse().unwrap());
        assert!(request_key(&headers).unwrap().is_some());
        headers.append("idempotency-key", "\"second\"".parse().unwrap());
        assert!(request_key(&headers).is_err());
    }
}
