// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M9 fixture: hand-construct the UpdateCart island.
//!
//! Preserves the historical M9 UpdateCart fixture shape:
//! The quoted block is not current authoring syntax.
//!
//! ```text
//! #[rsjs] impl ... for UpdateCart {
//!     async fn render(self, cx) -> ... {
//!         let cart = signal::<Cart>(Cart::empty());
//!         let user_query = query::<User>(
//!             QueryKey::new("cart", &self.user_id),
//!             Fetchers { server: ..., client: "/api/cart" },
//!             QueryOptions::default(),
//!         );
//!         let update_cart = mutation::<Cart, ApiError>(
//!             MutationKey::new("update_cart", &self.user_id),
//!             Runners {
//!                 server: ...,
//!                 client: ClientFetcher {
//!                     url: "/api/cart".into(),
//!                     method: HttpMethod::Post,
//!                     body: Some(RequestBody::Json),
//!                     ..Default::default()
//!                 },
//!             },
//!             MutationOptions {
//!                 invalidates: vec![QueryKey::new("cart", &self.user_id)],
//!                 ..Default::default()
//!             },
//!         );
//!         html_sync! {
//!             <div>
//!                 <button on_click={|_| update_cart.run(cart.get())}>"Save"</button>
//!                 <span>{if update_cart.is_pending() { "..." } else { "" }}</span>
//!             </div>
//!         }
//!     }
//! }
//! ```
//!
//! v0.1 (M9) parks the cross-crate runner contract behind opaque
//! [`rsjs_ir::ExprId`]s: this fixture stores `ClientExpr::Literal` for both
//! `server_runner` (an unused stub) and `client_runner` (the URL +
//! method config encoded as a JSON string the runtime parses).

use rsjs_ir::{
    emit_js, ClientBlock, ClientExpr, ClientStmt, EventName, ExprId, InvalidationStrategy,
    IslandBuilder, MutationAccessor, MutationId, MutationOptions, QueryAccessor, QueryId,
    QueryOptions, SignalId, Value,
};

struct UpdateCartFixture {
    island: rsjs_ir::Island,
    cart_signal: SignalId,
    cart_query: QueryId,
    update_cart_mutation: MutationId,
    pending_predicate_expr: ExprId,
}

fn build_update_cart_fixture() -> UpdateCartFixture {
    let mut b = IslandBuilder::new();

    // The cart signal: user-edited contents, the mutation argument.
    let cart_signal = b.signal(Value::String("[]".to_string()));

    // The cart query: cached server-side cart contents, refetched
    // after a successful mutation.
    let cart_query_key_expr = b.expr(ClientExpr::Literal(Value::String("cart".to_string())));
    let cart_query_server_initial = b.expr(ClientExpr::Literal(Value::String(
        r#"{"items":[]}"#.to_string(),
    )));
    let cart_query_client_fetcher =
        b.expr(ClientExpr::Literal(Value::String("/api/cart".to_string())));
    let cart_query = b.query(
        cart_query_key_expr,
        cart_query_server_initial,
        cart_query_client_fetcher,
        QueryOptions::default(),
    );

    // The update-cart mutation: POST to /api/cart, invalidate the
    // cart query on success.
    let mutation_key_expr = b.expr(ClientExpr::Literal(Value::String(
        "update_cart".to_string(),
    )));
    let server_runner = b.expr(ClientExpr::Literal(Value::Null));
    // v0.1 client runner config: a JSON string the runtime parses
    // into method + url + body shape.
    let client_runner = b.expr(ClientExpr::Literal(Value::String(
        r#"{"url":"/api/cart","method":"POST"}"#.to_string(),
    )));
    // The invalidation list: the same key the cart query was
    // registered under. The runtime prefix-matches against this
    // expression's evaluated value at mutation-success time.
    let invalidates = vec![b.expr(ClientExpr::Literal(Value::String("cart".to_string())))];

    let update_cart_mutation = b.mutation(
        mutation_key_expr,
        server_runner,
        client_runner,
        invalidates,
        MutationOptions {
            retry_count: 1,
            on_success_invalidate_strategy: InvalidationStrategy::Refetch,
        },
    );

    // The save handler: fire the mutation with the current cart
    // contents.
    let save_handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::RunMutation {
        mutation: update_cart_mutation,
        args: vec![ClientExpr::SignalRead(cart_signal)],
    }]));

    // The pending-status text expression (lowers to mutation.is_pending()
    // in JS).
    let pending_predicate_expr = b.expr(ClientExpr::MutationRead {
        id: update_cart_mutation,
        accessor: MutationAccessor::IsPending,
    });

    // View tree.
    let root = b.element("div", None);
    let save_button = b.element("button", Some(root));
    let status_span = b.element("span", Some(root));
    let pending_label = b.element("span", Some(status_span));

    b.bind_event(save_button, EventName::Click, save_handler);
    // Conditionally show the pending label only when the mutation is
    // in-flight.
    b.conditional_section(
        status_span,
        pending_predicate_expr,
        vec![pending_label],
        Vec::new(),
    );
    b.set_root(root);
    b.set_async(true);

    UpdateCartFixture {
        island: b.build(),
        cart_signal,
        cart_query,
        update_cart_mutation,
        pending_predicate_expr,
    }
}

