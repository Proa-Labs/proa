// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Find statically registered paths in the Rust source of a scaffolded router.

use std::collections::HashSet;

use anyhow::{bail, Context, Result};
use syn::visit::{self, Visit};

use super::canonical_route;

pub(super) fn ensure_routes_unregistered(
    source: &str,
    canonical: &str,
    alias: Option<&str>,
) -> Result<()> {
    let file = syn::parse_file(source).context("parse src/routes.rs before registering a route")?;
    let mut registrations = RegisteredRoutes::default();
    registrations.visit_file(&file);
    for path in std::iter::once(canonical).chain(alias) {
        if registrations.paths.contains(path) {
            bail!("route path is already registered: {path}");
        }
    }
    Ok(())
}

#[derive(Default)]
struct RegisteredRoutes {
    paths: HashSet<String>,
    spec_parent: String,
}

fn string_argument(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(path),
            ..
        }) => Some(path.value()),
        syn::Expr::Paren(expr) => string_argument(&expr.expr),
        _ => None,
    }
}

fn associated_function(call: &syn::ExprCall) -> Option<(&syn::Ident, &syn::Ident)> {
    let syn::Expr::Path(function) = call.func.as_ref() else {
        return None;
    };
    let mut segments = function.path.segments.iter().rev();
    let method = &segments.next()?.ident;
    Some((&segments.next()?.ident, method))
}

struct RouteSpec<'a> {
    path: String,
    has_endpoint: bool,
    children: Option<&'a syn::Expr>,
}

impl<'a> RouteSpec<'a> {
    fn from_expr(expr: &'a syn::Expr) -> Option<Self> {
        match expr {
            syn::Expr::Paren(expr) => Self::from_expr(&expr.expr),
            syn::Expr::MethodCall(call) if call.method.to_string().starts_with("with_") => {
                let mut spec = Self::from_expr(&call.receiver)?;
                if call.method == "with_endpoint" {
                    spec.has_endpoint = true;
                } else if call.method == "with_children" {
                    spec.children = call.args.first();
                }
                Some(spec)
            }
            syn::Expr::Call(call) => {
                let (owner, method) = associated_function(call)?;
                if owner != "Route" {
                    return None;
                }
                let (has_endpoint, children) = match method.to_string().as_str() {
                    "endpoint" | "page" | "page_with_metadata" => (true, None),
                    "layout" => (false, call.args.iter().nth(2)),
                    _ => return None,
                };
                Some(Self {
                    path: string_argument(call.args.first()?)?,
                    has_endpoint,
                    children,
                })
            }
            _ => None,
        }
    }
}

impl RegisteredRoutes {
    /// A relative spec inherits its layout's path; a leading slash starts at the root.
    fn visit_spec(&mut self, spec: RouteSpec<'_>) {
        let mut path = if spec.path.starts_with('/') {
            String::new()
        } else {
            self.spec_parent.trim_end_matches('/').to_owned()
        };
        for segment in spec.path.split('/').filter(|part| !part.is_empty()) {
            path.push('/');
            path.push_str(segment);
        }
        if path.is_empty() {
            path.push('/');
        }
        if spec.has_endpoint {
            self.paths.insert(canonical_route(&path));
        }
        if let Some(children) = spec.children {
            let parent = std::mem::replace(&mut self.spec_parent, path);
            self.visit_expr(children);
            self.spec_parent = parent;
        }
    }

    fn visit_axum_route<'a>(
        &mut self,
        method: &syn::Ident,
        mut args: impl Iterator<Item = &'a syn::Expr>,
    ) -> bool {
        let Some(path) = args.next().and_then(string_argument) else {
            return false;
        };
        match method.to_string().as_str() {
            "route" | "route_service" | "nest_service" => {
                self.paths.insert(canonical_route(&path));
            }
            "nest" => {
                let Some(router) = args.next() else {
                    return false;
                };
                let mut nested = Self::default();
                nested.visit_expr(router);
                for child in nested.paths {
                    let mounted = if path.ends_with('/') {
                        format!("{path}{}", child.trim_start_matches('/'))
                    } else if child == "/" {
                        path.clone()
                    } else {
                        format!("{path}{child}")
                    };
                    self.paths.insert(canonical_route(&mounted));
                }
            }
            _ => return false,
        }
        true
    }
}

