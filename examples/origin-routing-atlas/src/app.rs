// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use crate::loader::AtlasLoader;

/// Shared application state: one RIPEstat-backed loader for the process.
#[derive(Clone)]
pub struct AppState {
    pub loader: Arc<AtlasLoader>,
}
