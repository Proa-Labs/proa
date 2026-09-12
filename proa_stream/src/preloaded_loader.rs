// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use futures::future::{BoxFuture, FutureExt, Shared};
use proa_core::{DataLoader, DynDataLoader, LoadError, LoadKey, LoadRequest, LoadValue};

type SharedLoad = Shared<BoxFuture<'static, Result<LoadValue, LoadError>>>;

/// Starts one prepared load and shares it across later `load` calls.
///
/// This is the only runtime seam in `proa_stream`. With the `rt-tokio`
/// feature (the default) the load is spawned onto the ambient tokio runtime
/// at construction time, so prepared loads run eagerly and in parallel with
/// the render that follows. Without the feature the future is stored
/// unspawned: it starts on first await and overlaps with other loads only
/// through cooperative polling, for example when suspense boundaries await
/// different keys concurrently or when callers `join!` prepared keys.
///
/// Mode differences beyond eagerness, all user-visible:
/// - Panics: with `rt-tokio` a panicking load is caught at the task join
///   and surfaces as `Err(LoadError::Dropped)`; without it the panic
///   propagates into the first awaiter and poisons the shared future for
///   every other awaiter.
/// - Drop: with `rt-tokio` the spawned load is detached and runs to
///   completion, side effects included, even if the `PreparedLoader` and
///   all awaiters are dropped; without it un-awaited loads never start and
///   dropping every handle cancels an in-flight load.
/// - Context: with `rt-tokio`, constructing a `PreparedLoader` outside a
///   tokio runtime panics in `tokio::spawn`; without it construction is
///   runtime-free.
fn start_prepared(load: BoxFuture<'static, Result<LoadValue, LoadError>>) -> SharedLoad {
    #[cfg(feature = "rt-tokio")]
    {
        let task = tokio::spawn(load);
        async move { task.await.unwrap_or(Err(LoadError::Dropped)) }
            .boxed()
            .shared()
    }
    #[cfg(not(feature = "rt-tokio"))]
    {
        load.shared()
    }
}

/// Loader wrapper that shares known `LoadKey`s across later `cx.load(...)`
/// calls, deduplicating concurrent interest in the same key.
///
/// With the `rt-tokio` feature (the default), prepared loads start eagerly
/// at construction, before render begins. Without it they start lazily on
/// the first await; see `start_prepared` for the full list of semantic
/// differences between the modes.
///
/// This is the in-order pre-send layer. In the normal framework stack it sits
/// on top of request-local `DataResolver`, which itself wraps the application
/// loader that owns origin fetches and cache policy.
///
/// Generic over the wrapped loader type `L`. Use [`PreparedLoader::new_typed`]
/// to wrap a concrete typed loader; use [`PreparedLoader::new`] for the
/// type-erased dyn path.
pub struct PreparedLoader<L: ?Sized = dyn DynDataLoader> {
    inner: Arc<L>,
    prepared: HashMap<LoadKey, SharedLoad>,
}

impl PreparedLoader<dyn DynDataLoader> {
    pub fn new(inner: Arc<dyn DynDataLoader>, keys: impl IntoIterator<Item = LoadKey>) -> Self {
        Self::new_requests(inner, keys.into_iter().map(LoadRequest::from))
    }

    pub fn new_requests(
        inner: Arc<dyn DynDataLoader>,
        requests: impl IntoIterator<Item = LoadRequest>,
    ) -> Self {
        let mut prepared = HashMap::new();

        for request in requests {
            let key = request.key().clone();
            prepared.entry(key.clone()).or_insert_with(|| {
                let loader = Arc::clone(&inner);
                let load_request = request.clone();
                start_prepared(async move { loader.load_boxed(&load_request).await }.boxed())
            });
        }

        Self { inner, prepared }
    }
}

impl<L: DataLoader + 'static> PreparedLoader<L> {
    pub fn new_typed(inner: Arc<L>, keys: impl IntoIterator<Item = LoadKey>) -> Self {
        Self::new_typed_requests(inner, keys.into_iter().map(LoadRequest::from))
    }

    pub fn new_typed_requests(
        inner: Arc<L>,
        requests: impl IntoIterator<Item = LoadRequest>,
    ) -> Self {
        let mut prepared = HashMap::new();

        for request in requests {
            let key = request.key().clone();
            prepared.entry(key.clone()).or_insert_with(|| {
                let loader = Arc::clone(&inner);
                let load_request = request.clone();
                start_prepared(async move { loader.load(&load_request).await }.boxed())
            });
        }

        Self { inner, prepared }
    }
}

