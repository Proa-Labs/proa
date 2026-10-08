// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use rsjs_ir::client::{client_block_json, ClientObjectField, ClientType};

fn test_analyzer() -> Analyzer {
    Analyzer {
        builder: IslandBuilder::new(),
        shared_stores: HashMap::new(),
        shared_fields: HashMap::new(),
        signals: HashMap::new(),
        unsupported_integer_signals: HashMap::new(),
        memo_cells: HashSet::new(),
        queries: HashMap::new(),
        mutations: HashMap::new(),
        node_refs: HashMap::new(),
        inbound_node_refs: HashMap::new(),
        handlers: HashMap::new(),
        server_prelude_locals: HashSet::new(),
        client_prelude_locals: HashMap::new(),
        unsupported_client_prelude_locals: HashMap::new(),
        hir_sidecar: HirSidecar::default(),
        client_module_imports: Vec::new(),
        loop_scopes: Vec::new(),
        component_lifecycle_stack: Vec::new(),
        keyed_list_guards: Vec::new(),
        payload_counter: 0,
        next_handler_local_id: 0,
        direct_component_edges: Vec::new(),
        shared_state_provider_sites: Vec::new(),
        direct_helper_edges: Vec::new(),
        server_expressions: HashMap::new(),
        direct_component_site_nodes: HashMap::new(),
        inbound_inputs: InboundInputMetadata::default(),
        inbound_live_signals: HashMap::new(),
        generic_static_slot_field: None,
        inbound_temporal_context: InboundTemporalMode::LiveRead,
        preserve_direct_component_sites: false,
        is_async: false,
        client_origin_allowed: true,
        query_initializers: Vec::new(),
        mutation_initializers: Vec::new(),
        signal_initializers: Vec::new(),
        shared_state_initializers: Vec::new(),
        prelude_signal_ids: HashMap::new(),
        prelude_shared_store_ids: HashMap::new(),
        prelude_query_ids: HashMap::new(),
        prelude_mutation_ids: HashMap::new(),
        prelude_node_ref_ids: HashMap::new(),
        shared_prelude_local_indices: HashSet::new(),
        client_prelude_local_indices: HashSet::new(),
    }
}

fn local_from_stmt(stmt: syn::Stmt) -> syn::Local {
    match stmt {
        syn::Stmt::Local(local) => local,
        other => panic!("expected local statement, got {other:?}"),
    }
}

#[test]
fn inbound_signal_field_is_seed_once_not_live_read() {
    let (statements, lets) = prelude_from_block(syn::parse_quote!({
        let count = signal(self.initial);
    }));
    let mut analyzer = test_analyzer();
    analyzer
        .collect_prelude_bindings(&statements, &lets)
        .expect("signal seed should analyze");

    assert_eq!(analyzer.inbound_inputs.fields.len(), 1);
    assert_eq!(analyzer.inbound_inputs.fields[0].name, "initial");
    assert_eq!(
        analyzer.inbound_inputs.fields[0].role,
        InboundFieldRole::Value
    );
    assert_eq!(analyzer.inbound_inputs.prop_sites.len(), 1);
    assert_eq!(
        analyzer.inbound_inputs.prop_sites[0].temporal,
        InboundTemporalMode::SeedOnce
    );
    assert_eq!(
        analyzer.inbound_inputs.prop_sites[0].target,
        InboundPropTarget::SignalSeed(SignalId(0))
    );
}

#[test]
fn direct_inbound_field_reads_are_live_participation_and_reuse_one_signal() {
    let mut analyzer = test_analyzer();
    analyzer.client_origin_allowed = false;
    let expr: Expr = syn::parse_quote!(self.value);

    let first = analyzer
        .lower_reactive_expr(&expr)
        .expect("a direct inbound read is participation, not a client origin");
    let second = analyzer
        .lower_reactive_expr(&expr)
        .expect("repeated reads should reuse the same producer signal");

    assert_eq!(first, FrontendExpr::SignalRead(SignalId(0)));
    assert_eq!(second, FrontendExpr::SignalRead(SignalId(0)));
    assert_eq!(analyzer.signal_initializers.len(), 1);
    assert_eq!(
        analyzer.signal_initializers[0]
            .expr
            .to_token_stream()
            .to_string(),
        "self . value"
    );
    assert_eq!(analyzer.inbound_inputs.prop_sites.len(), 2);
    assert!(analyzer.inbound_inputs.prop_sites.iter().all(|site| {
        site.temporal == InboundTemporalMode::LiveRead
            && site.target == InboundPropTarget::LiveSignal(SignalId(0))
    }));
}

#[test]
fn inbound_query_key_field_is_a_query_dependency_and_reuses_the_live_signal() {
    let mut analyzer = test_analyzer();
    analyzer.client_origin_allowed = false;
    let key: Expr = syn::parse_quote! {
        rsjs::QueryKey::new(("resource", self.resource))
    };
    let ordinary_read: Expr = syn::parse_quote!(self.resource);
    let mutation_key: Expr = syn::parse_quote! {
        rsjs::QueryKey::new(("save-resource", self.resource))
    };

    let lowered_key = analyzer
        .lower_query_key_expr(&key)
        .expect("an inbound query-key dependency is participation, not a client origin");
    let lowered_read = analyzer
        .lower_reactive_expr(&ordinary_read)
        .expect("an ordinary inbound field read remains a live read");
    analyzer
        .lower_mutation_key_expr(&mutation_key)
        .expect("mutation metadata keeps ordinary current-value semantics");

    assert_eq!(
        lowered_key,
        FrontendExpr::QueryKey(vec![
            FrontendExpr::Literal(Value::String("resource".to_owned())),
            FrontendExpr::SignalRead(SignalId(0)),
        ])
    );
    assert_eq!(lowered_read, FrontendExpr::SignalRead(SignalId(0)));
    assert_eq!(
        analyzer.signal_initializers.len(),
        1,
        "query-key and ordinary reads must share one synthetic inbound signal"
    );
    assert_eq!(
        analyzer
            .inbound_inputs
            .prop_sites
            .iter()
            .map(|site| (site.temporal, site.target))
            .collect::<Vec<_>>(),
        vec![
            (
                InboundTemporalMode::QueryKeyDependency,
                InboundPropTarget::LiveSignal(SignalId(0)),
            ),
            (
                InboundTemporalMode::LiveRead,
                InboundPropTarget::LiveSignal(SignalId(0)),
            ),
            (
                InboundTemporalMode::LiveRead,
                InboundPropTarget::LiveSignal(SignalId(0)),
            ),
        ],
        "the query use must retain rebind semantics without changing ordinary reads or mutation metadata"
    );
}

#[test]
fn direct_inbound_projection_reuses_one_latent_identity_until_linking() {
    let mut analyzer = test_analyzer();
    analyzer.client_origin_allowed = false;
    let child: syn::ExprStruct = syn::parse_quote! {
        PresentationalChild {
            value: self.value,
            repeated: (self.value),
        }
    };

    analyzer
        .record_direct_component_struct(&child, DirectComponentBoundary::Composed)
        .expect("forwarding an inbound value is participation, not an origin");

    let projection = &analyzer.inbound_inputs.projection_fields[0];
    let edge = &analyzer.direct_component_edges[0];
    assert_eq!(
        edge.fields
            .iter()
            .map(|field| field.binding)
            .collect::<Vec<_>>(),
        vec![
            DirectComponentBindingClassification::DeferredProjection {
                field: projection.field,
                signal: projection.signal,
                node_ref: projection.node_ref,
            },
            DirectComponentBindingClassification::DeferredProjection {
                field: projection.field,
                signal: projection.signal,
                node_ref: projection.node_ref,
            },
        ],
        "each pass-through edge must retain the same authenticated latent identities"
    );
    assert_eq!(
        analyzer.signal_initializers.len(),
        1,
        "the Rust SSR value is evaluated once even when the inbound value is forwarded twice"
    );
    assert_eq!(
        analyzer.inbound_inputs.projection_fields.len(),
        1,
        "one parent field owns one deferred projection record"
    );
    assert!(
        analyzer.inbound_inputs.prop_sites.is_empty(),
        "temporal live-read sites are materialized only after the linked child proves a value role"
    );
}

#[test]
fn inbound_live_prop_is_not_a_writable_input_binding_source() {
    let mut analyzer = test_analyzer();
    analyzer.client_origin_allowed = false;
    let root = syn::parse2::<HtmlRoot>(quote::quote! {
        <input bind_value={self.value} />
    })
    .expect("test template should parse");

    let error = analyzer
        .walk_root(&root)
        .expect_err("a read-only inbound value cannot back a two-way DOM binding");
    assert!(
        error
            .to_string()
            .contains("expects a signal name (a bare ident)"),
        "writable bindings must continue to require a writable local signal: {error}"
    );
    assert!(
        analyzer.inbound_live_signals.is_empty(),
        "a rejected writable use must not synthesize a read-only live signal"
    );
}

#[test]
fn inbound_callback_dispatch_resolves_current_callback() {
    let mut analyzer = test_analyzer();
    let call: ExprMethodCall = syn::parse_quote!(self.on_change.call(1_i64));
    analyzer
        .lower_callback_slot_method_call(&call, &HandlerScope::default())
        .expect("callback use should analyze")
        .expect("callback call should be recognized");

    assert_eq!(analyzer.inbound_inputs.fields.len(), 1);
    assert_eq!(analyzer.inbound_inputs.fields[0].name, "on_change");
    assert_eq!(
        analyzer.inbound_inputs.fields[0].role,
        InboundFieldRole::Callback
    );
    assert_eq!(analyzer.inbound_inputs.fields[0].value, Some(0));
    assert_eq!(
        analyzer.inbound_inputs.fields[0].callback_shape,
        Some(InboundCallbackShape::Required)
    );
    assert_eq!(analyzer.inbound_inputs.values.len(), 1);
    assert_eq!(analyzer.inbound_inputs.values[0].field, 0);
    assert_eq!(
        analyzer.inbound_inputs.crossings[0].kind,
        InboundCrossingKind::CallbackResolveCurrent
    );
}

#[test]
fn inbound_optional_callback_preserves_shape_and_conflicts_fail_closed() {
    let mut analyzer = test_analyzer();
    let optional: ExprMethodCall = syn::parse_quote!(self.on_change.call_if_some(1_i64));
    analyzer
        .lower_callback_slot_method_call(&optional, &HandlerScope::default())
        .expect("optional callback use should analyze")
        .expect("optional callback call should be recognized");

    assert_eq!(
        analyzer.inbound_inputs.fields[0].callback_shape,
        Some(InboundCallbackShape::Optional)
    );

    let required: ExprMethodCall = syn::parse_quote!(self.on_change.call(1_i64));
    let error = match analyzer.lower_callback_slot_method_call(&required, &HandlerScope::default())
    {
        Err(error) => error,
        Ok(_) => panic!("one callback field cannot be both required and optional"),
    };
    assert!(error.to_string().contains("conflicting callback shapes"));
}

#[test]
fn dead_signal_seed_inputs_are_pruned_and_all_input_ids_are_compacted() {
    let mut analyzer = test_analyzer();
    let dead_signal = analyzer.builder.signal(Value::Int(0));
    analyzer
        .record_inbound_signal_seed(
            "dead".to_owned(),
            proc_macro2::Span::call_site(),
            dead_signal,
        )
        .unwrap();
    analyzer
        .record_inbound_callback(
            "on_change",
            proc_macro2::Span::call_site(),
            InboundCallbackShape::Optional,
        )
        .unwrap();
    let live_signal = analyzer.builder.signal(Value::Int(1));
    analyzer
        .record_inbound_signal_seed(
            "live".to_owned(),
            proc_macro2::Span::call_site(),
            live_signal,
        )
        .unwrap();
    analyzer
        .record_inbound_boundary_crossing("island_value".to_owned(), proc_macro2::Span::call_site())
        .unwrap();

    prune_dead_signal_seed_inputs(&mut analyzer.inbound_inputs, &[live_signal]);

    assert_eq!(
        analyzer
            .inbound_inputs
            .fields
            .iter()
            .map(|field| (field.id, field.name.as_str(), field.value, field.crossing))
            .collect::<Vec<_>>(),
        vec![
            (0, "on_change", Some(0), 0),
            (1, "live", Some(1), 1),
            (2, "island_value", Some(2), 2),
        ]
    );
    assert_eq!(
        analyzer
            .inbound_inputs
            .values
            .iter()
            .map(|value| (value.id, value.field))
            .collect::<Vec<_>>(),
        vec![(0, 0), (1, 1), (2, 2)]
    );
    assert_eq!(
        analyzer
            .inbound_inputs
            .crossings
            .iter()
            .map(|crossing| (crossing.id, crossing.field, crossing.kind))
            .collect::<Vec<_>>(),
        vec![
            (0, 0, InboundCrossingKind::CallbackResolveCurrent),
            (1, 1, InboundCrossingKind::Value),
            (2, 2, InboundCrossingKind::Value),
        ]
    );
    assert_eq!(
        analyzer
            .inbound_inputs
            .prop_sites
            .iter()
            .map(|site| (
                site.id,
                site.field,
                site.value,
                site.crossing,
                site.temporal,
                site.target
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                0,
                1,
                1,
                1,
                InboundTemporalMode::SeedOnce,
                InboundPropTarget::SignalSeed(live_signal),
            ),
            (
                1,
                2,
                2,
                2,
                InboundTemporalMode::ForbiddenBoundaryCrossing,
                InboundPropTarget::ExplicitIslandBoundary,
            ),
        ]
    );
    assert_eq!(
        analyzer.inbound_inputs.fields[0].callback_shape,
        Some(InboundCallbackShape::Optional)
    );
}

#[test]
fn explicit_island_self_field_is_static_at_the_edge_but_forbids_live_callers() {
    let mut analyzer = test_analyzer();
    let parent = analyzer.builder.element("section", None);
    let full_expr: Expr = syn::parse_quote!(rsjs::island(Child { value: self.value }));
    let Expr::Call(call) = &full_expr else {
        unreachable!()
    };
    analyzer
        .lower_explicit_island(call, &full_expr, parent)
        .expect("static Rust rendering remains valid");

    assert_eq!(
        analyzer.direct_component_edges[0].fields[0].binding,
        DirectComponentBindingClassification::ServerOnly,
        "a static caller may pass ordinary server data into an independent island"
    );
    assert!(
        analyzer.inbound_live_signals.is_empty() && analyzer.signal_initializers.is_empty(),
        "forbidden boundary crossings must not allocate an implicit snapshot signal"
    );
    assert_eq!(analyzer.inbound_inputs.prop_sites.len(), 1);
    assert_eq!(
        analyzer.inbound_inputs.prop_sites[0].temporal,
        InboundTemporalMode::ForbiddenBoundaryCrossing
    );
    assert_eq!(
        analyzer.inbound_inputs.prop_sites[0].target,
        InboundPropTarget::ExplicitIslandBoundary
    );
}

#[test]
fn shared_state_provider_is_server_topology_with_latent_live_seed_guards() {
    let mut analyzer = test_analyzer();
    analyzer.client_origin_allowed = false;
    let parent = analyzer.builder.element("section", None);
    let full_expr: Expr = syn::parse_quote! {
        rsjs::SharedStateProvider {
            state: WorkbenchState {
                mode: self.mode,
                enabled: self.enabled,
                tags: vec![self.tag],
            },
            children: WorkbenchChild {
                label: "static",
            },
        }
    };
    let Expr::Struct(provider) = &full_expr else {
        unreachable!()
    };

    analyzer
        .lower_shared_state_provider(provider, &full_expr, parent)
        .expect("static Rust values remain valid provider seeds");

    assert_eq!(analyzer.direct_component_edges.len(), 1);
    assert_eq!(analyzer.shared_state_provider_sites.len(), 1);
    let provider_site = &analyzer.shared_state_provider_sites[0];
    assert_eq!(provider_site.id.index(), 0);
    assert_eq!(provider_site.child_edge, 0);
    assert_eq!(
        analyzer.direct_component_edges[0].lifecycle,
        DirectComponentLifecycle::StaticSingle
    );
    assert_eq!(
        analyzer.direct_component_edges[0].fields[0].binding,
        DirectComponentBindingClassification::ServerOnly
    );
    assert_eq!(
        analyzer.direct_component_edges[0].site_node,
        Some(provider_site.site_node),
        "the ordinary child edge and provider claim share the whole provider server node"
    );
    assert!(
        analyzer.direct_component_site_nodes.is_empty(),
        "the whole provider expression must not enter ordinary LinkedComponent replacement"
    );
    assert!(
        analyzer.inbound_live_signals.is_empty()
            && analyzer.signal_initializers.is_empty()
            && analyzer.signals.is_empty(),
        "latent provider guards must not allocate a signal or browser origin"
    );
    assert_eq!(
        analyzer
            .inbound_inputs
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        vec!["mode", "enabled", "tag"]
    );
    assert!(analyzer.inbound_inputs.prop_sites.iter().all(|site| {
        site.temporal == InboundTemporalMode::ForbiddenProviderSeed
            && site.target == InboundPropTarget::SharedStateProviderSeed
    }));
    let preserved = analyzer
        .server_expressions
        .values()
        .next()
        .expect("the provider must remain an ordinary server expression")
        .to_token_stream()
        .to_string();
    assert!(
        preserved.contains("rsjs :: SharedStateProvider") && preserved.contains("WorkbenchChild"),
        "the lossless server expression preserves lexical provider rendering: {preserved}"
    );
}

#[test]
fn static_shared_state_provider_declares_no_client_owner_or_seed() {
    let mut analyzer = test_analyzer();
    analyzer.client_origin_allowed = false;
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <main>
            {rsjs::SharedStateProvider {
                state: WorkbenchState {
                    mode: "select".to_owned(),
                    enabled: true,
                    priorities: vec![1_u32, 2_u32],
                },
                children: WorkbenchChild {
                    label: "static",
                },
            }}
        </main>
    })
    .unwrap();

    analyzer
        .walk_root(&template)
        .expect("a static provider is ordinary server topology");
    assert!(analyzer.inbound_inputs.fields.is_empty());
    assert_eq!(analyzer.direct_component_edges.len(), 1);
    assert_eq!(analyzer.shared_state_provider_sites.len(), 1);
    assert!(analyzer.shared_state_initializers.is_empty());
    let island = analyzer.builder.build();
    assert!(
        rsjs_ir::classify_render_strategy(&island).is_server_only(),
        "provider declaration alone must not create a browser obligation or owner"
    );
}

