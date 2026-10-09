// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! View-tree walking: elements, attributes, children, conditionals, loops, keyed lists, and server/island boundaries.
use super::*;

fn reserved_rsjs_attribute_error(raw_name: &str, span: proc_macro2::Span) -> Option<syn::Error> {
    let normalized = raw_name
        .strip_prefix("r#")
        .unwrap_or(raw_name)
        .replace('_', "-")
        .to_ascii_lowercase();
    (normalized == "data-rsjs" || normalized.starts_with("data-rsjs-")).then(|| {
        syn::Error::new(
            span,
            format!(
                "`{normalized}` is reserved for compiler-owned RSJS runtime markers; \
                 remove it or rename application data to a different namespace such as \
                 `data-app-*`. RSJS emits `data-rsjs-*` attributes automatically when \
                 an occurrence belongs to a browser owner"
            ),
        )
    })
}

impl Analyzer {
    /// Walk the HTML root and find the (single) top-level element.
    pub(super) fn walk_root(&mut self, root: &HtmlRoot) -> syn::Result<()> {
        // Find the unique top-level element. Pure-text top-level
        // bodies are rejected (an island must have a structural
        // wrapper to attach hydration markers to). Multiple top-level
        // siblings are also rejected: rsjs needs a single root
        // element to host the `<rsjs-island>` wrapper.
        let elements: Vec<&Element> = root
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Element(e) => Some(e),
                _ => None,
            })
            .collect();
        if elements.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "`#[rsjs]` body must contain a top-level element \
                 (e.g. `html_sync! { <div>...</div> }`); \
                 a bare text or expression block has no place to attach the island wrapper",
            ));
        }
        if elements.len() > 1 {
            return Err(syn::Error::new(
                elements[1].name.span(),
                "`#[rsjs]` body must have a single top-level element; \
                 wrap multiple top-level siblings in a containing element \
                 (e.g. a `<div>`)",
            ));
        }

        let root_element = elements[0];
        let root_id = self.walk_element(root_element, None)?;
        self.builder.set_root(root_id);
        Ok(())
    }

    /// Walk an element: allocate a NodeId, process attributes, recurse
    /// into children. Returns the allocated NodeId.
    ///
    /// Round-3 review #4: errors from `process_attributes` and
    /// `process_children` are accumulated and combined via
    /// `syn::Error::combine`, mirroring the pattern inside each helper.
    /// This way a user with both an attribute mistake and a child
    /// mistake on the same element sees both errors in one
    /// compilation cycle.
    pub(super) fn walk_element(
        &mut self,
        el: &Element,
        parent: Option<NodeId>,
    ) -> syn::Result<NodeId> {
        let tag = el.tag_str();
        let id = self.builder.element(&tag, parent);
        let mut accumulated: Option<syn::Error> = None;
        if let Err(e) = self.process_attributes(el, id) {
            match &mut accumulated {
                Some(existing) => existing.combine(e),
                None => accumulated = Some(e),
            }
        }
        if let Err(e) = self.process_children(&el.children, id) {
            match &mut accumulated {
                Some(existing) => existing.combine(e),
                None => accumulated = Some(e),
            }
        }
        if let Some(err) = accumulated {
            return Err(err);
        }
        Ok(id)
    }

    /// Process every attribute on an element.
    ///
    /// Errors are accumulated and combined via `syn::Error::combine`
    /// so a user with multiple unrelated attribute mistakes sees them
    /// all at once, rather than fixing them one-at-a-time. We do not
    /// accumulate across stages where a later check depends on an
    /// earlier check's success (e.g. signal lookup): those still
    /// short-circuit.
    pub(super) fn process_attributes(&mut self, el: &Element, id: NodeId) -> syn::Result<()> {
        let mut input_binding_seen: Option<proc_macro2::Span> = None;
        let mut accumulated: Option<syn::Error> = None;
        let push_err = |slot: &mut Option<syn::Error>, err: syn::Error| match slot {
            Some(existing) => existing.combine(err),
            None => *slot = Some(err),
        };

        for attr in &el.attrs {
            let name_ident = match attr {
                Attribute::Static { name, .. } => name,
                Attribute::Dynamic { name, .. } => name,
                Attribute::Bool { name } => name,
            };
            let raw_name = name_ident.to_string();
            if let Some(error) = reserved_rsjs_attribute_error(&raw_name, name_ident.span()) {
                push_err(&mut accumulated, error);
                continue;
            }

            // Node refs: `let input = node_ref::<HtmlInputElement>();`
            // paired with `<input ref={input}>`.
            if raw_name == "ref" {
                if !self.loop_scopes.is_empty() {
                    push_err(
                        &mut accumulated,
                        syn::Error::new(
                            name_ident.span(),
                            "`ref={...}` inside template `for` loops is not supported yet; \
                             bind the ref on a stable element outside the loop",
                        ),
                    );
                    continue;
                }
                let value = match attr {
                    Attribute::Dynamic { value, .. } => value,
                    _ => {
                        push_err(
                            &mut accumulated,
                            syn::Error::new(
                                name_ident.span(),
                                "`ref` requires a node-ref value: `ref={input_ref}`",
                            ),
                        );
                        continue;
                    }
                };
                match self.resolve_node_ref_path(value) {
                    Ok(node_ref) => {
                        if self
                            .builder
                            .bindings()
                            .node_ref_bindings
                            .values()
                            .any(|binding| binding.node_ref == node_ref)
                        {
                            push_err(
                                &mut accumulated,
                                syn::Error::new(
                                    value.span(),
                                    format!(
                                        "node ref `{}` is already bound to another element; \
                                         each `node_ref()` must target exactly one stable DOM \
                                         element, including across conditional branches",
                                        value.to_token_stream()
                                    ),
                                ),
                            );
                        } else {
                            self.builder.bind_node_ref(id, node_ref);
                            if let Err(error) = self.record_inbound_node_ref_binding(
                                node_ref,
                                el.tag_str().as_str(),
                                value.span(),
                            ) {
                                push_err(&mut accumulated, error);
                            }
                        }
                    }
                    Err(e) => push_err(&mut accumulated, e),
                }
                continue;
            }

            // on_* event handlers.
            if let Some(event) = match_event_name(&raw_name) {
                let value = match attr {
                    Attribute::Dynamic { value, .. } => value,
                    _ => {
                        push_err(
                            &mut accumulated,
                            syn::Error::new(
                                name_ident.span(),
                                format!(
                                    "`{}` requires a closure value: `{}={{|event| ...}}`",
                                    raw_name, raw_name
                                ),
                            ),
                        );
                        continue;
                    }
                };
                if let Err(error) =
                    self.require_client_origin(value.span(), ClientOriginKind::Handler)
                {
                    push_err(&mut accumulated, error);
                    continue;
                }
                match self.lower_handler(value, id) {
                    Ok(lowered) => {
                        let body_hir = lowered.body_hir;
                        let handler_id = match lowered.debounce_ms {
                            Some(ms) => self.builder.debounce_handler_from_ir(ms, lowered.body_ir),
                            None => self.builder.handler_from_ir(lowered.body_ir),
                        };
                        self.builder.set_handler_source(
                            handler_id,
                            lowered.source_name.or_else(|| Some(raw_name.clone())),
                            lowered
                                .source_anchor
                                .or_else(|| Some(source_anchor(value.span()))),
                        );
                        self.hir_sidecar.push_handler(handler_id, body_hir);
                        self.builder.bind_event(id, event, handler_id);
                    }
                    Err(e) => push_err(&mut accumulated, e),
                }
                continue;
            }

            if raw_name.starts_with("on_") {
                push_err(
                    &mut accumulated,
                    unsupported_event_attribute_error(name_ident.span(), &raw_name),
                );
                continue;
            }

            // bind_* input bindings.
            if let Some(kind) = match_input_kind(&raw_name) {
                if let Some(prev) = input_binding_seen {
                    let mut e = syn::Error::new(
                        name_ident.span(),
                        "an element may carry at most one `bind_*` attribute in v0.1",
                    );
                    e.combine(syn::Error::new(prev, "previous `bind_*` attribute here"));
                    push_err(&mut accumulated, e);
                    continue;
                }
                input_binding_seen = Some(name_ident.span());

                let value = match attr {
                    Attribute::Dynamic { value, .. } => value,
                    _ => {
                        push_err(
                            &mut accumulated,
                            syn::Error::new(
                                name_ident.span(),
                                format!(
                                    "`{}` requires a signal value: `{}={{my_signal}}`",
                                    raw_name, raw_name
                                ),
                            ),
                        );
                        continue;
                    }
                };
                // resolve_signal_path is the dependent step: do not
                // try to bind_input on failure. Two-way input bindings
                // write the signal from DOM input, so a memo cell
                // can never back one.
                match self.resolve_signal_path(value).and_then(|signal_id| {
                    self.reject_memo_write(
                        signal_id,
                        value.span(),
                        &format!("bind it with `{raw_name}`"),
                    )?;
                    Ok(signal_id)
                }) {
                    Ok(signal_id) => self.builder.bind_input(id, kind, signal_id),
                    Err(e) => push_err(&mut accumulated, e),
                }
                continue;
            }

            // Any other attribute is author-written HTML (class, id,
            // type, aria-*, data-*, ...). Static and boolean attrs
            // pass through unchanged. Dynamic attrs become one-way
            // client bindings when the attribute is safe to update
            // from the runtime.
            let html_name = raw_name.replace('_', "-");
            match attr {
                Attribute::Static { value, .. } => {
                    self.builder.static_attr(id, &html_name, &value.value());
                }
                Attribute::Bool { .. } => {
                    self.builder.static_attr(id, &html_name, "");
                }
                Attribute::Dynamic { value, .. } => {
                    if let Some(call) = server_attr_call(value) {
                        if call.args.len() != 1 {
                            push_err(
                                &mut accumulated,
                                syn::Error::new(
                                    call.span(),
                                    "`rsjs::server_attr(...)` takes exactly one server-rendered attribute value",
                                ),
                            );
                            continue;
                        }
                        let inner = call.args.first().expect("len checked above");
                        match self.reject_reactive_captures_in_opaque_boundary(
                            inner,
                            OpaqueBoundary::ServerAttr,
                        ) {
                            Ok(()) => {
                                self.builder.server_attr_with_source_anchor(
                                    id,
                                    &html_name,
                                    &value.to_token_stream().to_string(),
                                    self.explicit_server_boundary_source(inner),
                                    Some(server_surface_anchor(call.span())),
                                );
                            }
                            Err(e) => push_err(&mut accumulated, e),
                        }
                        continue;
                    }
                    if is_dynamic_attr_rejected_for_safety(&html_name) {
                        push_err(
                            &mut accumulated,
                            syn::Error::new(
                                name_ident.span(),
                                format!(
                                    "dynamic `{}` attributes are not supported inside `#[rsjs]` islands; \
                                     use a static value or wrap vetted server-rendered content in \
                                     `rsjs::server_attr(...)`",
                                    html_name
                                ),
                            ),
                        );
                        continue;
                    }
                    if matches!(&**value, Expr::Call(_)) && self.can_infer_opaque_server_expr(value)
                    {
                        self.builder.server_attr_with_source_anchor(
                            id,
                            &html_name,
                            &format!("rsjs::server_attr({})", value.to_token_stream()),
                            rsjs_ir::ServerSurfaceSource::InferredOpaqueSsr,
                            Some(server_surface_anchor(value.span())),
                        );
                        continue;
                    }
                    if self.is_simple_server_slot_expr(value) {
                        let tokens = value.to_token_stream().to_string();
                        self.builder.server_attr_with_source_anchor(
                            id,
                            &html_name,
                            &format!("rsjs::server_attr({tokens})"),
                            rsjs_ir::ServerSurfaceSource::InferredSimpleSsr,
                            Some(server_surface_anchor(value.span())),
                        );
                        continue;
                    }
                    if self.can_infer_static_loop_server_expr(value) {
                        self.builder.server_attr_with_source_anchor(
                            id,
                            &html_name,
                            &format!("rsjs::server_attr({})", value.to_token_stream()),
                            rsjs_ir::ServerSurfaceSource::InferredOpaqueSsr,
                            Some(server_surface_anchor(value.span())),
                        );
                        continue;
                    }
                    let attr_scope = ExprScope {
                        node_payload_target: (!self.loop_scopes.is_empty()).then_some(id),
                        ..ExprScope::default()
                    };
                    match self.lower_reactive_expr_in(value, &attr_scope) {
                        Ok(ir)
                            if !frontend_expr_has_client_dependency(&ir)
                                && self.can_infer_opaque_server_expr(value) =>
                        {
                            self.builder.server_attr_with_source_anchor(
                                id,
                                &html_name,
                                &format!("rsjs::server_attr({})", value.to_token_stream()),
                                rsjs_ir::ServerSurfaceSource::InferredOpaqueSsr,
                                Some(server_surface_anchor(value.span())),
                            );
                        }
                        Ok(ir) => {
                            let expr_id = self.alloc_expr(ir, value.span())?;
                            let kind = if is_html_boolean_attr(&html_name) {
                                AttrBindingKind::Boolean
                            } else if is_dynamic_url_attr(&html_name) {
                                AttrBindingKind::Url
                            } else {
                                AttrBindingKind::Value
                            };
                            self.builder.bind_attr(id, &html_name, kind, expr_id);
                        }
                        Err(_) if self.can_infer_opaque_server_expr(value) => {
                            // An arbitrary Rust expression is valid SSR until a
                            // browser obligation depends on it. Attributes do
                            // not change DOM cardinality, so retaining this as
                            // an opaque server attr is safe even when another
                            // part of the owner hydrates.
                            self.builder.server_attr_with_source_anchor(
                                id,
                                &html_name,
                                &format!("rsjs::server_attr({})", value.to_token_stream()),
                                rsjs_ir::ServerSurfaceSource::InferredOpaqueSsr,
                                Some(server_surface_anchor(value.span())),
                            );
                        }
                        Err(e) => push_err(&mut accumulated, e),
                    }
                }
            }
        }
        match accumulated {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Process an element's children. Errors are accumulated across
    /// siblings and combined via
    /// `syn::Error::combine` so a user with several broken children
    /// sees every diagnostic in one compile cycle.
    pub(super) fn process_children(
        &mut self,
        children: &[Node],
        parent: NodeId,
    ) -> syn::Result<()> {
        self.process_children_with_text_slot_mode(children, parent, false)
    }

    fn process_children_with_text_slot_mode(
        &mut self,
        children: &[Node],
        parent: NodeId,
        inherited_mixed_content: bool,
    ) -> syn::Result<()> {
        let mut accumulated: Option<syn::Error> = None;
        let push_err = |slot: &mut Option<syn::Error>, err: syn::Error| match slot {
            Some(existing) => existing.combine(err),
            None => *slot = Some(err),
        };
        // A binding may own the parent element's whole `textContent` only
        // when it is the element's sole child. In mixed content, give each
        // reactive expression its own carrier so SSR source order is
        // preserved and a later client update cannot erase static or sibling
        // children. Propagate the decision through transparent fragments.
        let mixed_content = inherited_mixed_content || children.len() != 1;

        for child in children {
            match child {
                Node::Text(lit) => {
                    // Author-written literal text inside an element
                    // (e.g. `"Submit"` in `<button>Submit</button>`).
                    // The analyser allocates a `ViewNode::StaticText`
                    // child so emit_rust can preserve the text in
                    // the generated `html_sync!` body.
                    self.builder.text_node_static(parent, &lit.value());
                }
                Node::Doctype(_) => {
                    // Islands never contain doctypes; just skip.
                }
                Node::Element(e) => {
                    if let Err(err) = self.walk_element(e, Some(parent)) {
                        push_err(&mut accumulated, err);
                    }
                }
                Node::Fragment(nodes) => {
                    if let Err(err) =
                        self.process_children_with_text_slot_mode(nodes, parent, mixed_content)
                    {
                        push_err(&mut accumulated, err);
                    }
                }
                Node::Block(expr) => {
                    if let Some(call) = island_call(expr) {
                        match self.lower_explicit_island(call, expr, parent) {
                            Ok(()) => {}
                            Err(err) => push_err(&mut accumulated, err),
                        }
                    } else if let Some(call) = server_only_call(expr) {
                        match self.lower_server_only(call, expr, parent) {
                            Ok(()) => {}
                            Err(err) => push_err(&mut accumulated, err),
                        }
                    } else if let Expr::Struct(provider) = expr {
                        if is_shared_state_provider_struct(provider) {
                            if let Err(err) =
                                self.lower_shared_state_provider(provider, expr, parent)
                            {
                                push_err(&mut accumulated, err);
                            }
                            continue;
                        }
                        let result = self
                            .record_direct_component_struct(
                                provider,
                                super::DirectComponentBoundary::Composed,
                            )
                            .and_then(|edge_id| {
                                if self.preserve_direct_component_sites {
                                    let server_component =
                                        self.direct_component_server_expression(provider, edge_id);
                                    let node = self.record_server_expression(
                                        parent,
                                        &server_component,
                                        rsjs_ir::ServerSurfaceSource::DirectComponentSite,
                                        Some(server_surface_anchor(expr.span())),
                                    );
                                    self.record_direct_component_site(edge_id, node);
                                    Ok(())
                                } else {
                                    self.inline_component_struct(provider, parent, edge_id)
                                }
                            });
                        if let Err(err) = result {
                            push_err(&mut accumulated, err);
                        }
                    } else if let Expr::Path(component) = expr {
                        if self.direct_path_is_component_edge(component) {
                            let result = self
                                .record_direct_component_path(
                                    component,
                                    super::DirectComponentBoundary::Composed,
                                )
                                .map(|edge_id| {
                                    let node = self.record_server_expression(
                                        parent,
                                        expr,
                                        rsjs_ir::ServerSurfaceSource::DirectComponentSite,
                                        Some(server_surface_anchor(expr.span())),
                                    );
                                    self.record_direct_component_site(edge_id, node);
                                });
                            if let Err(err) = result {
                                push_err(&mut accumulated, err);
                            }
                        } else if let Err(err) =
                            self.lower_text_binding_expr(expr, parent, mixed_content)
                        {
                            push_err(&mut accumulated, err);
                        }
                    } else if let Expr::If(if_expr) = expr {
                        if !if_expr_is_template_conditional(if_expr) {
                            if let Err(err) =
                                self.lower_text_binding_expr(expr, parent, mixed_content)
                            {
                                push_err(&mut accumulated, err);
                            }
                            continue;
                        }
                        if let Err(err) = self.lower_conditional(if_expr, parent) {
                            push_err(&mut accumulated, err);
                        }
                    } else if let Expr::ForLoop(for_loop) = expr {
                        if let Err(err) = self.lower_for_loop(for_loop, parent) {
                            push_err(&mut accumulated, err);
                        }
                    } else {
                        if let Err(err) = self.lower_text_binding_expr(expr, parent, mixed_content)
                        {
                            push_err(&mut accumulated, err);
                        }
                    }
                }
            }
        }
        match accumulated {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    pub(super) fn lower_text_binding_expr(
        &mut self,
        expr: &Expr,
        parent: NodeId,
        dedicated_text_slot: bool,
    ) -> syn::Result<()> {
        if self
            .generic_static_slot_field
            .as_deref()
            .is_some_and(|expected| exact_self_field_name(expr).as_deref() == Some(expected))
        {
            self.record_server_expression(
                parent,
                expr,
                rsjs_ir::ServerSurfaceSource::InferredSimpleSsr,
                Some(server_surface_anchor(expr.span())),
            );
            return Ok(());
        }
        if self.is_simple_server_slot_expr(expr) {
            self.record_server_expression(
                parent,
                expr,
                rsjs_ir::ServerSurfaceSource::InferredSimpleSsr,
                Some(server_surface_anchor(expr.span())),
            );
            return Ok(());
        }
        if matches!(expr, Expr::Call(_)) && self.can_infer_opaque_server_expr(expr) {
            // A call over server values remains ordinary Rust. In particular,
            // `render_markdown(self.document)` must not become a provisional
            // client-helper edge merely because an exact `self.field` would be
            // live when rendered directly. Authors who want a portable helper
            // over that prop can first bind the field as a live local.
            self.record_server_expression(
                parent,
                expr,
                rsjs_ir::ServerSurfaceSource::InferredOpaqueSsr,
                Some(server_surface_anchor(expr.span())),
            );
            return Ok(());
        }
        if self.can_infer_static_loop_server_expr(expr) {
            self.record_server_expression(
                parent,
                expr,
                rsjs_ir::ServerSurfaceSource::InferredOpaqueSsr,
                Some(server_surface_anchor(expr.span())),
            );
            return Ok(());
        }

        // Plain text binding: the expr's value is displayed as the
        // parent's text content.
        let text_scope = ExprScope {
            node_payload_target: (!self.loop_scopes.is_empty()).then_some(parent),
            ..ExprScope::default()
        };
        match self.lower_reactive_expr_in(expr, &text_scope) {
            Ok(ir)
                if !frontend_expr_has_client_dependency(&ir)
                    && self.can_infer_opaque_server_expr(expr) =>
            {
                self.record_server_expression(
                    parent,
                    expr,
                    rsjs_ir::ServerSurfaceSource::InferredOpaqueSsr,
                    Some(server_surface_anchor(expr.span())),
                );
                Ok(())
            }
            Ok(ir) => {
                let expr_id = self.alloc_expr(ir, expr.span())?;
                if dedicated_text_slot {
                    self.builder.text_node(parent, expr_id);
                } else {
                    self.builder.bind_text(parent, expr_id);
                }
                Ok(())
            }
            Err(_) if self.can_infer_opaque_server_expr(expr) => {
                // Keep arbitrary server Rust opaque in the parallel analysis
                // graph. A later reachability/span check decides whether this
                // unknown-cardinality node is safe in the final owner; the
                // parser must not reject it merely because it is not Client IR.
                self.record_server_expression(
                    parent,
                    expr,
                    rsjs_ir::ServerSurfaceSource::InferredOpaqueSsr,
                    Some(server_surface_anchor(expr.span())),
                );
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub(super) fn is_simple_server_slot_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Lit(_) => true,
            Expr::Path(ExprPath { path, .. }) => path.get_ident().is_some_and(|ident| {
                ident != "self"
                    && (self.is_loop_local_name(ident) || !self.is_reactive_local_name(ident))
            }),
            Expr::Field(field) => self.is_simple_server_field_base(field.base.as_ref()),
            Expr::Paren(paren) => self.is_simple_server_slot_expr(&paren.expr),
            Expr::Group(group) => self.is_simple_server_slot_expr(&group.expr),
            Expr::Reference(reference) => self.is_simple_server_slot_expr(&reference.expr),
            _ => false,
        }
    }

    fn is_simple_server_field_base(&self, expr: &Expr) -> bool {
        match expr {
            // A direct `self.field` inside an analyzed component is an inbound
            // value read. Keep it in Client IR so a directly composed parent
            // can wire a live value edge instead of silently snapshotting the
            // server render. Authors can retain a deliberate SSR-only field
            // with `rsjs::server_only(...)` / `rsjs::server_attr(...)`.
            Expr::Path(ExprPath { path, .. }) if path.is_ident("self") => false,
            Expr::Path(ExprPath { path, .. }) => path.get_ident().is_some_and(|ident| {
                self.is_loop_local_name(ident) || !self.is_reactive_local_name(ident)
            }),
            Expr::Field(field) => self.is_simple_server_field_base(field.base.as_ref()),
            Expr::Paren(paren) => self.is_simple_server_field_base(&paren.expr),
            Expr::Group(group) => self.is_simple_server_field_base(&group.expr),
            Expr::Reference(reference) => self.is_simple_server_field_base(&reference.expr),
            _ => false,
        }
    }

    fn explicit_server_boundary_source(&self, expr: &Expr) -> rsjs_ir::ServerSurfaceSource {
        if self.loop_scopes.is_empty() && self.is_simple_explicit_server_slot_expr(expr) {
            rsjs_ir::ServerSurfaceSource::ExplicitSimpleSsrBoundary
        } else {
            rsjs_ir::ServerSurfaceSource::ExplicitBoundary
        }
    }

    fn is_simple_explicit_server_slot_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Field(field) => self.is_simple_explicit_server_field_base(field.base.as_ref()),
            Expr::Paren(paren) => self.is_simple_explicit_server_slot_expr(&paren.expr),
            Expr::Group(group) => self.is_simple_explicit_server_slot_expr(&group.expr),
            Expr::Reference(reference) => self.is_simple_explicit_server_slot_expr(&reference.expr),
            _ => self.is_simple_server_slot_expr(expr),
        }
    }

    fn is_simple_explicit_server_field_base(&self, expr: &Expr) -> bool {
        match expr {
            // The explicit wrapper is the author's deliberate snapshot
            // boundary. Unlike an unwrapped `self.field` LiveRead, this direct
            // projection is therefore a simple SSR value.
            Expr::Path(ExprPath { path, .. }) if path.is_ident("self") => true,
            Expr::Path(ExprPath { path, .. }) => path
                .get_ident()
                .is_some_and(|ident| !self.is_reactive_local_name(ident)),
            Expr::Field(field) => self.is_simple_explicit_server_field_base(field.base.as_ref()),
            Expr::Paren(paren) => self.is_simple_explicit_server_field_base(&paren.expr),
            Expr::Group(group) => self.is_simple_explicit_server_field_base(&group.expr),
            Expr::Reference(reference) => {
                self.is_simple_explicit_server_field_base(&reference.expr)
            }
            _ => false,
        }
    }

    pub(super) fn is_reactive_local_name(&self, ident: &syn::Ident) -> bool {
        let name = ident.to_string();
        self.signals.contains_key(&name)
            || self.queries.contains_key(&name)
            || self.mutations.contains_key(&name)
            || self.node_refs.contains_key(&name)
            || self.handlers.contains_key(&name)
            || self.client_prelude_locals.contains_key(&name)
            || self.unsupported_client_prelude_locals.contains_key(&name)
    }

    pub(super) fn is_loop_local_name(&self, ident: &syn::Ident) -> bool {
        let name = ident.to_string();
        self.loop_scopes
            .iter()
            .rev()
            .any(|scope| scope.locals.contains(&name))
    }

    fn current_loop_local_names(&self) -> HashSet<String> {
        self.loop_scopes
            .iter()
            .flat_map(|scope| scope.locals.iter().cloned())
            .collect()
    }

    fn can_infer_static_loop_server_expr(&self, expr: &Expr) -> bool {
        !self.loop_scopes.is_empty()
            && self.expr_mentions_loop_local(expr)
            && self.can_infer_opaque_server_expr(expr)
    }

    /// Return whether `expr` can remain an opaque server node.
    ///
    /// This is intentionally a negative, syntax-only proof. It does not claim
    /// to know the expression's type, purity, return shape, or callee body. It
    /// only proves that the visible syntax does not capture a compiler-known
    /// client local and does not contain a compiler-known client primitive or
    /// browser helper. Such an expression stays ordinary Rust for SSR.
    pub(super) fn can_infer_opaque_server_expr(&self, expr: &Expr) -> bool {
        // Closures are callable values, not renderable server fragments. Keep
        // the focused RSJS diagnostic instead of deferring this syntactically
        // obvious mistake to a broad WebRenderSync trait error.
        if matches!(expr, Expr::Closure(_)) {
            return false;
        }
        if let Expr::MethodCall(method) = expr {
            if method.method == "get"
                && method.args.is_empty()
                && method.turbofish.is_none()
                && matches!(
                    method.receiver.as_ref(),
                    Expr::Path(path)
                        if path.path.get_ident().is_some_and(|ident| {
                            let name = ident.to_string();
                            !self.server_prelude_locals.contains(&name)
                                && !self.is_reactive_local_name(ident)
                        })
                )
            {
                // Preserve the focused "unknown signal" diagnostic. A
                // declared ordinary server local named above remains free to
                // expose its own unrelated `.get()` method.
                return false;
            }
        }
        if expression_mentions_known_client_local_with_shadowed(
            expr,
            &self.signals,
            &self.queries,
            &self.mutations,
            &self.node_refs,
            &self.handlers,
            &self.client_prelude_locals,
            &self.unsupported_client_prelude_locals,
            self.current_loop_local_names(),
        ) {
            return false;
        }

        self.reject_reactive_captures_in_opaque_boundary(expr, OpaqueBoundary::ServerOnly)
            .is_ok()
    }
}

#[cfg(test)]
pub(super) fn expression_mentions_known_client_local(
    expr: &Expr,
    signals: &HashMap<String, SignalId>,
    queries: &HashMap<String, QueryId>,
    mutations: &HashMap<String, MutationId>,
    node_refs: &HashMap<String, NodeRefId>,
    handlers: &HashMap<String, HandlerLocal>,
    client_prelude_locals: &HashMap<String, FrontendExpr>,
    unsupported_client_prelude_locals: &HashMap<String, UnsupportedClientPreludeLocal>,
) -> bool {
    expression_mentions_known_client_local_with_shadowed(
        expr,
        signals,
        queries,
        mutations,
        node_refs,
        handlers,
        client_prelude_locals,
        unsupported_client_prelude_locals,
        HashSet::new(),
    )
}

fn expression_mentions_known_client_local_with_shadowed(
    expr: &Expr,
    signals: &HashMap<String, SignalId>,
    queries: &HashMap<String, QueryId>,
    mutations: &HashMap<String, MutationId>,
    node_refs: &HashMap<String, NodeRefId>,
    handlers: &HashMap<String, HandlerLocal>,
    client_prelude_locals: &HashMap<String, FrontendExpr>,
    unsupported_client_prelude_locals: &HashMap<String, UnsupportedClientPreludeLocal>,
    shadowed_names: HashSet<String>,
) -> bool {
    struct Finder<'a> {
        signals: &'a HashMap<String, SignalId>,
        queries: &'a HashMap<String, QueryId>,
        mutations: &'a HashMap<String, MutationId>,
        node_refs: &'a HashMap<String, NodeRefId>,
        handlers: &'a HashMap<String, HandlerLocal>,
        client_prelude_locals: &'a HashMap<String, FrontendExpr>,
        unsupported_client_prelude_locals: &'a HashMap<String, UnsupportedClientPreludeLocal>,
        shadowed_names: HashSet<String>,
        found: bool,
    }

    impl Finder<'_> {
        fn is_known_and_visible(&self, name: &str) -> bool {
            !self.shadowed_names.contains(name)
                && (self.signals.contains_key(name)
                    || self.queries.contains_key(name)
                    || self.mutations.contains_key(name)
                    || self.node_refs.contains_key(name)
                    || self.handlers.contains_key(name)
                    || self.client_prelude_locals.contains_key(name)
                    || self.unsupported_client_prelude_locals.contains_key(name))
        }

        fn with_pattern_scope(&mut self, pat: &Pat, visit: impl FnOnce(&mut Self)) {
            let saved = self.shadowed_names.clone();
            self.shadowed_names.extend(pattern_binding_names(pat));
            visit(self);
            self.shadowed_names = saved;
        }
    }

    impl<'ast> Visit<'ast> for Finder<'_> {
        fn visit_expr_path(&mut self, path: &'ast ExprPath) {
            if let Some(ident) = path.path.get_ident() {
                let name = ident.to_string();
                self.found |= self.is_known_and_visible(&name);
            }
            if !self.found {
                visit::visit_expr_path(self, path);
            }
        }

        fn visit_block(&mut self, block: &'ast Block) {
            let saved = self.shadowed_names.clone();
            for statement in &block.stmts {
                if self.found {
                    break;
                }
                if let Stmt::Local(local) = statement {
                    if let Some(init) = &local.init {
                        self.visit_expr(&init.expr);
                        if let Some((_, diverge)) = &init.diverge {
                            self.visit_expr(diverge);
                        }
                    }
                    self.shadowed_names
                        .extend(pattern_binding_names(&local.pat));
                } else {
                    self.visit_stmt(statement);
                }
            }
            self.shadowed_names = saved;
        }

        fn visit_expr_closure(&mut self, closure: &'ast ExprClosure) {
            let saved = self.shadowed_names.clone();
            for input in &closure.inputs {
                self.shadowed_names.extend(pattern_binding_names(input));
            }
            self.visit_expr(&closure.body);
            self.shadowed_names = saved;
        }

        fn visit_expr_for_loop(&mut self, for_loop: &'ast ExprForLoop) {
            self.visit_expr(&for_loop.expr);
            self.with_pattern_scope(&for_loop.pat, |this| this.visit_block(&for_loop.body));
        }

        fn visit_arm(&mut self, arm: &'ast syn::Arm) {
            self.with_pattern_scope(&arm.pat, |this| {
                if let Some((_, guard)) = &arm.guard {
                    this.visit_expr(guard);
                }
                this.visit_expr(&arm.body);
            });
        }

        fn visit_expr_if(&mut self, if_expr: &'ast ExprIf) {
            self.visit_expr(&if_expr.cond);
            let mut bindings = Vec::new();
            collect_condition_pattern_bindings(&if_expr.cond, &mut bindings);
            let saved = self.shadowed_names.clone();
            self.shadowed_names.extend(bindings);
            self.visit_block(&if_expr.then_branch);
            self.shadowed_names = saved;
            if let Some((_, else_expr)) = &if_expr.else_branch {
                self.visit_expr(else_expr);
            }
        }

        fn visit_expr_while(&mut self, while_expr: &'ast syn::ExprWhile) {
            self.visit_expr(&while_expr.cond);
            let mut bindings = Vec::new();
            collect_condition_pattern_bindings(&while_expr.cond, &mut bindings);
            let saved = self.shadowed_names.clone();
            self.shadowed_names.extend(bindings);
            self.visit_block(&while_expr.body);
            self.shadowed_names = saved;
        }

        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let saved = self.shadowed_names.clone();
            for input in &item.sig.inputs {
                if let syn::FnArg::Typed(input) = input {
                    self.shadowed_names
                        .extend(pattern_binding_names(&input.pat));
                }
            }
            self.visit_block(&item.block);
            self.shadowed_names = saved;
        }
    }

    let mut finder = Finder {
        signals,
        queries,
        mutations,
        node_refs,
        handlers,
        client_prelude_locals,
        unsupported_client_prelude_locals,
        shadowed_names,
        found: false,
    };
    finder.visit_expr(expr);
    finder.found
}

