// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use futures::future::{BoxFuture, FutureExt, Shared};
use proa_core::{DataLoader, DynDataLoader, LoadError, LoadKey, LoadRequest, LoadValue};
use tracing::trace;

type SharedLoad = Shared<BoxFuture<'static, Result<LoadValue, LoadError>>>;

/// Request-scoped loader wrapper that memoizes and deduplicates loads by
/// `LoadKey` for the lifetime of a single render/request.
///
/// This is intentionally not a replacement for application cache policy. A
/// custom `DataLoader` can still own arbitrarily large cross-request caches;
/// `DataResolver` only ensures that one request does not repeat the same load
/// work over and over again. It keys this memoization by `LoadKey`; hints are
/// treated as advisory policy, not payload identity.
///
/// Generic over the wrapped loader type `L`. Use [`DataResolver::new_typed`]
/// to wrap a concrete `Arc<L>` (which keeps the inner loader's
/// `cx.load(...)` calls monomorphized), or [`DataResolver::new_dyn`] /
/// [`DataResolver::new`] to wrap a `dyn DynDataLoader` (the type-erased
/// path used when the framework only has a runtime-erased loader).
pub struct DataResolver<L: ?Sized = dyn DynDataLoader> {
    inner: Arc<L>,
    inflight: Arc<Mutex<HashMap<LoadKey, SharedLoad>>>,
    resolved: Arc<Mutex<HashMap<LoadKey, Result<LoadValue, LoadError>>>>,
}

impl<L: ?Sized> DataResolver<L> {
    fn shared_load_with<F>(&self, key: LoadKey, miss: F) -> SharedLoad
    where
        F: FnOnce() -> BoxFuture<'static, Result<LoadValue, LoadError>>,
    {
        if let Some(result) = self
            .resolved
            .lock()
            .expect("data resolver resolved mutex poisoned")
            .get(&key)
            .cloned()
        {
            trace!(load_key = %key, "data resolver resolved hit");
            return futures::future::ready(result).boxed().shared();
        }

        let mut inflight = self
            .inflight
            .lock()
            .expect("data resolver inflight mutex poisoned");

        if let Some(shared) = inflight.get(&key) {
            trace!(load_key = %key, "data resolver inflight hit");
            return shared.clone();
        }

        trace!(load_key = %key, "data resolver miss");

        let load_fut = miss();
        let inflight_map = Arc::downgrade(&self.inflight);
        let resolved_map = Arc::downgrade(&self.resolved);
        let future_key = key.clone();
        let shared = async move {
            let result = load_fut.await;
            // The map owns this shared future. Weak backlinks prevent a
            // request -> map -> pending future -> map ownership cycle.
            if let Some(resolved_map) = resolved_map.upgrade() {
                resolved_map
                    .lock()
                    .expect("data resolver resolved mutex poisoned")
                    .insert(future_key.clone(), result.clone());
            }
            if let Some(inflight_map) = inflight_map.upgrade() {
                inflight_map
                    .lock()
                    .expect("data resolver inflight mutex poisoned")
                    .remove(&future_key);
            }
            result
        }
        .boxed()
        .shared();

        inflight.insert(key, shared.clone());
        shared
    }
}