#[test]
fn shared_state_provider_seed_rejects_every_direct_client_source() {
    fn reject(mut analyzer: Analyzer, state: Expr, expected: &str) {
        let parent = analyzer.builder.element("section", None);
        let full_expr: Expr = syn::parse_quote! {
            rsjs::SharedStateProvider {
                state: ProviderTestState { value: #state },
                children: WorkbenchChild {
                    label: "static",
                },
            }
        };
        let Expr::Struct(provider) = &full_expr else {
            unreachable!()
        };
        let error = analyzer
            .lower_shared_state_provider(provider, &full_expr, parent)
            .expect_err("a provider seed must not snapshot client-owned state");
        assert!(
            error.to_string().contains(expected),
            "unexpected provider-seed diagnostic: {error}"
        );
    }

    let mut signal = test_analyzer();
    signal.signals.insert("count".to_owned(), SignalId(0));
    reject(
        signal,
        syn::parse_quote!(WorkbenchState { value: count.get() }),
        "reactive signal `count`",
    );

    let mut signal_in_macro = test_analyzer();
    signal_in_macro
        .signals
        .insert("count".to_owned(), SignalId(0));
    reject(
        signal_in_macro,
        syn::parse_quote!(WorkbenchState {
            values: vec![count.get()],
        }),
        "reactive signal `count`",
    );

    let mut query = test_analyzer();
    query.queries.insert("result".to_owned(), QueryId(0));
    reject(
        query,
        syn::parse_quote!(WorkbenchState { value: result }),
        "query client handle `result`",
    );

    let mut mutation = test_analyzer();
    mutation.mutations.insert("save".to_owned(), MutationId(0));
    reject(
        mutation,
        syn::parse_quote!(WorkbenchState { value: save }),
        "mutation client handle `save`",
    );

    let mut node_ref = test_analyzer();
    node_ref.node_refs.insert("target".to_owned(), NodeRefId(0));
    reject(
        node_ref,
        syn::parse_quote!(WorkbenchState { value: target }),
        "node-ref client handle `target`",
    );

    let mut shared = test_analyzer();
    shared
        .shared_stores
        .insert("workbench".to_owned(), SharedStoreId(0));
    reject(
        shared,
        syn::parse_quote!(WorkbenchState { value: workbench }),
        "SharedState client handle `workbench`",
    );

    reject(
        test_analyzer(),
        syn::parse_quote!(rsjs::shared::<WorkbenchState>()),
        "`rsjs::shared(...)` state",
    );
}

#[test]
fn shared_state_provider_children_fail_closed_without_hiding_descendants() {
    fn reject(mut analyzer: Analyzer, children: Expr, expected: &str) {
        let parent = analyzer.builder.element("section", None);
        let full_expr: Expr = syn::parse_quote! {
            rsjs::SharedStateProvider {
                state: WorkbenchState { enabled: true },
                children: #children,
            }
        };
        let Expr::Struct(provider) = &full_expr else {
            unreachable!()
        };
        let error = analyzer
            .lower_shared_state_provider(provider, &full_expr, parent)
            .expect_err("unsupported provider children must fail closed");
        assert!(
            error.to_string().contains(expected),
            "unexpected provider-child diagnostic: {error}"
        );
    }

    reject(
        test_analyzer(),
        syn::parse_quote!(render_children()),
        "requires one direct component struct literal",
    );
    reject(
        test_analyzer(),
        syn::parse_quote!(html_sync! { <div>"hidden"</div> }),
        "requires one direct component struct literal",
    );

    let mut live = test_analyzer();
    live.signals.insert("count".to_owned(), SignalId(0));
    reject(
        live,
        syn::parse_quote!(WorkbenchChild { value: count.get() }),
        "cannot capture enclosing client state",
    );
    reject(
        test_analyzer(),
        syn::parse_quote!(WorkbenchChild { value: self.value }),
        "cannot capture inbound field `self.value`",
    );
    reject(
        test_analyzer(),
        syn::parse_quote!(rsjs::SharedStateProvider {
            state: NestedState { value: true },
            children: WorkbenchChild { label: "nested" },
        }),
        "requires one direct component struct literal",
    );
}

#[test]
fn shared_state_provider_requires_nameable_state_and_static_topology() {
    let mut non_nameable = test_analyzer();
    let parent = non_nameable.builder.element("section", None);
    let full_expr: Expr = syn::parse_quote! {
        rsjs::SharedStateProvider {
            state: WorkbenchState::default(),
            children: WorkbenchChild { label: "static" },
        }
    };
    let Expr::Struct(provider) = &full_expr else {
        unreachable!()
    };
    let error = non_nameable
        .lower_shared_state_provider(provider, &full_expr, parent)
        .expect_err("constructor calls do not expose a provider schema witness type");
    assert!(
        error
            .to_string()
            .contains("named SharedState struct literal"),
        "unexpected state-shape diagnostic: {error}"
    );

    let mut conditional = test_analyzer();
    conditional
        .component_lifecycle_stack
        .push(DirectConditionalLifecycleScope {
            parent: NodeId(0),
            section_ordinal: 0,
            branch: DirectConditionalBranch::Then,
        });
    let parent = conditional.builder.element("section", None);
    let full_expr: Expr = syn::parse_quote! {
        rsjs::SharedStateProvider {
            state: WorkbenchState { enabled: true },
            children: WorkbenchChild { label: "static" },
        }
    };
    let Expr::Struct(provider) = &full_expr else {
        unreachable!()
    };
    let error = conditional
        .lower_shared_state_provider(provider, &full_expr, parent)
        .expect_err("conditional provider topology is unsupported in v1");
    assert!(
        error.to_string().contains("must be statically present"),
        "unexpected conditional provider diagnostic: {error}"
    );

    let mut repeated = test_analyzer();
    repeated.loop_scopes.push(LoopScope {
        locals: HashSet::new(),
        integer_domains: HashMap::new(),
    });
    let parent = repeated.builder.element("section", None);
    let error = repeated
        .lower_shared_state_provider(provider, &full_expr, parent)
        .expect_err("repeated provider topology is unsupported in v1");
    assert!(
        error.to_string().contains("must be statically present"),
        "unexpected repeated provider diagnostic: {error}"
    );
}

#[test]
fn explicit_island_rejects_parent_client_handles_with_focused_diagnostics() {
    fn reject(mut analyzer: Analyzer, expr: Expr, expected: &str) {
        let parent = analyzer.builder.element("section", None);
        let Expr::Call(call) = &expr else {
            unreachable!()
        };
        let error = analyzer
            .lower_explicit_island(call, &expr, parent)
            .expect_err("client handles must not cross an independent owner cut");
        assert!(
            error.to_string().contains(expected),
            "unexpected boundary diagnostic: {error}"
        );
    }

    let mut query = test_analyzer();
    query.queries.insert("price".to_owned(), QueryId(0));
    reject(
        query,
        syn::parse_quote!(rsjs::island(Child { value: price })),
        "parent query handle `price`",
    );

    let mut mutation = test_analyzer();
    mutation.mutations.insert("save".to_owned(), MutationId(0));
    reject(
        mutation,
        syn::parse_quote!(rsjs::island(Child { value: save })),
        "parent mutation handle `save`",
    );

    let mut node_ref = test_analyzer();
    node_ref.node_refs.insert("target".to_owned(), NodeRefId(0));
    reject(
        node_ref,
        syn::parse_quote!(rsjs::island(Child { value: target })),
        "parent node-ref handle `target`",
    );

    let mut shared = test_analyzer();
    shared
        .shared_stores
        .insert("workbench".to_owned(), SharedStoreId(0));
    reject(
        shared,
        syn::parse_quote!(rsjs::island(Child { value: workbench })),
        "parent SharedState handle `workbench`",
    );
}

#[test]
fn inbound_value_and_callback_role_conflict_fails_closed() {
    let (statements, lets) = prelude_from_block(syn::parse_quote!({
        let local = signal(self.on_change);
    }));
    let mut analyzer = test_analyzer();
    analyzer
        .collect_prelude_bindings(&statements, &lets)
        .expect("value role should be recorded");
    let call: ExprMethodCall = syn::parse_quote!(self.on_change.call(1_i64));
    let error = match analyzer.lower_callback_slot_method_call(&call, &HandlerScope::default()) {
        Ok(_) => panic!("one field cannot be both a value and callback"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("conflicting client roles"));
}

fn prelude_from_block(block: syn::Block) -> (Vec<Stmt>, Vec<Local>) {
    let statements = block.stmts;
    let lets = statements
        .iter()
        .filter_map(|statement| match statement {
            Stmt::Local(local) => Some(local.clone()),
            _ => None,
        })
        .collect();
    (statements, lets)
}

#[test]
fn opaque_server_slot_keeps_its_exact_syn_expression_beside_redacted_ir() {
    let mut analyzer = test_analyzer();
    let root = analyzer.builder.element("main", None);
    analyzer.builder.set_root(root);
    let expression: Expr = syn::parse_quote! {
        render_markdown(self.document)
    };
    let expected_anchor = server_surface_anchor(expression.span());

    analyzer
        .lower_text_binding_expr(&expression, root, false)
        .expect("ordinary server Rust should lower as an opaque server slot");

    let server_expressions = analyzer.server_expressions.clone();
    let island = analyzer.builder.build();
    let ViewNode::Element { children, .. } = island.view_tree.get(root) else {
        panic!("expected the test root to remain an element");
    };
    let [server_node] = children.as_slice() else {
        panic!("expected exactly one opaque server child, got {children:?}");
    };
    let preserved = server_expressions
        .get(server_node)
        .expect("the macro-side expression table must retain the authored expression");

    assert_eq!(
        preserved.to_token_stream().to_string(),
        expression.to_token_stream().to_string()
    );
    let ViewNode::ServerOnly(server) = island.view_tree.get(*server_node) else {
        panic!("expected the parallel IR node to remain server-only");
    };
    assert_eq!(
        server.source_anchor,
        Some(expected_anchor),
        "diagnostic metadata must retain the authored expression's source location"
    );
    assert_eq!(
        server.tokens,
        expression.to_token_stream().to_string(),
        "the serializable IR keeps only diagnostic/descriptor tokens; Rust emission uses the side table"
    );
}

#[test]
fn explicit_self_field_boundary_retains_simple_ssr_provenance() {
    let mut analyzer = test_analyzer();
    let root = analyzer.builder.element("main", None);
    analyzer.builder.set_root(root);
    let expression: Expr = syn::parse_quote! {
        rsjs::server_only(self.label)
    };
    let Expr::Call(call) = &expression else {
        panic!("fixture should be a call");
    };

    analyzer
        .lower_server_only(call, &expression, root)
        .expect("an explicit direct self-field snapshot is valid SSR");

    let island = analyzer.builder.build();
    let ViewNode::Element { children, .. } = island.view_tree.get(root) else {
        panic!("expected the test root to remain an element");
    };
    let [server_node] = children.as_slice() else {
        panic!("expected one explicit server child");
    };
    let ViewNode::ServerOnly(surface) = island.view_tree.get(*server_node) else {
        panic!("expected an explicit server surface");
    };
    assert_eq!(
        surface.source,
        rsjs_ir::ServerSurfaceSource::ExplicitSimpleSsrBoundary
    );
}

#[test]
fn direct_component_metadata_classifies_live_values_and_static_server_sites() {
    let mut analyzer = test_analyzer();
    let static_child: syn::ExprStruct = syn::parse_quote! {
        ImportedBadge {
            label: self.title,
        }
    };
    analyzer
        .record_direct_component_struct(&static_child, DirectComponentBoundary::Composed)
        .unwrap();
    let static_edge = &analyzer.direct_component_edges[0];
    assert!(matches!(
        static_edge.lifecycle,
        DirectComponentLifecycle::StaticSingle
    ));
    assert!(matches!(
        static_edge.fields[0].binding,
        DirectComponentBindingClassification::DeferredProjection {
            field: 0,
            signal: SignalId(0),
            node_ref: NodeRefId(0),
        }
    ));

    analyzer
        .signals
        .insert("live_label".to_owned(), SignalId(1));
    let reactive_child: syn::ExprStruct = syn::parse_quote! {
        ImportedBadge {
            label: live_label.get(),
        }
    };
    analyzer
        .record_direct_component_struct(&reactive_child, DirectComponentBoundary::Composed)
        .unwrap();
    assert_eq!(
        analyzer.direct_component_edges[1].fields[0].binding,
        DirectComponentBindingClassification::LiveValue {
            signal: SignalId(1),
            source: DirectLiveValueSource::SignalRead,
        }
    );

    let static_island: syn::ExprStruct = syn::parse_quote! {
        Disclosure {
            children: html_sync! {
                <article class="prose">
                    <h2>"Terms"</h2>
                    <p>"Rendered once on the server."</p>
                </article>
            },
        }
    };
    analyzer
        .record_direct_component_struct(&static_island, DirectComponentBoundary::ExplicitIsland)
        .unwrap();
    assert!(matches!(
        analyzer.direct_component_edges[2].fields[0].binding,
        DirectComponentBindingClassification::StaticSlotExactOne
    ));

    let dynamic_island: syn::ExprStruct = syn::parse_quote! {
        Disclosure {
            children: html_sync! {
                <article>{live_label.get()}</article>
            },
        }
    };
    analyzer
        .record_direct_component_struct(&dynamic_island, DirectComponentBoundary::ExplicitIsland)
        .unwrap();
    assert!(matches!(
        analyzer.direct_component_edges[3].fields[0].binding,
        DirectComponentBindingClassification::Unsupported
    ));

    let dynamic_attr_island: syn::ExprStruct = syn::parse_quote! {
        Disclosure {
            children: html_sync! {
                <article class={self.article_class}>"Terms"</article>
            },
        }
    };
    analyzer
        .record_direct_component_struct(
            &dynamic_attr_island,
            DirectComponentBoundary::ExplicitIsland,
        )
        .unwrap();
    assert!(matches!(
        analyzer.direct_component_edges[4].fields[0].binding,
        DirectComponentBindingClassification::Unsupported
    ));

    let arbitrary_island: syn::ExprStruct = syn::parse_quote! {
        Disclosure {
            children: make_server_fragment(),
        }
    };
    analyzer
        .record_direct_component_struct(&arbitrary_island, DirectComponentBoundary::ExplicitIsland)
        .unwrap();
    assert!(matches!(
        analyzer.direct_component_edges[5].fields[0].binding,
        DirectComponentBindingClassification::Unsupported
    ));

    let multi_root_island: syn::ExprStruct = syn::parse_quote! {
        Disclosure {
            children: html_sync! {
                <h2>"Terms"</h2>
                <p>"Rendered once on the server."</p>
            },
        }
    };
    analyzer
        .record_direct_component_struct(&multi_root_island, DirectComponentBoundary::ExplicitIsland)
        .unwrap();
    assert!(matches!(
        analyzer.direct_component_edges[6].fields[0].binding,
        DirectComponentBindingClassification::Unsupported
    ));

    analyzer
        .component_lifecycle_stack
        .push(DirectConditionalLifecycleScope {
            parent: NodeId(0),
            section_ordinal: 0,
            branch: DirectConditionalBranch::Then,
        });
    analyzer
        .record_direct_component_struct(&static_child, DirectComponentBoundary::Composed)
        .unwrap();
    assert_eq!(
        analyzer.direct_component_edges[7].lifecycle,
        DirectComponentLifecycle::ConditionalSingle(DirectConditionalLifecycleScope {
            parent: NodeId(0),
            section_ordinal: 0,
            branch: DirectConditionalBranch::Then,
        })
    );
}

#[test]
fn conditional_direct_component_sites_retain_branch_polarity_and_site_nodes() {
    let mut analyzer = test_analyzer();
    analyzer.preserve_direct_component_sites = true;
    let visible = analyzer.builder.signal(Value::Bool(true));
    analyzer.signals.insert("visible".to_owned(), visible);
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <main>
            {if visible.get() {
                html_sync! {
                    <section>{ThenBadge { label: "then" }}</section>
                }
            } else {
                html_sync! {
                    <aside>{ElseBadge { label: "else" }}</aside>
                }
            }}
        </main>
    })
    .unwrap();

    analyzer.walk_root(&template).unwrap();

    assert_eq!(analyzer.direct_component_edges.len(), 2);
    assert_eq!(
        analyzer.direct_component_edges[0].lifecycle,
        DirectComponentLifecycle::ConditionalSingle(DirectConditionalLifecycleScope {
            parent: NodeId(0),
            section_ordinal: 0,
            branch: DirectConditionalBranch::Then,
        })
    );
    assert_eq!(
        analyzer.direct_component_edges[1].lifecycle,
        DirectComponentLifecycle::ConditionalSingle(DirectConditionalLifecycleScope {
            parent: NodeId(0),
            section_ordinal: 0,
            branch: DirectConditionalBranch::Else,
        })
    );
    for edge in &analyzer.direct_component_edges {
        let site = edge
            .site_node
            .expect("each direct edge keeps its exact view site");
        assert_eq!(
            analyzer.direct_component_site_nodes.get(&site),
            Some(&edge.local_id)
        );
        let ViewNode::ServerOnly(surface) = analyzer.builder.view_node(site) else {
            panic!("direct component site must remain a server surface");
        };
        assert_eq!(
            surface.source,
            rsjs_ir::ServerSurfaceSource::DirectComponentSite
        );
    }
}

#[test]
fn flattened_else_if_final_component_uses_its_canonical_then_control() {
    let mut analyzer = test_analyzer();
    analyzer.preserve_direct_component_sites = true;
    let first = analyzer.builder.signal(Value::Bool(false));
    let second = analyzer.builder.signal(Value::Bool(false));
    analyzer.signals.insert("first".to_owned(), first);
    analyzer.signals.insert("second".to_owned(), second);
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <main>
            {if first.get() {
                html_sync! { <p>"first"</p> }
            } else if second.get() {
                html_sync! { <p>"second"</p> }
            } else {
                html_sync! {
                    <section>{FinalBadge { label: "final" }}</section>
                }
            }}
        </main>
    })
    .unwrap();

    analyzer.walk_root(&template).unwrap();

    let [edge] = analyzer.direct_component_edges.as_slice() else {
        panic!("the final else-if branch should retain one component edge");
    };
    assert_eq!(
        edge.lifecycle,
        DirectComponentLifecycle::ConditionalSingle(DirectConditionalLifecycleScope {
            parent: NodeId(0),
            section_ordinal: 2,
            branch: DirectConditionalBranch::Then,
        })
    );
    assert!(edge.site_node.is_some());
}

#[test]
fn nested_conditional_component_site_is_conservatively_unknown() {
    let mut analyzer = test_analyzer();
    analyzer.preserve_direct_component_sites = true;
    let outer = analyzer.builder.signal(Value::Bool(true));
    let inner = analyzer.builder.signal(Value::Bool(true));
    analyzer.signals.insert("outer".to_owned(), outer);
    analyzer.signals.insert("inner".to_owned(), inner);
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <main>
            {if outer.get() {
                html_sync! {
                    <section>
                        {if inner.get() {
                            html_sync! {
                                <div>{NestedBadge { label: "nested" }}</div>
                            }
                        } else {
                            html_sync! { <div>"inner else"</div> }
                        }}
                    </section>
                }
            } else {
                html_sync! { <aside>"outer else"</aside> }
            }}
        </main>
    })
    .unwrap();

    analyzer.walk_root(&template).unwrap();

    let [edge] = analyzer.direct_component_edges.as_slice() else {
        panic!("the nested branch should retain one component edge");
    };
    assert_eq!(
        edge.lifecycle,
        DirectComponentLifecycle::UnknownNestedConditionals
    );
    assert!(edge.site_node.is_some());
    assert_eq!(
        analyzer.builder.bindings().conditional_sections[&NodeId(0)].len(),
        1,
        "the outer control retains its reserved ordinal even when its branch lowers another control"
    );
}

#[test]
fn top_level_component_branch_fails_instead_of_disappearing() {
    let mut analyzer = test_analyzer();
    analyzer.preserve_direct_component_sites = true;
    let visible = analyzer.builder.signal(Value::Bool(true));
    analyzer.signals.insert("visible".to_owned(), visible);
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <main>
            {if visible.get() {
                html_sync! { {DirectBadge { label: "visible" }} }
            } else {
                html_sync! { <aside>"hidden"</aside> }
            }}
        </main>
    })
    .unwrap();

    let error = analyzer
        .walk_root(&template)
        .expect_err("an unmarkerable direct branch root must fail explicitly");
    let message = error.to_string();
    assert!(
        message.contains("conditional branch cannot use a renderable expression as its DOM root"),
        "{message}"
    );
    assert!(message.contains("Wrap the expression"), "{message}");
    assert!(
        analyzer.direct_component_edges.is_empty(),
        "the unsupported block must not leave a half-recorded edge"
    );
}

#[test]
fn direct_component_inside_static_template_loop_is_repeated() {
    let mut analyzer = test_analyzer();
    analyzer.preserve_direct_component_sites = true;
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <main>
            {for label in labels {
                html_sync! {
                    <section>{LoopBadge { label }}</section>
                }
            }}
        </main>
    })
    .unwrap();

    analyzer.walk_root(&template).unwrap();

    let [edge] = analyzer.direct_component_edges.as_slice() else {
        panic!("the loop body should retain one source component edge");
    };
    assert_eq!(edge.lifecycle, DirectComponentLifecycle::Repeated);
    assert!(edge.site_node.is_some());
}