fn source_anchor(span: proc_macro2::Span) -> rsjs_ir::SourceAnchor {
    let start = span.start();
    rsjs_ir::SourceAnchor {
        start_line: start.line,
        start_column: start.column + 1,
    }
}

pub(super) fn server_surface_anchor(span: proc_macro2::Span) -> rsjs_ir::ServerSurfaceAnchor {
    let start = span.start();
    rsjs_ir::ServerSurfaceAnchor {
        start_line: start.line,
        start_column: start.column + 1,
    }
}

impl Analyzer {
    pub(super) fn lower_server_only(
        &mut self,
        call: &ExprCall,
        full_expr: &Expr,
        parent: NodeId,
    ) -> syn::Result<()> {
        if call.args.len() != 1 {
            return Err(syn::Error::new(
                call.span(),
                "`rsjs::server_only(...)` takes exactly one server-rendered child",
            ));
        }
        let inner = call.args.first().expect("len checked above");
        self.reject_reactive_captures_in_opaque_boundary(inner, OpaqueBoundary::ServerOnly)?;
        let source = self.explicit_server_boundary_source(inner);
        self.record_server_expression(
            parent,
            full_expr,
            source,
            Some(server_surface_anchor(call.span())),
        );
        Ok(())
    }

    pub(super) fn lower_explicit_island(
        &mut self,
        call: &ExprCall,
        full_expr: &Expr,
        parent: NodeId,
    ) -> syn::Result<()> {
        if call.args.len() != 1 {
            return Err(syn::Error::new(
                call.span(),
                "`rsjs::island(...)` takes exactly one child island",
            ));
        }
        let inner = call.args.first().expect("len checked above");
        let component_edge = match inner {
            Expr::Struct(component) => Some(self.record_direct_component_struct(
                component,
                super::DirectComponentBoundary::ExplicitIsland,
            )?),
            Expr::Path(component) if self.direct_path_is_component_edge(component) => {
                Some(self.record_direct_component_path(
                    component,
                    super::DirectComponentBoundary::ExplicitIsland,
                )?)
            }
            _ => None,
        };
        let mut inbound_fields = InboundSelfFieldVisitor::default();
        inbound_fields.visit_expr(inner);
        for (field_name, field_span) in inbound_fields.fields {
            // This remains a latent rejection: a static standalone render may
            // use the Rust value normally, but a linker must never turn a live
            // parent input at this site into an implicit independent-owner
            // snapshot.
            self.record_inbound_boundary_crossing(field_name, field_span)?;
        }
        self.reject_reactive_captures_in_opaque_boundary(inner, OpaqueBoundary::Island)?;
        let node = self.record_server_expression(
            parent,
            full_expr,
            rsjs_ir::ServerSurfaceSource::ExplicitIslandBoundary,
            Some(server_surface_anchor(call.span())),
        );
        if let Some(edge) = component_edge {
            self.record_direct_component_site(edge, node);
        }
        Ok(())
    }

