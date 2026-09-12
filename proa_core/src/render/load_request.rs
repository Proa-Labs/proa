// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use crate::render::LoadKey;

/// Full request sent to a [`crate::DataLoader`].
///
/// `LoadKey` remains the identity of the data being requested. `LoadHints`
/// carries optional advisory policy such as caching behavior without changing
/// that identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadRequest {
    key: LoadKey,
    hints: LoadHints,
}

impl LoadRequest {
    pub fn new(key: LoadKey) -> Self {
        Self {
            key,
            hints: LoadHints::default(),
        }
    }

    pub fn with_hints(key: LoadKey, hints: LoadHints) -> Self {
        Self { key, hints }
    }

    pub fn key(&self) -> &LoadKey {
        &self.key
    }

    pub fn hints(&self) -> &LoadHints {
        &self.hints
    }

    pub fn into_key(self) -> LoadKey {
        self.key
    }
}

impl From<LoadKey> for LoadRequest {
    fn from(value: LoadKey) -> Self {
        Self::new(value)
    }
}

/// Optional advisory policy for a load request.
///
/// These hints are intentionally separate from [`LoadKey`]. A custom
/// application `DataLoader` may choose to honor them, ignore them, or apply its
/// own policy based on namespace and key. They should not change the semantic
/// identity of the returned payload.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadHints {
    cache: Option<CacheHint>,
}

impl LoadHints {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cache(mut self, cache: CacheHint) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn cache(&self) -> Option<&CacheHint> {
        self.cache.as_ref()
    }
}

/// Advisory cache policy for a load request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheHint {
    ttl: Option<Duration>,
    mode: CacheMode,
}

impl CacheHint {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ttl(ttl: Duration) -> Self {
        Self::default().with_ttl(ttl)
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    pub fn with_mode(mut self, mode: CacheMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn ttl_hint(&self) -> Option<Duration> {
        self.ttl
    }

    pub fn mode(&self) -> CacheMode {
        self.mode
    }
}

impl Default for CacheHint {
    fn default() -> Self {
        Self {
            ttl: None,
            mode: CacheMode::UseLoaderDefault,
        }
    }
}

/// Advisory cache mode for a load request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheMode {
    /// Let the custom application loader choose its normal policy.
    #[default]
    UseLoaderDefault,
    /// Skip any loader-managed cache and refresh from origin.
    Bypass,
    /// Refresh the cache entry during this load.
    Refresh,
    /// Only satisfy the request from cache if present.
    OnlyIfCached,
}
