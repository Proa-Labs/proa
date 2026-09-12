// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;

use crate::render::{LoadError, LoadKey, LoadRequest, LoadValue};

/// Boxed-future alias used for dyn-compatible storage of loader futures.
///
/// User loaders return `impl Future + Send` directly. This alias only appears
/// at the `DynDataLoader` boundary where type-erased storage (`Arc<dyn ...>`)
/// is required, e.g. inside framework wrappers like `DataResolver`.
pub type LoadFuture<'a> = Pin<Box<dyn Future<Output = Result<LoadValue, LoadError>> + Send + 'a>>;

/// Request-scoped data loader used by `WebContext::load`.
///
/// This trait is the application-defined extension point for origin fetches and
/// cache policy. A custom loader may own any backing strategy it wants:
/// in-memory caches, Redis, disk, stale-while-revalidate, negative caching, and
/// custom invalidation.
///
/// `load` returns `impl Future + Send` so concrete loader calls monomorphize
/// (no per-call `Box::pin`). For type-erased storage, see [`DynDataLoader`],
/// which is automatically implemented for every `DataLoader`.
///
/// Framework-owned wrappers such as Proa's request-local `DataResolver` and the
/// in-order `PreparedLoader` may sit on top of a user loader, but those wrappers
/// should stay narrowly focused on request-local coordination. They are not the
/// place to impose global cache policy.
pub trait DataLoader: Send + Sync {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send;

    fn load_key(&self, key: &LoadKey) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let request = LoadRequest::from(key.clone());
        async move { self.load(&request).await }
    }

    fn load_bytes(&self, key: &LoadKey) -> impl Future<Output = Result<Bytes, LoadError>> + Send {
        async move { self.load_key(key).await?.into_bytes() }
    }
}

/// Object-safe shape for `Arc<dyn DynDataLoader>` storage.
///
/// Auto-implemented for every `DataLoader` via the blanket impl below. Use
/// this when a wrapper needs to hold a heterogenous collection of loaders or
/// otherwise erase the concrete loader type at runtime. The cost is one
/// `Box::pin` per call; compare with calling a concrete `DataLoader` directly
/// which monomorphizes and avoids the allocation.
pub trait DynDataLoader: Send + Sync {
    fn load_boxed<'a>(&'a self, request: &'a LoadRequest) -> LoadFuture<'a>;
}

impl<T: DataLoader + ?Sized> DynDataLoader for T {
    #[inline]
    fn load_boxed<'a>(&'a self, request: &'a LoadRequest) -> LoadFuture<'a> {
        Box::pin(self.load(request))
    }
}

/// Concrete `DataLoader` that wraps an `Arc<dyn DynDataLoader>`.
///
/// This is the bridge type used when the framework needs to install a
/// type-erased loader into a `WebContext`. The context is always generic over
/// a concrete `L: DataLoader`, so `DynLoaderHandle` provides a concrete
/// loader type that internally dispatches through the dyn boundary.
///
/// User app code that wants to monomorphize fully should use a concrete
/// loader type (e.g. `StandardDataLoader`) directly. `DynLoaderHandle` exists
/// for the framework integration layer where the user's loader type is not
/// statically known.
#[derive(Clone)]
pub struct DynLoaderHandle {
    inner: std::sync::Arc<dyn DynDataLoader>,
}

impl DynLoaderHandle {
    pub fn new(inner: std::sync::Arc<dyn DynDataLoader>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &std::sync::Arc<dyn DynDataLoader> {
        &self.inner
    }
}

impl DataLoader for DynLoaderHandle {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let inner = std::sync::Arc::clone(&self.inner);
        let request_owned = request.clone();
        async move { inner.load_boxed(&request_owned).await }
    }
}

/// Zero-sized "no loader configured" sentinel.
///
/// Defaults the `L` generic on `WebContext`, `WebRenderSync`, and `WebRender`
/// so existing call sites compile unchanged. Calling `load` on this loader
/// always fails with [`LoadError::NoLoader`].
#[derive(Debug, Default, Clone, Copy)]
pub struct NoLoader;

impl DataLoader for NoLoader {
    fn load(
        &self,
        _request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        std::future::ready(Err(LoadError::NoLoader))
    }
}