    /// Preserve `rsjs::SharedStateProvider { state, children }` as ordinary
    /// wrapperless server rendering while retaining its temporal contract.
    ///
    /// The declaration establishes lexical provider topology. The provider
    /// itself is not a client origin; its one direct child is retained as a
    /// normal rustc-resolved component edge. This frontend must not allocate a
    /// signal, owner, seed, or hydration obligation merely because the
    /// provider appears in an analyzed template. Direct `self.<field>` uses in
    /// `state` remain normal Rust for a static caller, but become latent
    /// `ForbiddenProviderSeed` prop-use sites so a canonical linker rejects a
    /// live caller instead of silently sampling it.
    pub(super) fn lower_shared_state_provider(
        &mut self,
        provider: &ExprStruct,
        full_expr: &Expr,
        parent: NodeId,
    ) -> syn::Result<()> {
        if !self.loop_scopes.is_empty() || !self.component_lifecycle_stack.is_empty() {
            return Err(guided_boundary_error(
                provider.path.span(),
                "a v1 `SharedStateProvider` must be statically present",
                "Provider occurrence identity currently names one unconditional lexical site. A \
                 provider inside a loop or reactive conditional could create, remove, or repeat \
                 response-local scopes without a keyed lifecycle identity.",
                [
                    "Move the provider outside the loop or conditional.",
                    "Keep conditional content inside the provider's direct child component.",
                    "Wait for keyed/dynamic provider occurrence identity before making provider topology data-dependent.",
                ],
                Some("shared_state.provider_dynamic_topology"),
            ));
        }

        let (state, children) = shared_state_provider_fields(provider)?;
        let Expr::Struct(state) = direct_binding_inner(state) else {
            return Err(shared_state_provider_state_shape_error(state.span()));
        };
        if is_shared_state_provider_struct(state) {
            return Err(shared_state_provider_state_shape_error(state.span()));
        }
        let child = self.validate_shared_state_provider_children(children)?;
        self.reject_reactive_captures_in_opaque_boundary(
            &Expr::Struct(state.clone()),
            OpaqueBoundary::SharedStateProviderSeed,
        )?;

        let mut inbound_fields = InboundSelfFieldVisitor::default();
        inbound_fields.visit_expr_struct(state);
        for (field_name, field_span) in inbound_fields.fields {
            self.record_inbound_provider_seed(field_name, field_span)?;
        }

        // The descendant remains an ordinary rustc-resolved composed edge.
        // Its view site is the provider's whole server node rather than a
        // separately rendered child node: final lowering must preserve the
        // wrapperless provider expression and replace only `children`.
        let child_edge =
            self.record_direct_component_struct(child, super::DirectComponentBoundary::Composed)?;
        let site_node = self.record_server_expression(
            parent,
            full_expr,
            rsjs_ir::ServerSurfaceSource::DirectComponentSite,
            Some(server_surface_anchor(provider.path.span())),
        );
        self.record_shared_state_provider_child_site(child_edge, site_node);

        let provider_index =
            u32::try_from(self.shared_state_provider_sites.len()).map_err(|_| {
                syn::Error::new(
                    provider.path.span(),
                    "an analyzed RSJS component cannot contain more than 2^32 SharedState provider sites",
                )
            })?;
        self.shared_state_provider_sites
            .push(super::DirectSharedStateProviderSite {
                id: rsjs_ir::abi::ProviderSiteId::new(provider_index),
                state_ty: state.path.clone(),
                child_edge,
                site_node,
                source_anchor: source_anchor(provider.path.span()),
            });
        Ok(())
    }

