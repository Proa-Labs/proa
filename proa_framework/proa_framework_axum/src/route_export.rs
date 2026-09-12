// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Route export functionality for TypeScript code generation
//!
//! This module allows exporting route definitions as JSON for client-side router codegen.

use serde::{Deserialize, Serialize};

use super::route::{Route, RouteMode};

/// Serializable route definition for code generation
#[derive(Debug, Clone, Serialize)]
pub struct RouteExport {
    /// The route path pattern (e.g., "/dashboard", "/settings/:section")
    pub path: String,
    /// Routing mode
    pub route_mode: RouteMode,
    /// Whether this route has an endpoint (is a page, not just a layout)
    pub has_endpoint: bool,
    /// Child routes
    pub children: Vec<RouteExport>,
}

impl RouteExport {
    /// Extract exportable route information from a Route
    pub fn from_route<State>(route: &Route<State>, parent_path: &str) -> Self
    where
        State: Clone + Send + Sync + 'static,
    {
        let full_path = resolve_export_path(parent_path, route.path);
        let has_endpoint = route.endpoint.is_some();

        let children = route
            .children
            .iter()
            .map(|child| Self::from_route(child, &full_path))
            .collect();

        Self {
            path: full_path,
            route_mode: route.route_mode,
            has_endpoint,
            children,
        }
    }

    /// Flatten nested routes into a flat list of page routes
    pub fn flatten(&self) -> Vec<FlatRouteExport> {
        let mut result = Vec::new();
        self.flatten_recursive(&mut result);
        result
    }

    fn flatten_recursive(&self, result: &mut Vec<FlatRouteExport>) {
        if self.has_endpoint {
            result.push(FlatRouteExport {
                path: self.path.clone(),
                route_mode: self.route_mode,
            });
        }

        for child in &self.children {
            child.flatten_recursive(result);
        }
    }
}

/// Flat route export for simple route manifests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatRouteExport {
    /// The full route path
    pub path: String,
    /// Routing mode
    pub route_mode: RouteMode,
}

/// Export routes from a route specification
pub fn export_routes<State>(routes: &[Route<State>]) -> Vec<RouteExport>
where
    State: Clone + Send + Sync + 'static,
{
    routes
        .iter()
        .map(|route| RouteExport::from_route(route, ""))
        .collect()
}

/// Export routes as a flat list
pub fn export_routes_flat<State>(routes: &[Route<State>]) -> Vec<FlatRouteExport>
where
    State: Clone + Send + Sync + 'static,
{
    let tree = export_routes(routes);
    tree.iter().flat_map(|r| r.flatten()).collect()
}

fn resolve_export_path(parent_path: &str, path: &str) -> String {
    if path.starts_with('/') {
        // Absolute path
        if path == "/" {
            "/".to_string()
        } else {
            path.to_string()
        }
    } else if path.is_empty() {
        // Empty path means use parent
        parent_path.to_string()
    } else {
        // Relative path
        if parent_path == "/" || parent_path.is_empty() {
            format!("/{}", path)
        } else {
            format!("{}/{}", parent_path, path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_export_path() {
        assert_eq!(resolve_export_path("", "/"), "/");
        assert_eq!(resolve_export_path("", "/dashboard"), "/dashboard");
        assert_eq!(resolve_export_path("/", "dashboard"), "/dashboard");
        assert_eq!(resolve_export_path("/app", "settings"), "/app/settings");
        assert_eq!(resolve_export_path("/app", "/dashboard"), "/dashboard");
    }
}
