// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Component inlining: descriptor props, fragment mounts, callback slots, and id remapping into the parent island.
use super::*;

impl Analyzer {
    pub(super) fn inline_component_struct(
        &mut self,
        expr: &ExprStruct,
        parent: NodeId,
        edge_id: u32,
    ) -> syn::Result<()> {
        let descriptor = match crate::component_descriptor::find_full_by_component_path(&expr.path)
        {
            crate::component_descriptor::ComponentDescriptorFullLookup::Found(descriptor) => {
                descriptor
            }
            crate::component_descriptor::ComponentDescriptorFullLookup::Missing => {
                // Descriptor discovery is still token-based during this
                // transitional compiler slice, so a normal imported alias or
                // re-export can be invisible here even though rustc knows its
                // exact type. Preserve the direct child as SSR when no live
                // parent value crosses the edge; the relocation record emitted
                // beside the parent then requires the Rust-resolved type to
                // implement ComponentAbi. Successful descriptor lookup keeps
                // using the existing inlining path below unchanged.
                if self
                    .reject_reactive_captures_in_opaque_boundary(
                        &Expr::Struct(expr.clone()),
                        OpaqueBoundary::ServerOnly,
                    )
                    .is_err()
                {
                    return Err(self.struct_literal_reactive_error(expr));
                }
                let node = self.record_server_expression(
                    parent,
                    &Expr::Struct(expr.clone()),
                    rsjs_ir::ServerSurfaceSource::DirectComponentSite,
                    Some(server_surface_anchor(expr.span())),
                );
                self.record_direct_component_site(edge_id, node);
                return Ok(());
            }
            crate::component_descriptor::ComponentDescriptorFullLookup::Ambiguous(matches) => {
                let ids = matches
                    .iter()
                    .map(|summary| summary.asset_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(syn::Error::new(
                    expr.path.span(),
                    format!(
                        "found multiple rsjs component descriptors for `{}` ({ids}); use a more \
                         specific component path",
                        expr.path.to_token_stream()
                    ),
                ));
            }
            crate::component_descriptor::ComponentDescriptorFullLookup::Invalid {
                path,
                reason,
            } => {
                return Err(syn::Error::new(
                    expr.path.span(),
                    format!(
                        "found an invalid rsjs component descriptor at `{}`: {reason}",
                        path.display()
                    ),
                ));
            }
        };

        if !descriptor.island.lists.is_empty() {
            return Err(syn::Error::new(
                expr.path.span(),
                format!(
                    "rsjs component `{}` declares a keyed list, and keyed lists inside reusable \
                     child components are not supported in v0.1: inlining would have to remap the \
                     list's signal, container, and item template across islands. Keep the keyed \
                     list in the parent island, or render this component outside the loop",
                    descriptor.summary.self_ty
                ),
            ));
        }
        if (descriptor.summary.is_async || descriptor.island.is_async) && !self.is_async {
            return Err(syn::Error::new(
                expr.path.span(),
                "async rsjs child components require an async parent `WebRender`; switch the \
                 parent from `WebRenderSync` to `WebRender` and use `html!` in the render body",
            ));
        }
        let inline_props = self.resolve_component_inline_props(expr, &descriptor.summary)?;
        let mut maps = ComponentInlineMaps::default();
        for slot in &descriptor.island.callback_slots {
            let Some(handler) = inline_props.callbacks.get(&slot.name) else {
                return Err(syn::Error::new(
                    expr.path.span(),
                    format!(
                        "missing rsjs handler prop `{}` for component `{}`",
                        slot.name, descriptor.summary.self_ty
                    ),
                ));
            };
            maps.callback_handlers.insert(slot.id, handler.clone());
        }
        for prop in &descriptor.prop_fields {
            let Some(value) = inline_props.props.get(&prop.name) else {
                return Err(syn::Error::new(
                    expr.path.span(),
                    format!(
                        "missing rsjs prop `{}` for component `{}`",
                        prop.name, descriptor.summary.self_ty
                    ),
                ));
            };
            maps.prop_signals.insert(prop.signal, value.expr.clone());
            maps.prop_signal_source_anchors
                .insert(prop.signal, value.source_anchor);
        }
        maps.server_props = inline_props.server_props;

        let fragment_mount_plan =
            self.component_fragment_mount_plan(&descriptor, &maps, expr.path.span())?;
        if fragment_mount_plan.is_none() {
            for import in &descriptor.island.client_module_imports {
                if !self.client_module_imports.contains(import) {
                    self.client_module_imports.push(import.clone());
                    self.builder.add_client_module_import(import.clone());
                }
            }
        }

        // Shared stores are provider-scope dependencies, not child-owned
        // values. Full component inlining remaps each child acquisition into
        // the parent island while preserving the Rust type for SSR lookup.
        for store in &descriptor.island.shared_stores {
            let ty = syn::parse_str::<syn::Type>(&store.rust_type).map_err(|error| {
                syn::Error::new(
                    expr.path.span(),
                    format!(
                        "rsjs/shared-state-descriptor-type: component `{}` records invalid shared-state type `{}`: {error}",
                        descriptor.summary.self_ty, store.rust_type
                    ),
                )
            })?;
            // The child recorded the type *name* it wrote. Emitting that name
            // into this module rebinds it to whatever it means here, so carry
            // a const proof obligation comparing the identity the name stands
            // for here against the identity it stood for in the child. Both
            // sides are trait-resolved by rustc in their own crates, so the
            // check consults no registry and cannot be stale, ambiguous, or
            // order-dependent. See `rsjs::IslandSharedStores`.
            let component_path = &expr.path;
            let store_index = store.id.0 as usize;
            let expected_type_key = quote::quote! {
                <#component_path as ::rsjs::IslandSharedStores>::SHARED_TYPE_KEYS[#store_index]
            };
            let source_name = quote::format_ident!(
                "__rsjs_component_shared_{}",
                self.shared_state_initializers.len()
            );
            let mapped = self.builder.shared_store(
                source_name.to_string(),
                store.rust_type.clone(),
                store.source_anchor,
            );
            self.shared_state_initializers.push(SharedStateInitializer {
                id: mapped,
                source_name,
                ty,
                expected_type_key: Some(expected_type_key),
            });
            maps.shared_stores.insert(store.id, mapped);
        }

        for signal in &descriptor.island.signals {
            if maps.prop_signals.contains_key(&signal.id) {
                continue;
            }
            let mapped = if let Some(shared) = &signal.shared {
                let store = maps.shared_stores.get(&shared.store).copied().ok_or_else(|| {
                    syn::Error::new(
                        expr.path.span(),
                        "rsjs/shared-state-descriptor-store: shared field references a missing store declaration",
                    )
                })?;
                self.builder
                    .shared_field(store, shared.field.clone(), signal.ty.clone())
            } else {
                self.builder
                    .signal_typed(signal.init.clone(), signal.ty.clone())
            };
            self.builder
                .set_signal_source(mapped, signal.source_name.clone(), None);
            self.builder
                .set_signal_integer_domain(mapped, signal.integer_domain);
            maps.signals.insert(signal.id, mapped);
        }
        for node_ref in &descriptor.island.node_refs {
            let mapped = self.builder.node_ref();
            self.builder
                .set_node_ref_source(mapped, node_ref.source_name.clone(), None);
            maps.node_refs.insert(node_ref.id, mapped);
        }

        self.copy_component_queries(&descriptor, &mut maps, expr.path.span())?;
        self.copy_component_query_initializers(&descriptor, &maps, expr.path.span())?;
        self.copy_component_mutations(&descriptor, &mut maps, expr.path.span())?;
        self.copy_component_mutation_initializers(&descriptor, &maps, expr.path.span())?;

        for idx in 0..descriptor.island.exprs.len() {
            let old = ExprId(idx as u32);
            self.copy_component_arena_expr(&descriptor.island, old, &mut maps)?;
        }

        // Child memo cells: the signal copy above ran before the
        // arena copy, so the remapped derived-body id only became
        // available now. Patch it in and record the parent-side id as
        // read-only so parent handlers cannot write a child derived cell.
        for signal in &descriptor.island.signals {
            let Some(body) = signal.derived else {
                continue;
            };
            let Some(mapped_signal) = maps.signals.get(&signal.id).copied() else {
                continue;
            };
            let mapped_body =
                self.copy_component_arena_expr(&descriptor.island, body, &mut maps)?;
            self.builder.set_signal_derived(mapped_signal, mapped_body);
            self.memo_cells.insert(mapped_signal);
        }

        for handler in &descriptor.island.handlers {
            let mut handler_maps = maps.clone();
            handler_maps.locals.clear();
            let body_ir =
                self.remap_component_client_block(handler.body_ir(), &mut handler_maps)?;
            let mapped = match handler.debounce_ms() {
                Some(ms) => self.builder.debounce_handler_from_ir(ms, body_ir),
                None => self.builder.handler_from_ir(body_ir),
            };
            self.builder
                .set_handler_source(mapped, handler.source_name.clone(), None);
            maps.handlers.insert(handler.id, mapped);
        }

        // Child interval timers. Only copied for full inlining: when
        // the child mounts as a shared fragment, the fragment function
        // (emitted from the child's own island IR) already runs its
        // intervals, and the parent module must not double-start them.
        if fragment_mount_plan.is_none() {
            for interval in &descriptor.island.intervals {
                let mut interval_maps = maps.clone();
                interval_maps.locals.clear();
                let body_ir =
                    self.remap_component_client_block(interval.body_ir(), &mut interval_maps)?;
                self.builder.interval(interval.ms, body_ir);
            }
        }

        let root = descriptor.island.view_tree.root().ok_or_else(|| {
            syn::Error::new(
                expr.path.span(),
                format!(
                    "rsjs component descriptor `{}` has no root node",
                    descriptor.summary.asset_id
                ),
            )
        })?;
        let mapped_root = self.copy_component_node(
            &descriptor.island,
            root,
            parent,
            &mut maps,
            expr.path.span(),
        )?;
        self.copy_component_bindings(&descriptor.island, &maps, expr.path.span())?;

        if let Some(plan) = fragment_mount_plan {
            self.builder
                .static_attr(mapped_root, MARKER_COMPONENT, &descriptor.summary.asset_id);
            self.builder.static_attr(
                mapped_root,
                MARKER_COMPONENT_MOUNT,
                &mapped_root.0.to_string(),
            );
            let mut owned_nodes: Vec<NodeId> = maps.nodes.values().copied().collect();
            owned_nodes.sort_by_key(|node| node.0);
            let mut node_aliases: Vec<ComponentFragmentNodeAlias> = maps
                .nodes
                .iter()
                .map(|(child, parent)| ComponentFragmentNodeAlias {
                    child: *child,
                    parent: *parent,
                })
                .collect();
            node_aliases.sort_by_key(|alias| alias.parent.0);
            let mut owned_signals: Vec<SignalId> = maps.signals.values().copied().collect();
            owned_signals.sort_by_key(|signal| signal.0);
            let mut owned_handlers: Vec<EventHandlerId> = maps.handlers.values().copied().collect();
            owned_handlers.sort_by_key(|handler| handler.0);
            let mut owned_node_refs: Vec<NodeRefId> = maps.node_refs.values().copied().collect();
            owned_node_refs.sort_by_key(|node_ref| node_ref.0);
            let ComponentFragmentMountPlan {
                callbacks,
                callback_hirs,
                signal_aliases,
            } = plan;
            self.builder.component_fragment_mount(
                mapped_root,
                descriptor.summary.asset_id.clone(),
                callbacks,
                signal_aliases,
                node_aliases,
                owned_nodes,
                owned_signals,
                owned_handlers,
                owned_node_refs,
            );
            self.hir_sidecar.push_component_fragment_mount(
                mapped_root,
                descriptor.summary.asset_id,
                callback_hirs,
            );
        }

        Ok(())
    }

    pub(super) fn resolve_component_inline_props(
        &mut self,
        expr: &ExprStruct,
        summary: &crate::component_descriptor::ComponentDescriptorSummary,
    ) -> syn::Result<ComponentInlineProps> {
        let handler_fields = summary
            .handler_fields
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let prop_fields = summary
            .prop_fields
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let server_prop_fields = summary
            .server_prop_fields
            .iter()
            .chain(summary.static_slot_fields.iter())
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let mut callbacks = HashMap::new();
        let mut props = HashMap::new();
        let mut server_props = HashMap::new();

        for field in &expr.fields {
            let syn::Member::Named(name) = &field.member else {
                return Err(syn::Error::new(
                    field.member.span(),
                    "rsjs component inlining only supports named struct fields",
                ));
            };
            let name = name.to_string();
            if handler_fields.contains(name.as_str()) {
                let handler = self.resolve_component_handler_value(&field.expr, &name)?;
                callbacks.insert(name, handler);
                continue;
            }
            if prop_fields.contains(name.as_str()) {
                let value = self.lower_reactive_expr(&field.expr)?;
                props.insert(
                    name.clone(),
                    ComponentInlineProp {
                        expr: value,
                        source_anchor: component_source_anchor(field.expr.span()),
                    },
                );
            }
            if server_prop_fields.contains(name.as_str()) {
                self.reject_reactive_captures_in_opaque_boundary(
                    &field.expr,
                    OpaqueBoundary::ServerOnly,
                )?;
                server_props.insert(name.clone(), field.expr.clone());
                continue;
            }
            if prop_fields.contains(name.as_str()) {
                continue;
            } else {
                return Err(syn::Error::new(
                    field.span(),
                    format!(
                        "`{}` is not an rsjs prop in descriptor `{}`. Component inlining currently \
                         supports handler props, signal-initializer props, and server props \
                         declared by the child descriptor. Render simple server values from the \
                         child as `self.field` / `self.field.path`, or use `rsjs::server_only(...)` \
                         / `rsjs::server_attr(...)` for opaque server expressions.",
                        name, summary.asset_id
                    ),
                ));
            }
        }

        for name in &summary.handler_fields {
            if !callbacks.contains_key(name) {
                return Err(syn::Error::new(
                    expr.path.span(),
                    format!(
                        "missing rsjs handler prop `{name}` for component `{}`",
                        summary.self_ty
                    ),
                ));
            }
        }
        for name in &summary.prop_fields {
            if !props.contains_key(name) {
                return Err(syn::Error::new(
                    expr.path.span(),
                    format!(
                        "missing rsjs prop `{name}` for component `{}`",
                        summary.self_ty
                    ),
                ));
            }
        }
        for name in &summary.server_prop_fields {
            if !server_props.contains_key(name) {
                return Err(syn::Error::new(
                    expr.path.span(),
                    format!(
                        "missing server-only rsjs prop `{name}` for component `{}`",
                        summary.self_ty
                    ),
                ));
            }
        }
        for name in &summary.static_slot_fields {
            if !server_props.contains_key(name) {
                return Err(syn::Error::new(
                    expr.path.span(),
                    format!(
                        "missing immutable static rsjs slot `{name}` for component `{}`",
                        summary.self_ty
                    ),
                ));
            }
        }

        Ok(ComponentInlineProps {
            callbacks,
            props,
            server_props,
        })
    }

    pub(super) fn component_fragment_mount_plan(
        &mut self,
        descriptor: &crate::component_descriptor::ComponentDescriptor,
        maps: &ComponentInlineMaps,
        span: proc_macro2::Span,
    ) -> syn::Result<Option<ComponentFragmentMountPlan>> {
        if rsjs_ir::classify_render_strategy(&descriptor.island).is_server_only() {
            // Server-only children have no runtime fragment asset or
            // manifest registration in v2. Full inlining preserves the
            // SSR nodes and keeps the parent from importing missing JS.
            return Ok(None);
        }

        if !descriptor.island.queries.is_empty() || !descriptor.island.mutations.is_empty() {
            // Query and mutation declarations need to be remapped into the
            // parent island so parent-owned handlers, bindings, and metadata
            // all agree on one set of ids.
            return Ok(None);
        }

        if !descriptor.island.shared_stores.is_empty() {
            // Fragment mounts currently address parent signal aliases but do
            // not yet carry a child-store-slot -> parent-store-slot map. Full
            // inlining preserves exact provider scope semantics meanwhile.
            return Ok(None);
        }

        let prop_signal_ids = descriptor
            .prop_fields
            .iter()
            .map(|prop| prop.signal)
            .collect::<HashSet<_>>();
        for signal in &descriptor.island.signals {
            if !prop_signal_ids.contains(&signal.id) && signal.init == Value::Null {
                // A non-prop dynamic signal initializer falls back to Null in
                // descriptor JSON. Keep the old full-inline path until the
                // descriptor records enough initializer metadata for a fragment
                // mount to seed it faithfully.
                return Ok(None);
            }
        }

        let mut signal_aliases = Vec::new();
        for prop in &descriptor.prop_fields {
            let Some(expr) = maps.prop_signals.get(&prop.signal) else {
                return Ok(None);
            };
            let FrontendExpr::SignalRead(parent) = expr else {
                // Literal/derived prop expressions still need the old inline
                // substitution path because a fragment can currently alias only
                // concrete parent signal cells.
                return Ok(None);
            };
            signal_aliases.push(ComponentFragmentSignalAlias {
                child: prop.signal,
                parent: *parent,
                prop_name: Some(prop.name.clone()),
                source_anchor: maps.prop_signal_source_anchors.get(&prop.signal).copied(),
            });
        }
        signal_aliases.sort_by_key(|alias| alias.child.0);

        let mut callbacks = Vec::new();
        let mut callback_hirs = Vec::new();
        for slot in &descriptor.island.callback_slots {
            let Some(handler) = maps.callback_handlers.get(&slot.id) else {
                return Ok(None);
            };
            let Some(handler) = handler else {
                continue;
            };
            let statements = self.inline_handler_local_hir_with_payload(
                handler,
                Some(FrontendExpr::CallbackPayload),
                &HandlerScope::default(),
                span,
            )?;
            let lowered = lower_handler_hir_to_client_lowering(statements)?;
            callback_hirs.push(ComponentFragmentCallbackHirArtifact {
                slot: slot.id,
                body: lowered.body_hir,
            });
            callbacks.push(ComponentFragmentCallback::from_ir(slot.id, lowered.body_ir));
        }
        callbacks.sort_by_key(|callback| callback.slot.0);
        callback_hirs.sort_by_key(|callback| callback.slot.0);

        // Descriptor schema v4 has only parent-mounted component fragments:
        // the child asset exports a fragment mount function, not an
        // independent island boundary. Even when callbacks and signal aliases
        // are empty, parent JS is still responsible for importing and invoking
        // the fragment. Descriptor v5 should split this into an explicit
        // composition plan so self-contained children in server-only parents
        // can emit their own wrapper, seed blobs, and `cx.rsjs_island_used`.
        Ok(Some(ComponentFragmentMountPlan {
            callbacks,
            callback_hirs,
            signal_aliases,
        }))
    }

    pub(super) fn resolve_component_handler_value(
        &self,
        value: &Expr,
        field_name: &str,
    ) -> syn::Result<Option<HandlerLocal>> {
        if is_none_expr(value) {
            return Ok(None);
        }
        if let Expr::Call(call) = value {
            if is_rsjs_noop_handler_call(&call.func) {
                if !call.args.is_empty() {
                    return Err(syn::Error::new(
                        call.span(),
                        "`rsjs::noop_handler()` does not take runtime arguments",
                    ));
                }
                return Ok(None);
            }
            if is_some_call(&call.func) {
                if call.args.len() != 1 {
                    return Err(syn::Error::new(
                        call.span(),
                        "`Some(...)` handler props must wrap exactly one rsjs handler value",
                    ));
                }
                return self.resolve_component_handler_value(&call.args[0], field_name);
            }
        }
        let Expr::Path(path) = value else {
            return Err(syn::Error::new(
                value.span(),
                format!(
                    "rsjs handler prop `{field_name}` must be a local `rsjs::handler(...)` value \
                     or optional handler value (`Some(handler)`, `None`, or `rsjs::noop_handler()`)"
                ),
            ));
        };
        let Some(ident) = path.path.get_ident() else {
            return Err(syn::Error::new(
                path.path.span(),
                format!(
                    "rsjs handler prop `{field_name}` must be passed as a bare local handler name"
                ),
            ));
        };
        let Some(handler) = self.handlers.get(&ident.to_string()).cloned() else {
            return Err(syn::Error::new(
                ident.span(),
                format!(
                    "`{ident}` is not a local `rsjs::handler(...)` value for handler prop \
                     `{field_name}`"
                ),
            ));
        };
        if handler.kind != HandlerLocalKind::Payload {
            return Err(syn::Error::new(
                ident.span(),
                format!(
                    "`{ident}` is an `rsjs::event_handler(...)` value; handler prop `{field_name}` \
                     must receive a local `rsjs::handler(...)` value"
                ),
            ));
        }
        Ok(Some(handler))
    }

    pub(super) fn copy_component_arena_expr(
        &mut self,
        island: &Island,
        old: ExprId,
        maps: &mut ComponentInlineMaps,
    ) -> syn::Result<ExprId> {
        if let Some(mapped) = maps.exprs.get(&old) {
            return Ok(*mapped);
        }
        let client = self.remap_component_client_expr(island.exprs.client_expr(old), maps)?;
        let new = self.builder.expr(client);
        maps.exprs.insert(old, new);
        Ok(new)
    }

    pub(super) fn copy_component_queries(
        &mut self,
        descriptor: &crate::component_descriptor::ComponentDescriptor,
        maps: &mut ComponentInlineMaps,
        span: proc_macro2::Span,
    ) -> syn::Result<()> {
        let island = &descriptor.island;
        for query in &island.queries {
            reject_runtime_initialized_child_query(
                island,
                query,
                descriptor
                    .query_initializers
                    .iter()
                    .any(|init| init.id == query.id),
                span,
            )?;
            let key_expr = self.copy_component_arena_expr(island, query.key_expr, maps)?;
            let server_initial =
                self.copy_component_arena_expr(island, query.server_initial, maps)?;
            let client_fetcher =
                self.copy_component_arena_expr(island, query.client_fetcher, maps)?;
            let mapped = self.builder.query_typed(
                key_expr,
                server_initial,
                client_fetcher,
                query.data_ty.clone(),
                query.options.clone(),
            );
            self.builder
                .set_query_source(mapped, query.source_name.clone(), None);
            maps.queries.insert(query.id, mapped);
        }
        Ok(())
    }

    pub(super) fn copy_component_query_initializers(
        &mut self,
        descriptor: &crate::component_descriptor::ComponentDescriptor,
        maps: &ComponentInlineMaps,
        span: proc_macro2::Span,
    ) -> syn::Result<()> {
        for init in &descriptor.query_initializers {
            self.query_initializers.push(QueryInitializer {
                id: mapped_query(init.id, maps)?,
                expr: rewrite_component_server_expr(&init.expr, maps, span)?,
                key_expr: init
                    .key_expr
                    .as_ref()
                    .map(|key_expr| rewrite_component_server_expr(key_expr, maps, span))
                    .transpose()?,
            });
        }
        Ok(())
    }

    pub(super) fn copy_component_mutations(
        &mut self,
        descriptor: &crate::component_descriptor::ComponentDescriptor,
        maps: &mut ComponentInlineMaps,
        span: proc_macro2::Span,
    ) -> syn::Result<()> {
        let island = &descriptor.island;
        for mutation in &island.mutations {
            reject_runtime_initialized_child_mutation(
                island,
                mutation,
                descriptor
                    .mutation_initializers
                    .iter()
                    .any(|init| init.id == mutation.id),
                span,
            )?;
            let key_expr = self.copy_component_arena_expr(island, mutation.key_expr, maps)?;
            let server_runner =
                self.copy_component_arena_expr(island, mutation.server_runner, maps)?;
            let client_runner =
                self.copy_component_arena_expr(island, mutation.client_runner, maps)?;
            let invalidates = mutation
                .invalidates
                .iter()
                .map(|expr| self.copy_component_arena_expr(island, *expr, maps))
                .collect::<syn::Result<Vec<_>>>()?;
            let mapped = self.builder.mutation(
                key_expr,
                server_runner,
                client_runner,
                invalidates,
                mutation.options.clone(),
            );
            self.builder
                .set_mutation_source(mapped, mutation.source_name.clone(), None);
            maps.mutations.insert(mutation.id, mapped);
        }
        Ok(())
    }

    pub(super) fn copy_component_mutation_initializers(
        &mut self,
        descriptor: &crate::component_descriptor::ComponentDescriptor,
        maps: &ComponentInlineMaps,
        span: proc_macro2::Span,
    ) -> syn::Result<()> {
        for init in &descriptor.mutation_initializers {
            self.mutation_initializers.push(MutationInitializer {
                id: mapped_mutation(init.id, maps)?,
                key_expr: rewrite_component_server_expr(&init.key_expr, maps, span)?,
                invalidates_expr: init
                    .invalidates_expr
                    .as_ref()
                    .map(|invalidates_expr| {
                        rewrite_component_server_expr(invalidates_expr, maps, span)
                    })
                    .transpose()?,
            });
        }
        Ok(())
    }

    pub(super) fn remap_component_client_block(
        &mut self,
        block: &ClientBlock,
        maps: &mut ComponentInlineMaps,
    ) -> syn::Result<ClientBlock> {
        let mut stmts = Vec::new();
        for stmt in &block.stmts {
            stmts.extend(self.remap_component_client_stmt(stmt, maps)?);
        }
        Ok(ClientBlock::from_stmts(stmts))
    }

    pub(super) fn remap_component_client_stmt(
        &mut self,
        stmt: &ClientStmt,
        maps: &mut ComponentInlineMaps,
    ) -> syn::Result<Vec<ClientStmt>> {
        Ok(match stmt {
            ClientStmt::Expr(expr) => {
                vec![ClientStmt::Expr(
                    self.remap_component_client_expr(expr, maps)?,
                )]
            }
            ClientStmt::Let {
                local,
                name,
                ty,
                value,
            } => {
                let mapped_value = value
                    .as_ref()
                    .map(|value| self.remap_component_client_expr(value, maps))
                    .transpose()?;
                vec![ClientStmt::Let {
                    local: self.get_or_alloc_component_client_local(*local, maps),
                    name: name.clone(),
                    ty: ty.clone(),
                    value: mapped_value,
                }]
            }
            ClientStmt::Assign { local, value } => vec![ClientStmt::Assign {
                local: mapped_client_local(*local, maps)?,
                value: self.remap_component_client_expr(value, maps)?,
            }],
            ClientStmt::SignalSet { id, value } => {
                reject_prop_signal_write(*id, maps)?;
                vec![ClientStmt::SignalSet {
                    id: mapped_signal(*id, maps)?,
                    value: self.remap_component_client_expr(value, maps)?,
                }]
            }
            ClientStmt::SignalUpdate { id, closure } => {
                reject_prop_signal_write(*id, maps)?;
                vec![ClientStmt::SignalUpdate {
                    id: mapped_signal(*id, maps)?,
                    closure: self.remap_component_client_simple_closure(closure, maps)?,
                }]
            }
            ClientStmt::EventPreventDefault => vec![ClientStmt::EventPreventDefault],
            ClientStmt::EventStopPropagation => vec![ClientStmt::EventStopPropagation],
            ClientStmt::Console { level, args } => vec![ClientStmt::Console {
                level: *level,
                args: args
                    .iter()
                    .map(|arg| self.remap_component_client_expr(arg, maps))
                    .collect::<syn::Result<Vec<_>>>()?,
            }],
            ClientStmt::CallbackInvoke { callback, payload } => {
                let handler = maps
                    .callback_handlers
                    .get(callback)
                    .cloned()
                    .ok_or_else(|| {
                        syn::Error::new(
                            proc_macro2::Span::call_site(),
                            "rsjs component descriptor references an unknown callback slot",
                        )
                    })?;
                let Some(handler) = handler else {
                    return Ok(Vec::new());
                };
                let payload = self.remap_component_client_expr(payload, maps)?;
                self.inline_handler_local_client_block_with_payload(
                    &handler,
                    Some(payload),
                    &HandlerScope::default(),
                    proc_macro2::Span::call_site(),
                )?
                .stmts
            }
            ClientStmt::RunMutation { mutation, args } => vec![ClientStmt::RunMutation {
                mutation: mapped_mutation(*mutation, maps)?,
                args: args
                    .iter()
                    .map(|arg| self.remap_component_client_expr(arg, maps))
                    .collect::<syn::Result<Vec<_>>>()?,
            }],
            ClientStmt::QueryRefetch { query } => vec![ClientStmt::QueryRefetch {
                query: mapped_query(*query, maps)?,
            }],
            ClientStmt::QueryInvalidate { query } => vec![ClientStmt::QueryInvalidate {
                query: mapped_query(*query, maps)?,
            }],
            ClientStmt::BrowserEffect(effect) => vec![ClientStmt::BrowserEffect(
                self.remap_component_client_browser_effect(effect, maps)?,
            )],
            ClientStmt::NodeRefEffect { id, effect } => vec![ClientStmt::NodeRefEffect {
                id: mapped_node_ref(*id, maps)?,
                effect: self.remap_component_client_node_ref_effect(effect, maps)?,
            }],
            ClientStmt::Return(value) => vec![ClientStmt::Return(
                value
                    .as_ref()
                    .map(|value| self.remap_component_client_expr(value, maps))
                    .transpose()?,
            )],
            ClientStmt::Block(block) => {
                vec![ClientStmt::Block(
                    self.remap_component_client_block(block, maps)?,
                )]
            }
            ClientStmt::If {
                condition,
                then_block,
                else_block,
            } => vec![ClientStmt::If {
                condition: self.remap_component_client_expr(condition, maps)?,
                then_block: self.remap_component_client_block(then_block, maps)?,
                else_block: else_block
                    .as_ref()
                    .map(|block| self.remap_component_client_block(block, maps))
                    .transpose()?,
            }],
            ClientStmt::ForRange {
                local,
                name,
                integer_domain,
                start,
                end,
                inclusive,
                body,
            } => {
                let mapped_local = self.get_or_alloc_component_client_local(*local, maps);
                vec![ClientStmt::ForRange {
                    local: mapped_local,
                    name: name.clone(),
                    integer_domain: *integer_domain,
                    start: self.remap_component_client_expr(start, maps)?,
                    end: self.remap_component_client_expr(end, maps)?,
                    inclusive: *inclusive,
                    body: self.remap_component_client_block(body, maps)?,
                }]
            }
            _ => {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "rsjs component inlining encountered an unsupported Client IR statement",
                ));
            }
        })
    }

    pub(super) fn remap_component_client_expr(
        &self,
        expr: &ClientExpr,
        maps: &ComponentInlineMaps,
    ) -> syn::Result<ClientExpr> {
        match expr {
            ClientExpr::Unit => Ok(ClientExpr::Unit),
            ClientExpr::Literal(value) => Ok(ClientExpr::Literal(value.clone())),
            ClientExpr::ArrayLit(elems) => Ok(ClientExpr::ArrayLit(elems.clone())),
            ClientExpr::Local(local) => Ok(ClientExpr::Local(mapped_client_local(*local, maps)?)),
            ClientExpr::SignalRead(id) => {
                if let Some(prop) = maps.prop_signals.get(id) {
                    lower_handler_ir_expr_to_client_expr(prop, proc_macro2::Span::call_site())
                } else {
                    Ok(ClientExpr::SignalRead(mapped_signal(*id, maps)?))
                }
            }
            ClientExpr::QueryRead { id, accessor } => Ok(ClientExpr::QueryRead {
                id: mapped_query(*id, maps)?,
                accessor: *accessor,
            }),
            ClientExpr::MutationRead { id, accessor } => Ok(ClientExpr::MutationRead {
                id: mapped_mutation(*id, maps)?,
                accessor: *accessor,
            }),
            ClientExpr::EventRead(value) => Ok(ClientExpr::EventRead(*value)),
            ClientExpr::EventCurrentTarget => Ok(ClientExpr::EventCurrentTarget),
            ClientExpr::DomRead { target, accessor } => Ok(ClientExpr::DomRead {
                target: Box::new(self.remap_component_client_expr(target, maps)?),
                accessor: accessor.clone(),
            }),
            ClientExpr::EventCollectionLen(collection) => {
                Ok(ClientExpr::EventCollectionLen(*collection))
            }
            ClientExpr::EventCollectionRead {
                collection,
                index,
                field,
            } => Ok(ClientExpr::EventCollectionRead {
                collection: *collection,
                index: Box::new(self.remap_component_client_expr(index, maps)?),
                field: *field,
            }),
            ClientExpr::NodeRefRead { id, accessor } => Ok(ClientExpr::NodeRefRead {
                id: mapped_node_ref(*id, maps)?,
                accessor: accessor.clone(),
            }),
            ClientExpr::EventTargetWithinNodeRef { id, inside } => {
                Ok(ClientExpr::EventTargetWithinNodeRef {
                    id: mapped_node_ref(*id, maps)?,
                    inside: *inside,
                })
            }
            ClientExpr::EventAttribute { source, attr } => Ok(ClientExpr::EventAttribute {
                source: *source,
                attr: attr.clone(),
            }),
            ClientExpr::EventPayload { attr } => {
                Ok(ClientExpr::EventPayload { attr: attr.clone() })
            }
            ClientExpr::CallbackPayload => Ok(ClientExpr::CallbackPayload),
            ClientExpr::NodePayload { attr, tokens } => Ok(ClientExpr::NodePayload {
                attr: attr.clone(),
                tokens: rewrite_component_server_only_tokens(
                    tokens,
                    maps,
                    proc_macro2::Span::call_site(),
                )?,
            }),
            ClientExpr::BrowserRead(read) => Ok(ClientExpr::BrowserRead(
                self.remap_component_client_browser_read(read, maps)?,
            )),
            ClientExpr::Encoding { op, value } => Ok(ClientExpr::Encoding {
                op: *op,
                value: Box::new(self.remap_component_client_expr(value, maps)?),
            }),
            ClientExpr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => Ok(ClientExpr::Conditional {
                condition: Box::new(self.remap_component_client_expr(condition, maps)?),
                then_expr: Box::new(self.remap_component_client_expr(then_expr, maps)?),
                else_expr: Box::new(self.remap_component_client_expr(else_expr, maps)?),
            }),
            ClientExpr::IsNonEmpty { target } => Ok(ClientExpr::IsNonEmpty {
                target: Box::new(self.remap_component_client_expr(target, maps)?),
            }),
            ClientExpr::StringToLowercase { target } => Ok(ClientExpr::StringToLowercase {
                target: Box::new(self.remap_component_client_expr(target, maps)?),
            }),
            ClientExpr::StringContains { haystack, needle } => Ok(ClientExpr::StringContains {
                haystack: Box::new(self.remap_component_client_expr(haystack, maps)?),
                needle: Box::new(self.remap_component_client_expr(needle, maps)?),
            }),
            ClientExpr::StringTransform { op, target } => Ok(ClientExpr::StringTransform {
                op: *op,
                target: Box::new(self.remap_component_client_expr(target, maps)?),
            }),
            ClientExpr::StringPredicate {
                op,
                haystack,
                needle,
            } => Ok(ClientExpr::StringPredicate {
                op: *op,
                haystack: Box::new(self.remap_component_client_expr(haystack, maps)?),
                needle: Box::new(self.remap_component_client_expr(needle, maps)?),
            }),
            ClientExpr::Project { target, path } => Ok(ClientExpr::Project {
                target: Box::new(self.remap_component_client_expr(target, maps)?),
                path: path.clone(),
            }),
            ClientExpr::AsType { target, ty } => Ok(ClientExpr::AsType {
                target: Box::new(self.remap_component_client_expr(target, maps)?),
                ty: *ty,
            }),
            ClientExpr::IsPresent { target } => Ok(ClientExpr::IsPresent {
                target: Box::new(self.remap_component_client_expr(target, maps)?),
            }),
            ClientExpr::Eq { target, value } => Ok(ClientExpr::Eq {
                target: Box::new(self.remap_component_client_expr(target, maps)?),
                value: value.clone(),
            }),
            ClientExpr::Neq { target, value } => Ok(ClientExpr::Neq {
                target: Box::new(self.remap_component_client_expr(target, maps)?),
                value: value.clone(),
            }),
            ClientExpr::Coalesce { target, fallback } => Ok(ClientExpr::Coalesce {
                target: Box::new(self.remap_component_client_expr(target, maps)?),
                fallback: fallback.clone(),
            }),
            ClientExpr::Unary { op, expr } => Ok(ClientExpr::Unary {
                op: *op,
                expr: Box::new(self.remap_component_client_expr(expr, maps)?),
            }),
            ClientExpr::Binary { op, left, right } => Ok(ClientExpr::Binary {
                op: *op,
                left: Box::new(self.remap_component_client_expr(left, maps)?),
                right: Box::new(self.remap_component_client_expr(right, maps)?),
            }),
            ClientExpr::Field { base, name } => Ok(ClientExpr::Field {
                base: Box::new(self.remap_component_client_expr(base, maps)?),
                name: name.clone(),
            }),
            ClientExpr::Index { base, index } => Ok(ClientExpr::Index {
                base: Box::new(self.remap_component_client_expr(base, maps)?),
                index: Box::new(self.remap_component_client_expr(index, maps)?),
            }),
            ClientExpr::Call { function, args } => Ok(ClientExpr::Call {
                function: function.clone(),
                args: args
                    .iter()
                    .map(|arg| self.remap_component_client_expr(arg, maps))
                    .collect::<syn::Result<Vec<_>>>()?,
            }),
            ClientExpr::QueryKey(parts) => Ok(ClientExpr::QueryKey(
                parts
                    .iter()
                    .map(|part| self.remap_component_client_expr(part, maps))
                    .collect::<syn::Result<Vec<_>>>()?,
            )),
            _ => Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "rsjs component inlining encountered an unsupported Client IR expression",
            )),
        }
    }

    pub(super) fn remap_component_client_browser_read(
        &self,
        read: &ClientBrowserRead,
        maps: &ComponentInlineMaps,
    ) -> syn::Result<ClientBrowserRead> {
        Ok(match read {
            ClientBrowserRead::Window(field) => ClientBrowserRead::Window(*field),
            ClientBrowserRead::Location(field) => ClientBrowserRead::Location(*field),
            ClientBrowserRead::Storage {
                area,
                value_type,
                key,
                fallback,
            } => ClientBrowserRead::Storage {
                area: *area,
                value_type: *value_type,
                key: Box::new(self.remap_component_client_expr(key, maps)?),
                fallback: fallback
                    .as_ref()
                    .map(|fallback| {
                        self.remap_component_client_expr(fallback, maps)
                            .map(Box::new)
                    })
                    .transpose()?,
            },
            ClientBrowserRead::UrlSearchParam { key, fallback } => {
                ClientBrowserRead::UrlSearchParam {
                    key: Box::new(self.remap_component_client_expr(key, maps)?),
                    fallback: fallback
                        .as_ref()
                        .map(|fallback| {
                            self.remap_component_client_expr(fallback, maps)
                                .map(Box::new)
                        })
                        .transpose()?,
                }
            }
            _ => {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "rsjs component inlining encountered an unsupported Client IR browser read",
                ));
            }
        })
    }

    pub(super) fn remap_component_client_simple_closure(
        &self,
        closure: &ClientSimpleClosure,
        maps: &ComponentInlineMaps,
    ) -> syn::Result<ClientSimpleClosure> {
        Ok(ClientSimpleClosure {
            param: closure.param.clone(),
            body: Box::new(self.remap_component_client_expr(&closure.body, maps)?),
        })
    }

    pub(super) fn remap_component_client_browser_effect(
        &self,
        effect: &ClientBrowserEffect,
        maps: &ComponentInlineMaps,
    ) -> syn::Result<ClientBrowserEffect> {
        Ok(match effect {
            ClientBrowserEffect::WindowScrollTo { x, y } => ClientBrowserEffect::WindowScrollTo {
                x: self.remap_component_client_expr(x, maps)?,
                y: self.remap_component_client_expr(y, maps)?,
            },
            ClientBrowserEffect::WindowScrollBy { x, y } => ClientBrowserEffect::WindowScrollBy {
                x: self.remap_component_client_expr(x, maps)?,
                y: self.remap_component_client_expr(y, maps)?,
            },
            ClientBrowserEffect::WindowPrint => ClientBrowserEffect::WindowPrint,
            ClientBrowserEffect::LocationSetHash { value } => {
                ClientBrowserEffect::LocationSetHash {
                    value: self.remap_component_client_expr(value, maps)?,
                }
            }
            ClientBrowserEffect::LocationAssign { url } => ClientBrowserEffect::LocationAssign {
                url: self.remap_component_client_expr(url, maps)?,
            },
            ClientBrowserEffect::LocationReplace { url } => ClientBrowserEffect::LocationReplace {
                url: self.remap_component_client_expr(url, maps)?,
            },
            ClientBrowserEffect::LocationReload => ClientBrowserEffect::LocationReload,
            ClientBrowserEffect::HistoryPushState { url } => {
                ClientBrowserEffect::HistoryPushState {
                    url: self.remap_component_client_expr(url, maps)?,
                }
            }
            ClientBrowserEffect::HistoryReplaceState { url } => {
                ClientBrowserEffect::HistoryReplaceState {
                    url: self.remap_component_client_expr(url, maps)?,
                }
            }
            ClientBrowserEffect::HistoryBack => ClientBrowserEffect::HistoryBack,
            ClientBrowserEffect::HistoryForward => ClientBrowserEffect::HistoryForward,
            ClientBrowserEffect::HistoryGo { delta } => ClientBrowserEffect::HistoryGo {
                delta: self.remap_component_client_expr(delta, maps)?,
            },
            ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
                ClientBrowserEffect::DocumentDispatchEvent {
                    name: self.remap_component_client_expr(name, maps)?,
                    detail: detail
                        .as_ref()
                        .map(|detail| self.remap_component_client_expr(detail, maps))
                        .transpose()?,
                }
            }
            ClientBrowserEffect::DocumentSetTitle { title } => {
                ClientBrowserEffect::DocumentSetTitle {
                    title: self.remap_component_client_expr(title, maps)?,
                }
            }
            ClientBrowserEffect::ClipboardWriteText { text } => {
                ClientBrowserEffect::ClipboardWriteText {
                    text: self.remap_component_client_expr(text, maps)?,
                }
            }
            ClientBrowserEffect::ClipboardReadTextInto { target, fallback } => {
                reject_prop_signal_write(*target, maps)?;
                ClientBrowserEffect::ClipboardReadTextInto {
                    target: mapped_signal(*target, maps)?,
                    fallback: fallback
                        .as_ref()
                        .map(|fallback| self.remap_component_client_expr(fallback, maps))
                        .transpose()?,
                }
            }
            ClientBrowserEffect::WebAuthnCreateJsonInto {
                options_json,
                target,
                fallback,
            } => {
                reject_prop_signal_write(*target, maps)?;
                ClientBrowserEffect::WebAuthnCreateJsonInto {
                    options_json: self.remap_component_client_expr(options_json, maps)?,
                    target: mapped_signal(*target, maps)?,
                    fallback: fallback
                        .as_ref()
                        .map(|fallback| self.remap_component_client_expr(fallback, maps))
                        .transpose()?,
                }
            }
            ClientBrowserEffect::WebAuthnGetJsonInto {
                options_json,
                target,
                fallback,
            } => {
                reject_prop_signal_write(*target, maps)?;
                ClientBrowserEffect::WebAuthnGetJsonInto {
                    options_json: self.remap_component_client_expr(options_json, maps)?,
                    target: mapped_signal(*target, maps)?,
                    fallback: fallback
                        .as_ref()
                        .map(|fallback| self.remap_component_client_expr(fallback, maps))
                        .transpose()?,
                }
            }
            ClientBrowserEffect::EventFileTextInto {
                source,
                target,
                fallback,
            } => {
                reject_prop_signal_write(*target, maps)?;
                ClientBrowserEffect::EventFileTextInto {
                    source: *source,
                    target: mapped_signal(*target, maps)?,
                    fallback: fallback
                        .as_ref()
                        .map(|fallback| self.remap_component_client_expr(fallback, maps))
                        .transpose()?,
                }
            }
            ClientBrowserEffect::StorageSetItem { area, key, value } => {
                ClientBrowserEffect::StorageSetItem {
                    area: *area,
                    key: self.remap_component_client_expr(key, maps)?,
                    value: self.remap_component_client_expr(value, maps)?,
                }
            }
            ClientBrowserEffect::StorageRemoveItem { area, key } => {
                ClientBrowserEffect::StorageRemoveItem {
                    area: *area,
                    key: self.remap_component_client_expr(key, maps)?,
                }
            }
            ClientBrowserEffect::UrlSetSearchParam { mode, key, value } => {
                ClientBrowserEffect::UrlSetSearchParam {
                    mode: *mode,
                    key: self.remap_component_client_expr(key, maps)?,
                    value: self.remap_component_client_expr(value, maps)?,
                }
            }
            ClientBrowserEffect::UrlDeleteSearchParam { mode, key } => {
                ClientBrowserEffect::UrlDeleteSearchParam {
                    mode: *mode,
                    key: self.remap_component_client_expr(key, maps)?,
                }
            }
            ClientBrowserEffect::Declared { helper, args } => ClientBrowserEffect::Declared {
                helper: helper.clone(),
                args: args
                    .iter()
                    .map(|arg| self.remap_component_client_expr(arg, maps))
                    .collect::<syn::Result<Vec<_>>>()?,
            },
        })
    }

    pub(super) fn remap_component_client_node_ref_effect(
        &self,
        effect: &ClientNodeRefEffect,
        maps: &ComponentInlineMaps,
    ) -> syn::Result<ClientNodeRefEffect> {
        Ok(match effect {
            ClientNodeRefEffect::Focus => ClientNodeRefEffect::Focus,
            ClientNodeRefEffect::Blur => ClientNodeRefEffect::Blur,
            ClientNodeRefEffect::ScrollIntoView => ClientNodeRefEffect::ScrollIntoView,
            ClientNodeRefEffect::Click => ClientNodeRefEffect::Click,
            ClientNodeRefEffect::RequestSubmit => ClientNodeRefEffect::RequestSubmit,
            ClientNodeRefEffect::Reset => ClientNodeRefEffect::Reset,
            ClientNodeRefEffect::ReportValidity => ClientNodeRefEffect::ReportValidity,
            ClientNodeRefEffect::ShowModal => ClientNodeRefEffect::ShowModal,
            ClientNodeRefEffect::Close => ClientNodeRefEffect::Close,
            ClientNodeRefEffect::ShowPopover => ClientNodeRefEffect::ShowPopover,
            ClientNodeRefEffect::HidePopover => ClientNodeRefEffect::HidePopover,
            ClientNodeRefEffect::TogglePopover => ClientNodeRefEffect::TogglePopover,
            ClientNodeRefEffect::ScrollTo { x, y } => ClientNodeRefEffect::ScrollTo {
                x: self.remap_component_client_expr(x, maps)?,
                y: self.remap_component_client_expr(y, maps)?,
            },
            ClientNodeRefEffect::ScrollBy { x, y } => ClientNodeRefEffect::ScrollBy {
                x: self.remap_component_client_expr(x, maps)?,
                y: self.remap_component_client_expr(y, maps)?,
            },
            ClientNodeRefEffect::SetPointerCapture(expr) => ClientNodeRefEffect::SetPointerCapture(
                self.remap_component_client_expr(expr, maps)?,
            ),
            ClientNodeRefEffect::ReleasePointerCapture(expr) => {
                ClientNodeRefEffect::ReleasePointerCapture(
                    self.remap_component_client_expr(expr, maps)?,
                )
            }
            ClientNodeRefEffect::Select => ClientNodeRefEffect::Select,
            ClientNodeRefEffect::SetValue(expr) => {
                ClientNodeRefEffect::SetValue(self.remap_component_client_expr(expr, maps)?)
            }
            ClientNodeRefEffect::SetChecked(expr) => {
                ClientNodeRefEffect::SetChecked(self.remap_component_client_expr(expr, maps)?)
            }
            ClientNodeRefEffect::SetCustomValidity(expr) => ClientNodeRefEffect::SetCustomValidity(
                self.remap_component_client_expr(expr, maps)?,
            ),
            ClientNodeRefEffect::SetSelectionRange(start, end) => {
                ClientNodeRefEffect::SetSelectionRange(
                    self.remap_component_client_expr(start, maps)?,
                    self.remap_component_client_expr(end, maps)?,
                )
            }
            ClientNodeRefEffect::SetTextContent(expr) => {
                ClientNodeRefEffect::SetTextContent(self.remap_component_client_expr(expr, maps)?)
            }
            ClientNodeRefEffect::AddClass(expr) => {
                ClientNodeRefEffect::AddClass(self.remap_component_client_expr(expr, maps)?)
            }
            ClientNodeRefEffect::RemoveClass(expr) => {
                ClientNodeRefEffect::RemoveClass(self.remap_component_client_expr(expr, maps)?)
            }
            ClientNodeRefEffect::ToggleClass(expr) => {
                ClientNodeRefEffect::ToggleClass(self.remap_component_client_expr(expr, maps)?)
            }
            ClientNodeRefEffect::SetAttribute { name, value } => {
                ClientNodeRefEffect::SetAttribute {
                    name: name.clone(),
                    value: self.remap_component_client_expr(value, maps)?,
                }
            }
            ClientNodeRefEffect::RemoveAttribute { name } => {
                ClientNodeRefEffect::RemoveAttribute { name: name.clone() }
            }
            ClientNodeRefEffect::SetStyleProperty { name, value } => {
                ClientNodeRefEffect::SetStyleProperty {
                    name: name.clone(),
                    value: self.remap_component_client_expr(value, maps)?,
                }
            }
        })
    }

    pub(super) fn get_or_alloc_component_client_local(
        &mut self,
        local: ClientLocalId,
        maps: &mut ComponentInlineMaps,
    ) -> ClientLocalId {
        let old = LocalId(local.0);
        if let Some(mapped) = maps.locals.get(&old) {
            return ClientLocalId(mapped.0);
        }
        let mapped = self.alloc_handler_local();
        maps.locals.insert(old, mapped);
        ClientLocalId(mapped.0)
    }

    pub(super) fn copy_component_node(
        &mut self,
        child: &Island,
        old: NodeId,
        parent: NodeId,
        maps: &mut ComponentInlineMaps,
        span: proc_macro2::Span,
    ) -> syn::Result<NodeId> {
        let mapped = match child.view_tree.get(old) {
            ViewNode::Element {
                tag,
                children,
                static_attrs,
                server_attrs,
                payload_attrs,
            } => {
                let new = self.builder.element(tag, Some(parent));
                maps.nodes.insert(old, new);
                for (name, value) in static_attrs {
                    self.builder.static_attr(new, name, value);
                }
                for attr in server_attrs {
                    let tokens = rewrite_component_server_only_tokens(&attr.tokens, maps, span)?;
                    self.builder
                        .server_attr_with_source(new, &attr.name, &tokens, attr.source);
                }
                for attr in payload_attrs {
                    let tokens = rewrite_component_server_only_tokens(&attr.tokens, maps, span)?;
                    self.builder.payload_attr_with_source(
                        new,
                        &attr.name,
                        &tokens,
                        attr.label.clone(),
                        None,
                    );
                }
                for child_id in children {
                    self.copy_component_node(child, *child_id, new, maps, span)?;
                }
                new
            }
            ViewNode::StaticText(text) => {
                let new = self.builder.text_node_static(parent, text);
                maps.nodes.insert(old, new);
                new
            }
            ViewNode::TextBindingSlot => {
                let Some(expr) = child.bindings.text_bindings.get(&old) else {
                    return Err(syn::Error::new(
                        span,
                        "rsjs component descriptor has a text-binding slot with no binding",
                    ));
                };
                let new = self.builder.text_node(parent, mapped_expr(*expr, maps)?);
                maps.nodes.insert(old, new);
                new
            }
            ViewNode::ServerOnly(server) => {
                let tokens = rewrite_component_server_only_tokens(&server.tokens, maps, span)?;
                let new = self
                    .builder
                    .server_only_node_with_source(parent, &tokens, server.source);
                maps.nodes.insert(old, new);
                new
            }
            ViewNode::ForLoop {
                attrs,
                label,
                pat,
                expr,
                body_prefix,
                children,
            } => {
                let expr = rewrite_component_server_only_tokens(expr, maps, span)?;
                let body_prefix = rewrite_component_statement_tokens(body_prefix, maps, span)?;
                let new = self
                    .builder
                    .for_loop(parent, attrs, label, pat, &expr, &body_prefix);
                maps.nodes.insert(old, new);
                for child_id in children {
                    self.copy_component_node(child, *child_id, new, maps, span)?;
                }
                new
            }
            _ => {
                return Err(syn::Error::new(
                    span,
                    "rsjs component inlining encountered an unsupported child view node",
                ));
            }
        };
        Ok(mapped)
    }

    pub(super) fn copy_component_bindings(
        &mut self,
        child: &Island,
        maps: &ComponentInlineMaps,
        span: proc_macro2::Span,
    ) -> syn::Result<()> {
        for (node, expr) in &child.bindings.text_bindings {
            if matches!(child.view_tree.get(*node), ViewNode::TextBindingSlot) {
                continue;
            }
            self.builder
                .bind_text(mapped_node(*node, maps)?, mapped_expr(*expr, maps)?);
        }
        for ((node, event), handler) in &child.bindings.event_bindings {
            self.builder.bind_event(
                mapped_node(*node, maps)?,
                *event,
                mapped_handler(*handler, maps)?,
            );
        }
        for binding in child.bindings.attr_bindings.values() {
            self.builder.bind_attr(
                mapped_node(binding.node, maps)?,
                &binding.name,
                binding.kind,
                mapped_expr(binding.expr, maps)?,
            );
        }
        for binding in child.bindings.input_bindings.values() {
            reject_prop_signal_write(binding.signal, maps)?;
            self.builder.bind_input(
                mapped_node(binding.node, maps)?,
                binding.kind,
                mapped_signal(binding.signal, maps)?,
            );
        }
        for binding in child.bindings.node_ref_bindings.values() {
            self.builder.bind_node_ref(
                mapped_node(binding.node, maps)?,
                mapped_node_ref(binding.node_ref, maps)?,
            );
        }
        for (parent, sections) in &child.bindings.conditional_sections {
            for section in sections {
                self.builder.conditional_section(
                    mapped_node(*parent, maps)?,
                    mapped_expr(section.condition, maps)?,
                    section
                        .then_children
                        .iter()
                        .map(|node| mapped_node(*node, maps))
                        .collect::<syn::Result<Vec<_>>>()?,
                    section
                        .else_children
                        .iter()
                        .map(|node| mapped_node(*node, maps))
                        .collect::<syn::Result<Vec<_>>>()?,
                );
            }
        }
        if maps.nodes.is_empty() {
            return Err(syn::Error::new(
                span,
                "rsjs component descriptor did not contribute any nodes",
            ));
        }
        Ok(())
    }

    pub(super) fn inline_handler_local_hir_with_payload(
        &mut self,
        handler: &HandlerLocal,
        payload: Option<FrontendExpr>,
        outer_scope: &HandlerScope<'_>,
        span: proc_macro2::Span,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        if handler.kind != HandlerLocalKind::Payload {
            return Err(syn::Error::new(
                span,
                format!(
                    "`{}` is an `rsjs::event_handler(...)` value; component callback props must use `rsjs::handler(...)`",
                    handler.name
                ),
            ));
        }
        let param = payload_handler_param(&handler.closure)?;
        match (param.arity, payload) {
            (0, None) => self.lower_captured_handler_body(handler, outer_scope),
            (0, Some(_)) => Err(syn::Error::new(
                span,
                format!(
                    "`{}` expects no payload argument based on its `rsjs::handler(...)` closure",
                    handler.name
                ),
            )),
            (1, None) => Err(syn::Error::new(
                span,
                format!(
                    "`{}` expects one payload argument based on its `rsjs::handler(...)` closure",
                    handler.name
                ),
            )),
            (1, Some(payload)) => {
                let Some(param_name) = param.name else {
                    return self.lower_captured_handler_body(handler, outer_scope);
                };
                let payload = match param.integer_source {
                    Some(IntegerSource::Unsupported(name)) => {
                        return Err(unsupported_integer_domain_error(span, &name));
                    }
                    Some(IntegerSource::Supported(domain)) => {
                        self.anchor_integer_domain(payload, domain)
                    }
                    None => payload,
                };
                let scoped = HandlerScope {
                    handler_param: Some((param_name.as_str(), &payload)),
                    event_param: outer_scope.event_param,
                    handler_locals: outer_scope.handler_locals.clone(),
                    event_node: outer_scope.event_node,
                };
                self.lower_captured_handler_body(handler, &scoped)
            }
            _ => Err(syn::Error::new(
                span,
                "`rsjs::handler(...)` closures accept at most one payload argument",
            )),
        }
    }

    pub(super) fn inline_handler_local_client_block_with_payload(
        &mut self,
        handler: &HandlerLocal,
        payload: Option<ClientExpr>,
        outer_scope: &HandlerScope<'_>,
        span: proc_macro2::Span,
    ) -> syn::Result<ClientBlock> {
        let statements = self.inline_handler_local_hir_with_payload(
            handler,
            payload.as_ref().map(|_| FrontendExpr::CallbackPayload),
            outer_scope,
            span,
        )?;
        let mut block = lower_handler_hir_to_client_block(&statements)?;
        if let Some(payload) = payload {
            substitute_client_callback_payload_block(&mut block, &payload);
        }
        Ok(block)
    }
}

