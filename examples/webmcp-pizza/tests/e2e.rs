// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use proa_core::{NoLoader, RenderBuffer, WebContext, WebRenderSync};
use tower::ServiceExt;

use proa_webmcp_pizza::app;
use proa_webmcp_pizza::components::{PizzaForm, PizzaPage};
use proa_webmcp_pizza::webmcp::{
    Confirmation, WebMcp, WebMcpComposeError, WebMcpContext, WebMcpParameter, WebMcpTool,
};

const SEARCH: WebMcpTool = WebMcpTool::new("search", "Search the catalog", Confirmation::Automatic);
const QUERY: WebMcpParameter = WebMcpParameter::new("query", Some("Search query"), true);
const LIMIT: WebMcpParameter = WebMcpParameter::new("limit", Some("Maximum results"), false);

struct SearchForm;

impl WebMcp for SearchForm {
    fn describe_webmcp(&self, cx: &mut WebMcpContext) -> Result<(), WebMcpComposeError> {
        cx.tool(SEARCH, &(QUERY, LIMIT))
    }
}

#[tokio::test]
async fn page_contains_webmcp_form_and_rsjs_runtime() {
    let response = app()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains(r#"toolname="create_pizza_order""#));
    assert!(html.contains("/rsjs-runtime.js"));
    assert!(html.contains("/webmcp-bridge.js"));
}

#[tokio::test]
async fn bridge_registers_an_imperative_adapter_that_delegates_to_webmcp() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/webmcp-bridge.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let javascript = String::from_utf8(body.to_vec()).unwrap();

    assert!(javascript.contains("document.modelContext.registerTool"));
    assert!(javascript.contains("document.modelContext.executeTool"));
    assert!(javascript.contains("populateNativeForm(form, input)"));
    assert!(javascript.contains("configure_${declarativeName"));
}

#[tokio::test]
async fn invalid_order_returns_wellformed_errors() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/orders")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "customer_name=A&size=medium&topping=margherita&quantity=1&delivery_notes=",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Enter at least two characters"));
    assert!(html.contains(r#"value="A""#));
}

#[tokio::test]
async fn valid_order_returns_normalized_receipt() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/orders")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "customer_name=++Ada+++Lovelace++&size=large&topping=mushroom&quantity=2&delivery_notes=++Ring+twice++",
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Ada Lovelace"));
    assert!(html.contains("Ring twice"));
    assert!(html.contains(r#""size": "large""#));
}

#[test]
fn form_exposes_webmcp_and_native_submission_contracts() {
    let mut cx = WebContext::<NoLoader, RenderBuffer>::new();
    PizzaForm::initial().render(&mut cx).unwrap();
    let html = cx.into_output().into_string();

    assert!(html.contains(r#"toolname="create_pizza_order""#));
    assert!(html.contains(r#"action="/orders""#));
    assert!(html.contains("toolparamdescription="));
    assert!(html.contains(r#"name="customer_name""#));
    assert!(html.contains(r#"name="quantity""#));
    assert!(html.contains("<rsjs-island"));
}

#[test]
fn page_composes_the_form_tool_and_its_parameters() {
    let tools = WebMcpContext::collect(&PizzaPage::initial()).unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].definition.name, "create_pizza_order");
    assert_eq!(tools[0].definition.confirmation, Confirmation::User);
    assert_eq!(tools[0].parameters.len(), 5);
    assert_eq!(tools[0].parameters[0].name, "customer_name");
    assert_eq!(tools[0].parameters[4].name, "delivery_notes");
    assert!(!tools[0].parameters[4].required);
}

#[test]
fn parent_tool_folds_child_parameters() {
    let tools = WebMcpContext::collect(&SearchForm).unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].definition, SEARCH);
    assert_eq!(tools[0].parameters, vec![QUERY, LIMIT]);
}

#[test]
fn parameter_without_operation_scope_is_rejected() {
    let error = WebMcpContext::collect(&QUERY).unwrap_err();

    assert_eq!(error, WebMcpComposeError::ParameterOutsideTool("query"));
}