impl DataLoader for PreparedLoader<dyn DynDataLoader> {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let request_owned = request.clone();
        let prepared = self.prepared.get(request.key()).cloned();
        let inner = Arc::clone(&self.inner);
        async move {
            if let Some(shared) = prepared {
                shared.await
            } else {
                inner.load_boxed(&request_owned).await
            }
        }
    }
}

impl<L: DataLoader + 'static> DataLoader for PreparedLoader<L> {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let request_owned = request.clone();
        let prepared = self.prepared.get(request.key()).cloned();
        let inner = Arc::clone(&self.inner);
        async move {
            if let Some(shared) = prepared {
                shared.await
            } else {
                inner.load(&request_owned).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use proa_core::{CacheHint, CacheMode, LoadHints};

    #[derive(Default)]
    struct TestLoader {
        calls: Arc<AtomicUsize>,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
    }

    impl DataLoader for TestLoader {
        fn load(
            &self,
            request: &LoadRequest,
        ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
            let calls = Arc::clone(&self.calls);
            let in_flight = Arc::clone(&self.in_flight);
            let max_in_flight = Arc::clone(&self.max_in_flight);
            let delay = if request.key() == &LoadKey::named("panel", "slow") {
                Duration::from_millis(120)
            } else {
                Duration::from_millis(40)
            };

            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                let concurrent = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max_in_flight.fetch_max(concurrent, Ordering::SeqCst);
                tokio::time::sleep(delay).await;
                in_flight.fetch_sub(1, Ordering::SeqCst);
                Ok(LoadValue::bytes(bytes::Bytes::from_static(b"null")))
            }
        }
    }

    /// Eager start is the `rt-tokio` guarantee: both loads begin at
    /// construction, before anything awaits them, and duplicates dedupe.
    #[cfg(feature = "rt-tokio")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn prepared_loader_starts_known_keys_eagerly_and_once() {
        let test_loader = TestLoader::default();
        let calls = Arc::clone(&test_loader.calls);
        let loader: Arc<dyn DynDataLoader> = Arc::new(test_loader);
        let prepared = PreparedLoader::new(
            loader,
            [
                LoadKey::named("panel", "slow"),
                LoadKey::named("panel", "fast"),
                LoadKey::named("panel", "slow"),
            ],
        );

        // No prepared key has been awaited yet, so only eager spawning can
        // have started the loads.
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "prepared loads must start at construction and dedupe duplicates"
        );

        prepared
            .load_key(&LoadKey::named("panel", "slow"))
            .await
            .unwrap();
        prepared
            .load_key(&LoadKey::named("panel", "fast"))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// Holds in both feature modes: concurrently awaited prepared keys
    /// overlap (observed via the peak in-flight count rather than
    /// wall-clock timing) and duplicate keys dedupe to one load.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn prepared_loader_overlaps_when_awaited_concurrently() {
        let test_loader = TestLoader::default();
        let calls = Arc::clone(&test_loader.calls);
        let max_in_flight = Arc::clone(&test_loader.max_in_flight);
        let loader: Arc<dyn DynDataLoader> = Arc::new(test_loader);
        let prepared = PreparedLoader::new(
            loader,
            [
                LoadKey::named("panel", "slow"),
                LoadKey::named("panel", "fast"),
                LoadKey::named("panel", "slow"),
            ],
        );

        let slow = LoadKey::named("panel", "slow");
        let fast = LoadKey::named("panel", "fast");
        let (slow_res, fast_res) =
            futures::join!(prepared.load_key(&slow), prepared.load_key(&fast));
        slow_res.unwrap();
        fast_res.unwrap();

        assert_eq!(
            max_in_flight.load(Ordering::SeqCst),
            2,
            "concurrently awaited prepared loads must overlap, not serialize"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
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
    async fn prepared_loader_forwards_full_preload_request() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let request = LoadRequest::with_hints(
            LoadKey::named("panel", "hinted"),
            LoadHints::new()
                .with_cache(CacheHint::ttl(Duration::from_secs(5)).with_mode(CacheMode::Refresh)),
        );
        let loader: Arc<dyn DynDataLoader> = Arc::new(HintLoader {
            seen: Arc::clone(&seen),
        });
        let prepared = PreparedLoader::new_requests(loader, [request.clone()]);

        prepared.load(&request).await.unwrap();

        let seen = seen.lock().expect("hint loader mutex poisoned");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0], request);
    }
}
