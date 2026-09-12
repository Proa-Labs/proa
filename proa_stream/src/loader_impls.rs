// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;

use bytes::Bytes;
use proa_core::{DataLoader, LoadError, LoadRequest, LoadValue};

/// Closure-backed loader for framework and test integration.
pub struct AxumLoader<F> {
    handler: F,
}

impl<F> AxumLoader<F> {
    pub const fn new(handler: F) -> Self {
        Self { handler }
    }
}

impl<F, Fut> DataLoader for AxumLoader<F>
where
    F: Fn(LoadRequest) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Bytes, LoadError>> + Send + 'static,
{
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let fut = (self.handler)(request.clone());
        async move { fut.await.map(LoadValue::bytes) }
    }
}

/// FFI loader placeholder for the later mobile/native integration phase.
#[derive(Debug, Default, Clone, Copy)]
pub struct FfiLoader;

impl DataLoader for FfiLoader {
    fn load(
        &self,
        _request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        std::future::ready(Err(LoadError::custom("FFI loader is not implemented yet")))
    }
}
