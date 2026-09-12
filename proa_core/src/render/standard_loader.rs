// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `StandardDataLoader`: in-memory cache + TTL + DashMap concurrency.
//!
//! Wraps any user-provided upstream loader and adds a DashMap-backed cache
//! keyed by [`LoadKey`]. Each cached entry has its own expiry computed from
//! the request's [`crate::render::LoadHints`] cache TTL (if set) or the
//! loader's `default_ttl`. Shape-agnostic — the loader stores
//! [`crate::render::LoadValue`] and leaves bytes conversion, JSON
//! deserialization, or typed downcasting to `WebContext`.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

use crate::render::{
    CacheMode, DataLoader, DynDataLoader, LoadError, LoadKey, LoadRequest, LoadValue,
};

/// Cached entry with expiry.
struct CachedEntry {
    value: LoadValue,
    expires_at: Instant,
}

/// In-memory caching `DataLoader` with TTL eviction.
///
/// Wraps any upstream `DataLoader` and adds a DashMap-backed cache keyed by
/// [`LoadKey`]. Each cached entry has its own expiry computed from the
/// request's cache TTL hint (if set) or the loader's `default_ttl`.
///
/// Concurrent requests for the same key currently each trigger an upstream
/// fetch on cache miss (no in-flight dedupe — see `DataResolver` for
/// request-local single-flight). This is intentional: app-global dedupe
/// across requests would need a different design.
pub struct StandardDataLoader {
    cache: Arc<DashMap<LoadKey, CachedEntry>>,
    upstream: Arc<dyn DynDataLoader>,
    default_ttl: Duration,
}

impl StandardDataLoader {
    /// Wrap a concrete upstream loader with the standard cache.
    pub fn new<L: DataLoader + 'static>(upstream: L, default_ttl: Duration) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            upstream: Arc::new(upstream),
            default_ttl,
        }
    }

    /// Wrap an already-erased upstream loader with the standard cache.
    pub fn from_arc(upstream: Arc<dyn DynDataLoader>, default_ttl: Duration) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            upstream,
            default_ttl,
        }
    }

    /// Drop a single cached entry (manual invalidation).
    pub fn invalidate(&self, key: &LoadKey) {
        self.cache.remove(key);
    }

    /// Drop all cached entries.
    pub fn clear(&self) {
        self.cache.clear();
    }

    /// Number of currently-cached entries (may include expired ones not yet evicted).
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns true if the cache currently holds no entries.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl DataLoader for StandardDataLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let key = request.key().clone();
        let request_owned = request.clone();
        let upstream = Arc::clone(&self.upstream);
        let cache = Arc::clone(&self.cache);
        let default_ttl = self.default_ttl;

        async move {
            let now = Instant::now();
            let cache_mode = request_owned
                .hints()
                .cache()
                .map(|cache| cache.mode())
                .unwrap_or(CacheMode::UseLoaderDefault);

            if !matches!(cache_mode, CacheMode::Bypass | CacheMode::Refresh) {
                if let Some(entry) = cache.get(&key) {
                    if entry.expires_at > now {
                        return Ok(entry.value.clone());
                    }
                }
            }

            if matches!(cache_mode, CacheMode::OnlyIfCached) {
                return Err(LoadError::CacheMiss);
            }

            let value = upstream.load_boxed(&request_owned).await?;

            if matches!(cache_mode, CacheMode::Bypass) {
                return Ok(value);
            }

            let ttl = request_owned
                .hints()
                .cache()
                .and_then(|c| c.ttl_hint())
                .unwrap_or(default_ttl);
            cache.insert(
                key,
                CachedEntry {
                    value: value.clone(),
                    expires_at: Instant::now() + ttl,
                },
            );
            Ok(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bytes::Bytes;

    use super::*;
    use crate::render::{CacheHint, CacheMode, LoadHints};

    #[derive(Debug)]
    struct Bundle {
        id: usize,
    }

    struct CountingTypedLoader {
        calls: Arc<AtomicUsize>,
    }

    impl DataLoader for CountingTypedLoader {
        fn load(
            &self,
            _request: &LoadRequest,
        ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
            let calls = Arc::clone(&self.calls);
            async move {
                let id = calls.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(LoadValue::typed(Bundle { id }))
            }
        }
    }

    struct CountingBytesLoader {
        calls: Arc<AtomicUsize>,
    }

    impl DataLoader for CountingBytesLoader {
        fn load(
            &self,
            _request: &LoadRequest,
        ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
            let calls = Arc::clone(&self.calls);
            async move {
                let id = calls.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(LoadValue::bytes(Bytes::from(format!("value-{id}"))))
            }
        }
    }

    #[tokio::test]
    async fn typed_cache_hit_clones_same_arc() {
        let calls = Arc::new(AtomicUsize::new(0));
        let loader = StandardDataLoader::new(
            CountingTypedLoader {
                calls: Arc::clone(&calls),
            },
            Duration::from_secs(60),
        );
        let key = LoadKey::named("bundle", "home");

        let first = loader
            .load_key(&key)
            .await
            .unwrap()
            .downcast::<Bundle>()
            .unwrap();
        let second = loader
            .load_key(&key)
            .await
            .unwrap()
            .downcast::<Bundle>()
            .unwrap();

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.id, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn only_if_cached_returns_cache_miss_without_entry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let loader = StandardDataLoader::new(
            CountingBytesLoader {
                calls: Arc::clone(&calls),
            },
            Duration::from_secs(60),
        );
        let request = LoadRequest::with_hints(
            LoadKey::named("bytes", "missing"),
            LoadHints::new().with_cache(CacheHint::new().with_mode(CacheMode::OnlyIfCached)),
        );

        assert_eq!(
            loader.load(&request).await.unwrap_err(),
            LoadError::CacheMiss
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn bypass_does_not_populate_cache() {
        let calls = Arc::new(AtomicUsize::new(0));
        let loader = StandardDataLoader::new(
            CountingBytesLoader {
                calls: Arc::clone(&calls),
            },
            Duration::from_secs(60),
        );
        let key = LoadKey::named("bytes", "bypass");
        let bypass = LoadRequest::with_hints(
            key.clone(),
            LoadHints::new().with_cache(CacheHint::new().with_mode(CacheMode::Bypass)),
        );

        assert_eq!(
            loader.load(&bypass).await.unwrap().into_bytes().unwrap(),
            Bytes::from_static(b"value-1")
        );
        assert_eq!(
            loader.load_key(&key).await.unwrap().into_bytes().unwrap(),
            Bytes::from_static(b"value-2")
        );
        assert_eq!(
            loader.load_key(&key).await.unwrap().into_bytes().unwrap(),
            Bytes::from_static(b"value-2")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