pub(super) fn mapped_signal(id: SignalId, maps: &ComponentInlineMaps) -> syn::Result<SignalId> {
    maps.signals.get(&id).copied().ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "rsjs component descriptor references unknown signal id {}",
                id.0
            ),
        )
    })
}

pub(super) fn mapped_node_ref(id: NodeRefId, maps: &ComponentInlineMaps) -> syn::Result<NodeRefId> {
    maps.node_refs.get(&id).copied().ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "rsjs component descriptor references unknown node-ref id {}",
                id.0
            ),
        )
    })
}

pub(super) fn mapped_query(id: QueryId, maps: &ComponentInlineMaps) -> syn::Result<QueryId> {
    maps.queries.get(&id).copied().ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "rsjs component descriptor references unknown query id {}",
                id.0
            ),
        )
    })
}

pub(super) fn mapped_mutation(
    id: MutationId,
    maps: &ComponentInlineMaps,
) -> syn::Result<MutationId> {
    maps.mutations.get(&id).copied().ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "rsjs component descriptor references unknown mutation id {}",
                id.0
            ),
        )
    })
}

pub(super) fn mapped_client_local(
    id: ClientLocalId,
    maps: &ComponentInlineMaps,
) -> syn::Result<ClientLocalId> {
    maps.locals
        .get(&LocalId(id.0))
        .copied()
        .map(|id| ClientLocalId(id.0))
        .ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "rsjs component descriptor references unknown handler local id {}",
                    id.0
                ),
            )
        })
}

