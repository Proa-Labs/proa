// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use http::Uri;
use proa_core::{RequestLocation, RouteParam, RouteParams, WebContext};

#[test]
fn web_context_exposes_request_location_path_and_query() {
    let mut out = Vec::new();
    let cx = WebContext::with_buffer(&mut out).with_request_location(RequestLocation::from(
        Uri::from_static("/products/42?tab=reviews"),
    ));

    let location = cx
        .request_location()
        .expect("request location should be set");
    assert_eq!(location.path(), "/products/42");
    assert_eq!(location.query(), Some("tab=reviews"));
    assert_eq!(
        location.uri(),
        &Uri::from_static("/products/42?tab=reviews")
    );
}

#[test]
fn web_context_exposes_route_params_and_param_lookup() {
    let mut out = Vec::new();
    let cx = WebContext::with_buffer(&mut out).with_route_params(RouteParams::from_vec(vec![
        RouteParam::new("id", "42"),
        RouteParam::new("section", "reviews"),
    ]));

    let params = cx.route_params().expect("route params should be set");
    assert_eq!(params.len(), 2);
    assert_eq!(params.get("id"), Some("42"));
    assert_eq!(params.get("section"), Some("reviews"));
    assert_eq!(params.get("missing"), None);
    assert_eq!(cx.param("id"), Some("42"));
    assert_eq!(cx.param("section"), Some("reviews"));
    assert_eq!(cx.param("missing"), None);
}
