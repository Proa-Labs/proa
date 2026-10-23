// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::metadata::{ComputedRouteMeta, MetadataRef, TitleOnly};

/// Function type for generating static paths at build time.
pub type StaticPathsFn = fn() -> Vec<HashMap<&'static str, String>>;

/// Routing mode for a route.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteMode {
    /// Static Site Generation (SSG) - pre-rendered at build time.
    Static,
    /// Server-Side Rendering (SSR) - rendered per request.
    Server,
    /// Client-Side Rendering (CSR) - client router handles navigation.
    #[default]
    ClientBrowser,
}

/// Portable route tree description with no server/router dependency.
#[derive(Clone)]
pub struct RouteSpec {
    pub path: &'static str,
    pub has_endpoint: bool,
    pub layout_metadata: Option<MetadataRef>,
    pub metadata: Option<MetadataRef>,
    pub children: Vec<RouteSpec>,
    pub route_mode: RouteMode,
    pub static_paths: Option<StaticPathsFn>,
}

/// A fully-resolved route node with concrete full path and metadata stack.
#[derive(Clone)]
pub struct ResolvedRouteSpec {
    pub path: &'static str,
    pub full_path: String,
    pub has_endpoint: bool,
    pub metadata_stack: Vec<MetadataRef>,
    pub route_mode: RouteMode,
    pub static_paths: Option<StaticPathsFn>,
    pub children: Vec<ResolvedRouteSpec>,
}

impl RouteSpec {
    pub fn endpoint(path: &'static str) -> Self {
        Self {
            path,
            has_endpoint: true,
            layout_metadata: None,
            metadata: None,
            children: Vec::new(),
            route_mode: RouteMode::default(),
            static_paths: None,
        }
    }

    pub fn page(path: &'static str, title: &'static str) -> Self {
        Self {
            metadata: Some(Box::leak(Box::new(TitleOnly::new(title)))),
            ..Self::endpoint(path)
        }
    }

    pub fn page_with_metadata(path: &'static str, metadata: MetadataRef) -> Self {
        Self {
            metadata: Some(metadata),
            ..Self::endpoint(path)
        }
    }

    pub fn layout(
        path: &'static str,
        layout_metadata: MetadataRef,
        children: Vec<RouteSpec>,
    ) -> Self {
        Self {
            path,
            has_endpoint: false,
            layout_metadata: Some(layout_metadata),
            metadata: None,
            children,
            route_mode: RouteMode::default(),
            static_paths: None,
        }
    }

    pub fn with_children(mut self, children: Vec<RouteSpec>) -> Self {
        self.children = children;
        self
    }

    pub fn with_metadata(mut self, metadata: MetadataRef) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn with_layout_metadata(mut self, layout_metadata: MetadataRef) -> Self {
        self.layout_metadata = Some(layout_metadata);
        self
    }

    pub fn with_route_mode(mut self, route_mode: RouteMode) -> Self {
        self.route_mode = route_mode;
        self
    }

    pub fn with_static_paths(mut self, paths_fn: StaticPathsFn) -> Self {
        self.static_paths = Some(paths_fn);
        self
    }
}

pub fn compute_metadata_map(routes: &[RouteSpec]) -> HashMap<String, ComputedRouteMeta> {
    let mut computed_metadata_map = HashMap::new();
    for route in routes {
        compute_metadata(
            route,
            &[],
            &ComputedRouteMeta::default(),
            &mut computed_metadata_map,
        );
    }
    computed_metadata_map
}

pub fn resolve_route_specs(routes: &[RouteSpec]) -> Vec<ResolvedRouteSpec> {
    routes
        .iter()
        .map(|route| resolve_route_spec(route, &[], &[]))
        .collect()
}

pub fn resolve_route_path(parent_segments: &[String], path: &str) -> (Vec<String>, String) {
    let mut segments = if path.starts_with('/') {
        split_route_segments(path)
    } else {
        let mut merged = parent_segments.to_vec();
        if !path.is_empty() {
            merged.extend(split_route_segments(path));
        }
        merged
    };

    if path.starts_with('/') && path.trim_matches('/').is_empty() {
        segments.clear();
    }

    let full_path = if segments.is_empty() {
        "/".to_string()
    } else {
        let mut path_buf = String::with_capacity(
            segments
                .iter()
                .map(|segment| segment.len() + 1)
                .sum::<usize>()
                + 1,
        );
        path_buf.push('/');
        path_buf.push_str(&segments.join("/"));
        path_buf
    };

    (segments, full_path)
}