pub(super) fn reject_prop_signal_write(
    id: SignalId,
    maps: &ComponentInlineMaps,
) -> syn::Result<()> {
    if maps.prop_signals.contains_key(&id) {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "rsjs component inlining cannot write to a prop-backed child signal; emit a callback \
             to the parent instead",
        ));
    }
    Ok(())
}

pub(super) fn reject_runtime_initialized_child_query(
    island: &Island,
    query: &rsjs_ir::QueryDecl,
    has_runtime_initializer: bool,
    span: proc_macro2::Span,
) -> syn::Result<()> {
    if !has_runtime_initializer
        && (matches!(
            island.exprs.client_expr(query.key_expr),
            ClientExpr::Literal(Value::Null)
        ) || matches!(
            island.exprs.client_expr(query.server_initial),
            ClientExpr::Literal(Value::Null)
        ))
    {
        return Err(syn::Error::new(
            span,
            "rsjs component inlining does not support child queries with server-local keys, \
             `query_with_initial(...)` yet; hoist the query into the async parent or render the \
             child as an explicit async island boundary",
        ));
    }
    Ok(())
}

pub(super) fn rewrite_component_server_expr(
    expr: &Expr,
    maps: &ComponentInlineMaps,
    span: proc_macro2::Span,
) -> syn::Result<Expr> {
    let tokens =
        rewrite_component_server_only_tokens(&expr.to_token_stream().to_string(), maps, span)?;
    let tokens = tokens.parse::<proc_macro2::TokenStream>().map_err(|err| {
        syn::Error::new(
            span,
            format!("failed to parse remapped child server expression tokens: {err}"),
        )
    })?;
    syn::parse2::<Expr>(tokens).map_err(|err| {
        syn::Error::new(
            span,
            format!("failed to parse remapped child server expression: {err}"),
        )
    })
}

