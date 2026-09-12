// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M8 fixture: hand-construct the StockTicker island.
//!
//! Preserves the historical M8 StockTicker fixture for a server-seeded query
//! that the client can refetch. The milestone's user-facing shape was:
//! The quoted block is not current authoring syntax.
//!
//! ```text
//! #[rsjs] impl ... for StockTicker {
//!     async fn render(self, cx) -> ... {
//!         let price = query::<StockPrice, ApiError>(
//!             QueryKey::new("stock", &self.symbol),
//!             Fetchers { server: ..., client: ... },
//!             QueryOptions {
//!                 refetch_interval: Some(Duration::from_secs(5)),
//!                 ..
//!             },
//!         );
//!         let current_price = price.data().at("/value").as_f64();
//!         html_sync! {
//!             <div>
//!                 <span class="symbol">{self.symbol.clone()}</span>
//!                 <span class="price">{current_price}</span>
//!                 {if price.is_refetching() {
//!                     html_sync!{ <span class="refreshing">"R"</span> }
//!                 }}
//!             </div>
//!         }
//!     }
//! }
//! ```
//!
//! v0.1 (M8) parks the query contracts behind opaque
//! [`rsjs_ir::ExprId`]s: this fixture stores `ClientExpr::Literal` for
//! both `server_initial` (the dehydrated initial value) and
//! `client_fetcher` (a URL string).

use rsjs_ir::{
    AsType, ClientExpr, ExprId, IslandBuilder, JsonPointer, NodeId, QueryAccessor, QueryId,
    QueryOptions, SignalId, Value,
};

/// All ids the assertions need to reference.
struct StockTickerFixture {
    island: rsjs_ir::Island,
    symbol_signal: SignalId,
    price_query: QueryId,
    price_text_node: NodeId,
    refetching_branch_node: NodeId,
    /// The text expression rendering `price.data().at("/value").as_f64()`.
    price_text_expr: ExprId,
    /// The conditional predicate expression `price.is_refetching()`.
    refetching_predicate_expr: ExprId,
}

fn build_stock_ticker_fixture() -> StockTickerFixture {
    let mut b = IslandBuilder::new();

    // Symbol is captured as a signal so the rendered span can show
    // the user-supplied stock symbol; it never changes after mount
    // but treating it as a signal exercises the cross-cutting init-
    // state path alongside the query.
    let symbol_signal = b.signal(Value::String("AAPL".to_string()));

    // The cache key. v0.1 uses a string literal; M8.5 extends to
    // structural arrays.
    let key_expr = b.expr(ClientExpr::Literal(Value::String("stock:AAPL".to_string())));

    // Server initial value: in v0.1 this is the dehydrated initial
    // JSON payload, encoded as a string literal. The emit_rust pass
    // unwraps the literal at server-render time.
    let server_initial = b.expr(ClientExpr::Literal(Value::String(
        r#"{"value":172.5,"currency":"USD"}"#.to_string(),
    )));

    // Client fetcher: in v0.1 this is the URL the client refetcher
    // calls; the runtime uses its own GET+JSON contract.
    let client_fetcher = b.expr(ClientExpr::Literal(Value::String(
        "/api/stocks/AAPL".to_string(),
    )));

    let price_query = b.query(
        key_expr,
        server_initial,
        client_fetcher,
        QueryOptions {
            interval_ms: Some(5_000),
            refetch_on_focus: true,
            download_progress: true,
            stream_text: false,
            retry_count: 3,
            stale_time_ms: 2_000,
        },
    );

    // Reactive expression: price.data().at("/value").as_f64().
    // Built bottom-up: QueryRead -> Project -> AsType.
    let price_text_expr = b.expr(ClientExpr::AsType {
        target: Box::new(ClientExpr::Project {
            target: Box::new(ClientExpr::QueryRead {
                id: price_query,
                accessor: QueryAccessor::Data,
            }),
            path: JsonPointer::new("/value"),
        }),
        ty: AsType::F64,
    });

    // Predicate: price.is_refetching().
    let refetching_predicate_expr = b.expr(ClientExpr::QueryRead {
        id: price_query,
        accessor: QueryAccessor::IsRefetching,
    });

    // View tree.
    let root = b.element("div", None);
    let symbol_span = b.element("span", Some(root));
    let price_span = b.element("span", Some(root));
    let refetching_span = b.element("span", Some(root));

    // The symbol span just shows the signal value.
    let symbol_read_expr = b.expr(ClientExpr::SignalRead(symbol_signal));
    b.bind_text(symbol_span, symbol_read_expr);

    // The price span shows the projection chain.
    b.bind_text(price_span, price_text_expr);

    // The refetching span is gated on the query's is_refetching
    // accessor.
    b.conditional_section(
        root,
        refetching_predicate_expr,
        vec![refetching_span],
        Vec::new(),
    );

    b.set_root(root);
    b.set_async(true);

    StockTickerFixture {
        island: b.build(),
        symbol_signal,
        price_query,
        price_text_node: price_span,
        refetching_branch_node: refetching_span,
        price_text_expr,
        refetching_predicate_expr,
    }
}