#[test]
fn direct_component_binding_provenance_is_exact_and_island_safe() {
    let (statements, lets) = prelude_from_block(syn::parse_quote!({
        let count = signal(0_i64);
        let live_alias = count.get();
        let derived_alias = count.get() + 1_i64;
        let on_change = rsjs::handler(|next: i64| count.set(next));
        let on_click = rsjs::event_handler(|_: rsjs::Event| count.set(1_i64));
    }));
    let mut analyzer = test_analyzer();
    analyzer
        .collect_prelude_bindings(&statements, &lets)
        .expect("direct binding fixtures should analyze");

    let composed: syn::ExprStruct = syn::parse_quote! {
        Child {
            direct: count.get(),
            alias_value: live_alias,
            derived: derived_alias,
            callback: on_change,
            optional_callback: Some(on_change),
            absent_callback: None,
            noop_callback: rsjs::noop_handler(),
            optional_noop_callback: Some(rsjs::noop_handler()),
            dom_handler: on_click,
        }
    };
    analyzer
        .record_direct_component_struct(&composed, DirectComponentBoundary::Composed)
        .unwrap();
    let bindings = analyzer.direct_component_edges[0]
        .fields
        .iter()
        .map(|field| field.binding)
        .collect::<Vec<_>>();

    assert_eq!(
        bindings[0],
        DirectComponentBindingClassification::LiveValue {
            signal: SignalId(0),
            source: DirectLiveValueSource::SignalRead,
        }
    );
    assert_eq!(
        bindings[1],
        DirectComponentBindingClassification::LiveValue {
            signal: SignalId(0),
            source: DirectLiveValueSource::ClientAlias,
        },
        "an exact alias must preserve the original parent signal identity"
    );
    assert_eq!(
        bindings[2],
        DirectComponentBindingClassification::Unsupported,
        "a derived alias has no stable parent value id yet"
    );
    assert_eq!(
        bindings[3],
        DirectComponentBindingClassification::LiveCallback {
            // `count`, `live_alias`, and `derived_alias` occupy the first
            // three authored prelude `let` positions.
            handler_prelude_local: 3,
            form: DirectLiveCallbackForm::Handler,
        }
    );
    assert_eq!(
        bindings[4],
        DirectComponentBindingClassification::LiveCallback {
            handler_prelude_local: 3,
            form: DirectLiveCallbackForm::SomeHandler,
        }
    );
    assert_eq!(
        bindings[5],
        DirectComponentBindingClassification::StaticCallback {
            form: DirectStaticCallbackForm::None,
        }
    );
    assert_eq!(
        bindings[6],
        DirectComponentBindingClassification::StaticCallback {
            form: DirectStaticCallbackForm::NoopHandler,
        }
    );
    assert_eq!(
        bindings[7],
        DirectComponentBindingClassification::StaticCallback {
            form: DirectStaticCallbackForm::SomeNoopHandler,
        }
    );
    assert_eq!(
        bindings[8],
        DirectComponentBindingClassification::Unsupported,
        "`rsjs::event_handler(...)` values are DOM handlers, not callback props"
    );

    let independent: syn::ExprStruct = syn::parse_quote! {
        Child {
            direct: count.get(),
            callback: on_change,
            optional_callback: Some(on_change),
            absent_callback: None,
            noop_callback: rsjs::noop_handler(),
        }
    };
    analyzer
        .record_direct_component_struct(&independent, DirectComponentBoundary::ExplicitIsland)
        .unwrap();
    let independent_bindings = analyzer.direct_component_edges[1]
        .fields
        .iter()
        .map(|field| field.binding)
        .collect::<Vec<_>>();
    assert_eq!(
        &independent_bindings[..3],
        &[
            DirectComponentBindingClassification::Unsupported,
            DirectComponentBindingClassification::Unsupported,
            DirectComponentBindingClassification::Unsupported,
        ],
        "live values and callbacks must not cross an explicit island"
    );
    assert_eq!(
        independent_bindings[3],
        DirectComponentBindingClassification::StaticCallback {
            form: DirectStaticCallbackForm::None,
        }
    );
    assert_eq!(
        independent_bindings[4],
        DirectComponentBindingClassification::StaticCallback {
            form: DirectStaticCallbackForm::NoopHandler,
        }
    );

    let mut repeated = test_analyzer();
    repeated
        .collect_prelude_bindings(&statements, &lets)
        .expect("repeated direct binding fixtures should analyze");
    repeated
        .record_direct_component_struct(&composed, DirectComponentBoundary::Composed)
        .unwrap();
    let repeated_bindings = repeated.direct_component_edges[0]
        .fields
        .iter()
        .map(|field| field.binding)
        .collect::<Vec<_>>();
    assert_eq!(
        bindings, repeated_bindings,
        "payload-handler prelude identity must be stable across equivalent analyses; \
         the canonical adapter still has to materialize that source binding as a graph value"
    );

    let values = analyzer
        .materialize_direct_payload_handler_values()
        .expect("referenced payload handler should materialize");
    assert_eq!(
        values.len(),
        1,
        "two callback fields share one source value"
    );
    assert_eq!(values[0].prelude_local_index, 3);
    assert_eq!(values[0].payload_arity, 1);
    assert!(
        client_block_json(&values[0].body_ir).contains("\"kind\":\"callback_payload\""),
        "the authored payload parameter must lower to the canonical callback payload value"
    );
    assert!(
        matches!(
            values[0].body_ir.stmts.as_slice(),
            [ClientStmt::SignalSet {
                id: SignalId(0),
                ..
            }]
        ),
        "the callback body must retain its exact signal write"
    );
    assert!(values[0].dependencies.signal_reads.is_empty());
    assert_eq!(values[0].dependencies.signal_writes, [SignalId(0)]);
    assert!(
        values[0].dependencies.helper_edges.is_empty(),
        "the DOM event handler and unreferenced declarations must not leak into callback metadata"
    );
}

#[test]
fn direct_payload_handler_materialization_fails_closed_on_missing_or_duplicate_sources() {
    let mut missing = test_analyzer();
    missing
        .direct_component_edges
        .push(DirectComponentSourceEdge {
            local_id: 0,
            component_ty: syn::parse_quote!(Child),
            fields: vec![DirectComponentSourceField {
                name: "on_change".to_string(),
                binding: DirectComponentBindingClassification::LiveCallback {
                    handler_prelude_local: 41,
                    form: DirectLiveCallbackForm::Handler,
                },
            }],
            has_rest: false,
            boundary: DirectComponentBoundary::Composed,
            lifecycle: DirectComponentLifecycle::StaticSingle,
            site_node: None,
            source_anchor: rsjs_ir::SourceAnchor {
                start_line: 1,
                start_column: 1,
            },
        });
    let error = missing
        .materialize_direct_payload_handler_values()
        .expect_err("a referenced source index must exist");
    assert!(
        error
            .to_string()
            .contains("direct-live-callback-missing-source"),
        "got: {error}"
    );

    let mut duplicate = test_analyzer();
    duplicate
        .direct_component_edges
        .push(DirectComponentSourceEdge {
            local_id: 0,
            component_ty: syn::parse_quote!(Child),
            fields: vec![DirectComponentSourceField {
                name: "on_change".to_string(),
                binding: DirectComponentBindingClassification::LiveCallback {
                    handler_prelude_local: 0,
                    form: DirectLiveCallbackForm::Handler,
                },
            }],
            has_rest: false,
            boundary: DirectComponentBoundary::Composed,
            lifecycle: DirectComponentLifecycle::StaticSingle,
            site_node: None,
            source_anchor: rsjs_ir::SourceAnchor {
                start_line: 1,
                start_column: 1,
            },
        });
    for name in ["first", "second"] {
        duplicate.handlers.insert(
            name.to_string(),
            HandlerLocal {
                name: syn::Ident::new(name, proc_macro2::Span::call_site()),
                closure: syn::parse_quote!(|value| value),
                kind: HandlerLocalKind::Payload,
                prelude_local_index: 0,
                source_anchor: rsjs_ir::SourceAnchor {
                    start_line: 1,
                    start_column: 1,
                },
                capture_env: None,
            },
        );
    }
    let error = duplicate
        .materialize_direct_payload_handler_values()
        .expect_err("duplicate source indices must fail closed");
    assert!(
        error
            .to_string()
            .contains("direct-live-callback-duplicate-source"),
        "got: {error}"
    );
}

#[test]
fn final_link_analysis_preserves_direct_component_site_and_live_inbound_identity() {
    let mut analyzer = test_analyzer();
    analyzer.preserve_direct_component_sites = true;
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <main>
            {ImportedBadge {
                label: self.title,
            }}
        </main>
    })
    .unwrap();

    analyzer.walk_root(&template).unwrap();

    assert_eq!(analyzer.direct_component_edges.len(), 1);
    assert_eq!(analyzer.direct_component_site_nodes.len(), 1);
    let (&node, &edge) = analyzer
        .direct_component_site_nodes
        .iter()
        .next()
        .expect("one preserved direct component site");
    assert_eq!(edge, 0);
    let ViewNode::ServerOnly(site) = analyzer.builder.view_node(node) else {
        panic!("a preserved direct component site must remain a server render node");
    };
    assert_eq!(
        site.source,
        rsjs_ir::ServerSurfaceSource::DirectComponentSite,
        "direct composition needs provenance distinct from arbitrary opaque SSR"
    );
    let preserved = analyzer
        .server_expressions
        .get(&node)
        .expect("the direct component must retain its server replay expression")
        .to_token_stream()
        .to_string();
    assert!(preserved.contains("ImportedBadge"), "{preserved}");
    assert!(
        preserved.contains("__rsjs_initial_signal_0"),
        "SSR replays the initial inbound value while transport keeps the live edge: {preserved}"
    );
    assert!(
        !preserved.contains("Clone"),
        "an exact inbound pass-through preserves Rust move semantics instead of inventing a \
         `Clone` requirement: {preserved}"
    );
    assert_eq!(
        analyzer.direct_component_edges[0].fields[0].binding,
        DirectComponentBindingClassification::DeferredProjection {
            field: 0,
            signal: SignalId(0),
            node_ref: NodeRefId(0),
        }
    );
    assert_eq!(
        analyzer.signal_initializers[0]
            .expr
            .to_token_stream()
            .to_string(),
        "self . title",
        "the inbound Rust expression remains the one source of the SSR snapshot"
    );
}

#[test]
fn one_node_ref_cannot_bind_two_static_elements() {
    let mut analyzer = test_analyzer();
    let target = analyzer.builder.node_ref();
    analyzer.node_refs.insert("target".to_string(), target);
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <section>
            <input ref={target} />
            <button ref={target}>"Focus"</button>
        </section>
    })
    .unwrap();

    let error = analyzer
        .walk_root(&template)
        .expect_err("one node ref must not have two static DOM targets");
    let message = error.to_string();
    assert!(
        message.contains("already bound to another element"),
        "{message}"
    );
    assert!(
        message.contains("exactly one stable DOM element"),
        "{message}"
    );
}

#[test]
fn one_node_ref_cannot_bind_both_conditional_branches() {
    let mut analyzer = test_analyzer();
    let visible = analyzer.builder.signal(Value::Bool(false));
    analyzer.signals.insert("visible".to_string(), visible);
    let target = analyzer.builder.node_ref();
    analyzer.node_refs.insert("target".to_string(), target);
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <section>
            {if visible.get() {
                html_sync! { <input ref={target} /> }
            } else {
                html_sync! { <button ref={target}>"Focus"</button> }
            }}
        </section>
    })
    .unwrap();

    let error = analyzer
        .walk_root(&template)
        .expect_err("conditional branches cannot share one node-ref target");
    let message = error.to_string();
    assert!(
        message.contains("already bound to another element"),
        "{message}"
    );
    assert!(message.contains("conditional branches"), "{message}");
}

#[test]
fn required_inbound_node_ref_is_participation_and_records_its_exact_target() {
    let mut analyzer = test_analyzer();
    analyzer.client_origin_allowed = false;
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <input ref={self.input_ref} />
    })
    .unwrap();

    analyzer
        .walk_root(&template)
        .expect("consuming a required inbound NodeRef<T> must not require `client`");

    assert_eq!(analyzer.inbound_inputs.fields.len(), 1);
    assert_eq!(analyzer.inbound_inputs.fields[0].name, "input_ref");
    assert_eq!(
        analyzer.inbound_inputs.fields[0].role,
        InboundFieldRole::NodeRef
    );
    assert!(
        analyzer.inbound_inputs.prop_sites.is_empty(),
        "a handle alias is not a serialized/live value prop"
    );
    assert_eq!(analyzer.inbound_inputs.node_ref_fields.len(), 1);
    assert_eq!(
        analyzer.inbound_inputs.node_ref_fields[0].bind_target,
        Some(NodeRefBindTarget::Input)
    );
}

#[test]
fn direct_node_ref_component_fields_preserve_one_parent_identity() {
    let mut analyzer = test_analyzer();
    let target = analyzer.builder.node_ref();
    analyzer.node_refs.insert("target".to_string(), target);
    let child: syn::ExprStruct = syn::parse_quote! {
        FocusControl { target }
    };

    analyzer
        .record_direct_component_struct(&child, DirectComponentBoundary::Composed)
        .expect("a required ref may cross one direct static component edge");

    assert_eq!(
        analyzer.direct_component_edges[0].fields[0].binding,
        DirectComponentBindingClassification::NodeRefHandle { node_ref: target }
    );
}

#[test]
fn inbound_projection_can_pass_through_without_becoming_a_client_origin() {
    let mut analyzer = test_analyzer();
    analyzer.client_origin_allowed = false;
    let child: syn::ExprStruct = syn::parse_quote! {
        FocusControl {
            target: self.target,
        }
    };

    analyzer
        .record_direct_component_struct(&child, DirectComponentBoundary::Composed)
        .expect("a projected field may participate without guessing its linked role");

    let projection = &analyzer.inbound_inputs.projection_fields[0];
    assert!(analyzer.inbound_inputs.node_ref_fields.is_empty());
    assert_eq!(
        analyzer.direct_component_edges[0].fields[0].binding,
        DirectComponentBindingClassification::DeferredProjection {
            field: projection.field,
            signal: projection.signal,
            node_ref: projection.node_ref,
        }
    );
    assert_eq!(
        analyzer.inbound_inputs.fields[0].role,
        InboundFieldRole::DeferredProjection
    );
}

#[test]
fn later_node_ref_use_upgrades_a_deferred_field_without_changing_the_edge_commitment() {
    let mut analyzer = test_analyzer();
    analyzer.client_origin_allowed = false;
    let child: syn::ExprStruct = syn::parse_quote! {
        FocusControl {
            target: self.target,
        }
    };

    analyzer
        .record_direct_component_struct(&child, DirectComponentBoundary::Composed)
        .expect("the initial pass-through remains deferred");
    let original_binding = analyzer.direct_component_edges[0].fields[0].binding;
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <input ref={self.target} />
    })
    .unwrap();
    analyzer
        .walk_root(&template)
        .expect("a later terminal ref use proves the concrete role");

    assert_eq!(
        analyzer.direct_component_edges[0].fields[0].binding, original_binding,
        "the authenticated transport edge remains deferred for the linker"
    );
    assert_eq!(
        analyzer.inbound_inputs.fields[0].role,
        InboundFieldRole::NodeRef
    );
    assert_eq!(analyzer.inbound_inputs.node_ref_fields.len(), 1);
    assert_eq!(
        analyzer.inbound_inputs.node_ref_fields[0].bind_target,
        Some(NodeRefBindTarget::Input)
    );
}

#[test]
fn ordinary_self_field_methods_do_not_infer_inbound_node_refs() {
    let mut analyzer = test_analyzer();
    let label: Expr = syn::parse_quote!(self.label);
    let items: Expr = syn::parse_quote!((self.items));

    assert_eq!(
        analyzer.node_ref_receiver(&label, false).unwrap(),
        None,
        "an ordinary self field is not a NodeRef merely because it is a method receiver"
    );
    assert_eq!(
        analyzer.node_ref_receiver(&items, false).unwrap(),
        None,
        "parentheses must preserve the same conservative classification"
    );
    assert!(analyzer.inbound_inputs.node_ref_fields.is_empty());
    assert!(analyzer.inbound_inputs.fields.is_empty());

    let input_ref: Expr = syn::parse_quote!(self.input_ref);
    assert!(
        analyzer
            .node_ref_receiver(&input_ref, true)
            .unwrap()
            .is_some(),
        "a recognized NodeRef method may prove a required inbound handle"
    );
    assert_eq!(analyzer.inbound_inputs.node_ref_fields.len(), 1);
}

#[test]
fn node_ref_component_edges_reject_owner_cuts_optional_fields_and_repetition() {
    let child: syn::ExprStruct = syn::parse_quote! {
        FocusControl { target }
    };

    let mut independent = test_analyzer();
    let target = independent.builder.node_ref();
    independent.node_refs.insert("target".to_string(), target);
    independent
        .record_direct_component_struct(&child, DirectComponentBoundary::ExplicitIsland)
        .expect("the recorder retains an unsupported edge for the boundary diagnostic");
    assert_eq!(
        independent.direct_component_edges[0].fields[0].binding,
        DirectComponentBindingClassification::Unsupported,
        "a node-ref alias must never become a live edge across an explicit island"
    );

    let mut optional = test_analyzer();
    let target = optional.builder.node_ref();
    optional.node_refs.insert("target".to_string(), target);
    let optional_child: syn::ExprStruct = syn::parse_quote! {
        FocusControl {
            target: Some(target),
        }
    };
    let error = optional
        .record_direct_component_struct(&optional_child, DirectComponentBoundary::Composed)
        .expect_err("optional node-ref fields are not part of the static alias ABI");
    assert!(
        error.to_string().contains("node-ref-handle-required"),
        "{error}"
    );

    let mut repeated = test_analyzer();
    let target = repeated.builder.node_ref();
    repeated.node_refs.insert("target".to_string(), target);
    repeated.loop_scopes.push(LoopScope {
        locals: HashSet::new(),
        integer_domains: HashMap::new(),
    });
    let error = repeated
        .record_direct_component_struct(&child, DirectComponentBoundary::Composed)
        .expect_err("a repeated edge cannot carry one stable ref identity");
    assert!(
        error.to_string().contains("node-ref-handle-static-only"),
        "{error}"
    );
}

#[test]
fn descriptor_expands_server_prelude_locals_inside_node_payload_expressions() {
    let lets = vec![
        local_from_stmt(syn::parse_quote! {
            let year = self.year;
        }),
        local_from_stmt(syn::parse_quote! {
            let month = self.month.clamp(1, 12);
        }),
    ];
    let mut builder = IslandBuilder::new();
    let expr = builder.expr(ClientExpr::Binary {
        op: ClientBinaryOp::Eq,
        left: Box::new(ClientExpr::SignalRead(SignalId(0))),
        right: Box::new(ClientExpr::NodePayload {
            attr: "data-rsjs-payload-p0".to_string(),
            tokens: "calendar_day(year, month, idx)".to_string(),
        }),
    });
    let mut island = builder.build();

    expand_server_prelude_locals_for_descriptor(&mut island, &lets).unwrap();

    let ClientExpr::Binary { right, .. } = island.exprs.client_expr(expr) else {
        panic!("expected binary expression");
    };
    let ClientExpr::NodePayload { tokens, .. } = right.as_ref() else {
        panic!("expected nested node payload");
    };
    assert_eq!(
        tokens,
        "calendar_day (self . year , self . month . clamp (1 , 12) , idx)"
    );
}

#[test]
fn explicit_string_signal_preserves_owned_string_initializer() {
    let local = local_from_stmt(syn::parse_quote! {
        let email = rsjs::signal::<String>(String::new());
    });

    let initializers = signal_initializers(&[local]).unwrap();

    assert_eq!(initializers.len(), 1);
    assert_eq!(initializers[0].0, SignalId(0));
    let Expr::Call(call) = &initializers[0].1 else {
        panic!("expected String::new() to be preserved as a runtime initializer");
    };
    assert!(path_ends_with(&call.func, "new"));
    let Some(ty) = &initializers[0].2 else {
        panic!("expected the explicit String type to be preserved");
    };
    let syn::Type::Path(path) = ty else {
        panic!("expected path type");
    };
    assert_eq!(path.path.segments.last().unwrap().ident, "String");
}

#[test]
fn string_from_signal_infers_owned_string_without_a_turbofish() {
    let local = local_from_stmt(syn::parse_quote! {
        let label = rsjs::signal(String::from("seven"));
    });

    let declaration = signal_local_decl(&local)
        .unwrap()
        .expect("String::from signal declaration");

    assert_eq!(declaration.explicit_ty, None);
    assert_eq!(declaration.fallback, Value::Null);
    assert!(matches!(declaration.runtime_expr, Some(Expr::Call(_))));
    assert_eq!(signal_decl_client_type(&declaration), ClientType::String);
}

#[test]
fn explicit_client_props_vector_signal_preserves_its_nominal_item_type() {
    let local = local_from_stmt(syn::parse_quote! {
        let rows = rsjs::signal::<Vec<SearchRow>>(load_rows());
    });

    let declaration = signal_local_decl(&local)
        .unwrap()
        .expect("typed client-props vector signal declaration");

    assert_eq!(
        signal_decl_client_type(&declaration),
        ClientType::vec(ClientType::Struct("SearchRow".to_string()))
    );
    assert!(
        declaration.runtime_expr.is_some(),
        "the real Rust initializer must remain available for ClientEncode"
    );
}

#[test]
fn unbound_unit_paths_are_component_edges_while_prelude_paths_are_values() {
    let mut analyzer = test_analyzer();
    analyzer.preserve_direct_component_sites = true;
    analyzer
        .server_prelude_locals
        .insert("rendered_value".to_string());
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <main>
            {ImportedBadge}
            {crate::ui::QualifiedBadge}
            {rendered_value}
        </main>
    })
    .unwrap();

    analyzer.walk_root(&template).unwrap();

    assert_eq!(analyzer.direct_component_edges.len(), 2);
    assert_eq!(
        analyzer
            .direct_component_edges
            .iter()
            .map(|edge| edge.component_ty.to_token_stream().to_string())
            .collect::<Vec<_>>(),
        vec!["ImportedBadge", "crate :: ui :: QualifiedBadge"]
    );
    assert!(
        analyzer
            .direct_component_edges
            .iter()
            .all(|edge| edge.fields.is_empty()
                && !edge.has_rest
                && matches!(edge.boundary, DirectComponentBoundary::Composed)
                && matches!(edge.lifecycle, DirectComponentLifecycle::StaticSingle)),
        "unit component edges carry an empty field ABI and normal static lifecycle"
    );
    assert_eq!(analyzer.direct_component_site_nodes.len(), 2);
    assert!(
        analyzer
            .server_expressions
            .values()
            .any(|expression| { expression.to_token_stream().to_string() == "rendered_value" }),
        "a path bound in the prelude must retain ordinary renderable semantics"
    );
}

#[test]
fn explicit_island_accepts_an_unbound_unit_component_path() {
    let mut analyzer = test_analyzer();
    analyzer.preserve_direct_component_sites = true;
    let template: HtmlRoot = syn::parse2(quote::quote! {
        <main>{rsjs::island(RenamedBadge)}</main>
    })
    .unwrap();

    analyzer.walk_root(&template).unwrap();

    let [edge] = analyzer.direct_component_edges.as_slice() else {
        panic!("the explicit unit component should produce exactly one edge");
    };
    assert_eq!(
        edge.component_ty.to_token_stream().to_string(),
        "RenamedBadge"
    );
    assert!(matches!(
        edge.boundary,
        DirectComponentBoundary::ExplicitIsland
    ));
    assert_eq!(analyzer.direct_component_site_nodes.len(), 1);
}