pub(super) fn reject_runtime_initialized_child_mutation(
    island: &Island,
    mutation: &rsjs_ir::MutationDecl,
    has_runtime_initializer: bool,
    span: proc_macro2::Span,
) -> syn::Result<()> {
    if !has_runtime_initializer
        && matches!(
            island.exprs.client_expr(mutation.key_expr),
            ClientExpr::Literal(Value::Null)
        )
    {
        return Err(syn::Error::new(
            span,
            "rsjs component inlining does not support child mutations with server-local keys yet; \
             hoist the mutation into the async parent or render the child as an explicit async \
             island boundary",
        ));
    }
    Ok(())
}

pub(super) fn rewrite_component_server_only_tokens(
    tokens: &str,
    maps: &ComponentInlineMaps,
    span: proc_macro2::Span,
) -> syn::Result<String> {
    if maps.server_props.is_empty() {
        return Ok(tokens.to_string());
    }
    let tokens = tokens.parse::<proc_macro2::TokenStream>().map_err(|err| {
        syn::Error::new(
            span,
            format!("failed to parse rsjs component server-only tokens: {err}"),
        )
    })?;
    let expr = syn::parse2::<Expr>(tokens).map_err(|err| {
        syn::Error::new(
            span,
            format!("failed to parse rsjs component server-only expression: {err}"),
        )
    })?;
    let mut folder = ServerOnlyPropSubstitution {
        props: &maps.server_props,
    };
    Ok(folder.fold_expr(expr).to_token_stream().to_string())
}

