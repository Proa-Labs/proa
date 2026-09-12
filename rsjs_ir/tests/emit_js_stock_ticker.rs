// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M8 emit_js coverage: feed the StockTicker fixture through
//! `emit_js::emit` and inspect the produced JS for the expected
//! query-runtime shape.
//!
//! Unlike Counter / ConditionalCounter / UserForm, the StockTicker
//! test does not bless a full byte-stable golden file: M8 still has
//! moving pieces in the runtime (`__rsjs_query_*` API shape, helper
//! names) and locking byte-equality this early would generate noisy
//! churn. Instead we assert each load-bearing fragment appears in
//! the expected position. Once the runtime stabilises (M9), this
//! test gets blessed into a golden the same way Counter does.

use rsjs_ir::{
    emit_js, AsType, ClientExpr, IslandBuilder, JsonPointer, QueryAccessor, QueryOptions, Value,
};

fn build_stock_ticker() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();

    let symbol = b.signal(Value::String("AAPL".to_string()));
    let key = b.expr(ClientExpr::Literal(Value::String("stock:AAPL".to_string())));
    let server = b.expr(ClientExpr::Literal(Value::String(
        r#"{"value":172.5}"#.to_string(),
    )));
    let client = b.expr(ClientExpr::Literal(Value::String(
        "/api/stocks/AAPL".to_string(),
    )));

    let q = b.query(
        key,
        server,
        client,
        QueryOptions {
            interval_ms: Some(5_000),
            refetch_on_focus: true,
            download_progress: true,
            stream_text: true,
            retry_count: 3,
            stale_time_ms: 2_000,
        },
    );

    // price.data().at("/value").as_f64()
    let price_expr = b.expr(ClientExpr::AsType {
        target: Box::new(ClientExpr::Project {
            target: Box::new(ClientExpr::QueryRead {
                id: q,
                accessor: QueryAccessor::Data,
            }),
            path: JsonPointer::new("/value"),
        }),
        ty: AsType::F64,
    });

    // is_refetching predicate.
    let refetching_expr = b.expr(ClientExpr::QueryRead {
        id: q,
        accessor: QueryAccessor::IsRefetching,
    });
    let total_expr = b.expr(ClientExpr::QueryRead {
        id: q,
        accessor: QueryAccessor::DownloadTotal,
    });

    let root = b.element("div", None);
    let symbol_span = b.element("span", Some(root));
    let price_span = b.element("span", Some(root));
    let total_span = b.element("span", Some(root));
    let refetching_span = b.element("span", Some(root));

    let symbol_read = b.expr(ClientExpr::SignalRead(symbol));
    b.bind_text(symbol_span, symbol_read);
    b.bind_text(price_span, price_expr);
    b.bind_text(total_span, total_expr);
    b.conditional_section(root, refetching_expr, vec![refetching_span], Vec::new());
    b.set_root(root);
    b.set_async(true);

    b.build()
}

#[test]
fn imports_query_runtime_when_island_has_queries() {
    let island = build_stock_ticker();
    let js = emit_js::emit(&island, "stockticker");
    assert!(
        js.contains(r#"from "/rsjs-runtime-query.js""#),
        "expected query-runtime import line; got:\n{js}"
    );
    assert!(
        js.contains("import { rsjs, dispose_query, register_query }"),
        "expected `rsjs`, `dispose_query`, and `register_query` named imports; got:\n{js}"
    );
}

#[test]
fn registers_each_query_with_options() {
    let island = build_stock_ticker();
    let js = emit_js::emit(&island, "stockticker");

    // The registration call.
    assert!(
        js.contains("const __rsjs_query_0 = register_query({"),
        "expected `register_query` registration with id 0; got:\n{js}"
    );

    // Asset id + query id wired through.
    assert!(
        js.contains(r#"island: "stockticker""#),
        "expected island id in registration; got:\n{js}"
    );
    assert!(
        js.contains("query_id: 0"),
        "expected query_id 0 in registration; got:\n{js}"
    );

    // Cache key emitted as a JS array literal of expression
    // values.
    assert!(
        js.contains(r#"key: ["stock:AAPL"]"#),
        "expected structural cache key; got:\n{js}"
    );

    // Client fetcher passed through.
    assert!(
        js.contains(r#"fetcher: "/api/stocks/AAPL""#),
        "expected client fetcher in registration; got:\n{js}"
    );
    assert!(
        js.contains("download_progress: true"),
        "expected download progress option in registration; got:\n{js}"
    );
    assert!(
        js.contains("stream_text: true"),
        "expected streaming text option in registration; got:\n{js}"
    );

    // Options. All configured knobs.
    assert!(
        js.contains(
            "options: { interval_ms: 5000, refetch_on_focus: true, download_progress: true, stream_text: true, retry_count: 3, stale_time_ms: 2000 }"
        ),
        "expected configured query options; got:\n{js}"
    );
}

#[test]
fn projection_chain_lowers_to_runtime_helpers() {
    let island = build_stock_ticker();
    let js = emit_js::emit(&island, "stockticker");

    // The `price.data().at("/value").as_f64()` chain lowers to the
    // sequence `__rsjs_query_0.data()` -> `rsjs.__project(...,
    // ["value"])` -> `rsjs.__as_f64(...)`. Each fragment should
    // appear; the bind_text wraps them.
    assert!(
        js.contains("__rsjs_query_0.data()"),
        "expected QueryRead lowering; got:\n{js}"
    );
    assert!(
        js.contains(r#"rsjs.__project(__rsjs_query_0.data(), ["value"])"#),
        "expected projection lowering; got:\n{js}"
    );
    assert!(
        js.contains("rsjs.__as_f64(rsjs.__project("),
        "expected AsType terminator wrapping the projection; got:\n{js}"
    );
}

#[test]
fn is_refetching_predicate_lowers_to_query_accessor() {
    let island = build_stock_ticker();
    let js = emit_js::emit(&island, "stockticker");

    // The conditional binding's predicate: __rsjs_query_0.is_refetching().
    assert!(
        js.contains("__rsjs_query_0.is_refetching()"),
        "expected is_refetching accessor lowering; got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_query_0.download_total()"),
        "expected download_total accessor lowering; got:\n{js}"
    );
    // The conditional section binding wraps the predicate.
    assert!(
        js.contains("bind_conditional("),
        "expected bind_conditional call; got:\n{js}"
    );
}

#[test]
fn stock_ticker_emit_is_deterministic() {
    let island = build_stock_ticker();
    let first = emit_js::emit(&island, "stockticker");
    for _ in 0..16 {
        assert_eq!(emit_js::emit(&island, "stockticker"), first);
    }
}