#[test]
fn direct_payload_handler_materialization_keeps_signal_and_helper_dependencies() {
    let (statements, lets) = prelude_from_block(syn::parse_quote!({
        let count = signal(0_i64);
        let on_change = rsjs::handler(|next: i64| {
            labels::normalize(next);
            count.set(next);
        });
        let unrelated = rsjs::handler(|_| count.set(99_i64));
    }));
    let mut analyzer = test_analyzer();
    analyzer
        .collect_prelude_bindings(&statements, &lets)
        .expect("payload handlers should analyze");
    let child: syn::ExprStruct = syn::parse_quote! {
        Child {
            on_change,
        }
    };
    analyzer
        .record_direct_component_struct(&child, DirectComponentBoundary::Composed)
        .expect("direct child should analyze");

    let values = analyzer
        .materialize_direct_payload_handler_values()
        .expect("referenced payload handler should materialize");
    assert_eq!(values.len(), 1, "unreferenced handler must not materialize");
    assert_eq!(values[0].prelude_local_index, 1);
    assert_eq!(values[0].dependencies.signal_writes, [SignalId(0)]);
    assert_eq!(
        values[0].dependencies.helper_edges,
        [HelperCallEdgeId::new(0)]
    );
    assert_eq!(analyzer.direct_helper_edges.len(), 1);
    assert_eq!(
        crate::parse::path_to_canonical_string(&analyzer.direct_helper_edges[0].helper_path),
        "labels::normalize"
    );
}

#[test]
fn literal_string_signal_stays_static() {
    let local = local_from_stmt(syn::parse_quote! {
        let email = rsjs::signal("");
    });

    let initializers = signal_initializers(&[local]).unwrap();

    assert!(initializers.is_empty());
}

#[test]
fn shared_acquisition_requires_an_explicit_type() {
    let local = local_from_stmt(syn::parse_quote! {
        let workbench = rsjs::shared();
    });

    let message = shared_store_local_decl(&local).unwrap_err().to_string();
    assert!(message.contains("rsjs/shared-state-missing-type"));
    assert!(message.contains("rsjs::shared::<YourState>()"));
    assert!(message.contains("dependency auditable"));
}

/// C9 regression: a zero-argument method call on an ordinary local must not be
/// read as a shared-field binding just because a store declared further down
/// happens to reuse the name. The receiver is already bound to a `String`
/// there, so `state.len()` is `String::len`.
#[test]
fn zero_arg_call_on_a_shadowed_plain_local_is_not_use_before_acquire() {
    let lets = vec![
        local_from_stmt(syn::parse_quote! { let state = self.label.clone(); }),
        local_from_stmt(syn::parse_quote! { let n = state.len(); }),
        local_from_stmt(syn::parse_quote! { let state = rsjs::shared::<WorkbenchState>(); }),
    ];

    let mut analyzer = test_analyzer();
    analyzer
        .collect_shared_stores_and_signals(&lets)
        .expect("a plain local shadowed by a later store must not trip use-before-acquire");
    assert!(analyzer.shared_stores.contains_key("state"));
    // Only the acquisition itself is claimed; the two plain locals stay in the
    // server replay.
    assert_eq!(
        analyzer.shared_prelude_local_indices,
        HashSet::from([2usize])
    );
}

#[test]
fn ordinary_local_shadow_deactivates_an_earlier_shared_provider() {
    let lets = vec![
        local_from_stmt(syn::parse_quote! {
            let state = rsjs::shared::<WorkbenchState>();
        }),
        local_from_stmt(syn::parse_quote! {
            let state = self.server_state();
        }),
        local_from_stmt(syn::parse_quote! {
            let label = state.label();
        }),
    ];

    let mut analyzer = test_analyzer();
    analyzer
        .collect_shared_stores_and_signals(&lets)
        .expect("the field-like call belongs to the ordinary shadow");

    assert_eq!(
        analyzer.shared_prelude_local_indices,
        HashSet::from([0usize]),
        "only the original SharedState acquisition may be consumed"
    );
    assert!(
        !analyzer.prelude_signal_ids.contains_key(&2),
        "a field-like call on an ordinary shadow must not become SharedState"
    );
}

#[test]
fn ordinary_local_shadow_deactivates_an_earlier_signal_for_memo_lowering() {
    let lets = vec![
        local_from_stmt(syn::parse_quote! {
            let count = rsjs::signal(1_i64);
        }),
        local_from_stmt(syn::parse_quote! {
            let count = self.server_counter();
        }),
        local_from_stmt(syn::parse_quote! {
            let doubled = rsjs::memo(|| count.get() * 2_i64);
        }),
    ];

    let message = test_analyzer()
        .collect_shared_stores_and_signals(&lets)
        .expect_err("memo cannot capture an ordinary server shadow")
        .to_string();
    assert!(
        message.contains("could not find a signal named `count`"),
        "the old signal must not remain visible after the ordinary shadow: {message}"
    );
}

#[test]
fn signal_seed_reading_an_earlier_signal_has_a_focused_diagnostic() {
    let lets = vec![
        local_from_stmt(syn::parse_quote! {
            let first = rsjs::signal(1_i64);
        }),
        local_from_stmt(syn::parse_quote! {
            let second = rsjs::signal(first.get() + 1_i64);
        }),
    ];

    let message = test_analyzer()
        .collect_shared_stores_and_signals(&lets)
        .expect_err("a one-time server seed cannot silently snapshot client state")
        .to_string();
    assert!(
        message.contains("rsjs/signal-seed-client-dependency"),
        "unexpected diagnostic: {message}"
    );
    assert!(message.contains("rsjs::memo"), "{message}");
}

#[test]
fn named_handlers_capture_the_exact_same_named_signal_binding_at_declaration() {
    let (statements, lets) = prelude_from_block(syn::parse_quote! {{
        let value = rsjs::signal(1_i32);
        let first = rsjs::event_handler(|_: rsjs::Event| value.set(2_i32));
        let value = rsjs::signal(3_i64);
        let second = rsjs::event_handler(|_: rsjs::Event| value.set(4_i64));
    }});
    let mut analyzer = test_analyzer();
    analyzer
        .collect_prelude_bindings(&statements, &lets)
        .unwrap();

    let first: Expr = syn::parse_quote! { first() };
    let second: Expr = syn::parse_quote! { second() };
    let first = analyzer.lower_handler(&first, NodeId(0)).unwrap();
    let second = analyzer.lower_handler(&second, NodeId(0)).unwrap();

    assert!(matches!(
        first.body_ir.stmts.as_slice(),
        [ClientStmt::SignalSet {
            id: SignalId(0),
            ..
        }]
    ));
    assert!(matches!(
        second.body_ir.stmts.as_slice(),
        [ClientStmt::SignalSet {
            id: SignalId(1),
            ..
        }]
    ));
}

#[test]
fn query_and_mutation_keys_bind_before_a_later_same_named_shadow() {
    let (statements, lets) = prelude_from_block(syn::parse_quote! {{
        let key = rsjs::signal("first");
        let query = rsjs::query_with_initial(
            rsjs::QueryKey::new(("item", key.get())),
            0_i64,
            "/api/item",
            rsjs::QueryOptions::default(),
        );
        let save = rsjs::mutation(
            rsjs::QueryKey::new(("save", key.get())),
            "/api/save",
            [rsjs::QueryKey::new(("item", key.get()))],
        );
        let key = rsjs::signal(9_i64);
    }});
    let mut analyzer = test_analyzer();
    analyzer.is_async = true;
    analyzer
        .collect_prelude_bindings(&statements, &lets)
        .unwrap();
    let island = analyzer.builder.build();

    let query_key = island.exprs.client_expr(island.queries[0].key_expr);
    let mutation_key = island.exprs.client_expr(island.mutations[0].key_expr);
    let invalidation = island.exprs.client_expr(island.mutations[0].invalidates[0]);
    for expr in [query_key, mutation_key, invalidation] {
        assert!(
            client_expr_contains_signal(expr, SignalId(0)),
            "declaration-site key should retain the first `key` signal: {expr:?}"
        );
        assert!(
            !client_expr_contains_signal(expr, SignalId(1)),
            "a later `key` shadow must not rewrite an earlier key: {expr:?}"
        );
    }
}

#[test]
fn query_key_rejects_asynchronous_query_dependencies() {
    let (statements, lets) = prelude_from_block(syn::parse_quote! {{
        let first = rsjs::query_with_initial(
            rsjs::QueryKey::new(("first",)),
            1_i64,
            "/api/first",
            rsjs::QueryOptions::default(),
        );
        let second = rsjs::query_with_initial(
            rsjs::QueryKey::new(("second", first.data())),
            2_i64,
            "/api/second",
            rsjs::QueryOptions::default(),
        );
    }});
    let mut analyzer = test_analyzer();
    analyzer.is_async = true;
    let message = analyzer
        .collect_prelude_bindings(&statements, &lets)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("rsjs/query-key-not-synchronous") && message.contains("another query"),
        "{message}"
    );
}

#[test]
fn unsupported_query_key_wrapper_around_signal_fails_closed() {
    let (statements, lets) = prelude_from_block(syn::parse_quote! {{
        let selected = rsjs::signal("old");
        let query = rsjs::query_with_initial(
            rsjs::QueryKey::new(("search", format!("{}", selected.get()))),
            1_i64,
            "/api/search",
            rsjs::QueryOptions::default(),
        );
    }});
    let mut analyzer = test_analyzer();
    analyzer.is_async = true;
    let message = analyzer
        .collect_prelude_bindings(&statements, &lets)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("rsjs/query-key-client-expression-unsupported")
            && message.contains("selected"),
        "{message}"
    );
}

#[test]
fn reactive_query_request_inputs_must_participate_in_the_key() {
    let (statements, lets) = prelude_from_block(syn::parse_quote! {{
        let url = rsjs::signal("/api/search?q=old");
        let query = rsjs::query_with_initial(
            rsjs::QueryKey::new(("search",)),
            1_i64,
            url.get(),
            rsjs::QueryOptions::default(),
        );
    }});
    let mut analyzer = test_analyzer();
    analyzer.is_async = true;
    let message = analyzer
        .collect_prelude_bindings(&statements, &lets)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("rsjs/query-fetcher-dependency-not-in-key") && message.contains("url"),
        "{message}"
    );
}

#[test]
fn browser_query_request_inputs_fail_closed() {
    let (statements, lets) = prelude_from_block(syn::parse_quote! {{
        let query = rsjs::query_with_initial(
            rsjs::QueryKey::new(("search",)),
            1_i64,
            rsjs::location::href(),
            rsjs::QueryOptions::default(),
        );
    }});
    let mut analyzer = test_analyzer();
    analyzer.is_async = true;
    let message = analyzer
        .collect_prelude_bindings(&statements, &lets)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("rsjs/query-request-not-synchronous") && message.contains("browser reads"),
        "{message}"
    );
}

#[test]
fn ordinary_server_local_query_key_remains_locator_backed() {
    let (statements, lets) = prelude_from_block(syn::parse_quote! {{
        let server_key = String::from("server");
        let query = rsjs::query_with_initial(
            rsjs::QueryKey::new(("search", server_key.as_str())),
            1_i64,
            "/api/search",
            rsjs::QueryOptions::default(),
        );
    }});
    let mut analyzer = test_analyzer();
    analyzer.is_async = true;
    analyzer
        .collect_prelude_bindings(&statements, &lets)
        .expect("ordinary server locals should use adjacent dehydrated key metadata");
    let island = analyzer.builder.build();
    assert!(matches!(
        island.exprs.client_expr(island.queries[0].key_expr),
        ClientExpr::Literal(Value::Null)
    ));
}

#[test]
fn interval_binds_the_signal_visible_at_its_statement_position() {
    let (statements, lets) = prelude_from_block(syn::parse_quote! {{
        let tick = rsjs::signal(0_i32);
        rsjs::interval(1000, || tick.update(|value| value + 1_i32));
        let tick = rsjs::signal(0_i64);
    }});
    let mut analyzer = test_analyzer();
    analyzer
        .collect_prelude_bindings(&statements, &lets)
        .unwrap();
    assert_eq!(analyzer.signals.get("tick"), Some(&SignalId(1)));
    let island = analyzer.builder.build();
    assert!(matches!(
        island.intervals[0].body_ir().stmts.as_slice(),
        [ClientStmt::SignalUpdate {
            id: SignalId(0),
            ..
        }]
    ));
}

fn client_expr_contains_signal(expr: &ClientExpr, expected: SignalId) -> bool {
    match expr {
        ClientExpr::SignalRead(id) => *id == expected,
        ClientExpr::QueryKey(parts) => parts
            .iter()
            .any(|part| client_expr_contains_signal(part, expected)),
        _ => false,
    }
}

/// C9 regression, other direction: a genuine forward reference to a store
/// declared later still fails, and keeps its diagnostic code.
#[test]
fn unbound_receiver_of_a_later_store_is_still_use_before_acquire() {
    let lets = vec![
        local_from_stmt(syn::parse_quote! { let button_bg = workbench.button_bg(); }),
        local_from_stmt(syn::parse_quote! { let workbench = rsjs::shared::<WorkbenchState>(); }),
    ];

    let message = test_analyzer()
        .collect_shared_stores_and_signals(&lets)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("rsjs/shared-state-use-before-acquire"),
        "{message}"
    );
    assert!(message.contains("`workbench`"), "{message}");
}

/// C27 regression: an acquisition the analyzer cannot claim must fail at
/// compile time instead of being replayed into the server render, where
/// `rsjs::shared` is a panicking stub.
#[test]
fn block_wrapped_shared_acquisition_is_rejected_at_compile_time() {
    let lets = vec![local_from_stmt(
        syn::parse_quote! { let workbench = { rsjs::shared::<WorkbenchState>() }; },
    )];

    let message = test_analyzer()
        .collect_shared_stores_and_signals(&lets)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("rsjs/shared-state-unrecognized-acquire"),
        "{message}"
    );
    assert!(
        message.contains("let workbench = rsjs::shared::<WorkbenchState>();"),
        "{message}"
    );
}

#[test]
fn conditional_shared_acquisition_is_rejected_at_compile_time() {
    let lets = vec![local_from_stmt(syn::parse_quote! {
        let workbench = if self.wide {
            rsjs::shared::<WideState>()
        } else {
            rsjs::shared::<WideState>()
        };
    })];

    let message = test_analyzer()
        .collect_shared_stores_and_signals(&lets)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("rsjs/shared-state-unrecognized-acquire"),
        "{message}"
    );
    assert!(message.contains("WideState"), "{message}");
}

/// The chained one-liner keeps its own, more specific diagnostic.
#[test]
fn chained_shared_acquisition_keeps_its_targeted_diagnostic() {
    let lets = vec![local_from_stmt(
        syn::parse_quote! { let bg = rsjs::shared::<WorkbenchState>().button_bg(); },
    )];

    let message = test_analyzer()
        .collect_shared_stores_and_signals(&lets)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("rsjs/shared-state-chained-acquire"),
        "{message}"
    );
}

/// A user function that merely happens to be called `shared` is not an rsjs
/// acquisition and must stay an ordinary server local.
#[test]
fn foreign_shared_function_is_not_claimed_as_an_acquisition() {
    let lets = vec![local_from_stmt(
        syn::parse_quote! { let position = { ecs::shared::<Position>() }; },
    )];

    let mut analyzer = test_analyzer();
    analyzer
        .collect_shared_stores_and_signals(&lets)
        .expect("`ecs::shared::<T>()` belongs to user code");
    assert!(analyzer.shared_prelude_local_indices.is_empty());
}

#[test]
fn query_local_data_type_reads_query_annotation() {
    let local = local_from_stmt(syn::parse_quote! {
        let search: rsjs::Query<SearchPayload> = rsjs::query_with_initial(
            rsjs::QueryKey::new(("search", "proa")),
            initial,
            "/api/search",
            rsjs::QueryOptions::default(),
        );
    });
    assert_eq!(
        query_local_data_type(&local),
        Some(ClientType::Struct("SearchPayload".to_string()))
    );

    let local = local_from_stmt(syn::parse_quote! {
        let rows: rsjs::Query<Vec<Option<String>>> = rsjs::query_with_initial(
            rsjs::QueryKey::new(("rows",)),
            initial,
            "/api/rows",
            rsjs::QueryOptions::default(),
        );
    });
    assert_eq!(
        query_local_data_type(&local),
        Some(ClientType::vec(ClientType::option(ClientType::String)))
    );
}

#[test]
fn query_initial_data_type_reads_local_struct_literal_shape() {
    let initial = local_from_stmt(syn::parse_quote! {
        let initial_search = SearchPayload {
            has_results: false,
            first_url: String::new(),
            results: Vec::new(),
        };
    });
    let query = local_from_stmt(syn::parse_quote! {
        let search: rsjs::Query<SearchPayload> = rsjs::query_with_initial(
            rsjs::QueryKey::new(("search", "proa")),
            initial_search,
            "/api/search",
            rsjs::QueryOptions::default(),
        );
    });
    let lets = vec![initial, query];
    let expr: Expr = syn::parse_quote! { initial_search };

    assert_eq!(
        query_initial_data_type(&expr, &lets),
        Some(ClientType::object(vec![
            ClientObjectField {
                name: "has_results".to_string(),
                ty: ClientType::Bool,
            },
            ClientObjectField {
                name: "first_url".to_string(),
                ty: ClientType::String,
            },
            ClientObjectField {
                name: "results".to_string(),
                ty: ClientType::vec(ClientType::Unknown),
            },
        ]))
    );
}

#[test]
fn query_local_data_type_keeps_wide_json_numbers_unknown() {
    let local = local_from_stmt(syn::parse_quote! {
        let total: rsjs::Query<i64> = rsjs::query_with_initial(
            rsjs::QueryKey::new(("total",)),
            initial,
            "/api/total",
            rsjs::QueryOptions::default(),
        );
    });
    assert_eq!(query_local_data_type(&local), Some(ClientType::Unknown));
}

#[test]
fn supported_wide_integer_literals_are_always_bigint() {
    assert_eq!(
        int_literal_value(9_007_199_254_740_993, "i64"),
        Value::BigInt(9_007_199_254_740_993)
    );
    assert_eq!(
        int_literal_value(-9_007_199_254_740_993, "i64"),
        Value::BigInt(-9_007_199_254_740_993)
    );
    assert_eq!(
        int_literal_value(9_007_199_254_740_993, "u64"),
        Value::BigInt(9_007_199_254_740_993)
    );
    assert_eq!(int_literal_value(0, "i64"), Value::BigInt(0));
    assert_eq!(
        int_literal_value(9_007_199_254_740_991, "i64"),
        Value::BigInt(9_007_199_254_740_991)
    );
    assert_eq!(int_literal_value(7, "u64"), Value::BigInt(7));

    // Unsupported pointer-width integers are not promoted into a supported
    // BigInt domain merely because they carry an explicit suffix.
    assert_eq!(int_literal_value(7, "usize"), Value::Int(7));

    // Unsuffixed and narrow-suffixed literals never widen while they fit the
    // internal Number carrier.
    assert_eq!(
        int_literal_value(9_007_199_254_740_991, ""),
        Value::Int(9_007_199_254_740_991)
    );
    assert_eq!(int_literal_value(100, "i32"), Value::Int(100));
}

#[test]
fn typed_signal_binding_supplies_the_integer_domain() {
    let local = local_from_stmt(syn::parse_quote! {
        let count: rsjs::Signal<i64> = rsjs::signal(0);
    });
    let decl = signal_local_decl(&local)
        .expect("typed signal should parse")
        .expect("typed signal should be recognized");
    assert_eq!(
        decl.integer_source,
        Some(IntegerSource::Supported(IntegerDomain::I64))
    );
    assert_eq!(decl.fallback, Value::BigInt(0));
    assert_eq!(signal_decl_client_type(&decl), ClientType::I64);
}

#[test]
fn option_none_turbofish_preserves_its_nested_integer_domain() {
    let local = local_from_stmt(syn::parse_quote! {
        let maybe = rsjs::signal(Option::<i64>::None);
    });
    let decl = signal_local_decl(&local)
        .expect("typed Option initializer should parse")
        .expect("signal should be recognized");

    assert_eq!(
        signal_decl_client_type(&decl),
        ClientType::option(ClientType::I64)
    );
    assert!(
        decl.runtime_expr.is_some(),
        "typed None must remain a runtime initializer for the hydration blob"
    );
}

#[test]
fn option_some_suffix_preserves_its_nested_integer_domain() {
    for local in [
        local_from_stmt(syn::parse_quote! {
            let maybe = rsjs::signal(Some(9_007_199_254_740_993_i64));
        }),
        local_from_stmt(syn::parse_quote! {
            let maybe = rsjs::signal(Option::<i64>::Some(9_007_199_254_740_993));
        }),
    ] {
        let decl = signal_local_decl(&local)
            .expect("typed Some initializer should parse")
            .expect("signal should be recognized");
        assert_eq!(
            signal_decl_client_type(&decl),
            ClientType::option(ClientType::I64)
        );
        assert!(
            decl.runtime_expr.is_some(),
            "typed Some must remain a runtime initializer for the hydration blob"
        );
    }
}