fn resolve_route_spec(
    route: &RouteSpec,
    parent_segments: &[String],
    parent_metadata: &[MetadataRef],
) -> ResolvedRouteSpec {
    let (segments, full_path) = resolve_route_path(parent_segments, route.path);

    let mut metadata_stack = parent_metadata.to_vec();
    if let Some(layout_metadata) = route.layout_metadata {
        metadata_stack.push(layout_metadata);
    }
    if let Some(metadata) = route.metadata {
        metadata_stack.push(metadata);
    }

    let children = route
        .children
        .iter()
        .map(|child| resolve_route_spec(child, &segments, &metadata_stack))
        .collect();

    ResolvedRouteSpec {
        path: route.path,
        full_path,
        has_endpoint: route.has_endpoint,
        metadata_stack,
        route_mode: route.route_mode,
        static_paths: route.static_paths,
        children,
    }
}

fn compute_metadata(
    route: &RouteSpec,
    parent_segments: &[String],
    parent_computed: &ComputedRouteMeta,
    map: &mut HashMap<String, ComputedRouteMeta>,
) {
    let (segments, full_path) = resolve_route_path(parent_segments, route.path);

    let mut local_providers = Vec::new();
    if let Some(layout_metadata) = route.layout_metadata {
        local_providers.push(layout_metadata);
    }
    if let Some(metadata) = route.metadata {
        local_providers.push(metadata);
    }

    let computed = ComputedRouteMeta::cascade(parent_computed, &local_providers);

    if route.has_endpoint {
        map.insert(full_path.clone(), computed.clone());
    }

    for child in &route.children {
        compute_metadata(child, &segments, &computed, map);
    }
}

fn split_route_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{CascadeValue, Metadata};

    struct BaseMeta;
    impl Metadata for BaseMeta {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("%s | Base")
        }
    }

    struct PageMeta;
    impl Metadata for PageMeta {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("Dashboard")
        }
    }

    #[test]
    fn resolve_route_path_builds_nested_paths() {
        let parent = vec!["app".to_string()];
        let (_, path) = resolve_route_path(&parent, "dashboard");
        assert_eq!(path, "/app/dashboard");
    }

    #[test]
    fn compute_metadata_map_cascades_layout_metadata() {
        let routes = vec![RouteSpec::layout(
            "/",
            &BaseMeta,
            vec![RouteSpec::page_with_metadata("dashboard", &PageMeta)],
        )];
        let map = compute_metadata_map(&routes);
        assert_eq!(
            map.get("/dashboard").and_then(|meta| meta.title.as_deref()),
            Some("Dashboard | Base")
        );
    }

    #[test]
    fn resolve_route_specs_builds_full_paths_and_metadata_stack() {
        let routes = vec![RouteSpec::layout(
            "/",
            &BaseMeta,
            vec![RouteSpec::page_with_metadata("dashboard", &PageMeta)],
        )];
        let resolved = resolve_route_specs(&routes);
        let child = &resolved[0].children[0];
        assert_eq!(child.full_path, "/dashboard");
        assert_eq!(child.metadata_stack.len(), 2);
    }

    struct HomeLayoutMeta;
    impl Metadata for HomeLayoutMeta {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("Home")
        }

        fn open_graph(&self) -> crate::metadata::CascadeOpenGraph {
            crate::metadata::CascadeOpenGraph {
                title: CascadeValue::Set("%s on OG"),
                ..Default::default()
            }
        }

        fn twitter(&self) -> crate::metadata::CascadeTwitter {
            crate::metadata::CascadeTwitter {
                title: CascadeValue::Set("%s on X"),
                ..Default::default()
            }
        }
    }

    #[test]
    fn compute_metadata_map_fills_templates_with_each_route_title() {
        let routes = vec![RouteSpec::layout(
            "/",
            &HomeLayoutMeta,
            vec![RouteSpec::page_with_metadata("dashboard", &PageMeta)],
        )];
        let map = compute_metadata_map(&routes);

        // The layout's own cascade fills "Home on OG" / "Home on X"; the child
        // must refill from its own title instead of inheriting that value.
        let dashboard = map.get("/dashboard").expect("child route");
        assert_eq!(
            dashboard.open_graph.title.as_deref(),
            Some("Dashboard on OG")
        );
        assert_eq!(dashboard.twitter.title.as_deref(), Some("Dashboard on X"));
    }
}