#[test]
fn fixture_shape_matches_authoring_intent() {
    let f = build_stock_ticker_fixture();
    let island = &f.island;

    // One signal (symbol), one query, no event handlers, one
    // conditional section, two text bindings.
    assert_eq!(island.signals.len(), 1);
    assert_eq!(island.queries.len(), 1);
    assert_eq!(island.handlers.len(), 0);
    assert_eq!(island.bindings.text_bindings.len(), 2);
    assert_eq!(island.bindings.conditional_sections.len(), 1);
    assert!(island.is_async);

    // The query has the configured options.
    let q = &island.queries[0];
    assert_eq!(q.id, f.price_query);
    assert_eq!(q.options.interval_ms, Some(5_000));
    assert!(q.options.refetch_on_focus);
    assert!(q.options.download_progress);
    assert!(!q.options.stream_text);
    assert_eq!(q.options.retry_count, 3);
    assert_eq!(q.options.stale_time_ms, 2_000);

    // The price text expression resolves to the AsType -> Project
    // -> QueryRead chain.
    let price_expr = island.exprs.client_expr(f.price_text_expr);
    let ClientExpr::AsType { target, ty } = price_expr else {
        panic!("expected AsType terminator, got {price_expr:?}");
    };
    assert_eq!(*ty, AsType::F64);
    let ClientExpr::Project { target, path } = target.as_ref() else {
        panic!("expected Project node, got {target:?}");
    };
    assert_eq!(path.as_str(), "/value");
    let ClientExpr::QueryRead { id, accessor } = target.as_ref() else {
        panic!("expected QueryRead leaf, got {target:?}");
    };
    assert_eq!(*id, f.price_query);
    assert_eq!(*accessor, QueryAccessor::Data);

    // The is_refetching predicate.
    let pred = island.exprs.client_expr(f.refetching_predicate_expr);
    assert!(matches!(
        pred,
        ClientExpr::QueryRead {
            accessor: QueryAccessor::IsRefetching,
            ..
        }
    ));

    // The refetching span is the then-branch of the section keyed
    // on the root.
    let root = island.view_tree.root().expect("root");
    let section = island
        .bindings
        .conditional_sections
        .get(&root)
        .and_then(|sections| sections.first())
        .expect("conditional section");
    assert_eq!(section.then_children, vec![f.refetching_branch_node]);
    assert!(section.else_children.is_empty());

    // Sanity-check the symbol_signal is still wired.
    let _ = (f.symbol_signal, f.price_text_node);
}

#[test]
fn json_pointer_segments_round_trip() {
    let p = JsonPointer::new("/user/profile/handle");
    assert_eq!(p.segments(), vec!["user", "profile", "handle"]);

    // Empty pointer is the root.
    let p = JsonPointer::new("");
    assert!(p.segments().is_empty());

    // Escapes per RFC 6901: ~1 -> /, ~0 -> ~.
    let p = JsonPointer::new("/foo~1bar/baz~0qux");
    assert_eq!(p.segments(), vec!["foo/bar", "baz~qux"]);
}

#[test]
fn bind_text_passes_query_handle_so_resubscribes_on_refetch() {
    // Round-3 review #4 regression: a `<span>{query.data().as_str()}</span>`
    // text binding must surface the QueryId as an extra-handle so
    // the runtime's `bind_text` calls `__rsjs_query_<id>.subscribe(apply)`.
    // Before the fix, the dep array was signal-only and the binding
    // applied once at hydration but never updated when the query
    // refetched.
    let f = build_stock_ticker_fixture();
    let js = rsjs_ir::emit_js::emit(&f.island, "stockticker");
    // `bind_text(__node_2, () => ..., [<signals>], [__rsjs_query_0])`
    // is the post-fix shape; the signals slot is empty here (the
    // price expression reads only the query) and the extra-handles
    // slot carries the query handle. Match conservatively: just
    // assert the query handle reference appears in the bind_text
    // call site.
    assert!(
        js.contains("__rsjs_query_0"),
        "expected __rsjs_query_0 reference somewhere in the JS:\n{js}"
    );
    // The `bind_text` for the price span should have a dep array
    // containing the query handle.
    let bind_text_lines: Vec<&str> = js.lines().filter(|l| l.contains("bind_text(")).collect();
    assert!(
        bind_text_lines.iter().any(|l| l.contains("__rsjs_query_0")),
        "expected at least one bind_text(...) line to reference __rsjs_query_0; got:\n{}",
        bind_text_lines.join("\n")
    );
}
