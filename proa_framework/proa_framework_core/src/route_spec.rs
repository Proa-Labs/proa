// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::metadata::{LayoutRef, RouteSetupPlan};

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
    pub layout: Option<LayoutRef>,
    pub title: Option<&'static str>,
    pub children: Vec<RouteSpec>,
    pub route_mode: RouteMode,
    pub static_paths: Option<StaticPathsFn>,
}

/// A fully-resolved route node with concrete full path and layout setup chain.
#[derive(Clone)]
pub struct ResolvedRouteSpec {
    pub path: &'static str,
    pub full_path: String,
    pub has_endpoint: bool,
    pub setup_plan: RouteSetupPlan,
    pub route_mode: RouteMode,
    pub static_paths: Option<StaticPathsFn>,
    pub children: Vec<ResolvedRouteSpec>,
}

impl RouteSpec {
    pub fn endpoint(path: &'static str) -> Self {
        Self {
            path,
            has_endpoint: true,
            layout: None,
            title: None,
            children: Vec::new(),
            route_mode: RouteMode::default(),
            static_paths: None,
        }
    }

    pub fn page(path: &'static str, title: &'static str) -> Self {
        Self {
            title: Some(title),
            ..Self::endpoint(path)
        }
    }

    pub fn layout(path: &'static str, layout: LayoutRef, children: Vec<RouteSpec>) -> Self {
        Self {
            path,
            has_endpoint: false,
            layout: Some(layout),
            title: None,
            children,
            route_mode: RouteMode::default(),
            static_paths: None,
        }
    }

    pub fn with_children(mut self, children: Vec<RouteSpec>) -> Self {
        self.children = children;
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

pub fn collect_route_setups(routes: &[RouteSpec]) -> HashMap<String, RouteSetupPlan> {
    let mut route_setups = HashMap::new();
    for route in routes {
        collect_setup(route, &[], &RouteSetupPlan::default(), &mut route_setups);
    }
    route_setups
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
    parent_layouts: &[LayoutRef],
) -> ResolvedRouteSpec {
    let (segments, full_path) = resolve_route_path(parent_segments, route.path);

    let mut layouts = parent_layouts.to_vec();
    if let Some(layout) = route.layout {
        layouts.push(layout);
    }

    let children = route
        .children
        .iter()
        .map(|child| resolve_route_spec(child, &segments, &layouts))
        .collect();

    ResolvedRouteSpec {
        path: route.path,
        full_path,
        has_endpoint: route.has_endpoint,
        setup_plan: RouteSetupPlan::new(&layouts, route.title),
        route_mode: route.route_mode,
        static_paths: route.static_paths,
        children,
    }
}

fn collect_setup(
    route: &RouteSpec,
    parent_segments: &[String],
    parent_plan: &RouteSetupPlan,
    map: &mut HashMap<String, RouteSetupPlan>,
) {
    let (segments, full_path) = resolve_route_path(parent_segments, route.path);

    let plan = match route.layout {
        Some(layout) => RouteSetupPlan::cascade(parent_plan, std::slice::from_ref(&layout)),
        None => parent_plan.clone(),
    };

    if route.has_endpoint {
        map.insert(full_path.clone(), plan.clone().with_page_title(route.title));
    }

    for child in &route.children {
        collect_setup(child, &segments, &plan, map);
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
    use crate::Layout;
    use proa_core::{DynLoaderHandle, WebContext, WriteError};
    struct NeverAtRegistration;
    impl Layout for NeverAtRegistration {
        fn setup(&self, _: &mut WebContext<DynLoaderHandle>) -> Result<(), WriteError> {
            panic!("registration must not execute request-time setup")
        }
    }
    #[test]
    fn registration_collects_ordered_layouts_and_titles_without_executing_them() {
        let routes = vec![RouteSpec::layout(
            "/app",
            &NeverAtRegistration,
            vec![RouteSpec::layout(
                "",
                &NeverAtRegistration,
                vec![RouteSpec::page("dashboard", "Dashboard")],
            )],
        )];
        let map = collect_route_setups(&routes);
        let plan = map.get("/app/dashboard").unwrap();
        assert_eq!(plan.layout_count(), 2);
        assert_eq!(plan.page_title(), Some("Dashboard"));
        let resolved = resolve_route_specs(&routes);
        let child = &resolved[0].children[0].children[0];
        assert_eq!(child.full_path, "/app/dashboard");
        assert_eq!(child.setup_plan.layout_count(), 2);
        assert_eq!(child.setup_plan.page_title(), Some("Dashboard"));
    }
    #[test]
    fn nested_and_absolute_route_paths() {
        let parent = vec!["app".to_string()];
        assert_eq!(resolve_route_path(&parent, "dashboard").1, "/app/dashboard");
        assert_eq!(resolve_route_path(&parent, "/account").1, "/account");
        assert_eq!(resolve_route_path(&parent, "/").1, "/");
    }
}