#[test]
fn typed_signal_binding_rejects_unsupported_inner_width() {
    let local = local_from_stmt(syn::parse_quote! {
        let count: rsjs::Signal<u8> = rsjs::signal(0);
    });
    let error = match signal_local_decl(&local) {
        Err(error) => error,
        Ok(_) => panic!("u8 signal must be rejected"),
    };
    assert!(
        error.to_string().contains("integer-domain-unsupported"),
        "{error}"
    );
}

#[test]
fn suffixed_signal_initializer_rejects_unsupported_width_without_turbofish() {
    let local = local_from_stmt(syn::parse_quote! {
        let count = rsjs::signal(0_u8);
    });
    let error = match signal_local_decl(&local) {
        Err(error) => error,
        Ok(_) => panic!("an explicitly unsupported signal suffix must be rejected at declaration"),
    };
    assert!(
        error
            .to_string()
            .contains("rsjs/integer-domain-unsupported")
            && error.to_string().contains("`u8` arithmetic"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn typed_signal_binding_and_turbofish_must_agree() {
    let local = local_from_stmt(syn::parse_quote! {
        let count: rsjs::Signal<i64> = rsjs::signal::<u64>(0);
    });
    let error = match signal_local_decl(&local) {
        Err(error) => error,
        Ok(_) => panic!("mismatched signal types must be rejected"),
    };
    assert!(
        error.to_string().contains("integer-domain-mismatch"),
        "{error}"
    );
}

#[test]
fn wide_signal_read_promotes_sibling_int_literal() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::BigInt(9_007_199_254_740_993));
    a.builder
        .set_signal_integer_domain(id, Some(IntegerDomain::I64));
    a.signals.insert("big".to_string(), id);
    let expr: Expr = syn::parse_quote! { big.get() + 1_i64 };
    let lowered = a
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .unwrap();
    assert_eq!(
        lowered,
        FrontendExpr::BinaryOp {
            op: BinOp::Checked {
                op: CheckedIntegerOp::Add,
                domain: IntegerDomain::I64,
            },
            lhs: Box::new(FrontendExpr::SignalRead(id)),
            rhs: Box::new(FrontendExpr::Literal(Value::BigInt(1))),
        },
        "an i64 signal and literal must use checked i64 arithmetic"
    );
}

#[test]
fn number_signal_read_rejects_a_mismatched_wide_literal_domain() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::Int(0));
    a.builder
        .set_signal_integer_domain(id, Some(IntegerDomain::I32));
    a.signals.insert("count".to_string(), id);
    let expr: Expr = syn::parse_quote! { count.get() + 9007199254740993_i64 };
    let error = a
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .unwrap_err();
    assert!(
        error.to_string().contains("rsjs/integer-domain-mismatch")
            && error.to_string().contains("i32")
            && error.to_string().contains("i64"),
        "mixed integer domains must fail before JavaScript lowering: {error}"
    );
}

#[test]
fn shared_set_suffix_establishes_a_checked_i64_domain_before_lowering() {
    let mut analyzer = test_analyzer();
    let store = analyzer
        .builder
        .shared_store("state".into(), "WideState".into(), None);
    let count = analyzer
        .builder
        .shared_field(store, "count".into(), ClientType::Dyn);
    analyzer.signals.insert("count".to_string(), count);
    let closure: ExprClosure = syn::parse_quote!(|_| count.set(count.get() + 1_i64));

    let statements = analyzer
        .lower_handler_closure_hir_body(&closure, &HandlerScope::default())
        .expect("the explicit i64 suffix should specialize the shared field");

    assert_eq!(
        analyzer.builder.signal_integer_domain(count),
        Some(IntegerDomain::I64)
    );
    let [statement] = statements.as_slice() else {
        panic!("expected one shared signal write");
    };
    let HandlerHirStmtKind::SignalSet { value, .. } = &statement.kind else {
        panic!("expected a shared signal set");
    };
    assert_eq!(
        value.expr,
        FrontendExpr::BinaryOp {
            op: BinOp::Checked {
                op: CheckedIntegerOp::Add,
                domain: IntegerDomain::I64,
            },
            lhs: Box::new(FrontendExpr::SignalRead(count)),
            rhs: Box::new(FrontendExpr::Literal(Value::BigInt(1))),
        }
    );
}

#[test]
fn handler_current_target_local_lowers_generated_scalar_reads() {
    let mut analyzer = test_analyzer();
    let closure: ExprClosure = syn::parse_quote!(|event| {
        let input = event.current_target();
        console::log(input.value(), input.client_width());
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: Some("event"),
        handler_locals: Vec::new(),
        event_node: Some(NodeId(0)),
    };

    let statements = analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .expect("typed current-target locals should lower");
    let [binding, log] = statements.as_slice() else {
        panic!("expected a handle binding and console statement");
    };
    let HandlerHirStmtKind::Let {
        local, ty, value, ..
    } = &binding.kind
    else {
        panic!("expected current-target let binding");
    };
    assert_eq!(ty, &ClientType::DomElement);
    assert_eq!(value.expr, FrontendExpr::EventCurrentTarget);
    let HandlerHirStmtKind::Console { args, .. } = &log.kind else {
        panic!("expected console statement");
    };
    assert_eq!(
        args.iter().map(|arg| &arg.expr).collect::<Vec<_>>(),
        vec![
            &FrontendExpr::DomRead {
                target: Box::new(FrontendExpr::Local(*local)),
                accessor: NodeRefAccessor::Value,
            },
            &FrontendExpr::DomRead {
                target: Box::new(FrontendExpr::Local(*local)),
                accessor: NodeRefAccessor::ClientWidth,
            },
        ]
    );
}

#[test]
fn handler_current_target_handle_cannot_escape_as_a_value() {
    let mut analyzer = test_analyzer();
    let closure: ExprClosure = syn::parse_quote!(|event| {
        let input = event.current_target();
        console::log(input);
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: Some("event"),
        handler_locals: Vec::new(),
        event_node: Some(NodeId(0)),
    };

    let error = match analyzer.lower_handler_closure_hir_body(&closure, &scope) {
        Ok(_) => panic!("DOM handles must remain receiver-only"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("typed DOM handles may only be used as the receiver"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn shared_set_rejects_a_suffix_that_mismatches_an_established_domain() {
    let mut analyzer = test_analyzer();
    let store = analyzer
        .builder
        .shared_store("state".into(), "WideState".into(), None);
    let count = analyzer
        .builder
        .shared_field(store, "count".into(), ClientType::Dyn);
    analyzer
        .builder
        .set_signal_integer_domain(count, Some(IntegerDomain::I64));
    analyzer.builder.set_signal_ty(count, ClientType::I64);
    analyzer.signals.insert("count".to_string(), count);
    let closure: ExprClosure = syn::parse_quote!(|_| count.set(1_u32));

    let error = match analyzer.lower_handler_closure_hir_body(&closure, &HandlerScope::default()) {
        Ok(_) => panic!("an established i64 shared field must reject a u32 write"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("rsjs/integer-domain-mismatch")
            && error.to_string().contains("u32")
            && error.to_string().contains("i64"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn f64_from_i32_signal_lowers_to_an_explicit_number_conversion() {
    let mut analyzer = test_analyzer();
    let id = analyzer.builder.signal(Value::Int(0));
    analyzer
        .builder
        .set_signal_integer_domain(id, Some(IntegerDomain::I32));
    analyzer.signals.insert("width".to_string(), id);

    let expr: Expr = syn::parse_quote! { f64::from(width.get()) };
    let lowered = analyzer
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect("i32 has an exact ordinary-Rust conversion to f64");
    assert_eq!(
        lowered,
        FrontendExpr::AsType {
            target: Box::new(FrontendExpr::SignalRead(id)),
            ty: AsType::F64FromInt,
        }
    );
}

#[test]
fn f64_from_u32_signal_lowers_to_an_explicit_number_conversion() {
    let mut analyzer = test_analyzer();
    let id = analyzer.builder.signal(Value::Int(0));
    analyzer
        .builder
        .set_signal_integer_domain(id, Some(IntegerDomain::U32));
    analyzer.signals.insert("width".to_string(), id);

    let expr: Expr = syn::parse_quote! { f64::from(width.get()) };
    let lowered = analyzer
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect("u32 has an exact ordinary-Rust conversion to f64");
    assert!(matches!(
        lowered,
        FrontendExpr::AsType {
            ty: AsType::F64FromInt,
            ..
        }
    ));
}

#[test]
fn f64_from_wide_signal_is_rejected_instead_of_rounding() {
    let mut analyzer = test_analyzer();
    let id = analyzer.builder.signal(Value::BigInt(0));
    analyzer
        .builder
        .set_signal_integer_domain(id, Some(IntegerDomain::I64));
    analyzer.signals.insert("width".to_string(), id);

    let expr: Expr = syn::parse_quote! { f64::from(width.get()) };
    let error = analyzer
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect_err("Rust does not implement From<i64> for f64");
    assert!(
        error.to_string().contains("only accepts Rust's exact")
            && error.to_string().contains("i64"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn client_prelude_local_alias_lowers_signal_read() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::Int(0));
    a.signals.insert("count".to_string(), id);
    let local = local_from_stmt(syn::parse_quote! {
        let display = count.get();
    });

    a.collect_client_prelude_locals(&[local]).unwrap();

    assert_eq!(
        a.client_prelude_locals.get("display"),
        Some(&FrontendExpr::SignalRead(id))
    );
    let expr: Expr = syn::parse_quote! { display };
    assert_eq!(
        a.lower_reactive_expr(&expr).unwrap(),
        FrontendExpr::SignalRead(id)
    );
    let ident: syn::Ident = syn::parse_quote! { display };
    assert!(a.is_reactive_local_name(&ident));
}

#[test]
fn client_prelude_local_alias_follows_transitive_aliases() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::Int(0));
    a.signals.insert("count".to_string(), id);
    let first = local_from_stmt(syn::parse_quote! {
        let current = count.get();
    });
    let second = local_from_stmt(syn::parse_quote! {
        let display = current;
    });

    a.collect_client_prelude_locals(&[first, second]).unwrap();

    assert_eq!(
        a.client_prelude_locals.get("display"),
        Some(&FrontendExpr::SignalRead(id))
    );
}

#[test]
fn client_prelude_local_alias_lowers_browser_read() {
    let mut a = test_analyzer();
    let local = local_from_stmt(syn::parse_quote! {
        let path = rsjs::location::pathname();
    });

    a.collect_client_prelude_locals(&[local]).unwrap();

    assert_eq!(
        a.client_prelude_locals.get("path"),
        Some(&FrontendExpr::BrowserRead(BrowserRead::Location(
            LocationField::Pathname
        )))
    );
}

#[test]
fn pure_client_helper_prelude_local_stays_server_local() {
    let mut a = test_analyzer();
    let local = local_from_stmt(syn::parse_quote! {
        let formatted = pricing::format(12_i64);
    });

    a.collect_client_prelude_locals(&[local]).unwrap();

    assert!(!a.client_prelude_locals.contains_key("formatted"));
}

#[test]
fn client_helper_prelude_local_with_signal_arg_becomes_alias() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::Int(0));
    a.signals.insert("count".to_string(), id);
    let local = local_from_stmt(syn::parse_quote! {
        let formatted = pricing::format(count.get());
    });

    a.collect_client_prelude_locals(&[local]).unwrap();

    assert!(matches!(
        a.client_prelude_locals.get("formatted"),
        Some(FrontendExpr::ClientCall {
            authored_path,
            name,
            args,
            ..
        })
            if authored_path == "pricing::format"
                && name == "format"
                && args == &[FrontendExpr::SignalRead(id)]
    ));
}

#[test]
fn client_helper_calls_retain_authored_alias_and_reexport_paths() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::Int(0));
    a.signals.insert("count".to_string(), id);

    let authored = [
        "same_module",
        "ImportedAlias",
        "reexports::PublicHelper",
        "pricing::legacy_helper",
        "same_module",
    ];
    for (expected_edge, path) in authored.iter().enumerate() {
        let expr: Expr = syn::parse_str(&format!("{path}(count.get())")).unwrap();
        let lowered = a.lower_reactive_expr(&expr).unwrap();
        assert!(matches!(
            lowered,
            FrontendExpr::ClientCall {
                edge,
                authored_path,
                args,
                ..
            } if edge.index() == expected_edge as u32
                && authored_path == *path
                && args == [FrontendExpr::SignalRead(id)]
        ));
    }

    assert_eq!(a.direct_helper_edges.len(), authored.len());
    for (expected_edge, edge) in a.direct_helper_edges.iter().enumerate() {
        assert_eq!(edge.local_id, expected_edge as u32);
        assert_eq!(edge.argument_count, 1);
        assert_eq!(edge.expected_return_domain, None);
        assert_eq!(
            crate::parse::path_to_canonical_string(&edge.helper_path),
            authored[expected_edge]
        );
    }
}

#[test]
fn direct_helper_arguments_preserve_explicit_but_not_contextual_literal_domains() {
    let mut analyzer = test_analyzer();
    let explicit: Expr = syn::parse_quote!(ImportedAlias(7_u32));
    let contextual: Expr = syn::parse_quote!(ImportedAlias(7));

    let explicit = analyzer.lower_reactive_expr(&explicit).unwrap();
    let contextual = analyzer.lower_reactive_expr(&contextual).unwrap();

    assert!(matches!(
        explicit,
        FrontendExpr::ClientCall { args, .. }
            if matches!(
                args.as_slice(),
                [FrontendExpr::BinaryOp {
                    op: BinOp::Checked {
                        op: CheckedIntegerOp::Add,
                        domain: IntegerDomain::U32,
                    },
                    lhs,
                    rhs,
                }] if matches!(lhs.as_ref(), FrontendExpr::Literal(Value::Int(7)))
                    && matches!(rhs.as_ref(), FrontendExpr::Literal(Value::Int(0)))
            )
    ));
    assert!(matches!(
        contextual,
        FrontendExpr::ClientCall { args, .. }
            if matches!(args.as_slice(), [FrontendExpr::Literal(Value::Int(7))])
    ));
}

#[test]
fn typed_boundary_contextualizes_direct_client_helper_return() {
    let mut a = test_analyzer();
    let count = a.builder.signal(Value::Int(0));
    a.builder
        .set_signal_integer_domain(count, Some(IntegerDomain::U32));
    a.builder.set_signal_ty(count, ClientType::U32);
    a.signals.insert("count".to_string(), count);
    let expr: Expr = syn::parse_quote!(ImportedAlias(count.get()));
    let mut lowered = a.lower_reactive_expr(&expr).unwrap();

    a.validate_integer_value_for_domain(
        &mut lowered,
        None,
        IntegerDomain::U32,
        &[],
        expr.span(),
        "typed helper result",
        false,
    )
    .expect("the surrounding u32 boundary should type the direct helper result");

    assert_eq!(
        a.frontend_integer_domain(&lowered, &[]),
        Some(IntegerDomain::U32)
    );
    assert_eq!(a.direct_helper_edges.len(), 1);
    assert_eq!(a.direct_helper_edges[0].argument_count, 1);
    assert_eq!(
        a.direct_helper_edges[0].expected_return_domain,
        Some(IntegerDomain::U32)
    );
}

#[test]
fn fixed_width_comparison_contextualizes_one_direct_helper_operand() {
    let mut a = test_analyzer();
    let count = a.builder.signal(Value::Int(0));
    a.builder
        .set_signal_integer_domain(count, Some(IntegerDomain::U32));
    a.builder.set_signal_ty(count, ClientType::U32);
    a.signals.insert("count".to_string(), count);
    let expr: Expr = syn::parse_quote!(ImportedAlias(count.get()) == count.get());

    let lowered = a
        .lower_reactive_expr(&expr)
        .expect("the known right operand should contextually type the helper result");

    assert!(matches!(
        lowered,
        FrontendExpr::BinaryOp { op: BinOp::Eq, .. }
    ));
    assert_eq!(
        a.direct_helper_edges[0].expected_return_domain,
        Some(IntegerDomain::U32)
    );
}

#[test]
fn unsupported_client_prelude_alias_reports_guided_error() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::Int(0));
    a.signals.insert("count".to_string(), id);
    let signal = local_from_stmt(syn::parse_quote! {
        let count = signal(0_i64);
    });
    let local = local_from_stmt(syn::parse_quote! {
        let display = format!("{}", count.get());
    });

    let err = a
        .collect_client_prelude_locals(&[signal, local])
        .expect_err("unsupported client-derived aliases fail at their declaration");
    let message = err.to_string();
    assert!(
        message.contains("depends on rsjs client state"),
        "unexpected diagnostic: {message}"
    );
    assert!(
        message.contains("count"),
        "unexpected diagnostic: {message}"
    );
}

#[test]
fn unsupported_transitive_client_prelude_alias_reports_guided_error() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::Int(0));
    a.signals.insert("count".to_string(), id);
    let signal = local_from_stmt(syn::parse_quote! {
        let count = signal(0_i64);
    });
    let first = local_from_stmt(syn::parse_quote! {
        let current = count.get();
    });
    let second = local_from_stmt(syn::parse_quote! {
        let display = format!("{}", current);
    });

    let err = a
        .collect_client_prelude_locals(&[signal, first, second])
        .expect_err("unsupported transitive aliases fail at their declaration");
    let message = err.to_string();
    assert!(
        message.contains("depends on rsjs client state"),
        "unexpected diagnostic: {message}"
    );
    assert!(
        message.contains("current"),
        "unexpected diagnostic: {message}"
    );
}

#[test]
fn opaque_server_boundary_rejects_client_prelude_alias() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::Int(0));
    a.signals.insert("count".to_string(), id);
    let local = local_from_stmt(syn::parse_quote! {
        let display = count.get();
    });
    a.collect_client_prelude_locals(&[local]).unwrap();
    let expr: Expr = syn::parse_quote! { display };

    let err = a
        .reject_reactive_captures_in_opaque_boundary(&expr, OpaqueBoundary::ServerOnly)
        .expect_err("client-derived aliases must not cross server_only boundaries");

    let message = err.to_string();
    assert!(
        message.contains("client-derived prelude local `display`"),
        "unexpected diagnostic: {message}"
    );
}

#[test]
fn opaque_expression_capture_scan_honors_nested_rust_scopes() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::Int(0));
    a.signals.insert("count".to_string(), id);
    let expr: Expr = syn::parse_quote! {
        render_server_fragment({
            let count = self.server_count();
            match self.row {
                Some(count) => render_row(count),
                None => render_row(0),
            }
        })
    };

    assert!(
        !expression_mentions_known_client_local(
            &expr,
            &a.signals,
            &a.queries,
            &a.mutations,
            &a.node_refs,
            &a.handlers,
            &a.client_prelude_locals,
            &a.unsupported_client_prelude_locals,
        ),
        "block and match bindings must shadow the outer client signal"
    );
    a.reject_reactive_captures_in_opaque_boundary(&expr, OpaqueBoundary::ServerOnly)
        .expect("the explicit boundary scan must honor the same Rust scopes");
}

#[test]
fn opaque_expression_capture_scan_checks_initializer_before_shadow_enters_scope() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::Int(0));
    a.signals.insert("count".to_string(), id);
    let expr: Expr = syn::parse_quote! {{
        let count = count.get();
        render_row(count)
    }};

    assert!(
        expression_mentions_known_client_local(
            &expr,
            &a.signals,
            &a.queries,
            &a.mutations,
            &a.node_refs,
            &a.handlers,
            &a.client_prelude_locals,
            &a.unsupported_client_prelude_locals,
        ),
        "the new pattern must not hide a client read in its own initializer"
    );
    a.reject_reactive_captures_in_opaque_boundary(&expr, OpaqueBoundary::ServerOnly)
        .expect_err("the explicit boundary must reject the initializer capture");
}

#[test]
fn loop_payload_projection_must_be_rooted_and_free_of_live_captures() {
    let mut analyzer = test_analyzer();
    let selected = analyzer.builder.signal(Value::Int(0));
    analyzer.signals.insert("selected".to_string(), selected);
    analyzer.loop_scopes.push(LoopScope {
        locals: HashSet::from(["row".to_string()]),
        integer_domains: HashMap::new(),
    });
    let node = analyzer.builder.element("button", None);
    let scope = ExprScope {
        node_payload_target: Some(node),
        ..ExprScope::default()
    };

    let rooted: Expr = syn::parse_quote! { (row.items[0].key as u32) };
    assert!(analyzer.can_lower_rooted_node_payload(&rooted, &scope));

    let mixed: Expr = syn::parse_quote! { (row.key, selected.get()).1 };
    assert!(analyzer.expr_mentions_loop_local(&mixed));
    assert!(
        !analyzer.can_lower_rooted_node_payload(&mixed, &scope),
        "a tuple projection is not rooted in the repeated item"
    );
    assert!(
        !analyzer.can_lower_node_payload(&mixed, &scope),
        "a payload fallback must never snapshot a visible signal read"
    );
}