    fn validate_shared_state_provider_children<'a>(
        &self,
        children: &'a Expr,
    ) -> syn::Result<&'a ExprStruct> {
        let Expr::Struct(child) = direct_binding_inner(children) else {
            return Err(shared_state_provider_children_shape_error(children.span()));
        };
        if is_shared_state_provider_struct(child) || child.rest.is_some() {
            return Err(shared_state_provider_children_shape_error(child.span()));
        }

        for field in &child.fields {
            let mut macros = ExprMacroPresence::default();
            macros.visit_expr(&field.expr);
            if let Some(span) = macros.first {
                return Err(guided_boundary_error(
                    span,
                    "a v1 `SharedStateProvider` child field cannot contain an opaque macro",
                    "The provider itself is server topology, so its descendant must remain \
                     independently visible to the RSJS compiler. An arbitrary nested macro could \
                     hide a live prop, handler, or separately owned descendant.",
                    [
                        "Precompute static server data before the template and pass the resulting value.",
                        "Use one direct component child with ordinary Rust field expressions.",
                        "Move richer child composition into the direct child component's own analyzed render body.",
                    ],
                    Some("shared_state.provider_children_opaque_macro"),
                ));
            }
            if !self.can_infer_opaque_server_expr(&field.expr) {
                return Err(guided_boundary_error(
                    field.expr.span(),
                    "a v1 `SharedStateProvider` child cannot capture enclosing client state",
                    "The provider's child edge cannot carry live inputs from the enclosing owner. \
                     Capturing a signal, handler, query, mutation, SharedState handle, node ref, \
                     or client-derived local here would silently turn a live edge into a server \
                     snapshot.",
                    [
                        "Pass static server data into the child.",
                        "Move the provider outside the component that owns the client value.",
                        "Acquire SharedState inside the child when cross-owner synchronization is intended.",
                    ],
                    Some("shared_state.provider_children_client_dependency"),
                ));
            }

            let mut inbound_fields = InboundSelfFieldVisitor::default();
            inbound_fields.visit_expr(&field.expr);
            if let Some((field_name, field_span)) = inbound_fields.fields.first() {
                return Err(guided_boundary_error(
                    *field_span,
                    format!(
                        "a v1 `SharedStateProvider` child cannot capture inbound field \
                         `self.{field_name}`"
                    ),
                    "The provider's direct child edge cannot carry an inbound value through the \
                     provider boundary. Allowing one here could snapshot a live caller without \
                     an ongoing graph edge.",
                    [
                        "Pass only static server data into the provider's direct child.",
                        "Move this value into SharedState and reacquire it inside the child.",
                        "Render the child outside the provider when it needs direct live composition.",
                    ],
                    Some("shared_state.provider_children_inbound_value"),
                ));
            }
        }
        Ok(child)
    }

    /// Retain a direct struct literal as a Rust-resolved component edge.
    ///
    /// Source order supplies a dense component-local id. The generated
    /// relocation record later hashes that id with the containing component's
    /// input fingerprint, while the actual child identity comes only from
    /// rustc resolving `component.path` through `ComponentAbi::WITNESS`.
    pub(super) fn record_direct_component_struct(
        &mut self,
        component: &ExprStruct,
        boundary: super::DirectComponentBoundary,
    ) -> syn::Result<u32> {
        let local_id = u32::try_from(self.direct_component_edges.len()).map_err(|_| {
            syn::Error::new(
                component.path.span(),
                "an analyzed RSJS component cannot contain more than 2^32 direct component edges",
            )
        })?;
        let fields = component
            .fields
            .iter()
            .map(|field| {
                Ok(super::DirectComponentSourceField {
                    name: field.member.to_token_stream().to_string(),
                    binding: self.classify_direct_component_binding(&field.expr, boundary)?,
                })
            })
            .collect::<syn::Result<Vec<_>>>()?;
        let lifecycle = self.direct_component_lifecycle();
        if fields.iter().any(|field| {
            matches!(
                field.binding,
                super::DirectComponentBindingClassification::NodeRefHandle { .. }
            )
        }) && !matches!(lifecycle, super::DirectComponentLifecycle::StaticSingle)
        {
            return Err(syn::Error::new(
                component.path.span(),
                "rsjs/node-ref-handle-static-only: a NodeRef<T> handle may cross only a \
                 statically present, direct component edge; move this component outside the \
                 conditional/list or keep the ref and its binding in the same component",
            ));
        }
        self.direct_component_edges.push(DirectComponentSourceEdge {
            local_id,
            component_ty: component.path.clone(),
            fields,
            has_rest: component.rest.is_some(),
            boundary,
            lifecycle,
            site_node: None,
            source_anchor: source_anchor(component.path.span()),
        });
        Ok(local_id)
    }

    /// Retain an imported, renamed, re-exported, or qualified unit component
    /// as the same rustc-resolved edge used by direct struct literals.
    ///
    /// A bare path that names a prelude local keeps ordinary value semantics;
    /// every other direct value path is required to prove `ComponentAbi` in
    /// generated Rust. This deliberately makes consts, statics, and function
    /// items fail with the "bind it to a local" remedy instead of asking the
    /// proc macro to guess their resolved kind.
    fn record_direct_component_path(
        &mut self,
        component: &ExprPath,
        boundary: super::DirectComponentBoundary,
    ) -> syn::Result<u32> {
        let local_id = u32::try_from(self.direct_component_edges.len()).map_err(|_| {
            syn::Error::new(
                component.path.span(),
                "an analyzed RSJS component cannot contain more than 2^32 direct component edges",
            )
        })?;
        self.direct_component_edges.push(DirectComponentSourceEdge {
            local_id,
            component_ty: component.path.clone(),
            fields: Vec::new(),
            has_rest: false,
            boundary,
            lifecycle: self.direct_component_lifecycle(),
            site_node: None,
            source_anchor: source_anchor(component.path.span()),
        });
        Ok(local_id)
    }

    fn direct_component_lifecycle(&self) -> super::DirectComponentLifecycle {
        if !self.loop_scopes.is_empty() {
            return super::DirectComponentLifecycle::Repeated;
        }
        match self.component_lifecycle_stack.as_slice() {
            [] => super::DirectComponentLifecycle::StaticSingle,
            [scope] => super::DirectComponentLifecycle::ConditionalSingle(*scope),
            [_, _, ..] => super::DirectComponentLifecycle::UnknownNestedConditionals,
        }
    }

    pub(super) fn record_direct_component_site(&mut self, edge_id: u32, node: NodeId) {
        let edge = self
            .direct_component_edges
            .get_mut(edge_id as usize)
            .expect("a direct component edge must exist before its view site");
        debug_assert!(
            edge.site_node.is_none(),
            "one direct component syntax edge cannot own multiple view sites"
        );
        edge.site_node = Some(node);
        let previous = self.direct_component_site_nodes.insert(node, edge_id);
        debug_assert!(
            previous.is_none(),
            "one schema-v9 view node cannot represent multiple component edges"
        );
    }

    /// Bind a provider's topology-only child edge to the whole provider node
    /// without registering that node for ordinary `LinkedComponent` wrapping.
    ///
    /// Final provider-aware server lowering must replace only the provider's
    /// `children` field. Treating this node as an ordinary direct component
    /// site would wrap the `SharedStateProvider` value itself with the child
    /// component plan and erase the provider's lexical scope semantics.
    fn record_shared_state_provider_child_site(&mut self, edge_id: u32, node: NodeId) {
        let edge = self
            .direct_component_edges
            .get_mut(edge_id as usize)
            .expect("a provider child edge must exist before its view site");
        debug_assert!(
            edge.site_node.is_none(),
            "one provider child syntax edge cannot own multiple view sites"
        );
        edge.site_node = Some(node);
    }

    fn direct_path_is_component_edge(&self, component: &ExprPath) -> bool {
        if component.path.is_ident("self") {
            return false;
        }
        component.path.get_ident().is_none_or(|ident| {
            let name = ident.to_string();
            !self.server_prelude_locals.contains(&name)
                && !self
                    .loop_scopes
                    .iter()
                    .any(|scope| scope.locals.contains(&name))
        })
    }

    /// Build the ordinary SSR value for one canonical direct-component edge.
    ///
    /// Compiler-owned live values and callbacks do not survive as authored
    /// Rust locals in the server program. The edge transport retains their
    /// client semantics; the server expression receives only the signal's
    /// already-evaluated initial snapshot and a correctly typed no-op callback
    /// container. This keeps descriptor compilation type-correct without
    /// executing or copying any child client graph.
    fn direct_component_server_expression(&self, component: &ExprStruct, edge_id: u32) -> Expr {
        let edge = self
            .direct_component_edges
            .get(edge_id as usize)
            .expect("a direct component edge must exist before its server expression");
        let mut component = component.clone();
        for (field, source) in component.fields.iter_mut().zip(&edge.fields) {
            field.expr = self.rewrite_direct_component_server_reads(field.expr.clone());
            match source.binding {
                super::DirectComponentBindingClassification::LiveValue {
                    signal,
                    source: super::DirectLiveValueSource::InboundField,
                } => {
                    let initial = quote::format_ident!("__rsjs_initial_signal_{}", signal.0);
                    field.colon_token.get_or_insert_with(Default::default);
                    // Preserve the ownership semantics of the authored
                    // `field: self.field` expression. The inbound Rust value
                    // is evaluated once into the synthetic LiveRead local and
                    // moved into this child exactly once; unlike a local
                    // signal `.get()`, pass-through does not introduce an
                    // implicit `Clone` requirement.
                    field.expr = syn::parse_quote! {
                        #initial
                    };
                }
                super::DirectComponentBindingClassification::LiveValue { signal, .. } => {
                    let initial = quote::format_ident!("__rsjs_initial_signal_{}", signal.0);
                    field.colon_token.get_or_insert_with(Default::default);
                    field.expr = syn::parse_quote! {
                        ::core::clone::Clone::clone(&#initial)
                    };
                }
                super::DirectComponentBindingClassification::LiveCallback {
                    form: super::DirectLiveCallbackForm::Handler,
                    ..
                } => {
                    field.colon_token.get_or_insert_with(Default::default);
                    field.expr = syn::parse_quote! {
                        ::rsjs::noop_handler()
                    };
                }
                super::DirectComponentBindingClassification::LiveCallback {
                    form: super::DirectLiveCallbackForm::SomeHandler,
                    ..
                } => {
                    field.colon_token.get_or_insert_with(Default::default);
                    field.expr = syn::parse_quote! {
                        ::core::option::Option::Some(::rsjs::noop_handler())
                    };
                }
                super::DirectComponentBindingClassification::DeferredProjection {
                    signal, ..
                } => {
                    let initial = quote::format_ident!("__rsjs_initial_signal_{}", signal.0);
                    field.colon_token.get_or_insert_with(Default::default);
                    // Evaluate the authored projection exactly once, then let
                    // this ordinary Rust assignment prove compatibility with
                    // the rustc-resolved child field.
                    field.expr = syn::parse_quote! {
                        #initial
                    };
                }
                _ => {}
            }
        }
        Expr::Struct(component)
    }

    /// Replace compiler-owned signal reads inside an otherwise unsupported
    /// field expression with the corresponding server snapshot.
    ///
    /// Canonical transport remains fail-closed (`Unsupported` stays
    /// unsupported); this rewrite only lets ordinary Rust construct the child
    /// during descriptor/local SSR so linker diagnostics are not masked by a
    /// missing compiler-owned local.
    fn rewrite_direct_component_server_reads(&self, expr: Expr) -> Expr {
        struct Rewriter<'a> {
            signals: &'a HashMap<String, SignalId>,
            aliases: &'a HashMap<String, FrontendExpr>,
        }

        impl Rewriter<'_> {
            fn snapshot(signal: SignalId) -> Expr {
                let initial = quote::format_ident!("__rsjs_initial_signal_{}", signal.0);
                syn::parse_quote! {
                    ::core::clone::Clone::clone(&#initial)
                }
            }
        }

        impl Fold for Rewriter<'_> {
            fn fold_expr(&mut self, expr: Expr) -> Expr {
                if let Expr::MethodCall(method) = &expr {
                    if method.method == "get"
                        && method.args.is_empty()
                        && method.turbofish.is_none()
                    {
                        if let Expr::Path(path) = method.receiver.as_ref() {
                            if let Some(ident) = path.path.get_ident() {
                                if let Some(signal) = self.signals.get(&ident.to_string()).copied()
                                {
                                    return Self::snapshot(signal);
                                }
                            }
                        }
                    }
                }
                if let Expr::Path(path) = &expr {
                    if let Some(ident) = path.path.get_ident() {
                        if let Some(FrontendExpr::SignalRead(signal)) =
                            self.aliases.get(&ident.to_string())
                        {
                            return Self::snapshot(*signal);
                        }
                    }
                }
                fold::fold_expr(self, expr)
            }
        }

        Rewriter {
            signals: &self.signals,
            aliases: &self.client_prelude_locals,
        }
        .fold_expr(expr)
    }

    fn classify_direct_component_binding(
        &mut self,
        expr: &Expr,
        boundary: super::DirectComponentBoundary,
    ) -> syn::Result<super::DirectComponentBindingClassification> {
        if let Some(node_ref) = self.exact_direct_node_ref_source(expr)? {
            if matches!(boundary, super::DirectComponentBoundary::ExplicitIsland) {
                // Preserve the ordinary explicit-island capture diagnostic,
                // which names the exact parent handle. The retained
                // Unsupported classification still fails closed if this
                // lower-level recorder is exercised without the boundary
                // capture pass.
                return Ok(super::DirectComponentBindingClassification::Unsupported);
            }
            return Ok(super::DirectComponentBindingClassification::NodeRefHandle { node_ref });
        }
        if let Expr::Call(call) = direct_binding_inner(expr) {
            if is_some_call(&call.func)
                && call.args.len() == 1
                && self.exact_direct_node_ref_source(&call.args[0])?.is_some()
            {
                return Err(syn::Error::new(
                    expr.span(),
                    "rsjs/node-ref-handle-required: optional NodeRef<T> component fields are \
                     unsupported; use a required NodeRef<T> field on one statically present \
                     direct component edge",
                ));
            }
        }
        if let Some(binding) = self.classify_direct_callback_binding(expr) {
            if matches!(boundary, super::DirectComponentBoundary::ExplicitIsland)
                && matches!(
                    binding,
                    super::DirectComponentBindingClassification::LiveCallback { .. }
                )
            {
                return Ok(super::DirectComponentBindingClassification::Unsupported);
            }
            return Ok(binding);
        }
        if matches!(boundary, super::DirectComponentBoundary::ExplicitIsland)
            && direct_inbound_self_field(expr).is_some()
        {
            // `lower_explicit_island` records the latent
            // `ForbiddenBoundaryCrossing` after retaining the component edge.
            // At this concrete edge the authored Rust value is still a valid
            // server input: a static caller may render it normally and the
            // independent child can serialize its own initial client seed.
            // If an outer component later supplies this field through a live
            // value edge, the retained temporal site rejects that graph
            // instead of silently snapshotting it across the owner cut.
            //
            // Do not allocate a live alias here. The binding remains
            // server-only at the explicit edge, while the separate inbound
            // temporal contract carries the conditional rejection.
            return Ok(super::DirectComponentBindingClassification::ServerOnly);
        }
        if matches!(boundary, super::DirectComponentBoundary::Composed) {
            if let Some(field) = direct_inbound_self_field(expr) {
                let projection = self.ensure_deferred_inbound_projection(field)?;
                return Ok(
                    super::DirectComponentBindingClassification::DeferredProjection {
                        field: projection.field,
                        signal: projection.signal,
                        node_ref: projection.node_ref,
                    },
                );
            }
        }
        if let Some((signal, source)) = self.exact_direct_live_signal_source(expr)? {
            if matches!(boundary, super::DirectComponentBoundary::ExplicitIsland) {
                return Ok(super::DirectComponentBindingClassification::Unsupported);
            }
            return Ok(super::DirectComponentBindingClassification::LiveValue { signal, source });
        }
        if matches!(boundary, super::DirectComponentBoundary::ExplicitIsland)
            && immutable_static_slot_exact_one(expr)
        {
            return Ok(super::DirectComponentBindingClassification::StaticSlotExactOne);
        }
        if matches!(boundary, super::DirectComponentBoundary::ExplicitIsland)
            && matches!(
                expr,
                Expr::Macro(expr_macro) if is_html_macro_path(&expr_macro.mac.path)
            )
        {
            // Syn's ordinary expression visitor cannot inspect another
            // macro's token stream. A nested HTML macro that missed the exact
            // static grammar above must therefore fail closed instead of
            // inheriting the generic opaque-expression classification.
            return Ok(super::DirectComponentBindingClassification::Unsupported);
        }
        if matches!(boundary, super::DirectComponentBoundary::ExplicitIsland)
            && !self.is_simple_server_slot_expr(expr)
        {
            // An independently owned child may receive a proven scalar SSR
            // value or the exact-one static HTML slot recognized above. An
            // arbitrary renderable expression has unknown DOM cardinality and
            // may register a nested owner, so opacity alone is not enough to
            // let it cross the boundary.
            return Ok(super::DirectComponentBindingClassification::Unsupported);
        }

        if self.can_infer_opaque_server_expr(expr) {
            Ok(super::DirectComponentBindingClassification::ServerOnly)
        } else {
            Ok(super::DirectComponentBindingClassification::Unsupported)
        }
    }

    /// Resolve the exact node-ref object passed through a direct component
    /// field. Optional/container-wrapped handles intentionally do not match.
    fn exact_direct_node_ref_source(&mut self, expr: &Expr) -> syn::Result<Option<NodeRefId>> {
        match direct_binding_inner(expr) {
            Expr::Path(path) => Ok(path
                .path
                .get_ident()
                .and_then(|ident| self.node_refs.get(&ident.to_string()).copied())),
            Expr::Field(field)
                if matches!(
                    field.base.as_ref(),
                    Expr::Path(path) if path.path.is_ident("self")
                ) =>
            {
                let syn::Member::Named(member) = &field.member else {
                    return Ok(None);
                };
                // A direct `self.field` projection is type-ambiguous to the
                // proc macro. It becomes a NodeRef alias only after an actual
                // node-ref context (`ref={self.field}`, a recognized ref
                // accessor/effect, or another compiler proof) has registered
                // the field. Otherwise preserve the established inbound
                // scalar/live-prop classification.
                Ok(self.inbound_node_refs.get(&member.to_string()).copied())
            }
            _ => Ok(None),
        }
    }

    /// Classify only compiler-known callback syntax.
    ///
    /// A bare `None` is necessarily deferred to the child schema: without
    /// rustc type information it could also be an ordinary `Option<T>` server
    /// field. Transport records the exact static form, and the semantic
    /// adapter may select `StaticCallback` only when the rustc-resolved child
    /// field is in fact a callback. No field-name heuristic is involved.
    fn classify_direct_callback_binding(
        &self,
        expr: &Expr,
    ) -> Option<super::DirectComponentBindingClassification> {
        let expr = direct_binding_inner(expr);
        if is_none_expr(expr) {
            return Some(
                super::DirectComponentBindingClassification::StaticCallback {
                    form: super::DirectStaticCallbackForm::None,
                },
            );
        }

        if let Expr::Call(call) = expr {
            if is_rsjs_noop_handler_call(&call.func) {
                return Some(if call.args.is_empty() {
                    super::DirectComponentBindingClassification::StaticCallback {
                        form: super::DirectStaticCallbackForm::NoopHandler,
                    }
                } else {
                    super::DirectComponentBindingClassification::Unsupported
                });
            }
            if is_some_call(&call.func) {
                if call.args.len() != 1 {
                    return Some(super::DirectComponentBindingClassification::Unsupported);
                }
                let inner = &call.args[0];
                return match self.classify_direct_callback_binding(inner) {
                    Some(super::DirectComponentBindingClassification::LiveCallback {
                        handler_prelude_local,
                        form: super::DirectLiveCallbackForm::Handler,
                    }) => Some(super::DirectComponentBindingClassification::LiveCallback {
                        handler_prelude_local,
                        form: super::DirectLiveCallbackForm::SomeHandler,
                    }),
                    Some(super::DirectComponentBindingClassification::StaticCallback {
                        form: super::DirectStaticCallbackForm::NoopHandler,
                    }) => Some(
                        super::DirectComponentBindingClassification::StaticCallback {
                            form: super::DirectStaticCallbackForm::SomeNoopHandler,
                        },
                    ),
                    Some(super::DirectComponentBindingClassification::Unsupported)
                    | Some(super::DirectComponentBindingClassification::LiveCallback { .. }) => {
                        Some(super::DirectComponentBindingClassification::Unsupported)
                    }
                    // `Some(None)` and arbitrary `Some(server_value)` are not
                    // compiler-known callback forms. Let ordinary opaque-SSR
                    // analysis classify them without guessing their Rust type.
                    _ => None,
                };
            }
        }

        let Expr::Path(path) = expr else {
            return None;
        };
        let ident = path.path.get_ident()?;
        let handler = self.handlers.get(&ident.to_string())?;
        Some(if handler.kind == HandlerLocalKind::Payload {
            super::DirectComponentBindingClassification::LiveCallback {
                handler_prelude_local: handler.prelude_local_index,
                form: super::DirectLiveCallbackForm::Handler,
            }
        } else {
            // DOM event handlers carry an event/currentTarget contract and
            // cannot be substituted for typed component callback props.
            super::DirectComponentBindingClassification::Unsupported
        })
    }

    /// Return the one stable legacy signal cell selected by an exact live
    /// value binding.
    ///
    /// This deliberately does not allocate a canonical `LocalValueId`. The
    /// canonical adapter owns that namespace and must map the proven
    /// `SignalId` when it imports the parent client graph.
    fn exact_direct_live_signal_source(
        &mut self,
        expr: &Expr,
    ) -> syn::Result<Option<(SignalId, super::DirectLiveValueSource)>> {
        let expr = direct_binding_inner(expr);
        if let Some(field) = direct_inbound_self_field(expr) {
            let (field_name, signal) = self.ensure_inbound_live_signal(field)?;
            self.record_inbound_live_read(field_name, field.span(), signal)?;
            return Ok(Some((signal, super::DirectLiveValueSource::InboundField)));
        }
        if let Expr::MethodCall(call) = expr {
            if call.method == "get" && call.args.is_empty() && call.turbofish.is_none() {
                let Expr::Path(receiver) = direct_binding_inner(&call.receiver) else {
                    return Ok(None);
                };
                let Some(ident) = receiver.path.get_ident() else {
                    return Ok(None);
                };
                return Ok(self
                    .signals
                    .get(&ident.to_string())
                    .copied()
                    .map(|signal| (signal, super::DirectLiveValueSource::SignalRead)));
            }
        }
        let Expr::Path(path) = expr else {
            return Ok(None);
        };
        let Some(ident) = path.path.get_ident() else {
            return Ok(None);
        };
        Ok(match self.client_prelude_locals.get(&ident.to_string()) {
            Some(FrontendExpr::SignalRead(signal)) => {
                Some((*signal, super::DirectLiveValueSource::ClientAlias))
            }
            _ => None,
        })
    }

    /// Lower an `if cond { html_sync!{...} } else { html_sync!{...} }`
    /// expression to a [`ConditionalSection`].
    pub(super) fn lower_conditional(
        &mut self,
        if_expr: &ExprIf,
        parent: NodeId,
    ) -> syn::Result<()> {
        if matches!(
            if_expr.else_branch.as_ref().map(|(_, expr)| expr.as_ref()),
            Some(Expr::If(_))
        ) {
            return self.lower_conditional_chain(if_expr, parent, None);
        }

        let cond_scope = ExprScope {
            node_payload_target: (!self.loop_scopes.is_empty()).then_some(parent),
            ..ExprScope::default()
        };
        let cond = self.lower_reactive_expr_in(&if_expr.cond, &cond_scope)?;
        self.require_bool_condition(&cond, &[], if_expr.cond.span())?;
        let cond_id = self.alloc_expr(cond, if_expr.cond.span())?;
        let section_ordinal = self.builder.reserve_conditional_section(parent, cond_id);
        let section_ordinal =
            checked_conditional_section_ordinal(section_ordinal, if_expr.if_token.span)?;

        let then_children = self.collect_branch_children(
            &if_expr.then_branch,
            parent,
            super::DirectConditionalLifecycleScope {
                parent,
                section_ordinal,
                branch: super::DirectConditionalBranch::Then,
            },
        )?;
        let else_children = match &if_expr.else_branch {
            Some((_, else_expr)) => self.lower_else_branch(
                else_expr,
                parent,
                super::DirectConditionalLifecycleScope {
                    parent,
                    section_ordinal,
                    branch: super::DirectConditionalBranch::Else,
                },
            )?,
            None => Vec::new(),
        };

        self.builder.complete_conditional_section(
            parent,
            section_ordinal as usize,
            then_children,
            else_children,
        );
        Ok(())
    }

    pub(super) fn lower_conditional_chain(
        &mut self,
        if_expr: &ExprIf,
        parent: NodeId,
        prefix: Option<FrontendExpr>,
    ) -> syn::Result<()> {
        let cond_scope = ExprScope {
            node_payload_target: (!self.loop_scopes.is_empty()).then_some(parent),
            ..ExprScope::default()
        };
        let raw_cond = self.lower_reactive_expr_in(&if_expr.cond, &cond_scope)?;
        self.require_bool_condition(&raw_cond, &[], if_expr.cond.span())?;
        let active_cond = and_optional(prefix.clone(), raw_cond.clone());
        let active_cond_id = self.alloc_expr(active_cond, if_expr.cond.span())?;
        let section_ordinal = self
            .builder
            .reserve_conditional_section(parent, active_cond_id);
        let section_ordinal =
            checked_conditional_section_ordinal(section_ordinal, if_expr.if_token.span)?;
        let then_children = self.collect_branch_children(
            &if_expr.then_branch,
            parent,
            super::DirectConditionalLifecycleScope {
                parent,
                section_ordinal,
                branch: super::DirectConditionalBranch::Then,
            },
        )?;
        self.builder.complete_conditional_section(
            parent,
            section_ordinal as usize,
            then_children,
            Vec::new(),
        );

        let else_prefix = and_optional(prefix, invert_condition(raw_cond));
        match &if_expr.else_branch {
            None => Ok(()),
            Some((_, else_expr)) => match else_expr.as_ref() {
                Expr::If(nested) => self.lower_conditional_chain(nested, parent, Some(else_prefix)),
                Expr::Block(ExprBlock { block, .. }) => {
                    let else_cond_id = self.alloc_expr(else_prefix, else_expr.span())?;
                    let section_ordinal = self
                        .builder
                        .reserve_conditional_section(parent, else_cond_id);
                    let section_ordinal =
                        checked_conditional_section_ordinal(section_ordinal, else_expr.span())?;
                    let else_children = self.collect_branch_children(
                        block,
                        parent,
                        super::DirectConditionalLifecycleScope {
                            parent,
                            section_ordinal,
                            branch: super::DirectConditionalBranch::Then,
                        },
                    )?;
                    self.builder.complete_conditional_section(
                        parent,
                        section_ordinal as usize,
                        else_children,
                        Vec::new(),
                    );
                    Ok(())
                }
                other => Err(syn::Error::new(
                    other.span(),
                    "`#[rsjs]` expects `else` to be a block containing an \
                     `html_sync!` / `html!` invocation",
                )),
            },
        }
    }

    /// Lower the `else` part of an `if`. Ordinary `else { html_sync!{...} }`
    /// branches stay as the native `else_children` side of one conditional
    /// section. `else if` chains are flattened by `lower_conditional_chain`.
    pub(super) fn lower_else_branch(
        &mut self,
        else_expr: &Expr,
        parent: NodeId,
        lifecycle: super::DirectConditionalLifecycleScope,
    ) -> syn::Result<Vec<NodeId>> {
        match else_expr {
            Expr::Block(ExprBlock { block, .. }) => {
                self.collect_branch_children(block, parent, lifecycle)
            }
            Expr::If(_) => Err(syn::Error::new(
                else_expr.span(),
                "`#[rsjs]` does not support `else if` chains in v0.1; \
                 nest the inner conditional inside an explicit `else { ... }` block",
            )),
            other => Err(syn::Error::new(
                other.span(),
                "`#[rsjs]` expects `else` to be a block containing an \
                 `html_sync!` / `html!` invocation",
            )),
        }
    }

    /// Lower a template `for` loop whose body ends in `html_sync!` / `html!`.
    ///
    /// This is intentionally a narrow first slice of repeatable DOM support:
    /// repeated event handlers use delegated dispatch, repeated form controls
    /// bind every marker-matched control, and repeated conditional sections
    /// bind each marker-matched branch row.
    pub(super) fn lower_for_loop(
        &mut self,
        for_loop: &ExprForLoop,
        parent: NodeId,
    ) -> syn::Result<()> {
        // Keyed lowering triggers only when the iterator is a direct
        // reactive list source. Static template loops over server data
        // keep their existing replay semantics unchanged.
        if let Some(signal) = self.keyed_list_signal_iterator(&for_loop.expr) {
            return self.lower_keyed_list(for_loop, parent, KeyedListIterator::Signal(signal));
        }
        if let Some(query) = self.keyed_list_query_iterator(&for_loop.expr)? {
            return self.lower_keyed_list(for_loop, parent, KeyedListIterator::Query(query));
        }
        self.reject_signal_in_static_loop_iterator(&for_loop.expr)?;
        let tail = loop_tail_html_macro(&for_loop.body)?;
        validate_nested_html_macro(&tail.path, "template loop")?;
        let prefix_len = for_loop.body.stmts.len() - 1;
        self.reject_reactive_captures_in_static_loop_prefix(
            &for_loop.body.stmts[..prefix_len],
            &for_loop.pat,
        )?;
        let mut attrs = proc_macro2::TokenStream::new();
        for attr in &for_loop.attrs {
            attr.to_tokens(&mut attrs);
        }

        let loop_node = self.builder.for_loop(
            parent,
            &attrs.to_string(),
            &for_loop
                .label
                .as_ref()
                .map(ToTokens::to_token_stream)
                .map(|tokens| tokens.to_string())
                .unwrap_or_default(),
            &for_loop.pat.to_token_stream().to_string(),
            &for_loop.expr.to_token_stream().to_string(),
            &tail.prefix.to_string(),
        );

        let inner: HtmlRoot = syn::parse2(tail.tokens).map_err(|e| {
            syn::Error::new(
                tail.span,
                format!("`#[rsjs]` failed to parse a template-loop body as HTML: {e}"),
            )
        })?;
        let mut integer_domains = template_loop_integer_domains(&for_loop.pat, &for_loop.expr)?;
        // A prefix `let` enters scope before the HTML body and can shadow the
        // iterator binding. Do not let the hidden iterator's proof leak into
        // payloads produced from that new binding.
        integer_domains.retain(|name, _| !tail.prefix_locals.contains(name));
        let mut loop_locals = loop_pattern_locals(&for_loop.pat);
        loop_locals.extend(tail.prefix_locals);
        self.loop_scopes.push(LoopScope {
            locals: loop_locals,
            integer_domains,
        });
        let process_result = self.process_children(&inner.nodes, loop_node);
        self.loop_scopes.pop();
        process_result
    }

    /// Whether a template `for` loop's iterator is a direct signal
    /// read (`items.get()`), which selects keyed-list lowering. A
    /// loop-scope local shadowing the signal name refers to the loop
    /// variable, not the signal.
    pub(super) fn keyed_list_signal_iterator(&self, expr: &Expr) -> Option<SignalId> {
        let Expr::MethodCall(mc) = expr else {
            return None;
        };
        if mc.method != "get" || !mc.args.is_empty() || mc.turbofish.is_some() {
            return None;
        }
        let Expr::Path(ExprPath { path, .. }) = mc.receiver.as_ref() else {
            return None;
        };
        let ident = path.get_ident()?;
        let name = ident.to_string();
        if self
            .loop_scopes
            .iter()
            .any(|scope| scope.locals.contains(&name))
        {
            return None;
        }
        self.signals.get(&name).copied()
    }

    fn keyed_list_query_iterator(&mut self, expr: &Expr) -> syn::Result<Option<QueryListIterator>> {
        let Some((query, suffix)) = self.query_data_iterator_suffix(expr)? else {
            return Ok(None);
        };
        let source = self.lower_reactive_expr_in(expr, &ExprScope::default())?;
        let expr_id = self.alloc_expr(source, expr.span())?;
        let typed_local = crate::emit_rust::query_initial_typed_value_local_ident(query);
        Ok(Some(QueryListIterator {
            expr: expr_id,
            ssr_iter: format!("& {typed_local}{suffix}"),
        }))
    }

    fn query_data_iterator_suffix(&self, expr: &Expr) -> syn::Result<Option<(QueryId, String)>> {
        match expr {
            Expr::MethodCall(mc)
                if mc.method == "data" && mc.args.is_empty() && mc.turbofish.is_none() =>
            {
                Ok(self
                    .query_receiver(&mc.receiver)
                    .map(|query| (query, String::new())))
            }
            Expr::Field(field) => {
                let Some((query, mut suffix)) = self.query_data_iterator_suffix(&field.base)?
                else {
                    return Ok(None);
                };
                suffix.push_str(&rust_member_suffix(&field.member));
                Ok(Some((query, suffix)))
            }
            Expr::Index(index) => {
                let Some((query, mut suffix)) = self.query_data_iterator_suffix(&index.expr)?
                else {
                    return Ok(None);
                };
                let segment = projection_index_segment(&index.index)?;
                suffix.push('[');
                suffix.push_str(&segment);
                suffix.push(']');
                Ok(Some((query, suffix)))
            }
            Expr::Paren(paren) => self.query_data_iterator_suffix(&paren.expr),
            Expr::Group(group) => self.query_data_iterator_suffix(&group.expr),
            _ => Ok(None),
        }
    }

    /// A static template loop replays its iterator as ordinary server
    /// Rust, where signal locals do not exist. A signal buried inside
    /// a larger iterator expression (`items.get().iter().rev()`,
    /// `0..n.get()`) therefore cannot render, and the client could not
    /// keep the rows in sync either. Reject it with the keyed-list
    /// shape as the way out.
    pub(super) fn reject_signal_in_static_loop_iterator(&self, expr: &Expr) -> syn::Result<()> {
        let mut visitor = IteratorSignalUseVisitor {
            signals: &self.signals,
            loop_scopes: &self.loop_scopes,
            found: None,
        };
        visitor.visit_expr(expr);
        let Some((span, name)) = visitor.found else {
            return Ok(());
        };
        Err(syn::Error::new(
            span,
            format!(
                "template `for` loops can read a signal only as the whole iterator: \
                 `for item in {name}.get()` lowers to a client-updated keyed list. \
                 Here `{name}` is mixed into a larger iterator expression, which the \
                 server render cannot replay and the client cannot keep in sync. \
                 Iterate `{name}.get()` directly and store the transformed list in \
                 the signal, or iterate a plain server value instead"
            ),
        ))
    }

    /// Lower a keyed list: a template `for` loop whose iterator is a
    /// direct signal read and whose body root carries `key={...}`.
    ///
    /// Produces (a) view-tree nodes that replay the initial items on
    /// the server (the item root gains `data-rsjs-key` in emit_rust),
    /// and (b) a [`ListDecl`] that the JS emitter lowers to one
    /// `bind_list(container, getter, { key, create, adopt, deps })`
    /// call.
    // proa-perf-ok(scope): diagnostic text includes HTML examples but never renders a response
    fn lower_keyed_list(
        &mut self,
        for_loop: &ExprForLoop,
        parent: NodeId,
        source: KeyedListIterator,
    ) -> syn::Result<()> {
        if !self.loop_scopes.is_empty() {
            return Err(syn::Error::new(
                for_loop.span(),
                "keyed lists cannot nest inside another template loop in v0.1: the \
                 inner `bind_list` container would repeat per outer row and the \
                 client has no per-row binding scope yet. Flatten the data into one \
                 signal and render a single keyed list, or keep the outer loop over \
                 plain server data",
            ));
        }
        if let KeyedListIterator::Signal(signal) = &source {
            if self.memo_cells.contains(signal) {
                return Err(syn::Error::new(
                    for_loop.expr.span(),
                    "keyed lists iterate a writable `signal(...)`; a `memo(...)` \
                     cell cannot drive a keyed list in v0.1. Store the list in a signal \
                     and write it from event handlers",
                ));
            }
        }
        let item_ident = match for_loop.pat.as_ref() {
            Pat::Ident(pi) if pi.by_ref.is_none() && pi.subpat.is_none() => pi.ident.clone(),
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "keyed list loops bind the item to a plain variable in v0.1 \
                     (`for item in items.get()`); destructuring patterns are not \
                     supported. Bind one variable and read fields off it \
                     (`{row.title}`)",
                ));
            }
        };

        let tail = loop_tail_html_macro(&for_loop.body)?;
        validate_nested_html_macro(&tail.path, "template loop")?;
        if !tail.prefix.is_empty() {
            return Err(syn::Error::new(
                tail.span,
                "keyed list bodies must be a single `html_sync!` invocation: \
                 statements before it would run per item on the server but cannot \
                 be replayed by the client's per-item `create()`. Precompute \
                 derived values into the items stored in the signal",
            ));
        }
        let inner: HtmlRoot = syn::parse2(tail.tokens).map_err(|e| {
            syn::Error::new(
                tail.span,
                format!("`#[rsjs]` failed to parse a keyed-list body as HTML: {e}"),
            )
        })?;
        let mut item_element: Option<&Element> = None;
        for node in &inner.nodes {
            match node {
                Node::Element(e) if item_element.is_none() => item_element = Some(e),
                Node::Element(e) => {
                    return Err(syn::Error::new(
                        e.name.span(),
                        "a keyed list body renders exactly one root element per item \
                         in v0.1: the runtime tracks one DOM node per key. Wrap the \
                         siblings in a single container element",
                    ));
                }
                _ => {
                    return Err(syn::Error::new(
                        tail.span,
                        "a keyed list body must be a single root element in v0.1; \
                         top-level text or expression content has no DOM node for \
                         the runtime to key. Wrap it in an element (e.g. `<li>`)",
                    ));
                }
            }
        }
        let Some(item_element) = item_element else {
            return Err(syn::Error::new(
                tail.span,
                "a keyed list body must render a root element per item \
                 (e.g. `html_sync! { <li key={item}>{item}</li> }`)",
            ));
        };

        // SSR replays the loop over the rendered initial-value local:
        // `items.get()` does not exist in the generated server code.
        // Iterating by reference keeps the local alive for the
        // dehydrated state blob.
        let ssr_iter = source.ssr_iter();
        let pat_tokens = for_loop.pat.to_token_stream().to_string();
        let loop_node = self
            .builder
            .for_loop(parent, "", "", &pat_tokens, &ssr_iter, "");

        let item_name = item_ident.to_string();
        let mut key: Option<(ListItemExpr, String)> = None;
        let (item_root, template) =
            self.lower_keyed_item_element(item_element, loop_node, &item_name, true, &mut key)?;
        let Some((key_expr, key_tokens)) = key else {
            return Err(syn::Error::new(
                item_element.name.span(),
                format!(
                    "keyed lists require a `key={{...}}` attribute on the item's \
                     root element so the client can match DOM nodes to items \
                     across inserts, removes, and reorders. Give each item a \
                     stable identity, e.g. `<li key={{{item_name}}}>` for value \
                     lists or `<li key={{{item_name}.id}}>` for struct rows"
                ),
            ));
        };

        // Container marker for the client-side node lookup.
        self.builder
            .static_attr(parent, MARKER_LIST, &parent.0.to_string());
        self.hir_sidecar
            .push_list(parent, source.sidecar_source(), key_expr.js());
        self.builder.keyed_list(ListDecl {
            container: parent,
            loop_node,
            item_root,
            source: source.list_source(),
            key: key_expr,
            key_tokens,
            item: template,
        });
        self.keyed_list_guards.push(KeyedListGuard {
            container: parent,
            loop_node,
            span: for_loop.span(),
        });
        Ok(())
    }

    /// Walk one element of a keyed-list item template, allocating the
    /// matching view-tree nodes for SSR replay and building the
    /// [`ListTemplateNode`] mirror for the client `create()` builder.
    ///
    /// Enforces the v0.1 body grammar: elements with static attributes,
    /// item-derived dynamic attributes, plus text-level children
    /// (literals and slots over the loop item or one of its fields),
    /// with `key={...}` required on the root.
    pub(super) fn lower_keyed_item_element(
        &mut self,
        el: &Element,
        parent: NodeId,
        item_name: &str,
        is_root: bool,
        key: &mut Option<(ListItemExpr, String)>,
    ) -> syn::Result<(NodeId, ListTemplateNode)> {
        let tag = el.tag_str();
        let id = self.builder.element(&tag, Some(parent));
        let mut static_attrs: Vec<(String, String)> = Vec::new();
        let mut dynamic_attrs: Vec<(String, ListItemExpr)> = Vec::new();

        for attr in &el.attrs {
            let name_ident = match attr {
                Attribute::Static { name, .. } => name,
                Attribute::Dynamic { name, .. } => name,
                Attribute::Bool { name } => name,
            };
            let raw_name = name_ident.to_string();
            if let Some(error) = reserved_rsjs_attribute_error(&raw_name, name_ident.span()) {
                return Err(error);
            }
            if raw_name == "key" {
                if !is_root {
                    return Err(syn::Error::new(
                        name_ident.span(),
                        "`key={...}` belongs on the keyed list item's root element: \
                         the runtime keys one DOM node per item. Move it to the \
                         element directly inside the loop",
                    ));
                }
                let Attribute::Dynamic { value, .. } = attr else {
                    return Err(syn::Error::new(
                        name_ident.span(),
                        "`key` takes an expression over the loop item: \
                         `key={item}` or `key={row.id}`",
                    ));
                };
                let Some((expr, tokens)) = keyed_list_item_expr(value, item_name) else {
                    return Err(syn::Error::new(
                        value.span(),
                        format!(
                            "keyed list keys read the loop item directly in v0.1: \
                             `key={{{item_name}}}` or `key={{{item_name}.id}}`. \
                             Computed keys cannot be replayed identically on the \
                             server and the client; store a precomputed id on the \
                             item instead"
                        ),
                    ));
                };
                *key = Some((expr, tokens));
                continue;
            }
            if raw_name == "ref" {
                return Err(syn::Error::new(
                    name_ident.span(),
                    "`ref={...}` inside keyed list bodies is not supported in v0.1: \
                     items are created and dropped as the list changes, so a single \
                     node ref has no stable target. Bind the ref on a stable element \
                     outside the list",
                ));
            }
            if raw_name.starts_with("on_") {
                return Err(syn::Error::new(
                    name_ident.span(),
                    "event handlers inside keyed list bodies are not supported in \
                     v0.1: client-created items have no per-item delegation scope \
                     yet. Put the handler on the list's container (DOM events \
                     bubble), or render the rows with a static template loop",
                ));
            }
            if raw_name.starts_with("bind_") {
                return Err(syn::Error::new(
                    name_ident.span(),
                    "`bind_*` form bindings inside keyed list bodies are not \
                     supported in v0.1: each item would need its own two-way \
                     binding lifecycle. Bind form controls outside the list, or \
                     render the rows with a static template loop",
                ));
            }
            match attr {
                Attribute::Static { value, .. } => {
                    let html_name = raw_name.replace('_', "-");
                    self.builder.static_attr(id, &html_name, &value.value());
                    static_attrs.push((html_name, value.value()));
                }
                Attribute::Bool { .. } => {
                    let html_name = raw_name.replace('_', "-");
                    self.builder.static_attr(id, &html_name, "");
                    static_attrs.push((html_name, String::new()));
                }
                Attribute::Dynamic { value, .. } => {
                    let Some((expr, tokens)) = keyed_list_item_expr(value, item_name) else {
                        return Err(syn::Error::new(
                            value.span(),
                            format!(
                                "dynamic attributes inside keyed list bodies can \
                                 only read the loop item directly in v0.1 \
                                 (`{item_name}` or `{item_name}.field`). Precompute \
                                 the attribute value into the item, or render the \
                                 rows with a static template loop"
                            ),
                        ));
                    };
                    let html_name = raw_name.replace('_', "-");
                    self.builder.server_attr_with_source_anchor(
                        id,
                        &html_name,
                        &format!("rsjs::server_attr(({tokens}).clone())"),
                        rsjs_ir::ServerSurfaceSource::KeyedListItem,
                        Some(server_surface_anchor(value.span())),
                    );
                    dynamic_attrs.push((html_name, expr));
                }
            }
        }

        let mut children: Vec<ListTemplateNode> = Vec::new();
        let mut has_element_child = false;
        let mut has_text_child = false;
        for child in &el.children {
            match child {
                Node::Text(lit) => {
                    self.builder.text_node_static(id, &lit.value());
                    children.push(ListTemplateNode::Text(lit.value()));
                    has_text_child = true;
                }
                Node::Element(child_el) => {
                    let (_, node) =
                        self.lower_keyed_item_element(child_el, id, item_name, false, key)?;
                    children.push(node);
                    has_element_child = true;
                }
                Node::Block(expr) => {
                    match expr {
                        Expr::If(_) => {
                            return Err(syn::Error::new(
                                expr.span(),
                                "conditionals inside keyed list bodies are not \
                                 supported in v0.1: the per-item `create()` builds \
                                 one fixed subtree per item. Encode the variation in \
                                 the item's data, or render the rows with a static \
                                 template loop",
                            ));
                        }
                        Expr::ForLoop(_) => {
                            return Err(syn::Error::new(
                                expr.span(),
                                "nested loops inside keyed list bodies are not \
                                 supported in v0.1: the runtime reconciles one flat \
                                 list per container. Flatten the data into one keyed \
                                 list, or render the inner rows with a static \
                                 template loop",
                            ));
                        }
                        _ => {}
                    }
                    let Some((slot, tokens)) = keyed_list_item_expr(expr, item_name) else {
                        return Err(syn::Error::new(
                            expr.span(),
                            format!(
                                "keyed list bodies can interpolate the loop item or \
                                 one of its fields in v0.1 (`{{{item_name}}}`, \
                                 `{{{item_name}.title}}`); other expressions cannot \
                                 be replayed by the client's per-item update. \
                                 Precompute the value into the item's data"
                            ),
                        ));
                    };
                    self.builder.server_only_node_with_source_anchor(
                        id,
                        &format!("({tokens}).clone()"),
                        rsjs_ir::ServerSurfaceSource::KeyedListItem,
                        Some(server_surface_anchor(expr.span())),
                    );
                    children.push(ListTemplateNode::Slot(slot));
                    has_text_child = true;
                }
                Node::Fragment(_) => {
                    return Err(syn::Error::new(
                        el.name.span(),
                        "fragments inside keyed list bodies are not supported in \
                         v0.1; wrap the content in a real element so the per-item \
                         template stays a single tree",
                    ));
                }
                Node::Doctype(_) => {
                    return Err(syn::Error::new(
                        el.name.span(),
                        "doctype declarations cannot appear inside a keyed list body",
                    ));
                }
            }
        }
        if has_element_child && has_text_child {
            return Err(syn::Error::new(
                el.name.span(),
                "an element inside a keyed list body cannot mix element children \
                 with text in v0.1: the per-item update re-applies the element's \
                 whole `textContent`, which would erase the element children. Wrap \
                 the text in its own element (e.g. a `<span>`)",
            ));
        }
        Ok((
            id,
            ListTemplateNode::Element {
                tag,
                static_attrs,
                dynamic_attrs,
                children,
            },
        ))
    }

    /// Post-walk structural check for every keyed list: the loop must
    /// be its container's only child and the container must not also
    /// carry a text binding, because the runtime's `bind_list` owns
    /// every child of the container and removes unkeyed siblings on
    /// the first update.
    pub(super) fn validate_keyed_list_containers(&self) -> syn::Result<()> {
        for guard in &self.keyed_list_guards {
            let ViewNode::Element { tag, children, .. } = self.builder.view_node(guard.container)
            else {
                continue;
            };
            if children.len() != 1 || children[0] != guard.loop_node {
                return Err(syn::Error::new(
                    guard.span,
                    format!(
                        "a keyed list must be the only content of its container \
                         `<{tag}>` in v0.1: the client's `bind_list` owns every \
                         child of the container and removes unkeyed siblings on its \
                         first update. Move the sibling content outside and give the \
                         list a dedicated container element"
                    ),
                ));
            }
            if self
                .builder
                .bindings()
                .text_bindings
                .contains_key(&guard.container)
            {
                return Err(syn::Error::new(
                    guard.span,
                    "a keyed list's container cannot also render its own text \
                     binding in v0.1: the client's `bind_list` owns the container's \
                     children. Move the text into its own element outside the \
                     container",
                ));
            }
        }
        Ok(())
    }

    /// Walk a branch block (then or else): the block must contain a
    /// single tail `html_sync!` / `html!` invocation. The invocation
    /// can appear either as an expression statement
    /// (`Stmt::Expr(Expr::Macro(...), _)`) or as a macro statement
    /// (`Stmt::Macro(...)`); syn produces the latter when the macro
    /// has trailing braces and no `.method()` chain after it. Both
    /// shapes lower the same way: we re-parse the macro's tokens as
    /// `HtmlRoot` and allocate child element nodes under `parent`.
    pub(super) fn collect_branch_children(
        &mut self,
        block: &Block,
        parent: NodeId,
        lifecycle: super::DirectConditionalLifecycleScope,
    ) -> syn::Result<Vec<NodeId>> {
        let (mac_path, mac_tokens, mac_span) = match block.stmts.last() {
            Some(Stmt::Expr(e, _)) => {
                let mac = locate_branch_macro(e).ok_or_else(|| {
                    syn::Error::new(
                        e.span(),
                        "`#[rsjs]` expects a conditional branch's body to be \
                         `html_sync! { ... }` (or `html! { ... }`)",
                    )
                })?;
                (mac.mac.path.clone(), mac.mac.tokens.clone(), mac.span())
            }
            Some(Stmt::Macro(sm)) => (sm.mac.path.clone(), sm.mac.tokens.clone(), sm.span()),
            Some(s) => {
                return Err(syn::Error::new(
                    s.span(),
                    "`#[rsjs]` expects a conditional branch's last statement to be an \
                     `html_sync!` / `html!` invocation",
                ));
            }
            None => {
                return Err(syn::Error::new(
                    block.span(),
                    "`#[rsjs]` expects a conditional branch to contain an \
                     `html_sync!` / `html!` invocation; empty branches are not supported",
                ));
            }
        };

        let path_last = mac_path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        if path_last != "html_sync" && path_last != "html" {
            return Err(syn::Error::new(
                mac_path.span(),
                "`#[rsjs]` conditional branches must use `html_sync!` / `html!`",
            ));
        }

        let inner: HtmlRoot = syn::parse2(mac_tokens).map_err(|e| {
            syn::Error::new(
                mac_span,
                format!("`#[rsjs]` failed to parse a conditional-branch body as HTML: {e}"),
            )
        })?;

        self.component_lifecycle_stack.push(lifecycle);
        let result = (|| {
            let mut ids = Vec::new();
            self.collect_conditional_root_nodes(&inner.nodes, parent, &mut ids)?;
            Ok(ids)
        })();
        let popped = self
            .component_lifecycle_stack
            .pop()
            .expect("conditional lifecycle stack must balance");
        debug_assert_eq!(popped, lifecycle);
        result
    }

    fn collect_conditional_root_nodes(
        &mut self,
        nodes: &[Node],
        parent: NodeId,
        ids: &mut Vec<NodeId>,
    ) -> syn::Result<()> {
        for node in nodes {
            match node {
                Node::Element(element) => {
                    ids.push(self.walk_element(element, Some(parent))?);
                }
                Node::Fragment(children) => {
                    self.collect_conditional_root_nodes(children, parent, ids)?;
                }
                Node::Block(expression) => {
                    return Err(guided_boundary_error(
                        expression.span(),
                        "a conditional branch cannot use a renderable expression as its DOM root",
                        "Conditional branch roots are the structural nodes the browser toggles. \
                         A direct component or arbitrary renderable has no markerable root in \
                         the current client IR, so accepting it would either drop output or \
                         require an implicit wrapper.",
                        [
                            "Wrap the expression in a real element, for example `<div>{Child { ... }}</div>`.",
                            "Move the conditional into the child when the child should own its own root lifecycle.",
                            "Keep the expression outside the reactive branch when it is server-static.",
                        ],
                        Some("conditional.branch_expression_root"),
                    ));
                }
                Node::Text(text) => {
                    return Err(guided_boundary_error(
                        text.span(),
                        "a conditional branch cannot use text as its DOM root",
                        "The current conditional runtime toggles element roots. Text roots do \
                         not have a stable element marker and must not be silently omitted.",
                        [
                            "Wrap the text in an element such as `<span>...</span>`.",
                            "Move static text outside the reactive branch.",
                            "Render a complete element for each branch.",
                        ],
                        Some("conditional.branch_text_root"),
                    ));
                }
                Node::Doctype(doctype) => {
                    return Err(syn::Error::new(
                        doctype.span(),
                        "doctype declarations cannot appear inside a conditional branch",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Exact integer-domain facts made visible by a static template iterator.
///
/// The proc macro cannot ask rustc for an arbitrary iterator's `Item` type, so
/// this deliberately recognizes only a direct identifier bound from a range
/// with at least one explicit fixed-width bound. Bare `0..42` and opaque
/// iterators remain untyped; later client-helper validation must reject them
/// rather than guess Rust's inferred type.
pub(super) fn template_loop_integer_domains(
    pattern: &Pat,
    iterator: &Expr,
) -> syn::Result<HashMap<String, IntegerDomain>> {
    let Pat::Ident(binding) = pattern else {
        return Ok(HashMap::new());
    };
    if binding.by_ref.is_some() || binding.subpat.is_some() {
        return Ok(HashMap::new());
    }

    let range = match iterator {
        Expr::Range(range) => range,
        Expr::Paren(paren) => {
            return template_loop_integer_domains(pattern, &paren.expr);
        }
        Expr::Group(group) => {
            return template_loop_integer_domains(pattern, &group.expr);
        }
        _ => return Ok(HashMap::new()),
    };
    let source = merge_range_integer_sources(
        range.start.as_deref().and_then(integer_expr_source_hint),
        range.end.as_deref().and_then(integer_expr_source_hint),
        range.span(),
    )?;
    let Some(IntegerSource::Supported(domain)) = source else {
        return Ok(HashMap::new());
    };
    Ok(HashMap::from([(binding.ident.to_string(), domain)]))
}

fn checked_conditional_section_ordinal(
    ordinal: usize,
    span: proc_macro2::Span,
) -> syn::Result<u32> {
    u32::try_from(ordinal).map_err(|_| {
        syn::Error::new(
            span,
            "an analyzed RSJS parent cannot contain more than 2^32 conditional sections",
        )
    })
}

fn is_shared_state_provider_struct(expr: &ExprStruct) -> bool {
    let mut segments = expr.path.segments.iter();
    matches!(
        (segments.next(), segments.next(), segments.next()),
        (Some(root), Some(provider), None)
            if root.ident == "rsjs" && provider.ident == "SharedStateProvider"
    )
}

fn shared_state_provider_fields(provider: &ExprStruct) -> syn::Result<(&Expr, &Expr)> {
    if let Some(rest) = &provider.rest {
        return Err(syn::Error::new(
            rest.span(),
            "rsjs/shared-state-provider-shape: `rsjs::SharedStateProvider` is a closed \
             structural intrinsic and does not accept struct-update syntax; provide exactly \
             `state` and `children`",
        ));
    }

    let mut state = None;
    let mut children = None;
    for field in &provider.fields {
        let syn::Member::Named(name) = &field.member else {
            return Err(syn::Error::new(
                field.member.span(),
                "rsjs/shared-state-provider-shape: `rsjs::SharedStateProvider` requires named \
                 `state` and `children` fields",
            ));
        };
        let slot = match name.to_string().as_str() {
            "state" => &mut state,
            "children" => &mut children,
            _ => {
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "rsjs/shared-state-provider-shape: unknown provider field `{name}`; \
                         expected exactly `state` and `children`"
                    ),
                ));
            }
        };
        if slot.replace(&field.expr).is_some() {
            return Err(syn::Error::new(
                name.span(),
                format!(
                    "rsjs/shared-state-provider-shape: duplicate `{name}` field in \
                     `rsjs::SharedStateProvider`"
                ),
            ));
        }
    }

    let state = state.ok_or_else(|| {
        syn::Error::new(
            provider.path.span(),
            "rsjs/shared-state-provider-shape: `rsjs::SharedStateProvider` is missing its \
             required `state` field",
        )
    })?;
    let children = children.ok_or_else(|| {
        syn::Error::new(
            provider.path.span(),
            "rsjs/shared-state-provider-shape: `rsjs::SharedStateProvider` is missing its \
             required `children` field",
        )
    })?;
    Ok((state, children))
}

fn shared_state_provider_children_shape_error(span: proc_macro2::Span) -> syn::Error {
    guided_boundary_error(
        span,
        "v1 `rsjs::SharedStateProvider` requires one direct component struct literal in `children`",
        "Provider declarations are wrapperless server topology, not component edges. Restricting \
         the initial structural intrinsic to one visible component prevents an arbitrary or \
         multi-child renderable from hiding hydration ownership.",
        [
            "Use `children: ChildComponent { ... }` with static server inputs.",
            "Move multi-child composition into that child component's analyzed render body.",
            "Keep provider topology outside opaque helper or nested HTML-macro output.",
        ],
        Some("shared_state.provider_children_shape"),
    )
}

fn shared_state_provider_state_shape_error(span: proc_macro2::Span) -> syn::Error {
    guided_boundary_error(
        span,
        "v1 `rsjs::SharedStateProvider` requires a named SharedState struct literal in `state`",
        "The provider topology record needs a rustc-resolved nominal SharedState type so its \
         schema witness can be authenticated by relocation. Constructor calls, opaque values, \
         and another provider do not expose that type to the RSJS frontend.",
        [
            "Use `state: WorkbenchState { ... }` with a type derived as `rsjs::SharedState`.",
            "Move server calculations into the struct literal's field expressions.",
            "Keep provider nesting in `children`, not in the provider's state value.",
        ],
        Some("shared_state.provider_state_shape"),
    )
}

fn direct_binding_inner(mut expr: &Expr) -> &Expr {
    loop {
        expr = match expr {
            Expr::Paren(paren) => &paren.expr,
            Expr::Group(group) => &group.expr,
            _ => return expr,
        };
    }
}

fn direct_inbound_self_field(expr: &Expr) -> Option<&syn::ExprField> {
    let Expr::Field(field) = direct_binding_inner(expr) else {
        return None;
    };
    expr_is_self_path(&field.base).then_some(field)
}

/// Recognize the first intentionally small immutable island-slot grammar.
///
/// The slot must be a direct `html_sync!` / `html!` invocation with exactly
/// one root element, and every nested node/attribute must be static markup.
/// Expression blocks and dynamic attributes are rejected even when their
/// visible syntax happens not to mention a tracked client local: without a
/// Rust-resolved static-fragment witness an arbitrary renderable could emit a
/// separately owned island. The conservative grammar therefore proves both an
/// `Exact(1)` span and absence of nested owner registrations.
fn immutable_static_slot_exact_one(expr: &Expr) -> bool {
    let Expr::Macro(expr_macro) = expr else {
        return false;
    };
    if !is_html_macro_path(&expr_macro.mac.path) {
        return false;
    }
    let Ok(root) = syn::parse2::<HtmlRoot>(expr_macro.mac.tokens.clone()) else {
        return false;
    };
    matches!(root.nodes.as_slice(), [Node::Element(element)] if static_slot_element(element))
}

fn static_slot_element(element: &Element) -> bool {
    element
        .attrs
        .iter()
        .all(|attr| matches!(attr, Attribute::Static { .. } | Attribute::Bool { .. }))
        && element.children.iter().all(static_slot_node)
}

fn static_slot_node(node: &Node) -> bool {
    match node {
        Node::Element(element) => static_slot_element(element),
        Node::Fragment(nodes) => nodes.iter().all(static_slot_node),
        Node::Text(_) => true,
        Node::Block(_) | Node::Doctype(_) => false,
    }
}

pub(super) fn server_only_call(expr: &Expr) -> Option<&ExprCall> {
    explicit_server_call(expr, "server_only")
}

pub(super) fn island_call(expr: &Expr) -> Option<&ExprCall> {
    explicit_server_call(expr, "island")
}

pub(super) fn server_attr_call(expr: &Expr) -> Option<&ExprCall> {
    explicit_server_call(expr, "server_attr")
}

pub(super) fn explicit_server_call<'a>(expr: &'a Expr, name: &str) -> Option<&'a ExprCall> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Path(ExprPath {
        path, qself: None, ..
    }) = call.func.as_ref()
    else {
        return None;
    };
    path.segments
        .last()
        .filter(|seg| seg.ident == name)
        .map(|_| call)
}