impl DataResolver<dyn DynDataLoader> {
    /// Construct a resolver around a type-erased loader handle.
    pub fn new(inner: Arc<dyn DynDataLoader>) -> Self {
        Self {
            inner,
            inflight: Arc::new(Mutex::new(HashMap::new())),
            resolved: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Alias for [`new`](Self::new) that documents the type-erased intent
    /// at the call site.
    pub fn new_dyn(inner: Arc<dyn DynDataLoader>) -> Self {
        Self::new(inner)
    }
}

impl<L: DataLoader + 'static> DataResolver<L> {
    /// Construct a fresh request-scoped resolver around an application-wide
    /// typed loader.
    ///
    /// This is the preferred application-facing spelling. It is equivalent to
    /// [`new_typed`](Self::new_typed), but makes the cache lifetime explicit at
    /// the call site: callers must invoke it once per render/request rather
    /// than storing the resolver in application state.
    pub fn request_scope(inner: Arc<L>) -> Arc<Self> {
        Arc::new(Self::new_typed(inner))
    }

    /// Construct a resolver around a concrete typed loader.
    ///
    /// The wrapped loader's `load(...)` is called directly (no `Box::pin`),
    /// so the inner load path stays monomorphized on `L`.
    pub fn new_typed(inner: Arc<L>) -> Self {
        Self {
            inner,
            inflight: Arc::new(Mutex::new(HashMap::new())),
            resolved: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl DataLoader for DataResolver<dyn DynDataLoader> {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let inner = Arc::clone(&self.inner);
        let request_for_miss = request.clone();
        let shared = self.shared_load_with(request.key().clone(), || {
            async move { inner.load_boxed(&request_for_miss).await }.boxed()
        });
        shared
    }
}

impl<L: DataLoader + 'static> DataLoader for DataResolver<L> {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let inner = Arc::clone(&self.inner);
        let request_for_miss = request.clone();
        let shared = self.shared_load_with(request.key().clone(), || {
            async move { inner.load(&request_for_miss).await }.boxed()
        });
        shared
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;
    use proa_core::{CacheHint, CacheMode, LoadHints};

    struct CountingLoader {
        calls: Arc<AtomicUsize>,
        fail: bool,
    }

    impl DataLoader for CountingLoader {
        fn load(
            &self,
            _request: &LoadRequest,
        ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
            let calls = Arc::clone(&self.calls);
            let fail = self.fail;
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                if fail {
                    Err(LoadError::custom("boom"))
                } else {
                    Ok(LoadValue::bytes(bytes::Bytes::from_static(b"null")))
                }
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_duplicate_keys_share_one_underlying_load() {
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = DataResolver::new(Arc::new(CountingLoader {
            calls: Arc::clone(&calls),
            fail: false,
        }));
        let key = LoadKey::named("panel", "shared");

        let (left, right) = tokio::join!(resolver.load_key(&key), resolver.load_key(&key));
        assert_eq!(
            left.unwrap().into_bytes().unwrap(),
            bytes::Bytes::from_static(b"null")
        );
        assert_eq!(
            right.unwrap().into_bytes().unwrap(),
            bytes::Bytes::from_static(b"null")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sequential_duplicate_keys_share_one_underlying_load() {
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = DataResolver::new(Arc::new(CountingLoader {
            calls: Arc::clone(&calls),
            fail: false,
        }));
        let key = LoadKey::named("panel", "shared");

        assert_eq!(
            resolver.load_key(&key).await.unwrap().into_bytes().unwrap(),
            bytes::Bytes::from_static(b"null")
        );
        assert_eq!(
            resolver.load_key(&key).await.unwrap().into_bytes().unwrap(),
            bytes::Bytes::from_static(b"null")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn request_scope_is_fresh_for_each_request() {
        let calls = Arc::new(AtomicUsize::new(0));
        let loader = Arc::new(CountingLoader {
            calls: Arc::clone(&calls),
            fail: false,
        });
        let key = LoadKey::named("panel", "shared");

        let first = DataResolver::request_scope(Arc::clone(&loader));
        first.load_key(&key).await.unwrap();
        first.load_key(&key).await.unwrap();

        let second = DataResolver::request_scope(loader);
        second.load_key(&key).await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_duplicate_key_errors_are_shared() {
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver = DataResolver::new(Arc::new(CountingLoader {
            calls: Arc::clone(&calls),
            fail: true,
        }));
        let key = LoadKey::named("panel", "shared");

        let (left, right) = tokio::join!(resolver.load_key(&key), resolver.load_key(&key));
        assert_eq!(left.unwrap_err(), LoadError::custom("boom"));
        assert_eq!(right.unwrap_err(), LoadError::custom("boom"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    struct HintLoader {
        seen: Arc<Mutex<Vec<LoadRequest>>>,
    }

    impl DataLoader for HintLoader {
        fn load(
            &self,
            request: &LoadRequest,
        ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
            let seen = Arc::clone(&self.seen);
            let request = request.clone();
            async move {
                seen.lock()
                    .expect("hint loader mutex poisoned")
                    .push(request);
                Ok(LoadValue::bytes(bytes::Bytes::from_static(b"null")))
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resolver_forwards_full_request_on_miss() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let resolver = DataResolver::new_typed(Arc::new(HintLoader {
            seen: Arc::clone(&seen),
        }));
        let request = LoadRequest::with_hints(
            LoadKey::named("panel", "hinted"),
            LoadHints::new()
                .with_cache(CacheHint::ttl(Duration::from_secs(5)).with_mode(CacheMode::Refresh)),
        );

        resolver.load(&request).await.unwrap();

        let seen = seen.lock().expect("hint loader mutex poisoned");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0], request);
    }
}