#[test]
fn static_loop_prefix_rejects_live_provenance_before_local_shadowing() {
    let mut analyzer = test_analyzer();
    let selected = analyzer.builder.signal(Value::Int(0));
    analyzer.signals.insert("selected".to_string(), selected);
    let for_loop: ExprForLoop = syn::parse_quote! {
        for row in rows {
            let active = selected.get() == row.key;
            proa_macros::html_sync! {
                <button data_active={active}>{row.label}</button>
            }
        }
    };
    let parent = analyzer.builder.element("section", None);

    let error = analyzer
        .lower_for_loop(&for_loop, parent)
        .expect_err("a signal-derived prefix local would freeze live state");
    let message = error.to_string();
    assert!(
        message.contains("static template-loop prefix")
            && message.contains("reactive signal `selected`")
            && message.contains("template_loop.reactive_prefix_capture"),
        "unexpected diagnostic: {message}"
    );
}

#[test]
fn loop_local_shadows_same_named_signal_during_payload_lowering() {
    let mut analyzer = test_analyzer();
    let outer = analyzer.builder.signal(Value::String("client".to_string()));
    analyzer.signals.insert("row".to_string(), outer);
    analyzer.loop_scopes.push(LoopScope {
        locals: HashSet::from(["row".to_string()]),
        integer_domains: HashMap::new(),
    });
    let node = analyzer.builder.element("button", None);
    let scope = ExprScope {
        node_payload_target: Some(node),
        ..ExprScope::default()
    };
    let expr: Expr = syn::parse_quote! { row };

    assert!(analyzer.is_simple_server_slot_expr(&expr));
    assert!(matches!(
        analyzer.lower_reactive_expr_in(&expr, &scope).unwrap(),
        FrontendExpr::NodePayload { tokens, .. } if tokens == "row"
    ));
}

#[test]
fn static_range_domain_is_preserved_on_direct_loop_payload_reads() {
    let pattern: Pat = syn::parse_quote!(idx);
    let iterator: Expr = syn::parse_quote!(0_u32..42_u32);
    assert_eq!(
        template_loop_integer_domains(&pattern, &iterator).unwrap(),
        HashMap::from([("idx".to_string(), IntegerDomain::U32)])
    );

    let mut analyzer = test_analyzer();
    analyzer.loop_scopes.push(LoopScope {
        locals: HashSet::from(["idx".to_string()]),
        integer_domains: HashMap::from([("idx".to_string(), IntegerDomain::U32)]),
    });
    let node = analyzer.builder.element("button", None);
    let scope = ExprScope {
        node_payload_target: Some(node),
        ..ExprScope::default()
    };
    let expr: Expr = syn::parse_quote!(idx);

    let lowered = analyzer
        .lower_reactive_expr_in(&expr, &scope)
        .expect("the proven range item lowers as a typed node payload");
    assert!(matches!(
        lowered,
        FrontendExpr::BinaryOp {
            op: BinOp::Checked {
                op: CheckedIntegerOp::Add,
                domain: IntegerDomain::U32,
            },
            lhs,
            rhs,
        } if matches!(lhs.as_ref(), FrontendExpr::NodePayload { tokens, .. } if tokens == "idx")
            && matches!(rhs.as_ref(), FrontendExpr::Literal(Value::Int(0)))
    ));
}

