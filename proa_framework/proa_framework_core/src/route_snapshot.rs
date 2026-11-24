// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::mem;
use std::sync::Arc;

use http::Uri;
use proa_core::{RouteParam, RouteParams};

use crate::metadata::{resolve_metadata_stack, ComputedRouteMeta, MetadataRef, ResolvedMetadata};

/// Portable per-request route snapshot used during route finalization.
///
/// This type is runtime and server agnostic; adapter crates are responsible for
/// storing it in task-local state, request extensions, or whatever mechanism
/// their server stack uses.
#[derive(Clone)]
pub struct FrameworkRouteSnapshot {
    metadata: Option<Arc<[MetadataRef]>>,
    resolved_metadata: Option<Box<ResolvedMetadata>>,
    computed_metadata: Arc<HashMap<String, ComputedRouteMeta>>,
    current_uri: Option<Uri>,
    current_route_params: RouteParams,
    is_client_request: bool,
}

impl FrameworkRouteSnapshot {
    pub fn new(computed_metadata: Arc<HashMap<String, ComputedRouteMeta>>) -> Self {
        Self {
            metadata: None,
            resolved_metadata: None,
            computed_metadata,
            current_uri: None,
            current_route_params: RouteParams::default(),
            is_client_request: false,
        }
    }

    pub fn set_is_client_request(&mut self, is_client: bool) {
        self.is_client_request = is_client;
    }

    pub fn is_client_request(&self) -> bool {
        self.is_client_request
    }

    pub fn set_current_uri(&mut self, uri: Uri) {
        self.current_uri = Some(uri);
    }

    pub fn current_path(&self) -> Option<&str> {
        self.current_uri.as_ref().map(Uri::path)
    }

    pub fn current_uri(&self) -> Option<&Uri> {
        self.current_uri.as_ref()
    }

    pub fn get_computed_metadata(&self, path: &str) -> Option<ComputedRouteMeta> {
        self.computed_metadata.get(path).cloned()
    }

    pub fn current_route_params(&self) -> &RouteParams {
        &self.current_route_params
    }

    pub fn swap_route_params(&mut self, route_params: RouteParams) -> RouteParams {
        mem::replace(&mut self.current_route_params, route_params)
    }

    pub fn restore_route_params(&mut self, route_params: RouteParams) {
        self.current_route_params = route_params;
    }

    /// Whether there are endpoint metadata providers in the current scope.
    pub fn has_metadata(&self) -> bool {
        self.metadata
            .as_ref()
            .is_some_and(|metadata| !metadata.is_empty())
    }

    pub fn swap_metadata(
        &mut self,
        metadata: Arc<[MetadataRef]>,
    ) -> (Arc<[MetadataRef]>, Option<ResolvedMetadata>) {
        let previous_metadata = self.metadata.replace(metadata).unwrap_or_default();
        let previous_resolved = self.resolved_metadata.take().map(|metadata| *metadata);
        (previous_metadata, previous_resolved)
    }

    pub fn restore_metadata(
        &mut self,
        metadata: Arc<[MetadataRef]>,
        resolved: Option<ResolvedMetadata>,
    ) {
        self.metadata = Some(metadata);
        self.resolved_metadata = resolved.map(Box::new);
    }

    pub fn resolve_metadata(&mut self) -> Option<ResolvedMetadata> {
        let providers = self.metadata.as_deref()?;
        if providers.is_empty() {
            return None;
        }
        if self.resolved_metadata.is_none() {
            self.resolved_metadata = resolve_metadata_stack(providers).map(Box::new);
        }
        self.resolved_metadata.as_deref().cloned()
    }
}

/// Extract route params by matching a declarative `{param}` route pattern
/// against a concrete request path.
pub fn extract_route_params(route_pattern: &str, request_path: &str) -> RouteParams {
    let mut params = Vec::new();
    if visit_route_params(route_pattern, request_path, |name, value| {
        params.push(RouteParam::new(name, value));
    }) {
        RouteParams::from_vec(params)
    } else {
        RouteParams::default()
    }
}

/// Whether the pattern matches this path and captures at least one named segment.
pub fn has_named_route_params(route_pattern: &str, request_path: &str) -> bool {
    let mut found = false;
    visit_route_params(route_pattern, request_path, |_, _| found = true) && found
}

fn visit_route_params(
    route_pattern: &str,
    request_path: &str,
    mut visit: impl FnMut(&str, &str),
) -> bool {
    let mut pattern_segments = route_pattern
        .split('/')
        .filter(|segment| !segment.is_empty());
    let mut request_segments = request_path
        .split('/')
        .filter(|segment| !segment.is_empty());

    loop {
        match (pattern_segments.next(), request_segments.next()) {
            (None, None) => return true,
            (Some(pattern_segment), Some(request_segment)) => {
                let name = pattern_segment
                    .strip_prefix('{')
                    .and_then(|s| s.strip_suffix('}'))
                    .filter(|s| !s.is_empty() && !s.contains(['{', '}', '*']))
                    .or_else(|| pattern_segment.strip_prefix(':'));
                if let Some(name) = name {
                    visit(name, request_segment);
                } else if pattern_segment != request_segment || pattern_segment.contains(['{', '}'])
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::extract_route_params;

    #[test]
    fn extract_route_params_matches_named_segments() {
        let params =
            extract_route_params("/products/{id}/reviews/{slug}", "/products/42/reviews/top");
        assert_eq!(params.get("id"), Some("42"));
        assert_eq!(params.get("slug"), Some("top"));
    }

    #[test]
    fn extract_route_params_returns_empty_for_non_match() {
        let params = extract_route_params("/products/{id}", "/categories/42");
        assert!(params.is_empty());
    }
}