pub(super) fn rewrite_component_statement_tokens(
    tokens: &str,
    maps: &ComponentInlineMaps,
    span: proc_macro2::Span,
) -> syn::Result<String> {
    if maps.server_props.is_empty() || tokens.trim().is_empty() {
        return Ok(tokens.to_string());
    }
    let tokens = tokens.parse::<proc_macro2::TokenStream>().map_err(|err| {
        syn::Error::new(
            span,
            format!("failed to parse rsjs component server-only statements: {err}"),
        )
    })?;
    let block_tokens = {
        let mut out = proc_macro2::TokenStream::new();
        proc_macro2::Group::new(proc_macro2::Delimiter::Brace, tokens).to_tokens(&mut out);
        out
    };
    let block = syn::parse2::<Block>(block_tokens).map_err(|err| {
        syn::Error::new(
            span,
            format!("failed to parse rsjs component server-only statement block: {err}"),
        )
    })?;
    let mut folder = ServerOnlyPropSubstitution {
        props: &maps.server_props,
    };
    let rewritten = folder.fold_block(block);
    Ok(rewritten
        .stmts
        .iter()
        .map(ToTokens::to_token_stream)
        .collect::<proc_macro2::TokenStream>()
        .to_string())
}

pub(super) fn collect_server_prelude_local_exprs(lets: &[Local]) -> HashMap<String, Expr> {
    let mut locals = HashMap::new();
    for local in lets {
        if is_signal_local(local)
            || is_handler_local(local)
            || is_node_ref_local(local)
            || is_query_local(local)
            || is_mutation_local(local)
        {
            continue;
        }
        let Some(name) = local_ident_name(local) else {
            continue;
        };
        let Some(init) = &local.init else {
            continue;
        };
        if init.diverge.is_some() {
            continue;
        }

        let mut folder = ServerPreludeLocalSubstitution { locals: &locals };
        let expr = folder.fold_expr((*init.expr).clone());
        let expr = if matches!(&local.pat, Pat::Type(_)) {
            let pat = &local.pat;
            let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
            syn::parse_quote! {{
                let #pat = #expr;
                #ident
            }}
        } else {
            expr
        };
        locals.insert(name, expr);
    }
    locals
}