#[test]
fn static_range_domain_requires_consistent_explicit_source_evidence() {
    let pattern: Pat = syn::parse_quote!(idx);

    let contextual: Expr = syn::parse_quote!(0_u32..end);
    assert_eq!(
        template_loop_integer_domains(&pattern, &contextual).unwrap(),
        HashMap::from([("idx".to_string(), IntegerDomain::U32)]),
        "one explicit bound determines the other bound and range item in Rust"
    );

    for iterator in [
        syn::parse_quote!(0..42),
        syn::parse_quote!(start..end),
        syn::parse_quote!(items),
        syn::parse_quote!(0_u8..42_u8),
    ] {
        assert!(
            template_loop_integer_domains(&pattern, &iterator)
                .unwrap()
                .is_empty(),
            "an unproven iterator must not assign a client integer domain: {iterator:?}"
        );
    }

    let by_reference: Pat = syn::parse_quote!(ref idx);
    let proven_range: Expr = syn::parse_quote!(0_u32..42_u32);
    assert!(
        template_loop_integer_domains(&by_reference, &proven_range)
            .unwrap()
            .is_empty(),
        "a `ref` pattern binds `&u32`, not the range item's scalar domain"
    );

    let mismatched: Expr = syn::parse_quote!(0_u32..42_u64);
    let error = template_loop_integer_domains(&pattern, &mismatched)
        .expect_err("conflicting range suffixes must not choose either domain");
    assert!(
        error.to_string().contains("integer-range-domain-mismatch"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn untyped_inner_loop_binding_shadows_outer_range_domain() {
    let mut analyzer = test_analyzer();
    analyzer.loop_scopes.push(LoopScope {
        locals: HashSet::from(["idx".to_string()]),
        integer_domains: HashMap::from([("idx".to_string(), IntegerDomain::U32)]),
    });
    analyzer.loop_scopes.push(LoopScope {
        locals: HashSet::from(["idx".to_string()]),
        integer_domains: HashMap::new(),
    });
    let node = analyzer.builder.element("button", None);
    let scope = ExprScope {
        node_payload_target: Some(node),
        ..ExprScope::default()
    };
    let expr: Expr = syn::parse_quote!(idx);

    assert!(matches!(
        analyzer.lower_reactive_expr_in(&expr, &scope).unwrap(),
        FrontendExpr::NodePayload { .. }
    ));
}

#[test]
fn server_helper_call_shadowed_by_a_later_signal_stays_a_server_local() {
    // A prelude that calls a server helper function and only later binds a
    // signal of the same name. At the call site the name still resolves to the
    // function, so neither the call nor anything derived from it may be
    // reclassified as client-derived and dropped from the server replay.
    let mut a = test_analyzer();
    let lets = vec![
        local_from_stmt(syn::parse_quote! {
            let label = selected_label(self.colors, self.selected);
        }),
        local_from_stmt(syn::parse_quote! {
            let display_label = label.unwrap_or(self.placeholder);
        }),
        local_from_stmt(syn::parse_quote! {
            let selected_label = rsjs::signal(display_label.to_string());
        }),
    ];
    let statements = lets.iter().cloned().map(Stmt::Local).collect::<Vec<_>>();
    a.collect_prelude_bindings(&statements, &lets).unwrap();

    assert!(
        !a.unsupported_client_prelude_locals.contains_key("label"),
        "a call to a server helper function must not be read as a signal use"
    );
    assert!(
        !a.unsupported_client_prelude_locals
            .contains_key("display_label"),
        "a server local derived from a server helper call must stay server-side"
    );
    assert!(
        !a.client_prelude_locals.contains_key("label")
            && !a.client_prelude_locals.contains_key("display_label"),
        "neither local is client-derived"
    );
    assert!(
        a.client_prelude_local_indices.is_empty(),
        "both server lets must survive the replay filter, got dropped indices {:?}",
        a.client_prelude_local_indices
    );
}

#[test]
fn ordinary_method_on_self_field_stays_in_server_prelude() {
    let mut analyzer = test_analyzer();
    let lets = vec![local_from_stmt(syn::parse_quote! {
        let label = self.label.as_ref();
    })];
    let statements = lets.iter().cloned().map(Stmt::Local).collect::<Vec<_>>();

    analyzer
        .collect_prelude_bindings(&statements, &lets)
        .expect("an ordinary Rust method on a server field must remain lossless server code");

    assert!(analyzer.client_prelude_local_indices.is_empty());
    assert!(analyzer.client_prelude_locals.is_empty());
    assert!(analyzer.inbound_inputs.fields.is_empty());
    assert!(analyzer.inbound_inputs.node_ref_fields.is_empty());
    assert!(
        analyzer.builder.signal_decls().is_empty(),
        "speculative lowering must not leak a null inbound signal into hydration seeds"
    );
}

#[test]
fn a_signal_read_before_its_declaration_is_not_a_client_dependency() {
    // Lexical scope, the other direction: `count` is a plain identifier from
    // outside the prelude until the `let` that binds the signal runs.
    let mut a = test_analyzer();
    let lets = vec![
        local_from_stmt(syn::parse_quote! {
            let seed = count;
        }),
        local_from_stmt(syn::parse_quote! {
            let count = rsjs::signal(seed);
        }),
    ];
    let statements = lets.iter().cloned().map(Stmt::Local).collect::<Vec<_>>();
    a.collect_prelude_bindings(&statements, &lets).unwrap();

    assert!(
        !a.client_prelude_locals.contains_key("seed")
            && !a.unsupported_client_prelude_locals.contains_key("seed"),
        "a name used before its signal `let` is not the signal"
    );
    assert!(
        a.client_prelude_local_indices.is_empty(),
        "the server let must survive the replay filter, got dropped indices {:?}",
        a.client_prelude_local_indices
    );
}

#[test]
fn wide_division_uses_checked_i64_division() {
    let mut a = test_analyzer();
    let id = a.builder.signal(Value::BigInt(9_007_199_254_740_993));
    a.builder
        .set_signal_integer_domain(id, Some(IntegerDomain::I64));
    a.signals.insert("big".to_string(), id);
    let expr: Expr = syn::parse_quote! { big.get() / 2_i64 };
    let lowered = a
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .unwrap();
    assert_eq!(
        lowered,
        FrontendExpr::BinaryOp {
            op: BinOp::Checked {
                op: CheckedIntegerOp::Div,
                domain: IntegerDomain::I64,
            },
            lhs: Box::new(FrontendExpr::SignalRead(id)),
            rhs: Box::new(FrontendExpr::Literal(Value::BigInt(2))),
        },
        "wide integer division must preserve Rust's checked edge cases"
    );
}

#[test]
fn div_int_turbofish_specializes_dyn_shared_operands_in_every_supported_domain() {
    for (name, domain) in [
        ("i32", IntegerDomain::I32),
        ("u32", IntegerDomain::U32),
        ("i64", IntegerDomain::I64),
        ("u64", IntegerDomain::U64),
    ] {
        let mut analyzer = test_analyzer();
        let store = analyzer
            .builder
            .shared_store("state".into(), "NumericState".into(), None);
        let lhs = analyzer
            .builder
            .shared_field(store, "lhs".into(), ClientType::Dyn);
        let rhs = analyzer
            .builder
            .shared_field(store, "rhs".into(), ClientType::Dyn);
        analyzer.signals.insert("lhs".to_string(), lhs);
        analyzer.signals.insert("rhs".to_string(), rhs);
        let expr: Expr = syn::parse_str(&format!("rsjs::div_int::<{name}>(lhs.get(), rhs.get())"))
            .expect("valid helper expression");

        let lowered = analyzer
            .lower_reactive_expr_in(&expr, &ExprScope::default())
            .unwrap_or_else(|error| panic!("{name} explicit shared division failed: {error}"));
        assert_eq!(
            analyzer.builder.signal_integer_domain(lhs),
            Some(domain),
            "{name} left shared operand was not specialized"
        );
        assert_eq!(
            analyzer.builder.signal_integer_domain(rhs),
            Some(domain),
            "{name} right shared operand was not specialized"
        );
        assert_eq!(
            lowered,
            FrontendExpr::BinaryOp {
                op: BinOp::Checked {
                    op: CheckedIntegerOp::Div,
                    domain,
                },
                lhs: Box::new(FrontendExpr::SignalRead(lhs)),
                rhs: Box::new(FrontendExpr::SignalRead(rhs)),
            }
        );
    }
}

#[test]
fn div_int_turbofish_contextualizes_nested_shared_arithmetic() {
    let mut analyzer = test_analyzer();
    let store = analyzer
        .builder
        .shared_store("state".into(), "NumericState".into(), None);
    let lhs = analyzer
        .builder
        .shared_field(store, "lhs".into(), ClientType::Dyn);
    let rhs = analyzer
        .builder
        .shared_field(store, "rhs".into(), ClientType::Dyn);
    analyzer.signals.insert("lhs".to_string(), lhs);
    analyzer.signals.insert("rhs".to_string(), rhs);
    let expr: Expr = syn::parse_quote! { rsjs::div_int::<i64>(lhs.get() + 1_i64, rhs.get()) };

    let lowered = analyzer
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect("the explicit outer i64 domain should contextualize the nested add");
    assert_eq!(
        lowered,
        FrontendExpr::BinaryOp {
            op: BinOp::Checked {
                op: CheckedIntegerOp::Div,
                domain: IntegerDomain::I64,
            },
            lhs: Box::new(FrontendExpr::BinaryOp {
                op: BinOp::Checked {
                    op: CheckedIntegerOp::Add,
                    domain: IntegerDomain::I64,
                },
                lhs: Box::new(FrontendExpr::SignalRead(lhs)),
                rhs: Box::new(FrontendExpr::Literal(Value::BigInt(1))),
            }),
            rhs: Box::new(FrontendExpr::SignalRead(rhs)),
        }
    );

    let mut mismatch = test_analyzer();
    let store = mismatch
        .builder
        .shared_store("state".into(), "NumericState".into(), None);
    let lhs = mismatch
        .builder
        .shared_field(store, "lhs".into(), ClientType::Dyn);
    let rhs = mismatch
        .builder
        .shared_field(store, "rhs".into(), ClientType::Dyn);
    mismatch.signals.insert("lhs".to_string(), lhs);
    mismatch.signals.insert("rhs".to_string(), rhs);
    let expr: Expr = syn::parse_quote! { rsjs::div_int::<i64>(lhs.get() + 1_u32, rhs.get()) };
    let error = mismatch
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect_err("a nested explicit u32 suffix must still mismatch the outer i64 domain");
    assert!(
        error.to_string().contains("integer-domain-mismatch")
            && error.to_string().contains("i64")
            && error.to_string().contains("u32"),
        "unexpected nested mismatch diagnostic: {error}"
    );

    let mut literals = test_analyzer();
    let expr: Expr = syn::parse_quote! { rsjs::div_int::<i64>(4_000_000_000 + 2, 3) };
    let lowered = literals
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect("the explicit i64 argument context should reach nested unsuffixed arithmetic");
    assert_eq!(
        lowered,
        FrontendExpr::BinaryOp {
            op: BinOp::Checked {
                op: CheckedIntegerOp::Div,
                domain: IntegerDomain::I64,
            },
            lhs: Box::new(FrontendExpr::BinaryOp {
                op: BinOp::Checked {
                    op: CheckedIntegerOp::Add,
                    domain: IntegerDomain::I64,
                },
                lhs: Box::new(FrontendExpr::Literal(Value::BigInt(4_000_000_000))),
                rhs: Box::new(FrontendExpr::Literal(Value::BigInt(2))),
            }),
            rhs: Box::new(FrontendExpr::Literal(Value::BigInt(3))),
        },
        "nested unsuffixed literals must not default to i32 inside an explicit i64 call"
    );
}

#[test]
fn div_int_turbofish_requires_an_exact_primitive_type_path() {
    for source in [
        "rsjs::div_int::<&i32>(8_i32, 2_i32)",
        "rsjs::div_int::<Option<i32>>(8_i32, 2_i32)",
        "rsjs::div_int::<(i32)>(8_i32, 2_i32)",
        "rsjs::div_int::<lookalike::i32>(8_i32, 2_i32)",
    ] {
        let mut analyzer = test_analyzer();
        let expr: Expr = syn::parse_str(source).expect("valid Rust call expression");
        let error = analyzer
            .lower_reactive_expr_in(&expr, &ExprScope::default())
            .expect_err("only an exact primitive type path may set the helper domain");
        let message = error.to_string();
        assert!(
            message.contains("rsjs/integer-domain-unproven")
                && message.contains("primitive type")
                && message.contains("lookalike paths"),
            "unexpected diagnostic for `{source}`: {message}"
        );
    }

    for source in [
        "rsjs::div_int::<i32>(8_i32, 2_i32)",
        "rsjs::div_int::<core::primitive::i32>(8_i32, 2_i32)",
        "rsjs::div_int::<std::primitive::i32>(8_i32, 2_i32)",
        "rsjs::div_int::<::core::primitive::i32>(8_i32, 2_i32)",
    ] {
        let mut analyzer = test_analyzer();
        let expr: Expr = syn::parse_str(source).expect("valid primitive helper expression");
        let lowered = analyzer
            .lower_reactive_expr_in(&expr, &ExprScope::default())
            .unwrap_or_else(|error| panic!("canonical primitive path `{source}` failed: {error}"));
        assert!(matches!(
            lowered,
            FrontendExpr::BinaryOp {
                op: BinOp::Checked {
                    op: CheckedIntegerOp::Div,
                    domain: IntegerDomain::I32,
                },
                ..
            }
        ));
    }
}

#[test]
fn div_int_turbofish_validates_conditional_branch_integer_provenance() {
    for (source, expected_code) in [
        (
            "rsjs::div_int::<i64>(if flag.get() { 1_i32 } else { 2 }, 3)",
            "rsjs/integer-domain-mismatch",
        ),
        (
            "rsjs::div_int::<i64>(if flag.get() { 1_u8 } else { 2 }, 3)",
            "rsjs/integer-domain-unsupported",
        ),
        (
            "rsjs::div_int::<i64>((if flag.get() { 1_i32 } else { 2 }), 3)",
            "rsjs/integer-domain-mismatch",
        ),
        (
            "rsjs::div_int::<i64>(-(if flag.get() { 1_i32 } else { 2 }), 3)",
            "rsjs/integer-domain-mismatch",
        ),
    ] {
        let mut analyzer = test_analyzer();
        let flag = analyzer.builder.signal(Value::Bool(true));
        analyzer.signals.insert("flag".to_string(), flag);
        let expr: Expr = syn::parse_str(source).expect("valid conditional helper expression");
        let error = analyzer
            .lower_reactive_expr_in(&expr, &ExprScope::default())
            .expect_err("nested explicit branch provenance must not be erased");
        assert!(
            error.to_string().contains(expected_code),
            "unexpected diagnostic for `{source}`: {error}"
        );
    }

    for source in [
        "rsjs::div_int::<i64>(if flag.get() { 1_i64 } else { 2 }, 3)",
        "rsjs::div_int::<i64>(if flag.get() { 1 } else { 2 }, 3)",
    ] {
        let mut analyzer = test_analyzer();
        let flag = analyzer.builder.signal(Value::Bool(true));
        analyzer.signals.insert("flag".to_string(), flag);
        let expr: Expr = syn::parse_str(source).expect("valid conditional helper expression");
        let lowered = analyzer
            .lower_reactive_expr_in(&expr, &ExprScope::default())
            .unwrap_or_else(|error| panic!("valid conditional `{source}` failed: {error}"));
        let FrontendExpr::BinaryOp {
            op:
                BinOp::Checked {
                    op: CheckedIntegerOp::Div,
                    domain: IntegerDomain::I64,
                },
            lhs,
            rhs,
        } = lowered
        else {
            panic!("unexpected conditional div_int lowering for `{source}`");
        };
        assert!(matches!(*rhs, FrontendExpr::Literal(Value::BigInt(3))));
        let FrontendExpr::Conditional {
            then_expr,
            else_expr,
            ..
        } = *lhs
        else {
            panic!("expected a conditional left operand for `{source}`");
        };
        assert_eq!(*then_expr, FrontendExpr::Literal(Value::BigInt(1)));
        assert_eq!(*else_expr, FrontendExpr::Literal(Value::BigInt(2)));
    }
}

#[test]
fn bare_shared_div_int_is_rejected_independent_of_prior_specialization() {
    let mut analyzer = test_analyzer();
    let store = analyzer
        .builder
        .shared_store("state".into(), "NumericState".into(), None);
    let lhs = analyzer
        .builder
        .shared_field(store, "lhs".into(), ClientType::Dyn);
    let rhs = analyzer
        .builder
        .shared_field(store, "rhs".into(), ClientType::Dyn);
    analyzer.signals.insert("lhs".to_string(), lhs);
    analyzer.signals.insert("rhs".to_string(), rhs);

    let explicit: Expr = syn::parse_quote! { rsjs::div_int::<i32>(lhs.get(), rhs.get()) };
    analyzer
        .lower_reactive_expr_in(&explicit, &ExprScope::default())
        .expect("the explicit helper should establish checked shared domains");
    assert_eq!(
        analyzer.builder.signal_integer_domain(lhs),
        Some(IntegerDomain::I32)
    );
    assert_eq!(
        analyzer.builder.signal_integer_domain(rhs),
        Some(IntegerDomain::I32)
    );

    for source in [
        "rsjs::div_int(lhs.get(), rhs.get())",
        "rsjs::div_int(lhs.get() + 1_i32, rhs.get())",
    ] {
        let bare: Expr = syn::parse_str(source).expect("valid bare helper expression");
        let error = analyzer
            .lower_reactive_expr_in(&bare, &ExprScope::default())
            .expect_err("shared reads require an explicit width at every helper boundary");
        assert!(
            error.to_string().contains("rsjs/integer-domain-unproven")
                && error.to_string().contains("div_int::<i32>"),
            "unexpected order-independent shared diagnostic for `{source}`: {error}"
        );
    }

    let mut reverse = test_analyzer();
    let store = reverse
        .builder
        .shared_store("state".into(), "NumericState".into(), None);
    let lhs = reverse
        .builder
        .shared_field(store, "lhs".into(), ClientType::Dyn);
    let rhs = reverse
        .builder
        .shared_field(store, "rhs".into(), ClientType::Dyn);
    reverse.signals.insert("lhs".to_string(), lhs);
    reverse.signals.insert("rhs".to_string(), rhs);
    let bare: Expr = syn::parse_quote! { rsjs::div_int(lhs.get(), rhs.get()) };
    reverse
        .lower_reactive_expr_in(&bare, &ExprScope::default())
        .expect_err("the same bare call must also fail before specialization");
    reverse
        .lower_reactive_expr_in(&explicit, &ExprScope::default())
        .expect("a later explicit helper remains valid after a rejected bare call");
}

#[test]
fn div_int_without_turbofish_rejects_a_dyn_operand_even_with_a_typed_literal() {
    for source in [
        "rsjs::div_int(value.get(), 2_i32)",
        "rsjs::div_int(2_i32, value.get())",
    ] {
        let mut analyzer = test_analyzer();
        let store = analyzer
            .builder
            .shared_store("state".into(), "NumericState".into(), None);
        let value = analyzer
            .builder
            .shared_field(store, "value".into(), ClientType::Dyn);
        analyzer.signals.insert("value".to_string(), value);
        let expr: Expr = syn::parse_str(source).expect("valid helper expression");

        let error = analyzer
            .lower_reactive_expr_in(&expr, &ExprScope::default())
            .expect_err("a Dyn shared operand requires an explicit helper domain");
        let message = error.to_string();
        assert!(
            message.contains("rsjs/integer-domain-unproven") && message.contains("div_int::<i32>"),
            "unexpected diagnostic for `{source}`: {message}"
        );
        assert_eq!(
            analyzer.builder.signal_integer_domain(value),
            None,
            "a rejected inference must not mutate the shared declaration"
        );
    }
}

#[test]
fn div_int_without_turbofish_infers_two_matching_proven_operands() {
    let mut analyzer = test_analyzer();
    let lhs = analyzer.builder.signal(Value::BigInt(8));
    let rhs = analyzer.builder.signal(Value::BigInt(2));
    for id in [lhs, rhs] {
        analyzer
            .builder
            .set_signal_integer_domain(id, Some(IntegerDomain::I64));
        analyzer.builder.set_signal_ty(id, ClientType::I64);
    }
    analyzer.signals.insert("lhs".to_string(), lhs);
    analyzer.signals.insert("rhs".to_string(), rhs);
    let expr: Expr = syn::parse_quote! { rsjs::div_int(lhs.get(), rhs.get()) };

    let lowered = analyzer
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect("two independently proven i64 operands may infer the helper domain");
    assert!(matches!(
        lowered,
        FrontendExpr::BinaryOp {
            op: BinOp::Checked {
                op: CheckedIntegerOp::Div,
                domain: IntegerDomain::I64,
            },
            ..
        }
    ));
}

#[test]
fn div_int_rejects_unsupported_and_mismatched_explicit_widths() {
    let mut unsupported = test_analyzer();
    let expr: Expr = syn::parse_quote! { rsjs::div_int::<u8>(8_u8, 2_u8) };
    let error = unsupported
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect_err("u8 is outside the supported integer contract");
    assert!(
        error.to_string().contains("integer-domain-unsupported")
            && error.to_string().contains("u8"),
        "unexpected unsupported-width diagnostic: {error}"
    );

    let mut mismatch = test_analyzer();
    let value = mismatch.builder.signal(Value::Int(8));
    mismatch
        .builder
        .set_signal_integer_domain(value, Some(IntegerDomain::I32));
    mismatch.builder.set_signal_ty(value, ClientType::I32);
    mismatch.signals.insert("value".to_string(), value);
    let expr: Expr = syn::parse_quote! { rsjs::div_int::<i64>(value.get(), 2_i64) };
    let error = mismatch
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect_err("an explicit i64 helper must reject an i32 operand");
    assert!(
        error.to_string().contains("integer-domain-mismatch")
            && error.to_string().contains("i32")
            && error.to_string().contains("i64"),
        "unexpected mismatch diagnostic: {error}"
    );
}

#[test]
fn typed_conditions_reject_non_bool_and_keep_unknown_conservative() {
    let mut analyzer = test_analyzer();
    let count = analyzer.builder.signal(Value::Int(0));
    let visible = analyzer.builder.signal(Value::Bool(false));
    let runtime = analyzer
        .builder
        .signal_typed(Value::Null, ClientType::Unknown);

    let error = analyzer
        .require_bool_condition(
            &FrontendExpr::SignalRead(count),
            &[],
            proc_macro2::Span::call_site(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("statically known type"));

    analyzer
        .require_bool_condition(
            &FrontendExpr::SignalRead(visible),
            &[],
            proc_macro2::Span::call_site(),
        )
        .unwrap();
    analyzer
        .require_bool_condition(
            &FrontendExpr::SignalRead(runtime),
            &[],
            proc_macro2::Span::call_site(),
        )
        .unwrap();
}

#[test]
fn value_level_if_rejects_a_statically_numeric_condition() {
    let mut analyzer = test_analyzer();
    let count = analyzer.builder.signal(Value::Int(0));
    analyzer.signals.insert("count".to_string(), count);
    let expr: Expr = syn::parse_quote! {
        if count.get() { 1 } else { 0 }
    };

    let error = analyzer
        .lower_reactive_expr(&expr)
        .expect_err("value-level if must reject a condition proven numeric");
    let message = error.to_string();
    assert!(message.contains("statically known type"), "{message}");
    assert!(message.contains("`I32`"), "{message}");
}

#[test]
fn logical_and_rejects_a_statically_numeric_operand() {
    let mut analyzer = test_analyzer();
    let count = analyzer.builder.signal(Value::Int(0));
    analyzer.signals.insert("count".to_string(), count);
    let expr: Expr = syn::parse_quote! {
        if count.get() && true { 1 } else { 0 }
    };

    let error = analyzer
        .lower_reactive_expr(&expr)
        .expect_err("logical && must reject an operand proven numeric");
    let message = error.to_string();
    assert!(message.contains("statically known type"), "{message}");
    assert!(message.contains("`I32`"), "{message}");
}

#[test]
fn unary_not_rejects_a_statically_numeric_operand() {
    let mut analyzer = test_analyzer();
    let count = analyzer.builder.signal(Value::Int(0));
    analyzer.signals.insert("count".to_string(), count);
    let expr: Expr = syn::parse_quote! {
        if !count.get() { 1 } else { 0 }
    };

    let error = analyzer
        .lower_reactive_expr(&expr)
        .expect_err("unary ! must reject an operand proven numeric");
    let message = error.to_string();
    assert!(message.contains("statically known type"), "{message}");
    assert!(message.contains("`I32`"), "{message}");
}

#[test]
fn handler_match_guard_rejects_a_statically_numeric_condition() {
    let mut analyzer = test_analyzer();
    let count = analyzer.builder.signal(Value::Int(0));
    analyzer.signals.insert("count".to_string(), count);
    let closure: ExprClosure = syn::parse_quote!(|_| {
        match 1_i64 {
            1 if count.get() => count.set(2),
            _ => count.set(3),
        }
    });

    let error = match analyzer.lower_handler_closure_hir_body(&closure, &HandlerScope::default()) {
        Ok(_) => panic!("handler match guard must reject a condition proven numeric"),
        Err(error) => error,
    };
    let message = error.to_string();
    assert!(message.contains("statically known type"), "{message}");
    assert!(message.contains("`I32`"), "{message}");
}

#[test]
fn template_if_rejects_a_statically_numeric_condition() {
    let mut analyzer = test_analyzer();
    let count = analyzer.builder.signal(Value::Int(0));
    analyzer.signals.insert("count".to_string(), count);
    let parent = analyzer.builder.element("div", None);
    let if_expr: ExprIf = syn::parse_quote! {
        if count.get() {
            html_sync! {
                <span>"yes"</span>
            }
        } else {
            html_sync! {
                <span>"no"</span>
            }
        }
    };

    let error = analyzer
        .lower_conditional(&if_expr, parent)
        .expect_err("template if must reject a condition proven numeric");
    let message = error.to_string();
    assert!(message.contains("statically known type"), "{message}");
    assert!(message.contains("`I32`"), "{message}");
}

#[test]
fn template_else_if_rejects_a_statically_numeric_condition() {
    let mut analyzer = test_analyzer();
    let visible = analyzer.builder.signal(Value::Bool(false));
    analyzer.signals.insert("visible".to_string(), visible);
    let count = analyzer.builder.signal(Value::Int(0));
    analyzer.signals.insert("count".to_string(), count);
    let parent = analyzer.builder.element("div", None);
    let if_expr: ExprIf = syn::parse_quote! {
        if visible.get() {
            html_sync! {
                <span>"visible"</span>
            }
        } else if count.get() {
            html_sync! {
                <span>"count"</span>
            }
        } else {
            html_sync! {
                <span>"hidden"</span>
            }
        }
    };

    let error = analyzer
        .lower_conditional(&if_expr, parent)
        .expect_err("template else-if must reject a condition proven numeric");
    let message = error.to_string();
    assert!(message.contains("statically known type"), "{message}");
    assert!(message.contains("`I32`"), "{message}");
}

#[test]
fn dynamic_event_handler_attrs_are_rejected() {
    // Event handlers lower to `node.setAttribute('on...', String(value))`
    // client-side, which executes attacker JS with no escaping defense.
    for attr in [
        "onclick",
        "onerror",
        "onmouseover",
        "onload",
        "onfocus",
        "onsubmit",
        // Case-insensitive: source authors may write camelCase.
        "onClick",
        "ONERROR",
    ] {
        assert!(
            is_dynamic_attr_rejected_for_safety(attr),
            "expected `{attr}` to be rejected as a dynamic event handler"
        );
    }
}

#[test]
fn dynamic_html_injection_attrs_are_rejected() {
    assert!(is_dynamic_attr_rejected_for_safety("srcdoc"));
    assert!(is_dynamic_attr_rejected_for_safety("srcset"));
    assert!(is_dynamic_attr_rejected_for_safety("ping"));
    assert!(is_dynamic_attr_rejected_for_safety("style"));
}

#[test]
fn single_url_dynamic_attrs_use_sanitized_bindings() {
    for attr in [
        "href",
        "src",
        "action",
        "formaction",
        "poster",
        "cite",
        "data",
        "longdesc",
        "manifest",
        "background",
        "xlink:href",
        "xlink-href",
    ] {
        assert!(
            !is_dynamic_attr_rejected_for_safety(attr),
            "expected `{attr}` to pass analyzer safety checks"
        );
        assert!(
            is_dynamic_url_attr(attr),
            "expected `{attr}` to lower as a sanitized URL binding"
        );
    }
}

#[test]
fn safe_dynamic_attrs_are_allowed() {
    // Normal presentational / semantic attrs must remain bindable. The
    // `on*` check must not over-match these (none start with `on` + more
    // ascii letters).
    for attr in ["class", "id", "data-foo", "aria-label", "title", "role"] {
        assert!(
            !is_dynamic_attr_rejected_for_safety(attr),
            "expected `{attr}` to remain an allowed dynamic attribute"
        );
    }
}

#[test]
fn authored_rsjs_marker_namespace_is_reserved_after_html_normalization() {
    for (tokens, normalized) in [
        (
            quote::quote! { <div data_rsjs_list="author"></div> },
            "data-rsjs-list",
        ),
        (
            quote::quote! { <div DATA_RSJS_COMPONENT></div> },
            "data-rsjs-component",
        ),
        (
            quote::quote! { <div Data_RsJs_Payload={self.value}></div> },
            "data-rsjs-payload",
        ),
    ] {
        let root = syn::parse2::<HtmlRoot>(tokens).expect("reserved-attribute fixture must parse");
        let mut analyzer = test_analyzer();
        let error = analyzer
            .walk_root(&root)
            .expect_err("authored data-rsjs attributes must not enter compiler marker storage");
        let message = error.to_string();
        assert!(message.contains(normalized), "{message}");
        assert!(
            message.contains("reserved for compiler-owned RSJS"),
            "{message}"
        );
        assert!(message.contains("data-app-*"), "{message}");
    }
}

#[test]
fn keyed_list_items_reject_authored_rsjs_marker_attributes() {
    let root = syn::parse2::<HtmlRoot>(quote::quote! {
        <li key={item} data_rsjs_key="author">{item}</li>
    })
    .expect("keyed-item fixture must parse");
    let Node::Element(element) = &root.nodes[0] else {
        panic!("fixture root must be an element");
    };
    let mut analyzer = test_analyzer();
    let parent = analyzer.builder.element("ul", None);
    let mut key = None;
    let error = analyzer
        .lower_keyed_item_element(element, parent, "item", true, &mut key)
        .expect_err("keyed item marker namespace must be compiler-owned");
    let message = error.to_string();
    assert!(message.contains("data-rsjs-key"), "{message}");
    assert!(
        message.contains("reserved for compiler-owned RSJS"),
        "{message}"
    );
}

#[test]
fn event_cmd_or_ctrl_shortcut_lowers_to_modifier_and_key_check() {
    let mut analyzer = test_analyzer();
    let closure: ExprClosure = syn::parse_quote!(|event| {
        let pressed = event.is_cmd_or_ctrl("K");
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: Some("event"),
        handler_locals: Vec::new(),
        event_node: Some(NodeId(1)),
    };

    let statements = analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .unwrap();
    let [statement] = statements.as_slice() else {
        panic!(
            "expected one lowered let statement, got {} statements",
            statements.len()
        );
    };
    let HandlerHirStmtKind::Let {
        name, ty, value, ..
    } = &statement.kind
    else {
        panic!("expected cmd/ctrl shortcut to lower inside a handler let");
    };

    assert_eq!(name, "pressed");
    assert_eq!(*ty, ClientType::Bool);
    assert_eq!(
        value.expr,
        FrontendExpr::BinaryOp {
            op: BinOp::And,
            lhs: Box::new(FrontendExpr::BinaryOp {
                op: BinOp::Or,
                lhs: Box::new(FrontendExpr::EventValue(EventValue::MetaKey)),
                rhs: Box::new(FrontendExpr::EventValue(EventValue::CtrlKey)),
            }),
            rhs: Box::new(FrontendExpr::BinaryOp {
                op: BinOp::Eq,
                lhs: Box::new(FrontendExpr::StringToLowercase {
                    target: Box::new(FrontendExpr::EventValue(EventValue::Key)),
                }),
                rhs: Box::new(FrontendExpr::Literal(Value::String("k".to_string()))),
            }),
        }
    );
}

#[test]
fn component_handler_remap_preserves_descriptor_client_block() {
    let mut analyzer = test_analyzer();
    let mut maps = ComponentInlineMaps::default();
    maps.signals.insert(SignalId(0), SignalId(2));

    let body_ir = ClientBlock::from_stmts(vec![
        ClientStmt::Let {
            local: ClientLocalId(0),
            name: "next".to_string(),
            ty: ClientType::String,
            value: Some(ClientExpr::EventRead(EventValue::TargetValue)),
        },
        ClientStmt::SignalSet {
            id: SignalId(0),
            value: ClientExpr::Local(ClientLocalId(0)),
        },
        ClientStmt::ForRange {
            local: ClientLocalId(1),
            name: "wide_index".to_owned(),
            integer_domain: IntegerDomain::U64,
            start: ClientExpr::Literal(Value::BigInt(0)),
            end: ClientExpr::Literal(Value::BigInt(3)),
            inclusive: false,
            body: ClientBlock::from_stmts(vec![ClientStmt::Expr(ClientExpr::Local(
                ClientLocalId(1),
            ))]),
        },
    ]);

    let remapped_body_ir = analyzer
        .remap_component_client_block(&body_ir, &mut maps)
        .unwrap();
    let handler = analyzer.builder.handler_from_ir(remapped_body_ir);
    let island = analyzer.builder.build();
    let handler = &island.handlers[handler.0 as usize];

    let ClientStmt::Let { local, ty, .. } = &handler.body_ir().stmts[0] else {
        panic!("expected remapped descriptor body_ir to keep the let statement");
    };
    assert_eq!(*local, ClientLocalId(0));
    assert_eq!(*ty, ClientType::String);

    let ClientStmt::SignalSet { id, value } = &handler.body_ir().stmts[1] else {
        panic!("expected remapped descriptor body_ir to keep the signal set");
    };
    assert_eq!(*id, SignalId(2));
    assert_eq!(*value, ClientExpr::Local(*local));

    let ClientStmt::ForRange {
        local,
        integer_domain,
        body,
        ..
    } = &handler.body_ir().stmts[2]
    else {
        panic!("expected remapped descriptor body_ir to keep the range statement");
    };
    assert_eq!(*local, ClientLocalId(1));
    assert_eq!(*integer_domain, IntegerDomain::U64);
    assert_eq!(
        body.stmts,
        vec![ClientStmt::Expr(ClientExpr::Local(ClientLocalId(1)))]
    );
}

#[test]
fn component_arena_remap_keeps_explicit_client_form() {
    let mut child_builder = IslandBuilder::new();
    let child_signal = child_builder.signal(Value::Int(0));
    let expr = child_builder.expr(ClientExpr::Field {
        base: Box::new(ClientExpr::SignalRead(child_signal)),
        name: "value".to_string(),
    });
    let child = child_builder.build();

    let analyzer = test_analyzer();
    let mut maps = ComponentInlineMaps::default();
    maps.signals.insert(child_signal, SignalId(2));

    let remapped = analyzer
        .remap_component_client_expr(child.exprs.client_expr(expr), &maps)
        .unwrap();

    assert_eq!(
        remapped,
        ClientExpr::Field {
            base: Box::new(ClientExpr::SignalRead(SignalId(2))),
            name: "value".to_string(),
        }
    );
}

#[test]
fn component_fragment_mount_plan_preserves_prop_source_anchor() {
    let mut child_builder = IslandBuilder::new();
    let root = child_builder.element("span", None);
    let child_signal = child_builder.signal(Value::String("child".to_string()));
    let expr = child_builder.expr(ClientExpr::SignalRead(child_signal));
    child_builder.bind_text(root, expr);
    child_builder.set_root(root);

    let descriptor = crate::component_descriptor::ComponentDescriptor {
        summary: crate::component_descriptor::ComponentDescriptorSummary {
            path: std::path::PathBuf::from("child.json"),
            asset_id: "child_fragment".to_string(),
            struct_name: "Child".to_string(),
            self_ty: "Child".to_string(),
            is_async: false,
            client_origin_permission:
                crate::component_descriptor::ComponentOriginPermission::OriginCapable,
            handler_fields: Vec::new(),
            prop_fields: vec!["name".to_string()],
            server_prop_fields: Vec::new(),
            static_slot_fields: Vec::new(),
            js_hash: "sha256:child".to_string(),
            descriptor_hash: "sha256:descriptor".to_string(),
        },
        island: child_builder.build(),
        prop_fields: vec![crate::component_descriptor::ComponentPropField {
            name: "name".to_string(),
            signal: child_signal,
        }],
        inbound_inputs: crate::component_descriptor::ParsedInboundInputs::default(),
        query_initializers: Vec::new(),
        mutation_initializers: Vec::new(),
    };
    let source_anchor = rsjs_ir::SourceAnchor {
        start_line: 42,
        start_column: 17,
    };
    let mut maps = ComponentInlineMaps::default();
    maps.prop_signals
        .insert(child_signal, FrontendExpr::SignalRead(SignalId(9)));
    maps.prop_signal_source_anchors
        .insert(child_signal, source_anchor);

    let plan = test_analyzer()
        .component_fragment_mount_plan(&descriptor, &maps, proc_macro2::Span::call_site())
        .unwrap()
        .expect("client child descriptor should mount as a fragment");

    assert_eq!(plan.signal_aliases.len(), 1);
    assert_eq!(plan.signal_aliases[0].child, child_signal);
    assert_eq!(plan.signal_aliases[0].parent, SignalId(9));
    assert_eq!(plan.signal_aliases[0].prop_name.as_deref(), Some("name"));
    assert_eq!(plan.signal_aliases[0].source_anchor, Some(source_anchor));
}

#[test]
fn component_with_shared_state_uses_full_inline_path() {
    let mut child_builder = IslandBuilder::new();
    let store = child_builder.shared_store(
        "workbench".to_string(),
        "crate::WorkbenchState".to_string(),
        None,
    );
    let color = child_builder.shared_field(store, "button_bg".to_string(), ClientType::String);
    let root = child_builder.element("span", None);
    let expr = child_builder.expr(ClientExpr::SignalRead(color));
    child_builder.bind_text(root, expr);
    child_builder.set_root(root);

    let descriptor = crate::component_descriptor::ComponentDescriptor {
        summary: crate::component_descriptor::ComponentDescriptorSummary {
            path: std::path::PathBuf::from("child.json"),
            asset_id: "child_shared_fragment".to_string(),
            struct_name: "Child".to_string(),
            self_ty: "Child".to_string(),
            is_async: false,
            client_origin_permission:
                crate::component_descriptor::ComponentOriginPermission::OriginCapable,
            handler_fields: Vec::new(),
            prop_fields: Vec::new(),
            server_prop_fields: Vec::new(),
            static_slot_fields: Vec::new(),
            js_hash: "sha256:child".to_string(),
            descriptor_hash: "sha256:descriptor".to_string(),
        },
        island: child_builder.build(),
        prop_fields: Vec::new(),
        inbound_inputs: crate::component_descriptor::ParsedInboundInputs::default(),
        query_initializers: Vec::new(),
        mutation_initializers: Vec::new(),
    };

    let plan = test_analyzer()
        .component_fragment_mount_plan(
            &descriptor,
            &ComponentInlineMaps::default(),
            proc_macro2::Span::call_site(),
        )
        .expect("shared-state child should be a valid component");

    assert!(
        plan.is_none(),
        "shared-state components must currently inline so provider scope is preserved"
    );
}

#[test]
fn component_callback_inline_keeps_client_only_payload_expr() {
    let mut analyzer = test_analyzer();
    analyzer.signals.insert("count".to_string(), SignalId(0));
    let handler = HandlerLocal {
        name: syn::parse_quote!(on_change),
        closure: syn::parse_quote!(|value| count.set(value)),
        kind: HandlerLocalKind::Payload,
        prelude_local_index: 0,
        source_anchor: rsjs_ir::SourceAnchor {
            start_line: 1,
            start_column: 1,
        },
        capture_env: None,
    };
    let payload = ClientExpr::Field {
        base: Box::new(ClientExpr::EventRead(EventValue::TargetValue)),
        name: "detail".to_string(),
    };

    let block = analyzer
        .inline_handler_local_client_block_with_payload(
            &handler,
            Some(payload.clone()),
            &HandlerScope::default(),
            proc_macro2::Span::call_site(),
        )
        .unwrap();

    assert_eq!(
        block.stmts,
        vec![ClientStmt::SignalSet {
            id: SignalId(0),
            value: payload,
        }]
    );
}

#[test]
fn handler_hir_lowers_directly_to_client_block() {
    let lowered =
        lower_handler_hir_to_client_lowering(handler_basic_event_signal_hir_fixture()).unwrap();

    assert_eq!(lowered.body_ir.stmts.len(), 2);
    let ClientStmt::Let {
        local, ty, value, ..
    } = &lowered.body_ir.stmts[0]
    else {
        panic!("expected first Client IR statement to be a let");
    };
    assert_eq!(*local, ClientLocalId(0));
    assert_eq!(*ty, ClientType::String);
    assert_eq!(*value, Some(ClientExpr::EventRead(EventValue::TargetValue)));

    let ClientStmt::SignalSet { id, value } = &lowered.body_ir.stmts[1] else {
        panic!("expected second Client IR statement to be a signal set");
    };
    assert_eq!(*id, SignalId(0));
    assert_eq!(*value, ClientExpr::Local(ClientLocalId(0)));
}

#[test]
fn handler_basic_event_signal_hir_matches_conformance_snapshot() {
    let statements = handler_basic_event_signal_hir_fixture();
    let actual = handler_hir_snapshot_json(&statements);
    assert_hir_shape_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/hir/handler_basic_event_signal.hir.json",
    );
}

#[test]
fn handler_basic_event_signal_client_ir_matches_conformance_snapshot() {
    let statements = handler_basic_event_signal_hir_fixture();
    let lowered = lower_handler_hir_to_client_lowering(statements).unwrap();
    let actual = client_block_snapshot_json(&lowered.body_ir);
    assert_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/client_ir/handler_basic_event_signal.body-ir.json",
    );
}

#[test]
fn handler_control_flow_hir_matches_conformance_snapshot() {
    let statements = handler_control_flow_hir_fixture();
    let actual = handler_hir_snapshot_json(&statements);
    assert_hir_shape_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/hir/handler_control_flow_if_else.hir.json",
    );
}

#[test]
fn handler_control_flow_client_ir_matches_conformance_snapshot() {
    let statements = handler_control_flow_hir_fixture();
    let lowered = lower_handler_hir_to_client_lowering(statements).unwrap();
    let actual = client_block_snapshot_json(&lowered.body_ir);
    assert_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/client_ir/handler_control_flow_if_else.body-ir.json",
    );
}

#[test]
fn dynamic_wide_range_hir_json_preserves_its_integer_domain() {
    let mut analyzer = test_analyzer();
    let current = analyzer.builder.signal(Value::BigInt(0));
    analyzer
        .builder
        .set_signal_integer_domain(current, Some(IntegerDomain::I64));
    analyzer.signals.insert("current".to_owned(), current);
    let closure: ExprClosure = syn::parse_quote!(|event| {
        for index in current.get()..3_i64 {
            current.set(index);
        }
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: Some("event"),
        handler_locals: Vec::new(),
        event_node: Some(NodeId(1)),
    };

    let statements = analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .expect("the dynamic i64 range must lower to handler HIR");
    let [statement] = statements.as_slice() else {
        panic!("expected one dynamic range statement");
    };
    let json = handler_hir_stmt_json(statement);

    assert_eq!(json["kind"], "for_range");
    assert_eq!(json["integer_domain"], "i64");
    assert_eq!(json["start"]["kind"], "signal_read");
    assert_eq!(json["end"]["kind"], "literal");
    assert_eq!(json["end"]["value"]["kind"], "bigint");
    assert_eq!(json["end"]["value"]["value"], 3);
}

#[test]
fn handler_named_event_refs_hir_matches_conformance_snapshot() {
    let statements = handler_named_event_refs_hir_fixture();
    let actual = handler_hir_snapshot_json(&statements);
    assert_hir_shape_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/hir/handler_named_event_refs.hir.json",
    );
}

#[test]
fn handler_named_event_refs_client_ir_matches_conformance_snapshot() {
    let statements = handler_named_event_refs_hir_fixture();
    let lowered = lower_handler_hir_to_client_lowering(statements).unwrap();
    let actual = client_block_snapshot_json(&lowered.body_ir);
    assert_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/client_ir/handler_named_event_refs.body-ir.json",
    );
}