#[derive(Clone, Copy)]
pub(super) enum OpaqueBoundary {
    ServerOnly,
    ServerAttr,
    Island,
    SharedStateProviderSeed,
    StaticLoopPrefix,
}

impl OpaqueBoundary {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::ServerOnly => "rsjs::server_only(...)",
            Self::ServerAttr => "rsjs::server_attr(...)",
            Self::Island => "rsjs::island(...)",
            Self::SharedStateProviderSeed => "rsjs::SharedStateProvider { state, .. }",
            Self::StaticLoopPrefix => "a static template-loop prefix",
        }
    }

    pub(super) fn signal_capture_error(self, span: proc_macro2::Span, name: &str) -> syn::Error {
        match self {
            Self::ServerOnly => guided_boundary_error(
                span,
                format!("`{}` cannot capture reactive signal `{name}`", self.label()),
                "Server-only slots emit static SSR content. Client hydration does not re-run that \
                 subtree, so a signal read there would never update.",
                [
                    "Move the signal read into a normal rsjs reactive slot outside `rsjs::server_only(...)`.",
                    "If the value is intentionally static, pass plain server data into the server-only child.",
                ],
                Some("server_boundary.server_only_signal_capture"),
            ),
            Self::ServerAttr => guided_boundary_error(
                span,
                format!("`{}` cannot capture reactive signal `{name}`", self.label()),
                "Server-only attributes emit static SSR values. Client hydration does not attach \
                 reactive updates to that attribute.",
                [
                    "Use a normal dynamic attribute for values that should update on the client.",
                    "If the value is intentionally static, pass plain server data through `rsjs::server_attr(...)`.",
                ],
                Some("server_boundary.server_attr_signal_capture"),
            ),
            Self::Island => guided_boundary_error(
                span,
                format!("`{}` cannot capture parent reactive signal `{name}`", self.label()),
                "Explicit child islands hydrate as independent client scopes. A parent signal is \
                 not serializable as live child state across that boundary.",
                [
                    "Pass a plain server value into the child island.",
                    "Keep the reactive markup directly inside the parent island.",
                    "Give the child island its own query, mutation, signal, or custom-event channel.",
                ],
                Some("server_boundary.island_signal_capture"),
            ),
            Self::SharedStateProviderSeed => provider_seed_dependency_error(
                span,
                format!("reactive signal `{name}`"),
            ),
            Self::StaticLoopPrefix => static_loop_prefix_dependency_error(
                span,
                format!("reactive signal `{name}`"),
            ),
        }
    }

    pub(super) fn handler_capture_error(self, span: proc_macro2::Span, name: &str) -> syn::Error {
        match self {
            Self::ServerOnly | Self::ServerAttr => guided_boundary_error(
                span,
                format!("`{}` cannot capture rsjs handler `{name}`", self.label()),
                "rsjs handlers are client IR callbacks. Opaque server-only boundaries render \
                 server HTML, so rsjs cannot inline or replay that callback inside the boundary \
                 during hydration.",
                [
                    "Render the interactive child directly inside the parent island so the handler can be inlined.",
                    "Keep the callback on a DOM event handler in the parent island.",
                    "If the child must remain server-only, pass ordinary server data instead of an rsjs handler.",
                ],
                Some("component.server_only_handler_capture"),
            ),
            Self::Island => guided_boundary_error(
                span,
                format!("`{}` cannot capture parent rsjs handler `{name}`", self.label()),
                "Explicit child islands hydrate as independent client scopes. A parent handler \
                 closes over parent client state, so it cannot be serialized into the child island \
                 as an ordinary prop.",
                [
                    "Render the component directly inside the parent island so callback slots can inline.",
                    "Give the child island independent state and communicate through query, mutation, or a custom event.",
                    "Pass `rsjs::noop_handler()` when the child boundary should be inert.",
                ],
                Some("component.island_boundary_handler_prop"),
            ),
            Self::SharedStateProviderSeed => {
                provider_seed_dependency_error(span, format!("rsjs handler `{name}`"))
            }
            Self::StaticLoopPrefix => {
                static_loop_prefix_dependency_error(span, format!("rsjs handler `{name}`"))
            }
        }
    }

    pub(super) fn client_handle_capture_error(
        self,
        span: proc_macro2::Span,
        name: &str,
        kind: &str,
    ) -> syn::Error {
        match self {
            Self::ServerOnly | Self::ServerAttr => guided_boundary_error(
                span,
                format!(
                    "`{}` cannot capture client {kind} handle `{name}`",
                    self.label()
                ),
                "Client handles belong to an RSJS owner and have browser lifecycle semantics. \
                 An opaque server boundary renders once and cannot preserve that ownership.",
                [
                    "Keep the handle use in a normal analyzed client position.",
                    "Pass plain server data into the opaque server expression instead.",
                    "Reacquire query or SharedState identity inside the owner that needs it.",
                ],
                Some("server_boundary.client_handle_capture"),
            ),
            Self::Island => guided_boundary_error(
                span,
                format!(
                    "`{}` cannot capture parent {kind} handle `{name}`",
                    self.label()
                ),
                "Explicit child islands hydrate as independent client owners. Query, mutation, \
                 SharedState, and node-ref handles cannot cross that owner cut.",
                [
                    "Reacquire query or SharedState identity independently inside the child island.",
                    "Keep mutation and node-ref ownership entirely inside one island.",
                    "Pass plain server data across the explicit island boundary.",
                ],
                Some("server_boundary.island_client_handle_capture"),
            ),
            Self::SharedStateProviderSeed => provider_seed_dependency_error(
                span,
                format!("{kind} client handle `{name}`"),
            ),
            Self::StaticLoopPrefix => static_loop_prefix_dependency_error(
                span,
                format!("{kind} client handle `{name}`"),
            ),
        }
    }

    pub(super) fn state_declaration_error(self, span: proc_macro2::Span, name: &str) -> syn::Error {
        match self {
            Self::ServerOnly => guided_boundary_error(
                span,
                format!("`{}` cannot declare or read `{name}(...)`", self.label()),
                "Server-only slots are opaque SSR content. Declaring or reading rsjs state there \
                 would create client state that the hydrated island cannot see.",
                [
                    "Declare `signal`, `query`, or `mutation` handles before the template or in a normal rsjs slot.",
                    "Keep the server-only slot limited to plain server-rendered Proa content.",
                ],
                Some("server_boundary.state_declaration"),
            ),
            Self::ServerAttr => guided_boundary_error(
                span,
                format!("`{}` cannot declare or read `{name}(...)`", self.label()),
                "Server-only attributes are static SSR values, not reactive client state.",
                [
                    "Declare state before the template and use a normal dynamic attribute for reactive values.",
                    "Use `rsjs::server_attr(...)` only for values that are intentionally static after hydration.",
                ],
                Some("server_boundary.state_declaration"),
            ),
            Self::Island => guided_boundary_error(
                span,
                format!("`{}` cannot declare or read parent `{name}(...)` state", self.label()),
                "Explicit child islands are independent client scopes. State must belong to the \
                 parent island or the child island, not both through the boundary expression.",
                [
                    "Declare the state in the parent and keep the reactive markup in the parent island.",
                    "Declare independent state inside the child island.",
                    "Pass plain server data across the island boundary and coordinate later through query, mutation, or custom events.",
                ],
                Some("server_boundary.state_declaration"),
            ),
            Self::SharedStateProviderSeed => {
                provider_seed_dependency_error(span, format!("`rsjs::{name}(...)` state"))
            }
            Self::StaticLoopPrefix => static_loop_prefix_dependency_error(
                span,
                format!("`rsjs::{name}(...)` state"),
            ),
        }
    }

    pub(super) fn client_primitive_error(self, span: proc_macro2::Span, name: &str) -> syn::Error {
        match self {
            Self::ServerOnly | Self::ServerAttr => guided_boundary_error(
                span,
                format!("`{}` cannot declare or read `{name}(...)`", self.label()),
                "Opaque server-only boundaries render static SSR content and are not lowered \
                 into Client IR. A client primitive declared there would either be dropped or \
                 would never hydrate.",
                [
                    "Declare client primitives before the template and render their reachable uses normally.",
                    "Keep `rsjs::server_only(...)` and `rsjs::server_attr(...)` limited to plain server-rendered content.",
                    "Move browser behavior into a handler, binding, client helper, or descriptor prop that rsjs can lower.",
                ],
                Some("server_boundary.client_primitive"),
            ),
            Self::Island => guided_boundary_error(
                span,
                format!("`{}` cannot declare or read parent `{name}(...)` state", self.label()),
                "Explicit child islands are independent client scopes. Client primitives belong \
                 to the parent island or the child island, not both through the boundary expression.",
                [
                    "Declare the primitive in the parent and keep the reactive markup in the parent island.",
                    "Declare independent state inside the child island.",
                    "Pass plain server data across the island boundary and coordinate later through query, mutation, or custom events.",
                ],
                Some("server_boundary.client_primitive"),
            ),
            Self::SharedStateProviderSeed => {
                provider_seed_dependency_error(span, format!("`rsjs::{name}(...)` client value"))
            }
            Self::StaticLoopPrefix => static_loop_prefix_dependency_error(
                span,
                format!("`rsjs::{name}(...)` client value"),
            ),
        }
    }

    pub(super) fn browser_helper_error(self, span: proc_macro2::Span, helper: &str) -> syn::Error {
        match self {
            Self::ServerOnly | Self::ServerAttr => guided_boundary_error(
                span,
                format!("`{}` cannot call browser helper `{helper}`", self.label()),
                "Browser helpers are client IR reads or effects. Opaque server-only boundaries \
                 render once on the server and are not revisited during hydration.",
                [
                    "Move the browser helper into a normal reactive slot, handler, binding, or client helper.",
                    "If the value should be static HTML, compute it from server data instead of browser state.",
                    "Keep `rsjs::server_only(...)` and `rsjs::server_attr(...)` limited to plain server-rendered content.",
                ],
                Some("server_boundary.browser_helper"),
            ),
            Self::Island => guided_boundary_error(
                span,
                format!("`{}` cannot call parent browser helper `{helper}`", self.label()),
                "Explicit child islands hydrate independently. Parent-side browser work cannot \
                 be smuggled through the server-rendered boundary expression.",
                [
                    "Give the child island its own browser helper call.",
                    "Render the child directly inside the parent island if parent browser state must feed it.",
                    "Pass plain server data across the explicit island boundary.",
                ],
                Some("server_boundary.browser_helper"),
            ),
            Self::SharedStateProviderSeed => {
                provider_seed_dependency_error(span, format!("browser helper `{helper}`"))
            }
            Self::StaticLoopPrefix => static_loop_prefix_dependency_error(
                span,
                format!("browser helper `{helper}`"),
            ),
        }
    }

    pub(super) fn client_alias_capture_error(
        self,
        span: proc_macro2::Span,
        name: &str,
    ) -> syn::Error {
        match self {
            Self::ServerOnly | Self::ServerAttr => guided_boundary_error(
                span,
                format!("`{}` cannot capture client-derived prelude local `{name}`", self.label()),
                "This local was inferred from a reachable client expression such as a signal read, \
                 browser read, query/mutation read, node ref, or client helper call. Opaque \
                 server-only boundaries render once on the server and are not lowered into \
                 Client IR.",
                [
                    "Render the local in a normal rsjs reactive slot or dynamic attribute if it should update on the client.",
                    "If the value is intentionally static, compute it from plain server data before the template.",
                    "Keep `rsjs::server_only(...)` and `rsjs::server_attr(...)` limited to plain server-rendered content.",
                ],
                Some("server_boundary.client_alias_capture"),
            ),
            Self::Island => guided_boundary_error(
                span,
                format!("`{}` cannot capture parent client-derived prelude local `{name}`", self.label()),
                "Explicit child islands hydrate as independent client scopes. A parent client \
                 alias closes over parent client state and cannot be serialized as live child \
                 state across that boundary.",
                [
                    "Pass plain server data across the explicit island boundary.",
                    "Keep the reactive markup directly inside the parent island.",
                    "Give the child island its own signal, query, mutation, or custom event channel.",
                ],
                Some("server_boundary.client_alias_capture"),
            ),
            Self::SharedStateProviderSeed => provider_seed_dependency_error(
                span,
                format!("client-derived prelude local `{name}`"),
            ),
            Self::StaticLoopPrefix => static_loop_prefix_dependency_error(
                span,
                format!("client-derived prelude local `{name}`"),
            ),
        }
    }
}