pub(super) fn local_ident_name(local: &Local) -> Option<String> {
    match &local.pat {
        Pat::Ident(pi) => Some(pi.ident.to_string()),
        Pat::Type(pt) => match pt.pat.as_ref() {
            Pat::Ident(pi) => Some(pi.ident.to_string()),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn rewrite_expr_tokens_with_server_locals(
    tokens: &str,
    locals: &HashMap<String, Expr>,
) -> syn::Result<String> {
    if tokens.trim().is_empty() || locals.is_empty() {
        return Ok(tokens.to_string());
    }
    let tokens = tokens.parse::<proc_macro2::TokenStream>().map_err(|err| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to parse rsjs descriptor server expression tokens: {err}"),
        )
    })?;
    let expr = syn::parse2::<Expr>(tokens).map_err(|err| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to parse rsjs descriptor server expression: {err}"),
        )
    })?;
    let mut folder = ServerPreludeLocalSubstitution { locals };
    Ok(folder.fold_expr(expr).to_token_stream().to_string())
}

pub(super) fn rewrite_statement_tokens_with_server_locals(
    tokens: &str,
    locals: &HashMap<String, Expr>,
) -> syn::Result<String> {
    if tokens.trim().is_empty() || locals.is_empty() {
        return Ok(tokens.to_string());
    }
    let tokens = tokens.parse::<proc_macro2::TokenStream>().map_err(|err| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to parse rsjs descriptor server statement tokens: {err}"),
        )
    })?;
    let block_tokens = {
        let mut out = proc_macro2::TokenStream::new();
        proc_macro2::Group::new(proc_macro2::Delimiter::Brace, tokens).to_tokens(&mut out);
        out
    };
    let block = syn::parse2::<Block>(block_tokens).map_err(|err| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to parse rsjs descriptor server statement block: {err}"),
        )
    })?;
    let mut folder = ServerPreludeLocalSubstitution { locals };
    let rewritten = folder.fold_block(block);
    Ok(rewritten
        .stmts
        .iter()
        .map(ToTokens::to_token_stream)
        .collect::<proc_macro2::TokenStream>()
        .to_string())
}