impl<'ast> Visit<'ast> for RegisteredRoutes {
    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        if let Some(spec) = RouteSpec::from_expr(expr) {
            self.visit_spec(spec);
        } else {
            visit::visit_expr(self, expr);
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Some((owner, method)) = associated_function(call) {
            if owner == "Router" && self.visit_axum_route(method, call.args.iter().skip(1)) {
                if let Some(router) = call.args.first() {
                    self.visit_expr(router);
                }
                return;
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if self.visit_axum_route(&call.method, call.args.iter()) {
            self.visit_expr(&call.receiver);
        } else {
            visit::visit_expr_method_call(self, call);
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // The scaffold passes its route specs in vec![...]. Syn leaves macro bodies as tokens.
        if mac
            .path
            .segments
            .last()
            .is_some_and(|name| name.ident == "vec")
        {
            let tokens = &mac.tokens;
            if let Ok(elements) = syn::parse2::<syn::Expr>(quote::quote!([#tokens])) {
                self.visit_expr(&elements);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_registered_route_literals_and_ignores_unrelated_text() {
        let routes = r##"
            fn routes() {
                // Accept a literal " in the query.
                let help = "Try \"/help\"";
                let example = r#".route("/example", get(handler))"#;
                let specs = vec![
                    Route::endpoint("/blog/:slug", get(handler)),
                    Route::endpoint("/files/*rest", get(handler)),
                    proa::axum::Route::page(r#"/shop/{id}"#, "/title", get(handler)),
                ];
                let router = Router::new()
                    .route("/he\u{61}lthz", get(handler))
                    .route_service(r#"/feed"#, service);
            }
        "##;
        for canonical in [
            "/blog/{slug}",
            "/files/{*rest}",
            "/shop/{id}",
            "/healthz",
            "/feed",
        ] {
            let error = ensure_routes_unregistered(routes, canonical, None).unwrap_err();
            assert!(error.to_string().contains("already registered"), "{error}");
        }
        for available in ["/blog", "/blog/{slug}/edit", "/help", "/example", "/title"] {
            ensure_routes_unregistered(routes, available, None).unwrap();
        }
    }

    #[test]
    fn reserves_layout_paths_only_where_the_route_tree_has_an_endpoint() {
        let routes = r#"
            fn routes() {
                let specs = vec![
                    Route::layout("/shop", &layout, vec![
                        Route::page("deals", "Deals", get(handler)),
                        Route::endpoint("/sale", get(handler)),
                    ]),
                    Route::layout("/account", &layout, vec![])
                        .with_metadata(&meta)
                        .with_endpoint(get(handler))
                        .with_children(vec![Route::endpoint("profile", get(handler))]),
                    Route::layout("/catalog", &layout, vec![Route::endpoint("", get(handler))]),
                ];
            }
        "#;
        for occupied in [
            "/shop/deals",
            "/sale",
            "/account",
            "/account/profile",
            "/catalog",
        ] {
            assert!(
                ensure_routes_unregistered(routes, occupied, None).is_err(),
                "{occupied}"
            );
        }
        for available in ["/shop", "/deals", "/shop/sale", "/profile"] {
            ensure_routes_unregistered(routes, available, None).unwrap();
        }
    }

    #[test]
    fn reserves_nested_axum_routes_at_their_mounted_paths() {
        let routes = r#"
            fn routes() {
                Router::new()
                    .nest("/api", Router::new().route("/status", get(handler)))
                    .nest("/docs", Router::new().route("/", get(handler)))
                    .nest_service("/static", files);
            }
        "#;
        for occupied in ["/api/status", "/docs", "/static"] {
            assert!(
                ensure_routes_unregistered(routes, occupied, None).is_err(),
                "{occupied}"
            );
        }
        for available in ["/api", "/status"] {
            ensure_routes_unregistered(routes, available, None).unwrap();
        }
    }
}
