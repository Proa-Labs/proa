// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::LoadKey;

/// Explicit request-shape discovery surface for `ChunkedInOrder` pre-send.
///
/// Implement this on route roots or helper page structs when the load keys are
/// knowable from already-selected request state. The collected keys can then be
/// passed to `RouteResponse::with_preload_keys(...)` so the in-order driver can
/// start them before render begins.
pub trait CollectLoadKeys {
    /// Append every known render-path key to `target`.
    fn collect_load_keys(&self, target: &mut Vec<LoadKey>);

    /// Convenience helper that materializes the collected keys.
    fn load_keys(&self) -> Vec<LoadKey> {
        let mut keys = Vec::new();
        self.collect_load_keys(&mut keys);
        keys
    }
}
