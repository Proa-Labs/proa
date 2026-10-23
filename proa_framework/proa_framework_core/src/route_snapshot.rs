// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::mem;
use std::sync::Arc;

use http::Uri;
use proa_core::{RouteParam, RouteParams};

use crate::manifest::IslandManifest;
use crate::metadata::RouteSetupPlan;

/// Portable per-request route snapshot used during route finalization.
///
/// This type is runtime and server agnostic; adapter crates are responsible for
/// storing it in task-local state, request extensions, or whatever mechanism
/// their server stack uses.
#[derive(Clone)]
pub struct FrameworkRouteSnapshot {
    manifest: Arc<IslandManifest>,
    setup: RouteSetupPlan,
    route_setups: Arc<HashMap<String, RouteSetupPlan>>,
    current_uri: Option<Uri>,
    current_route_pattern: Option<Arc<str>>,
    current_route_params: RouteParams,
    is_client_request: bool,
}

impl FrameworkRouteSnapshot {
    pub fn new(
        manifest: Arc<IslandManifest>,
        route_setups: Arc<HashMap<String, RouteSetupPlan>>,
    ) -> Self {
        Self {
            manifest,
            setup: RouteSetupPlan::default(),
            route_setups,
            current_uri: None,
            current_route_pattern: None,
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

    pub fn set_current_route_pattern(&mut self, pattern: Arc<str>) {
        self.current_route_pattern = Some(pattern);
    }

    pub fn setup_plan(&self) -> RouteSetupPlan {
        self.current_route_pattern
            .as_deref()
            .or_else(|| self.current_path())
            .and_then(|path| self.route_setups.get(path))
            .cloned()
            .unwrap_or_else(|| self.setup.clone())
    }

    pub fn current_path(&self) -> Option<&str> {
        self.current_uri.as_ref().map(Uri::path)
    }

    pub fn current_uri(&self) -> Option<&Uri> {
        self.current_uri.as_ref()
    }

    pub fn get_route_setups(&self, path: &str) -> Option<RouteSetupPlan> {
        self.route_setups.get(path).cloned()
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

    pub fn manifest_arc(&self) -> Arc<IslandManifest> {
        self.manifest.clone()
    }

    pub fn swap_setup(&mut self, setup: RouteSetupPlan) -> RouteSetupPlan {
        mem::replace(&mut self.setup, setup)
    }
    pub fn restore_setup(&mut self, setup: RouteSetupPlan) {
        self.setup = setup;
    }
}

/// Extract route params by matching a declarative `:param` route pattern
/// against a concrete request path.
pub fn extract_route_params(route_pattern: &str, request_path: &str) -> RouteParams {
    let mut params = Vec::new();
    let mut pattern_segments = route_pattern
        .split('/')
        .filter(|segment| !segment.is_empty());
    let mut request_segments = request_path
        .split('/')
        .filter(|segment| !segment.is_empty());

    loop {
        match (pattern_segments.next(), request_segments.next()) {
            (None, None) => break,
            (Some(pattern_segment), Some(request_segment)) => {
                if let Some(name) = pattern_segment.strip_prefix(':') {
                    params.push(RouteParam::new(
                        name.to_owned().into_boxed_str(),
                        request_segment.to_owned().into_boxed_str(),
                    ));
                } else if pattern_segment != request_segment {
                    return RouteParams::default();
                }
            }
            _ => return RouteParams::default(),
        }
    }

    RouteParams::from_vec(params)
}

#[cfg(test)]
mod tests {
    use super::extract_route_params;

    #[test]
    fn extract_route_params_matches_named_segments() {
        let params =
            extract_route_params("/products/:id/reviews/:slug", "/products/42/reviews/top");
        assert_eq!(params.get("id"), Some("42"));
        assert_eq!(params.get("slug"), Some("top"));
    }

    #[test]
    fn extract_route_params_returns_empty_for_non_match() {
        let params = extract_route_params("/products/:id", "/categories/42");
        assert!(params.is_empty());
    }
}
