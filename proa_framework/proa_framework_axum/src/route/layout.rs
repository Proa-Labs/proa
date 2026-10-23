// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::metadata::{DynMetadata, LocalMetadata, LocalMetadataProvider};

pub type LayoutRef = &'static dyn Layout;

/// Metadata boundary shared by a branch of the route tree.
///
/// Page chrome belongs in the page's render tree (usually a shared
/// `RootDocument` component), where it works uniformly for sync, buffered
/// async, and streaming routes. A route layout contributes only cascading
/// metadata such as title templates and descriptions.
pub trait Layout: DynMetadata + Send + Sync + 'static {}

impl<T: LocalMetadata> Layout for LocalMetadataProvider<T> {}
