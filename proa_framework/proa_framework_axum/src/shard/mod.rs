// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Caching for Proa framework
//!
//! This module provides LRU caching for SSR-rendered components.

mod cache;

pub use cache::{ShardCache, ShardCacheConfig};
