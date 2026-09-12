// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use http::Uri;

/// Minimal request location metadata available during a render.
///
/// This stays intentionally narrow and transport-agnostic. It exposes the
/// request URI shape needed by route-aware components without pulling the full
/// framework request type into `proa_core`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLocation {
    uri: Uri,
}

impl RequestLocation {
    pub const fn new(uri: Uri) -> Self {
        Self { uri }
    }

    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    pub fn path(&self) -> &str {
        self.uri.path()
    }

    pub fn query(&self) -> Option<&str> {
        self.uri.query()
    }

    pub fn path_and_query(&self) -> Option<&http::uri::PathAndQuery> {
        self.uri.path_and_query()
    }
}

impl From<Uri> for RequestLocation {
    fn from(value: Uri) -> Self {
        Self::new(value)
    }
}