pub(super) struct ServerPreludeLocalSubstitution<'a> {
    pub(super) locals: &'a HashMap<String, Expr>,
}

impl Fold for ServerPreludeLocalSubstitution<'_> {
    fn fold_expr(&mut self, expr: Expr) -> Expr {
        if let Expr::Path(path) = &expr {
            if path.qself.is_none() && path.path.segments.len() == 1 {
                if let Some(ident) = path.path.get_ident() {
                    if let Some(replacement) = self.locals.get(&ident.to_string()) {
                        return replacement.clone();
                    }
                }
            }
        }
        fold::fold_expr(self, expr)
    }

    fn fold_expr_macro(&mut self, expr: ExprMacro) -> ExprMacro {
        fold_text_macro_expr_args(self, expr)
    }
}

pub(super) struct ServerOnlyPropSubstitution<'a> {
    pub(super) props: &'a HashMap<String, Expr>,
}

impl Fold for ServerOnlyPropSubstitution<'_> {
    fn fold_expr(&mut self, expr: Expr) -> Expr {
        if let Expr::Field(field) = &expr {
            if let Some(name) = direct_self_field_name(field) {
                if let Some(replacement) = self.props.get(&name) {
                    return replacement.clone();
                }
            }
        }
        fold::fold_expr(self, expr)
    }

    fn fold_expr_macro(&mut self, expr: ExprMacro) -> ExprMacro {
        fold_text_macro_expr_args(self, expr)
    }
}