fn provider_seed_dependency_error(
    span: proc_macro2::Span,
    dependency: impl std::fmt::Display,
) -> syn::Error {
    guided_boundary_error(
        span,
        format!(
            "`rsjs::SharedStateProvider {{ state, .. }}` cannot initialize from {dependency}"
        ),
        "Provider initialization is a one-time server seed, not an implicit synchronization \
         channel. Sampling a client-owned value here would freeze the current value and make \
         later updates diverge from SharedState.",
        [
            "Initialize the provider from plain static server data.",
            "Make SharedState the source of truth instead of copying another client cell into it.",
            "Perform an explicit SharedState write from client behavior when synchronization is intentional.",
        ],
        Some("shared_state.forbidden_provider_seed"),
    )
}

fn static_loop_prefix_dependency_error(
    span: proc_macro2::Span,
    dependency: impl std::fmt::Display,
) -> syn::Error {
    guided_boundary_error(
        span,
        format!("a static template-loop prefix cannot capture {dependency}"),
        "Statements before the loop body's tail `html_sync!` / `html!` invocation run only during \
         server rendering. Capturing client state there would freeze its initial value while the \
         generated browser graph continued to treat the local as ordinary server data.",
        [
            "Compute the local only from the loop item and other plain server values.",
            "Move reactive reads into the analyzed HTML expression that should update in the browser.",
            "Keep client effects in a handler, effect, query, mutation, or other explicit client position.",
        ],
        Some("template_loop.reactive_prefix_capture"),
    )
}

