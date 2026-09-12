// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::sync::Arc;

use axum::routing::MethodRouter;
use axum::Router;
use proa_framework_core::{compute_metadata_map, resolve_route_specs, ResolvedRouteSpec};
pub use proa_framework_core::{RouteMode, RouteSpec, StaticPathsFn};

use super::context::{EndpointContextLayer, Stack};
use super::layout::LayoutRef;
use super::metadata::{ComputedRouteMeta, MetadataRef};

/// Declarative route specification
#[derive(Clone)]
pub struct Route<State: Clone + Send + Sync + 'static> {
    pub path: &'static str,
    pub endpoint: Option<MethodRouter<State>>,
    pub layout: Option<LayoutRef>,
    pub metadata: Option<MetadataRef>, // For endpoints without layouts
    pub children: Vec<Route<State>>,
    pub route_mode: RouteMode,
    /// Optional function to generate static paths for dynamic routes at build time
    pub static_paths: Option<StaticPathsFn>,
    /// Cache policy for this endpoint only; child routes configure their own.
    pub page_cache: Option<super::PageCache>,
}

impl<State> Route<State>
where
    State: Clone + Send + Sync + 'static,
{
    pub fn spec(&self) -> RouteSpec {
        RouteSpec {
            path: self.path,
            has_endpoint: self.endpoint.is_some(),
            layout_metadata: self.layout.map(|layout| layout as MetadataRef),
            metadata: self.metadata,
            children: self.children.iter().map(Self::spec).collect(),
            route_mode: self.route_mode,
            static_paths: self.static_paths,
        }
    }

    /// Creates a route with just an endpoint
    pub fn endpoint(path: &'static str, router: MethodRouter<State>) -> Self {
        Self {
            path,
            endpoint: Some(router),
            layout: None,
            metadata: None,
            children: Vec::new(),
            route_mode: RouteMode::default(),
            static_paths: None,
            page_cache: None,
        }
    }

    /// Creates a route with an endpoint and title
    pub fn page(path: &'static str, title: &'static str, router: MethodRouter<State>) -> Self {
        Self {
            path,
            endpoint: Some(router),
            layout: None,
            metadata: Some(Box::leak(Box::new(super::metadata::TitleOnly::new(title)))),
            children: Vec::new(),
            route_mode: RouteMode::default(),
            static_paths: None,
            page_cache: None,
        }
    }

    /// Creates a route with an endpoint and full metadata
    pub fn page_with_metadata(
        path: &'static str,
        router: MethodRouter<State>,
        metadata: MetadataRef,
    ) -> Self {
        Self {
            path,
            endpoint: Some(router),
            layout: None,
            metadata: Some(metadata),
            children: Vec::new(),
            route_mode: RouteMode::default(),
            static_paths: None,
            page_cache: None,
        }
    }

    /// Creates a metadata layout route with children.
    ///
    /// Shared page chrome should be composed in a render component. The
    /// layout contributes cascading metadata such as title templates.
    pub fn layout(path: &'static str, layout: LayoutRef, children: Vec<Route<State>>) -> Self {
        Self {
            path,
            endpoint: None,
            layout: Some(layout),
            metadata: None,
            children,
            route_mode: RouteMode::default(),
            static_paths: None,
            page_cache: None,
        }
    }

    /// Builder method to add children
    pub fn with_children(mut self, children: Vec<Route<State>>) -> Self {
        self.children = children;
        self
    }

    /// Builder method to add an endpoint
    pub fn with_endpoint(mut self, router: MethodRouter<State>) -> Self {
        self.endpoint = Some(router);
        self
    }

    /// Configure CDN caching for this public page with `PageCache::new()`.
    ///
    /// Add `.revalidate(interval)` to the policy for CDN-backed ISR. Edge hits
    /// skip the application server; no in-process HTML cache is required.
    /// Configure HTML caching and post-rollout invalidation with your CDN.
    /// `proa deploy run` can invoke the integration in `deploy.cdn`.
    /// A new build alone does not purge the CDN.
    ///
    /// Applied when the route is mounted, so replacing the endpoint with
    /// `with_endpoint` retains its cache policy. A layout without an endpoint
    /// does not cache its children. The policy does not apply to child routes.
    /// Existing `PageCacheLayer` conversions remain supported for compatibility.
    pub fn with_page_cache(mut self, cache: impl Into<super::PageCache>) -> Self {
        self.page_cache = Some(cache.into());
        self
    }

    /// Convert this route and its children into a router for `Router::merge`.
    ///
    /// Endpoint layers retain resolved metadata stacks and route parameters.
    /// Mount through `FrameworkBuilder` or install `RouteContextLayer` to
    /// establish the framework request context. Use `router_from_spec` when
    /// the caller also needs the precomputed metadata map.
    pub fn into_router(self) -> Router<State> {
        router_from_spec(vec![self]).0
    }

    /// Builder method to add a layout
    pub fn with_layout(mut self, layout: LayoutRef) -> Self {
        self.layout = Some(layout);
        self
    }

    /// Builder method to add metadata
    pub fn with_metadata(mut self, metadata: MetadataRef) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Builder method to set routing mode
    pub fn with_route_mode(mut self, route_mode: RouteMode) -> Self {
        self.route_mode = route_mode;
        self
    }

    /// Builder method to provide static paths for dynamic routes.
    /// Only used when route_mode is RouteMode::Static.
    ///
    /// # Example
    /// ```rust
    /// use std::collections::HashMap;
    /// use axum::routing::get;
    /// use proa_framework_axum::{Route, RouteMode};
    ///
    /// fn get_blog_slugs() -> Vec<HashMap<&'static str, String>> {
    ///     vec![
    ///         HashMap::from([("slug", "intro".to_string())]),
    ///         HashMap::from([("slug", "tutorial".to_string())]),
    ///     ]
    /// }
    ///
    /// async fn blog_handler() -> &'static str {
    ///     "ok"
    /// }
    ///
    /// let _route = Route::<()>::page("/blog/:slug", "Blog Post", get(blog_handler))
    ///     .with_route_mode(RouteMode::Static)
    ///     .with_static_paths(get_blog_slugs);
    /// ```
    pub fn with_static_paths(mut self, paths_fn: StaticPathsFn) -> Self {
        self.static_paths = Some(paths_fn);
        self
    }
}