/// Syn deliberately treats a macro invocation's body as opaque tokens. A
/// direct `text!(template, expr, ...)` is different for descriptor purposes:
/// its arguments are ordinary server expressions, so child prelude locals and
/// `self.field` props inside them need the same substitutions as surrounding
/// Rust expressions before a component descriptor is consumed by its parent.
///
/// Keep this exception narrow. Re-parsing arbitrary macro bodies as a
/// comma-separated expression list would change the grammar of unrelated
/// macros such as `html!`, `vec!`, or user-defined DSLs.
fn fold_text_macro_expr_args<F: Fold>(folder: &mut F, mut expr: ExprMacro) -> ExprMacro {
    expr = fold::fold_expr_macro(folder, expr);
    if !crate::is_text_intrinsic_path(&expr.mac.path) {
        return expr;
    }

    let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
    let Ok(args) = syn::parse::Parser::parse2(parser, expr.mac.tokens.clone()) else {
        // Leave malformed text! input to text!'s own parser so its diagnostic
        // remains the source of truth for the formatting grammar.
        return expr;
    };
    if !matches!(
        args.first(),
        Some(Expr::Lit(ExprLit {
            lit: Lit::Str(_),
            ..
        }))
    ) {
        // text!'s actual grammar starts with a string literal. Do not turn an
        // invalid invocation into a valid one by substituting its first token.
        return expr;
    }
    let args: syn::punctuated::Punctuated<Expr, syn::Token![,]> = args
        .into_iter()
        .map(|argument| folder.fold_expr(argument))
        .collect();
    expr.mac.tokens = args.to_token_stream();
    expr
}

pub(super) fn direct_self_field_name(field: &syn::ExprField) -> Option<String> {
    if !expr_is_self_path(&field.base) {
        return None;
    }
    match &field.member {
        syn::Member::Named(ident) => Some(ident.to_string()),
        syn::Member::Unnamed(_) => None,
    }
}

fn component_source_anchor(span: proc_macro2::Span) -> rsjs_ir::SourceAnchor {
    let start = span.start();
    rsjs_ir::SourceAnchor {
        start_line: start.line,
        start_column: start.column + 1,
    }
}

pub(super) fn mapped_expr(id: ExprId, maps: &ComponentInlineMaps) -> syn::Result<ExprId> {
    maps.exprs.get(&id).copied().ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "rsjs component descriptor references unknown expression id {}",
                id.0
            ),
        )
    })
}

pub(super) fn mapped_handler(
    id: EventHandlerId,
    maps: &ComponentInlineMaps,
) -> syn::Result<EventHandlerId> {
    maps.handlers.get(&id).copied().ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "rsjs component descriptor references unknown handler id {}",
                id.0
            ),
        )
    })
}

pub(super) fn mapped_node(id: NodeId, maps: &ComponentInlineMaps) -> syn::Result<NodeId> {
    maps.nodes.get(&id).copied().ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "rsjs component descriptor references unknown node id {}",
                id.0
            ),
        )
    })
}