#[test]
fn component_callback_hir_matches_conformance_snapshot() {
    let statements = component_callback_hir_fixture();
    let actual = handler_hir_snapshot_json(&statements);
    assert_hir_shape_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/hir/component_callback_basic.hir.json",
    );
}

#[test]
fn component_callback_client_ir_matches_conformance_snapshot() {
    let statements = component_callback_hir_fixture();
    let lowered = lower_handler_hir_to_client_lowering(statements).unwrap();
    let actual = client_block_snapshot_json(&lowered.body_ir);
    assert_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/client_ir/component_callback_basic.body-ir.json",
    );
}

#[test]
fn node_refs_hir_matches_conformance_snapshot() {
    let statements = node_refs_hir_fixture();
    let actual = handler_hir_snapshot_json(&statements);
    assert_hir_shape_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/hir/node_refs_p0.hir.json",
    );
}

#[test]
fn node_refs_client_ir_matches_conformance_snapshot() {
    let statements = node_refs_hir_fixture();
    let lowered = lower_handler_hir_to_client_lowering(statements).unwrap();
    let actual = client_block_snapshot_json(&lowered.body_ir);
    assert_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/client_ir/node_refs_p0.body-ir.json",
    );
}

#[test]
fn server_boundary_child_hir_matches_conformance_snapshot() {
    let statements = server_boundary_child_hir_fixture();
    let actual = handler_hir_snapshot_json(&statements);
    assert_hir_shape_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/hir/server_boundary_child.hir.json",
    );
}

#[test]
fn server_boundary_child_client_ir_matches_conformance_snapshot() {
    let statements = server_boundary_child_hir_fixture();
    let lowered = lower_handler_hir_to_client_lowering(statements).unwrap();
    let actual = client_block_snapshot_json(&lowered.body_ir);
    assert_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/client_ir/server_boundary_child.body-ir.json",
    );
}

#[test]
fn template_repetition_hir_matches_conformance_snapshot() {
    let statements = template_repetition_hir_fixture();
    let actual = handler_hir_snapshot_json(&statements);
    assert_hir_shape_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/hir/template_repetition_basic.hir.json",
    );
}

#[test]
fn template_repetition_client_ir_matches_conformance_snapshot() {
    let statements = template_repetition_hir_fixture();
    let lowered = lower_handler_hir_to_client_lowering(statements).unwrap();
    let actual = client_block_snapshot_json(&lowered.body_ir);
    assert_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/client_ir/template_repetition_basic.body-ir.json",
    );
}

#[test]
fn browser_effects_hir_matches_conformance_snapshot() {
    let statements = browser_effects_hir_fixture();
    let actual = handler_hir_snapshot_json(&statements);
    assert_hir_shape_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/hir/browser_effects_p0.hir.json",
    );
}

#[test]
fn browser_effects_client_ir_matches_conformance_snapshot() {
    let statements = browser_effects_hir_fixture();
    let lowered = lower_handler_hir_to_client_lowering(statements).unwrap();
    let actual = client_block_snapshot_json(&lowered.body_ir);
    assert_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/client_ir/browser_effects_p0.body-ir.json",
    );
}

#[test]
fn query_mutation_handler_hir_matches_conformance_snapshot() {
    let statements = query_mutation_handler_hir_fixture();
    let actual = handler_hir_snapshot_json(&statements);
    assert_hir_shape_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/hir/query_mutation_authoring.hir.json",
    );
}

#[test]
fn query_mutation_handler_client_ir_matches_conformance_snapshot() {
    let statements = query_mutation_handler_hir_fixture();
    let lowered = lower_handler_hir_to_client_lowering(statements).unwrap();
    let actual = client_block_snapshot_json(&lowered.body_ir);
    assert_conformance_snapshot(
        actual,
        "rsjs_conformance/snapshots/client_ir/query_mutation_authoring.body-ir.json",
    );
}

fn handler_basic_event_signal_hir_fixture() -> Vec<HandlerHirStmt> {
    vec![
        HandlerHirStmt::new(
            proc_macro2::Span::call_site(),
            HandlerHirStmtKind::Let {
                local: LocalId(0),
                name: "next".to_string(),
                ty: ClientType::String,
                value: HandlerHirExpr::new(
                    proc_macro2::Span::call_site(),
                    FrontendExpr::EventValue(EventValue::TargetValue),
                ),
            },
        ),
        HandlerHirStmt::new(
            proc_macro2::Span::call_site(),
            HandlerHirStmtKind::SignalSet {
                id: SignalId(0),
                value: HandlerHirExpr::new(
                    proc_macro2::Span::call_site(),
                    FrontendExpr::Local(LocalId(0)),
                ),
            },
        ),
    ]
}

fn handler_control_flow_hir_fixture() -> Vec<HandlerHirStmt> {
    let mut analyzer = test_analyzer();
    analyzer.signals.insert("selected".to_string(), SignalId(0));
    let closure: ExprClosure = syn::parse_quote!(|event| {
        if event.shift_key() {
            if event.ctrl_key() {
                selected.set("both");
            } else {
                selected.set("shift");
            }
        } else {
            selected.set("plain");
        }
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: Some("event"),
        handler_locals: Vec::new(),
        event_node: Some(NodeId(1)),
    };

    analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .unwrap()
}

fn handler_named_event_refs_hir_fixture() -> Vec<HandlerHirStmt> {
    let mut analyzer = test_analyzer();
    analyzer.signals.insert("picked".to_string(), SignalId(0));
    analyzer.signals.insert("logged".to_string(), SignalId(1));
    analyzer.handlers.insert(
        "on_pick".to_string(),
        HandlerLocal {
            name: syn::Ident::new("on_pick", proc_macro2::Span::call_site()),
            closure: syn::parse_quote!(|event: rsjs::Event, label: &'static str| {
                console::log(label);
                logged.set(label);
                picked.set(event.closest_attr("data-date"));
            }),
            kind: HandlerLocalKind::Event,
            prelude_local_index: 0,
            source_anchor: rsjs_ir::SourceAnchor {
                start_line: 1,
                start_column: 1,
            },
            capture_env: None,
        },
    );
    let closure: ExprClosure = syn::parse_quote!(|event| {
        on_pick("today");
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: Some("event"),
        handler_locals: Vec::new(),
        event_node: Some(NodeId(1)),
    };

    analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .unwrap()
}

fn component_callback_hir_fixture() -> Vec<HandlerHirStmt> {
    let mut analyzer = test_analyzer();
    let closure: ExprClosure = syn::parse_quote!(|event| {
        let next = event.value();
        console::log(next);
        self.on_change.call(next);
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: Some("event"),
        handler_locals: Vec::new(),
        event_node: Some(NodeId(1)),
    };

    analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .unwrap()
}

fn node_refs_hir_fixture() -> Vec<HandlerHirStmt> {
    let mut analyzer = test_analyzer();
    analyzer.signals.insert("current".to_string(), SignalId(0));
    analyzer.node_refs.insert("input".to_string(), NodeRefId(0));
    analyzer.node_refs.insert("form".to_string(), NodeRefId(1));
    analyzer
        .node_refs
        .insert("dialog".to_string(), NodeRefId(2));
    analyzer
        .node_refs
        .insert("popover".to_string(), NodeRefId(3));
    let closure: ExprClosure = syn::parse_quote!(|_| {
        input.focus();
        input.click();
        form.request_submit();
        form.reset();
        dialog.show_modal();
        dialog.close();
        popover.show_popover();
        popover.hide_popover();
        popover.toggle_popover();
        input.scroll_to(0_i32, 32_i32);
        input.scroll_by(0_i32, 8_i32);
        input.select_all();
        input.set_value("");
        input.set_selection_range(0_u32, 0_u32);
        input.set_attribute("aria-expanded", "false");
        input.remove_attribute("hidden");
        input.set_style_property("outline-color", "transparent");
        console::log(
            input.get_attribute("data-kind"),
            input.has_attribute("hidden"),
        );
        current.set(input.value());
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: None,
        handler_locals: Vec::new(),
        event_node: Some(NodeId(7)),
    };

    analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .unwrap()
}

fn server_boundary_child_hir_fixture() -> Vec<HandlerHirStmt> {
    let mut analyzer = test_analyzer();
    let closure: ExprClosure = syn::parse_quote!(|_| {
        self.on_press.call(());
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: None,
        handler_locals: Vec::new(),
        event_node: Some(NodeId(0)),
    };

    analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .unwrap()
}

fn template_repetition_hir_fixture() -> Vec<HandlerHirStmt> {
    let mut analyzer = test_analyzer();
    analyzer.loop_scopes.push(LoopScope {
        locals: HashSet::from(["label".to_string()]),
        integer_domains: HashMap::new(),
    });
    analyzer.payload_counter = 1;
    let event_node = analyzer.builder.element("button", None);
    let closure: ExprClosure = syn::parse_quote!(|_| {
        let selected = label.to_string();
        console::log(selected);
        self.on_select.call(selected);
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: None,
        handler_locals: Vec::new(),
        event_node: Some(event_node),
    };

    analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .unwrap()
}

fn browser_effects_hir_fixture() -> Vec<HandlerHirStmt> {
    let mut analyzer = test_analyzer();
    analyzer.signals.insert("hash".to_string(), SignalId(0));
    analyzer
        .signals
        .insert("credential_options".to_string(), SignalId(1));
    analyzer
        .signals
        .insert("credential_json".to_string(), SignalId(2));
    let closure: ExprClosure = syn::parse_quote!(|_| {
        hash.set(rsjs::local_storage::get_item_or(
            String::new(),
            String::new(),
        ));
        console::log(rsjs::session_storage::get_item(String::new()));
        console::log(
            rsjs::local_storage::get_bool_or("enabled", false),
            rsjs::local_storage::get_i64_or("count", 0_i64),
            rsjs::session_storage::get_f64_or("ratio", 0.0_f64),
        );
        rsjs::window::scroll_to(0_i32, 100_i32);
        rsjs::window::scroll_by(0_i32, 20_i32);
        rsjs::window::print();
        rsjs::location::set_hash(hash.get());
        rsjs::location::assign("/catalog/filters");
        rsjs::location::replace(hash.get());
        rsjs::location::reload();
        rsjs::history::push_state("/catalog#filters");
        rsjs::history::replace_state(rsjs::location::pathname());
        rsjs::history::back();
        rsjs::history::forward();
        rsjs::history::go(1_i32);
        rsjs::document::dispatch_event("filters:changed");
        rsjs::document::dispatch_event_with_detail("filters:changed", hash.get());
        rsjs::document::set_title(hash.get());
        rsjs::clipboard::write_text(hash.get());
        rsjs::webauthn::create_json_into(credential_options.get(), &credential_json);
        rsjs::webauthn::get_json_into_or(credential_options.get(), &credential_json, "unavailable");
        rsjs::local_storage::set_item("filters", hash.get());
        rsjs::local_storage::remove_item("filters:stale");
        rsjs::session_storage::set_item("filters", hash.get());
        rsjs::session_storage::remove_item("filters:stale");
        rsjs::url::set_search_param("filters", hash.get());
        rsjs::url::delete_search_param("stale");
        rsjs::url::push_search_param("page", "2");
        rsjs::url::push_delete_search_param("modal");
        console::log(rsjs::encoding::base64url_encode(
            rsjs::encoding::utf8_bytes(hash.get()),
        ));
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: None,
        handler_locals: Vec::new(),
        event_node: Some(NodeId(1)),
    };

    analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .unwrap()
}

#[test]
fn storage_i64_read_coerces_an_unsuffixed_fallback_to_bigint() {
    let mut analyzer = test_analyzer();
    let expr: Expr = syn::parse_quote! { rsjs::local_storage::get_i64_or("count", 0) };
    let lowered = analyzer
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect("an unsuffixed literal should inherit the storage i64 domain");
    let FrontendExpr::BrowserRead(BrowserRead::Storage {
        value_type: rsjs_ir::StorageValueType::I64,
        fallback: Some(fallback),
        ..
    }) = lowered
    else {
        panic!("expected an i64 storage read");
    };
    assert_eq!(*fallback, FrontendExpr::Literal(Value::BigInt(0)));
}

#[test]
fn storage_i64_read_rejects_a_number_backed_fallback() {
    let mut analyzer = test_analyzer();
    let expr: Expr = syn::parse_quote! { rsjs::local_storage::get_i64_or("count", 0_i32) };
    let error = analyzer
        .lower_reactive_expr_in(&expr, &ExprScope::default())
        .expect_err("i64 storage fallback must not retain a Number representation");
    assert!(
        error.to_string().contains("integer-domain-mismatch")
            && error.to_string().contains("i32")
            && error.to_string().contains("i64"),
        "unexpected diagnostic: {error}"
    );
}

fn query_mutation_handler_hir_fixture() -> Vec<HandlerHirStmt> {
    let mut analyzer = test_analyzer();
    analyzer.signals.insert("draft".to_string(), SignalId(0));
    analyzer.mutations.insert("save".to_string(), MutationId(0));
    let closure: ExprClosure = syn::parse_quote!(|_| {
        save.run(draft.get());
    });
    let scope = HandlerScope {
        handler_param: None,
        event_param: None,
        handler_locals: Vec::new(),
        event_node: Some(NodeId(3)),
    };

    analyzer
        .lower_handler_closure_hir_body(&closure, &scope)
        .unwrap()
}

fn assert_conformance_snapshot(actual: serde_json::Value, relative_path: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative_path);
    if std::env::var_os("RSJS_UPDATE_SNAPSHOTS").is_some() {
        let json = format!(
            "{}\n",
            serde_json::to_string_pretty(&actual).expect("conformance snapshot should serialize")
        );
        std::fs::write(&path, json).expect("conformance snapshot should be writable");
        return;
    }

    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let expected: serde_json::Value =
        serde_json::from_str(&expected).expect("conformance snapshot should be valid JSON");
    assert_eq!(actual, expected);
}

fn assert_hir_shape_conformance_snapshot(mut actual: serde_json::Value, relative_path: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative_path);
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let mut expected: serde_json::Value =
        serde_json::from_str(&expected).expect("conformance snapshot should be valid JSON");

    remove_hir_span_fields(&mut actual);
    remove_hir_span_fields(&mut expected);

    assert_eq!(actual, expected);
}

fn remove_hir_span_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                remove_hir_span_fields(item);
            }
        }
        serde_json::Value::Object(fields) => {
            fields.remove("span");
            for field in fields.values_mut() {
                remove_hir_span_fields(field);
            }
        }
        _ => {}
    }
}

fn handler_hir_snapshot_json(statements: &[HandlerHirStmt]) -> serde_json::Value {
    super::handler_hir_json(statements)
}

fn client_block_snapshot_json(block: &ClientBlock) -> serde_json::Value {
    serde_json::from_str(&client_block_json(block)).expect("Client IR JSON should parse")
}