#[test]
fn update_cart_fixture_has_one_query_one_mutation_one_handler() {
    let fixture = build_update_cart_fixture();
    let island = fixture.island;

    // One signal (cart), one query, one mutation, one handler.
    assert_eq!(island.signals.len(), 1);
    assert_eq!(island.queries.len(), 1);
    assert_eq!(island.mutations.len(), 1);
    assert_eq!(island.handlers.len(), 1);
    assert!(island.is_async);

    // The mutation has the configured options.
    let m = &island.mutations[0];
    assert_eq!(m.id, fixture.update_cart_mutation);
    assert_eq!(m.invalidates.len(), 1);
    assert_eq!(m.options.retry_count, 1);
    assert_eq!(
        m.options.on_success_invalidate_strategy,
        InvalidationStrategy::Refetch,
    );
    // The mutation key resolves to the literal "update_cart".
    let key_expr = island.exprs.client_expr(m.key_expr);
    assert!(matches!(key_expr, ClientExpr::Literal(Value::String(s)) if s == "update_cart"));

    // The invalidates entry resolves to the literal "cart" (matching
    // the cart query's key).
    let inv_expr = island.exprs.client_expr(m.invalidates[0]);
    assert!(matches!(inv_expr, ClientExpr::Literal(Value::String(s)) if s == "cart"));

    // The handler fires the mutation with the cart signal as its arg.
    let stmt = &island.handlers[0].body_ir().stmts[0];
    let ClientStmt::RunMutation { mutation, args } = stmt else {
        panic!("expected RunMutation; got {stmt:?}");
    };
    assert_eq!(*mutation, fixture.update_cart_mutation);
    assert_eq!(args.len(), 1);
    assert!(matches!(args[0], ClientExpr::SignalRead(s) if s == fixture.cart_signal));

    // The pending predicate lowers to MutationAccessor::IsPending.
    let pred = island.exprs.client_expr(fixture.pending_predicate_expr);
    let ClientExpr::MutationRead { id, accessor } = pred else {
        panic!("expected MutationRead; got {pred:?}");
    };
    assert_eq!(*id, fixture.update_cart_mutation);
    assert_eq!(*accessor, MutationAccessor::IsPending);

    // Sanity: the cart query is still wired and uses the data
    // accessor against the server initial literal value.
    assert_eq!(island.queries[0].id, fixture.cart_query);
    let _ = QueryAccessor::Data;
}

#[test]
fn update_cart_can_emit_handlers_through_client_ir_v3() {
    let fixture = build_update_cart_fixture();
    let js = emit_js::emit(&fixture.island, "updatecart");

    assert!(
        js.contains("__rsjs_mutation_0.run(__sig_0.get());"),
        "handler emission should emit mutation run, got:\n{js}"
    );
    assert!(
        js.contains("bind_delegated_events(root, \"click\""),
        "handler emission should preserve click binding, got:\n{js}"
    );
}