/// Builds an Axum Router from a declarative route specification
/// Returns the router and the precomputed metadata map
pub fn router_from_spec<State>(
    routes: Vec<Route<State>>,
) -> (Router<State>, Arc<HashMap<String, ComputedRouteMeta>>)
where
    State: Clone + Send + Sync + 'static,
{
    let route_specs: Vec<RouteSpec> = routes.iter().map(Route::spec).collect();
    let computed_metadata = Arc::new(compute_metadata_map(&route_specs));
    let resolved_specs = resolve_route_specs(&route_specs);

    // Second pass: build the router
    let router = routes
        .into_iter()
        .zip(resolved_specs)
        .fold(Router::new(), |router, (route, resolved)| {
            router.merge(into_router(route, resolved))
        });

    (router, computed_metadata)
}

fn into_router<State>(route: Route<State>, resolved: ResolvedRouteSpec) -> Router<State>
where
    State: Clone + Send + Sync + 'static,
{
    let Route {
        path: _,
        endpoint,
        layout: _,
        metadata: _,
        children,
        route_mode: _,
        static_paths: _,
        page_cache,
    } = route;

    let metadata_stack_arc = Stack::from_slice(&resolved.metadata_stack);

    let mut router = Router::new();

    if let Some(method_router) = endpoint {
        let method_router = match page_cache {
            Some(cache) => cache.apply_to(method_router),
            None => method_router,
        };
        let method_router = method_router.layer(EndpointContextLayer::new(
            metadata_stack_arc.clone(),
            Arc::from(resolved.full_path.as_str()),
        ));
        router = router.route(resolved.full_path.as_str(), method_router);
    }

    debug_assert_eq!(children.len(), resolved.children.len());
    for (child, child_resolved) in children.into_iter().zip(resolved.children) {
        router = router.merge(into_router(child, child_resolved));
    }

    router
}
