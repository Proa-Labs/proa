// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Public assets are finalized after successful Cargo compilation, without
//! compiling their URLs back into Rust or extracting embedded JavaScript.

pub use proa_assets::build::{CleanupReport, OutputStore, PublishReport};
pub use proa_assets::{AssetCatalog, AssetError, AssetManifest, MANIFEST_FILE, OUTPUT_ENV};