impl Analyzer {
    fn opaque_boundary_capture_visitor(
        &self,
        boundary: OpaqueBoundary,
        shadowed_names: HashSet<String>,
    ) -> OpaqueBoundaryReactiveCapture<'_> {
        OpaqueBoundaryReactiveCapture {
            signals: &self.signals,
            queries: &self.queries,
            mutations: &self.mutations,
            node_refs: &self.node_refs,
            shared_stores: &self.shared_stores,
            handlers: &self.handlers,
            client_prelude_locals: &self.client_prelude_locals,
            unsupported_client_prelude_locals: &self.unsupported_client_prelude_locals,
            boundary,
            shadowed_names,
            err: None,
        }
    }

    pub(super) fn reject_reactive_captures_in_opaque_boundary(
        &self,
        expr: &Expr,
        boundary: OpaqueBoundary,
    ) -> syn::Result<()> {
        let mut visitor =
            self.opaque_boundary_capture_visitor(boundary, self.current_loop_local_names());
        visitor.visit_expr(expr);
        match visitor.err {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    fn reject_reactive_captures_in_static_loop_prefix(
        &self,
        statements: &[Stmt],
        pattern: &Pat,
    ) -> syn::Result<()> {
        let mut shadowed_names = self.current_loop_local_names();
        shadowed_names.extend(loop_pattern_locals(pattern));
        let mut visitor =
            self.opaque_boundary_capture_visitor(OpaqueBoundary::StaticLoopPrefix, shadowed_names);
        visitor.visit_statement_sequence(statements);
        match visitor.err {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

pub(super) struct OpaqueBoundaryReactiveCapture<'a> {
    pub(super) signals: &'a HashMap<String, SignalId>,
    pub(super) queries: &'a HashMap<String, QueryId>,
    pub(super) mutations: &'a HashMap<String, MutationId>,
    pub(super) node_refs: &'a HashMap<String, NodeRefId>,
    pub(super) shared_stores: &'a HashMap<String, SharedStoreId>,
    pub(super) handlers: &'a HashMap<String, HandlerLocal>,
    pub(super) client_prelude_locals: &'a HashMap<String, FrontendExpr>,
    pub(super) unsupported_client_prelude_locals:
        &'a HashMap<String, UnsupportedClientPreludeLocal>,
    pub(super) boundary: OpaqueBoundary,
    pub(super) shadowed_names: HashSet<String>,
    pub(super) err: Option<syn::Error>,
}

impl OpaqueBoundaryReactiveCapture<'_> {
    pub(super) fn push_err(&mut self, err: syn::Error) {
        match &mut self.err {
            Some(existing) => existing.combine(err),
            None => self.err = Some(err),
        }
    }

    fn is_shadowed(&self, name: &str) -> bool {
        self.shadowed_names.contains(name)
    }

    fn with_pattern_scope(&mut self, pat: &Pat, visit: impl FnOnce(&mut Self)) {
        let saved = self.shadowed_names.clone();
        self.shadowed_names.extend(pattern_binding_names(pat));
        visit(self);
        self.shadowed_names = saved;
    }

    fn visit_statement_sequence(&mut self, statements: &[Stmt]) {
        for statement in statements {
            if let Stmt::Local(local) = statement {
                // The initializer and `let ... else` divergence execute before
                // the pattern enters lexical scope.
                if let Some(init) = &local.init {
                    self.visit_expr(&init.expr);
                    if let Some((_, diverge)) = &init.diverge {
                        self.visit_expr(diverge);
                    }
                }
                self.shadowed_names
                    .extend(pattern_binding_names(&local.pat));
            } else {
                self.visit_stmt(statement);
            }
        }
    }

    fn report_visible_identifier(&mut self, ident: &syn::Ident) {
        let name = ident.to_string();
        if self.is_shadowed(&name) {
            return;
        }
        if self.signals.contains_key(&name) {
            self.push_err(self.boundary.signal_capture_error(ident.span(), &name));
        }
        if self.handlers.contains_key(&name) {
            self.push_err(self.boundary.handler_capture_error(ident.span(), &name));
        }
        let client_handle_kind = if self.queries.contains_key(&name) {
            Some("query")
        } else if self.mutations.contains_key(&name) {
            Some("mutation")
        } else if self.node_refs.contains_key(&name) {
            Some("node-ref")
        } else if self.shared_stores.contains_key(&name) {
            Some("SharedState")
        } else {
            None
        };
        if let Some(kind) = client_handle_kind {
            self.push_err(
                self.boundary
                    .client_handle_capture_error(ident.span(), &name, kind),
            );
        }
        if self.client_prelude_locals.contains_key(&name)
            || self.unsupported_client_prelude_locals.contains_key(&name)
        {
            self.push_err(
                self.boundary
                    .client_alias_capture_error(ident.span(), &name),
            );
        }
    }
}

impl<'ast> Visit<'ast> for OpaqueBoundaryReactiveCapture<'_> {
    fn visit_expr_path(&mut self, path: &'ast ExprPath) {
        if let Some(ident) = path.path.get_ident() {
            self.report_visible_identifier(ident);
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_macro(&mut self, expression: &'ast syn::ExprMacro) {
        if !matches!(
            self.boundary,
            OpaqueBoundary::SharedStateProviderSeed | OpaqueBoundary::StaticLoopPrefix
        ) {
            return;
        }
        for ident in macro_identifier_references(&expression.mac.tokens) {
            self.report_visible_identifier(&ident);
        }
        for (name, span, kind) in macro_rsjs_client_calls(&expression.mac.tokens) {
            match kind {
                MacroClientCallKind::State => {
                    self.push_err(self.boundary.state_declaration_error(span, &name));
                }
                MacroClientCallKind::Primitive => {
                    self.push_err(self.boundary.client_primitive_error(span, &name));
                }
            }
        }
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(ExprPath {
            path, qself: None, ..
        }) = call.func.as_ref()
        {
            if let Some(last) = path.segments.last() {
                let name = last.ident.to_string();
                if matches!(
                    name.as_str(),
                    "signal"
                        | "shared"
                        | "query"
                        | "query_with_options"
                        | "query_with_initial"
                        | "mutation"
                        | "mutation_with_options"
                ) && !self.is_shadowed(&name)
                    && is_rsjs_root_path(path, &name)
                {
                    self.push_err(
                        self.boundary
                            .state_declaration_error(last.ident.span(), &name),
                    );
                } else if matches!(
                    name.as_str(),
                    "memo" | "node_ref" | "interval" | "handler" | "event_handler"
                ) {
                    self.push_err(
                        self.boundary
                            .client_primitive_error(last.ident.span(), &name),
                    );
                } else if let Some(helper) = rsjs_browser_helper_call_label(path) {
                    self.push_err(self.boundary.browser_helper_error(path.span(), &helper));
                }
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_block(&mut self, block: &'ast Block) {
        let saved = self.shadowed_names.clone();
        self.visit_statement_sequence(&block.stmts);
        self.shadowed_names = saved;
    }

    fn visit_expr_closure(&mut self, closure: &'ast ExprClosure) {
        let saved = self.shadowed_names.clone();
        for input in &closure.inputs {
            self.shadowed_names.extend(pattern_binding_names(input));
        }
        self.visit_expr(&closure.body);
        self.shadowed_names = saved;
    }

    fn visit_expr_for_loop(&mut self, for_loop: &'ast ExprForLoop) {
        self.visit_expr(&for_loop.expr);
        self.with_pattern_scope(&for_loop.pat, |this| this.visit_block(&for_loop.body));
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        self.with_pattern_scope(&arm.pat, |this| {
            if let Some((_, guard)) = &arm.guard {
                this.visit_expr(guard);
            }
            this.visit_expr(&arm.body);
        });
    }

    fn visit_expr_if(&mut self, if_expr: &'ast ExprIf) {
        self.visit_expr(&if_expr.cond);
        let mut bindings = Vec::new();
        collect_condition_pattern_bindings(&if_expr.cond, &mut bindings);
        let saved = self.shadowed_names.clone();
        self.shadowed_names.extend(bindings);
        self.visit_block(&if_expr.then_branch);
        self.shadowed_names = saved;
        if let Some((_, else_expr)) = &if_expr.else_branch {
            self.visit_expr(else_expr);
        }
    }

    fn visit_expr_while(&mut self, while_expr: &'ast syn::ExprWhile) {
        self.visit_expr(&while_expr.cond);
        let mut bindings = Vec::new();
        collect_condition_pattern_bindings(&while_expr.cond, &mut bindings);
        let saved = self.shadowed_names.clone();
        self.shadowed_names.extend(bindings);
        self.visit_block(&while_expr.body);
        self.shadowed_names = saved;
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let saved = self.shadowed_names.clone();
        for input in &item.sig.inputs {
            if let syn::FnArg::Typed(input) = input {
                self.shadowed_names
                    .extend(pattern_binding_names(&input.pat));
            }
        }
        self.visit_block(&item.block);
        self.shadowed_names = saved;
    }
}

pub(super) fn rsjs_browser_helper_call_label(path: &syn::Path) -> Option<String> {
    let mut segments = path.segments.iter();
    let first = segments.next()?;
    if first.ident != "rsjs" {
        return None;
    }
    let module = segments.next()?.ident.to_string();
    let method = segments.next()?.ident.to_string();
    if rsjs_ir::browser_registry::find(&module, &method).is_some()
        || browser_read_method(&module, &method).is_some()
        || browser_effect_method(&module, &method).is_some()
    {
        Some(format!("rsjs::{module}::{method}"))
    } else {
        None
    }
}

/// Walk a branch's tail expression to find the `html_sync!` /
/// `html!` invocation, peeling off `.render(cx)` and similar wrappers.
pub(super) fn locate_branch_macro(expr: &Expr) -> Option<&ExprMacro> {
    match expr {
        Expr::Macro(m) => Some(m),
        Expr::MethodCall(mc) => locate_branch_macro(&mc.receiver),
        Expr::Await(a) => locate_branch_macro(&a.base),
        Expr::Paren(p) => locate_branch_macro(&p.expr),
        Expr::Block(ExprBlock { block, .. }) => match block.stmts.last() {
            Some(Stmt::Expr(e, _)) => locate_branch_macro(e),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn if_expr_is_template_conditional(if_expr: &ExprIf) -> bool {
    branch_tail_is_html_macro(&if_expr.then_branch)
        || if_expr
            .else_branch
            .as_ref()
            .is_some_and(|(_, else_expr)| match else_expr.as_ref() {
                Expr::Block(ExprBlock { block, .. }) => branch_tail_is_html_macro(block),
                Expr::If(nested) => if_expr_is_template_conditional(nested),
                _ => false,
            })
}

pub(super) fn branch_tail_is_html_macro(block: &Block) -> bool {
    match block.stmts.last() {
        Some(Stmt::Expr(expr, _)) => locate_branch_macro(expr)
            .map(|mac| is_html_macro_path(&mac.mac.path))
            .unwrap_or(false),
        Some(Stmt::Macro(stmt)) => is_html_macro_path(&stmt.mac.path),
        _ => false,
    }
}

pub(super) fn is_html_macro_path(path: &syn::Path) -> bool {
    path.segments
        .last()
        .map(|s| {
            let ident = s.ident.to_string();
            ident == "html_sync" || ident == "html"
        })
        .unwrap_or(false)
}

pub(super) struct TailHtmlMacro {
    pub(super) path: syn::Path,
    pub(super) tokens: proc_macro2::TokenStream,
    pub(super) span: proc_macro2::Span,
    pub(super) prefix: proc_macro2::TokenStream,
    /// Top-level bindings declared before the tail HTML macro. They remain in
    /// lexical scope inside that repeated template and therefore have ordinary
    /// loop-local value semantics rather than unbound unit-component syntax.
    pub(super) prefix_locals: HashSet<String>,
}

pub(super) fn loop_tail_html_macro(block: &Block) -> syn::Result<TailHtmlMacro> {
    let (last, prefix) = block.stmts.split_last().ok_or_else(|| {
        syn::Error::new(
            block.span(),
            "`#[rsjs]` expects a template loop body to end with an \
             `html_sync!` / `html!` invocation; empty loop bodies are not supported",
        )
    })?;

    let (path, tokens, span) = match last {
        Stmt::Expr(expr, _) => {
            let mac = locate_branch_macro(expr).ok_or_else(|| {
                syn::Error::new(
                    expr.span(),
                    "`#[rsjs]` expects a template loop body's last expression to be \
                     `html_sync! { ... }` (or `html! { ... }`)",
                )
            })?;
            (mac.mac.path.clone(), mac.mac.tokens.clone(), mac.span())
        }
        Stmt::Macro(sm) => (sm.mac.path.clone(), sm.mac.tokens.clone(), sm.span()),
        stmt => {
            return Err(syn::Error::new(
                stmt.span(),
                "`#[rsjs]` expects a template loop body's last statement to be an \
                 `html_sync!` / `html!` invocation",
            ));
        }
    };

    let mut prefix_tokens = proc_macro2::TokenStream::new();
    let mut prefix_locals = HashSet::new();
    for stmt in prefix {
        stmt.to_tokens(&mut prefix_tokens);
        if let Stmt::Local(local) = stmt {
            prefix_locals.extend(loop_pattern_locals(&local.pat));
        }
    }

    Ok(TailHtmlMacro {
        path,
        tokens,
        span,
        prefix: prefix_tokens,
        prefix_locals,
    })
}

pub(super) fn validate_nested_html_macro(path: &syn::Path, context: &str) -> syn::Result<()> {
    if is_html_macro_path(path) {
        Ok(())
    } else {
        Err(syn::Error::new(
            path.span(),
            format!("`#[rsjs]` {context}s must use `html_sync!` / `html!`"),
        ))
    }
}

/// Match the v0.1 keyed-list item-expression grammar: the loop item
/// itself (`item`) or one named field on it (`row.id`). Returns the
/// lowered [`ListItemExpr`] plus the original Rust tokens so emit_rust
/// can splice the same expression into the SSR replay.
pub(super) fn keyed_list_item_expr(expr: &Expr, item_name: &str) -> Option<(ListItemExpr, String)> {
    match expr {
        Expr::Path(ExprPath { path, .. }) if path.is_ident(item_name) => {
            Some((ListItemExpr::Item, expr.to_token_stream().to_string()))
        }
        Expr::Field(field) => {
            let Expr::Path(ExprPath { path, .. }) = field.base.as_ref() else {
                return None;
            };
            if !path.is_ident(item_name) {
                return None;
            }
            let syn::Member::Named(member) = &field.member else {
                return None;
            };
            Some((
                ListItemExpr::ItemField(member.to_string()),
                expr.to_token_stream().to_string(),
            ))
        }
        _ => None,
    }
}

enum KeyedListIterator {
    Signal(SignalId),
    Query(QueryListIterator),
}

impl KeyedListIterator {
    fn ssr_iter(&self) -> String {
        match self {
            Self::Signal(signal) => {
                format!("& {}", crate::emit_rust::signal_local_ident(*signal))
            }
            Self::Query(query) => query.ssr_iter.clone(),
        }
    }

    fn list_source(&self) -> ListSource {
        match self {
            Self::Signal(signal) => ListSource::signal(*signal),
            Self::Query(query) => ListSource::expr(query.expr),
        }
    }

    fn sidecar_source(&self) -> ListHirSource {
        match self {
            Self::Signal(signal) => ListHirSource::Signal(*signal),
            Self::Query(query) => ListHirSource::Expr(query.expr),
        }
    }
}

fn rust_member_suffix(member: &syn::Member) -> String {
    match member {
        syn::Member::Named(ident) => {
            format!(".{}", ident.to_token_stream().to_string().replace(' ', ""))
        }
        syn::Member::Unnamed(index) => format!(".{}", index.index),
    }
}

/// Finds the first un-shadowed signal read inside a static template
/// loop's iterator expression; see
/// [`Analyzer::reject_signal_in_static_loop_iterator`].
pub(super) struct IteratorSignalUseVisitor<'a> {
    pub(super) signals: &'a HashMap<String, SignalId>,
    pub(super) loop_scopes: &'a [LoopScope],
    pub(super) found: Option<(proc_macro2::Span, String)>,
}

impl<'ast> Visit<'ast> for IteratorSignalUseVisitor<'_> {
    fn visit_expr_path(&mut self, path: &'ast ExprPath) {
        if self.found.is_none() {
            if let Some(ident) = path.path.get_ident() {
                let name = ident.to_string();
                let shadowed = self
                    .loop_scopes
                    .iter()
                    .any(|scope| scope.locals.contains(&name));
                if !shadowed && self.signals.contains_key(&name) {
                    self.found = Some((ident.span(), name));
                }
            }
        }
        visit::visit_expr_path(self, path);
    }
}

/// Match `vec![<literal>, ...]` (including the empty `vec![]`) and
/// lower the elements to [`Value`]s for an [`FrontendExpr::ArrayLit`].
/// Returns `Ok(None)` when the expression is not a `vec!` macro at
/// all, and an error when it is one whose elements fall outside the
/// v0.1 literal grammar.
pub(super) fn vec_literal_array_value(expr: &Expr) -> syn::Result<Option<Vec<Value>>> {
    let Expr::Macro(em) = expr else {
        return Ok(None);
    };
    let is_vec = em
        .mac
        .path
        .segments
        .last()
        .map(|seg| seg.ident == "vec")
        .unwrap_or(false);
    if !is_vec {
        return Ok(None);
    }
    if em.mac.tokens.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let elems = em
        .mac
        .parse_body_with(syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated)
        .map_err(|_| vec_literal_elements_error(em.span()))?;
    let mut out = Vec::with_capacity(elems.len());
    for elem in &elems {
        out.push(
            vec_literal_element_value(elem).map_err(|_| vec_literal_elements_error(elem.span()))?,
        );
    }
    Ok(Some(out))
}

/// Lower the representation-transparent `Option` constructors accepted by
/// reactive expression lowering inside a literal vector. Keeping this grammar
/// here (rather than guessing from a final JSON value) preserves the source
/// distinction long enough for final helper-signature typing to specialize a
/// nested `Vec<Option<i64/u64>>` leaf.
fn vec_literal_element_value(expr: &Expr) -> syn::Result<Value> {
    match expr {
        Expr::Paren(paren) => vec_literal_element_value(&paren.expr),
        Expr::Group(group) => vec_literal_element_value(&group.expr),
        Expr::Call(call)
            if matches!(call.func.as_ref(), Expr::Path(path) if path.path.is_ident("Some"))
                && call.args.len() == 1 =>
        {
            vec_literal_element_value(
                call.args
                    .first()
                    .expect("one-argument Some was checked above"),
            )
        }
        Expr::Path(path) if path.path.is_ident("None") => Ok(Value::Null),
        _ => literal_to_value(expr),
    }
}

pub(super) fn vec_literal_elements_error(span: proc_macro2::Span) -> syn::Error {
    syn::Error::new(
        span,
        "`vec![...]` in an rsjs client expression supports literal elements in v0.1 \
         (`\"a\".to_string()`, `1_i64`, `Some(true)`, `None`); computed elements and the \
         `vec![value; n]` repeat form cannot be lowered to client JS yet. \
         Build the list on the server, or drive the signal from client \
         runtime code",
    )
}

pub(super) fn loop_pattern_locals(pat: &Pat) -> HashSet<String> {
    let mut visitor = LoopPatternLocalVisitor {
        locals: HashSet::new(),
    };
    visitor.visit_pat(pat);
    visitor.locals
}

pub(super) struct LoopPatternLocalVisitor {
    pub(super) locals: HashSet<String>,
}

impl<'ast> Visit<'ast> for LoopPatternLocalVisitor {
    fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
        self.locals.insert(pat.ident.to_string());
        visit::visit_pat_ident(self, pat);
    }
}

pub(super) struct LoopLocalVisitor<'a> {
    pub(super) scopes: &'a [LoopScope],
    pub(super) found: bool,
}

impl<'ast> Visit<'ast> for LoopLocalVisitor<'_> {
    fn visit_expr_path(&mut self, path: &'ast ExprPath) {
        if let Some(ident) = path.path.get_ident() {
            let name = ident.to_string();
            if self
                .scopes
                .iter()
                .rev()
                .any(|scope| scope.locals.contains(&name))
            {
                self.found = true;
                return;
            }
        }
        visit::visit_expr_path(self, path);
    }
}

#[derive(Default)]
struct InboundSelfFieldVisitor {
    fields: Vec<(String, proc_macro2::Span)>,
}

impl<'ast> Visit<'ast> for InboundSelfFieldVisitor {
    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        if expr_is_self_path(&field.base) {
            if let syn::Member::Named(ident) = &field.member {
                self.fields.push((ident.to_string(), ident.span()));
            }
        }
        visit::visit_expr_field(self, field);
    }

    fn visit_expr_macro(&mut self, expression: &'ast syn::ExprMacro) {
        collect_macro_self_fields(&expression.mac.tokens, &mut self.fields);
    }
}

#[derive(Default)]
struct ExprMacroPresence {
    first: Option<proc_macro2::Span>,
}

impl<'ast> Visit<'ast> for ExprMacroPresence {
    fn visit_expr_macro(&mut self, expression: &'ast syn::ExprMacro) {
        if self.first.is_none() {
            self.first = Some(expression.span());
        }
    }
}

#[derive(Clone, Copy)]
enum MacroClientCallKind {
    State,
    Primitive,
}

fn macro_identifier_references(tokens: &proc_macro2::TokenStream) -> Vec<syn::Ident> {
    fn collect(tokens: &proc_macro2::TokenStream, output: &mut Vec<syn::Ident>) {
        let tokens = tokens.clone().into_iter().collect::<Vec<_>>();
        for (index, token) in tokens.iter().enumerate() {
            if let proc_macro2::TokenTree::Group(group) = token {
                collect(&group.stream(), output);
            }
            let proc_macro2::TokenTree::Ident(ident) = token else {
                continue;
            };
            let follows_member_dot = index
                .checked_sub(1)
                .and_then(|index| tokens.get(index))
                .is_some_and(|token| matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '.'));
            let follows_path_separator = index >= 2
                && matches!(&tokens[index - 1], proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ':')
                && matches!(&tokens[index - 2], proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ':');
            let is_field_label = matches!(
                tokens.get(index + 1),
                Some(proc_macro2::TokenTree::Punct(punct)) if punct.as_char() == ':'
            ) && !matches!(
                tokens.get(index + 2),
                Some(proc_macro2::TokenTree::Punct(punct)) if punct.as_char() == ':'
            );
            if !follows_member_dot && !follows_path_separator && !is_field_label {
                output.push(ident.clone());
            }
        }
    }

    let mut output = Vec::new();
    collect(tokens, &mut output);
    output
}

fn macro_rsjs_client_calls(
    tokens: &proc_macro2::TokenStream,
) -> Vec<(String, proc_macro2::Span, MacroClientCallKind)> {
    fn collect(
        tokens: &proc_macro2::TokenStream,
        output: &mut Vec<(String, proc_macro2::Span, MacroClientCallKind)>,
    ) {
        let tokens = tokens.clone().into_iter().collect::<Vec<_>>();
        for token in &tokens {
            if let proc_macro2::TokenTree::Group(group) = token {
                collect(&group.stream(), output);
            }
        }
        for window in tokens.windows(4) {
            let (
                proc_macro2::TokenTree::Ident(root),
                proc_macro2::TokenTree::Punct(first_colon),
                proc_macro2::TokenTree::Punct(second_colon),
                proc_macro2::TokenTree::Ident(name),
            ) = (&window[0], &window[1], &window[2], &window[3])
            else {
                continue;
            };
            if root != "rsjs" || first_colon.as_char() != ':' || second_colon.as_char() != ':' {
                continue;
            }
            let name_string = name.to_string();
            let kind = if matches!(
                name_string.as_str(),
                "signal"
                    | "shared"
                    | "query"
                    | "query_with_options"
                    | "query_with_initial"
                    | "mutation"
                    | "mutation_with_options"
            ) {
                Some(MacroClientCallKind::State)
            } else if matches!(
                name_string.as_str(),
                "memo" | "node_ref" | "interval" | "handler" | "event_handler"
            ) {
                Some(MacroClientCallKind::Primitive)
            } else {
                None
            };
            if let Some(kind) = kind {
                output.push((name_string, name.span(), kind));
            }
        }
    }

    let mut output = Vec::new();
    collect(tokens, &mut output);
    output
}

fn collect_macro_self_fields(
    tokens: &proc_macro2::TokenStream,
    output: &mut Vec<(String, proc_macro2::Span)>,
) {
    let tokens = tokens.clone().into_iter().collect::<Vec<_>>();
    for token in &tokens {
        if let proc_macro2::TokenTree::Group(group) = token {
            collect_macro_self_fields(&group.stream(), output);
        }
    }
    for window in tokens.windows(3) {
        let (
            proc_macro2::TokenTree::Ident(base),
            proc_macro2::TokenTree::Punct(dot),
            proc_macro2::TokenTree::Ident(field),
        ) = (&window[0], &window[1], &window[2])
        else {
            continue;
        };
        if base == "self" && dot.as_char() == '.' {
            output.push((field.to_string(), field.span()));
        }
    }
}
