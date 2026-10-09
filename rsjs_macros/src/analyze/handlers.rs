// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Event handler closure lowering: statement grammar, assignments, match/if/loop forms, and payload handling.
use super::*;

fn merge_handler_loop_integer_sources(
    binding: Option<IntegerSource>,
    iterable: Option<IntegerSource>,
    span: proc_macro2::Span,
) -> syn::Result<Option<IntegerSource>> {
    match (binding, iterable) {
        (Some(IntegerSource::Unsupported(name)), _)
        | (_, Some(IntegerSource::Unsupported(name))) => {
            Err(unsupported_integer_domain_error(span, &name))
        }
        (Some(IntegerSource::Supported(binding)), Some(IntegerSource::Supported(iterable)))
            if binding != iterable =>
        {
            Err(integer_domain_mismatch_error(
                span,
                iterable,
                binding,
                "typed handler loop binding",
            ))
        }
        (Some(source), _) | (None, Some(source)) => Ok(Some(source)),
        (None, None) => Ok(None),
    }
}

impl Analyzer {
    /// Lower an event-handler attribute value to compatibility and Client IR.
    ///
    /// Accepts either:
    ///
    /// - a closure `move |event| <body>` / `|event| <body>`;
    /// - a direct call to a local `rsjs::handler(...)` ref, e.g.
    ///   `on_click={on_save(())}`.
    ///
    /// The body can be a single statement or a block of statements.
    pub(super) fn lower_handler(
        &mut self,
        value: &Expr,
        event_node: NodeId,
    ) -> syn::Result<HandlerLowering> {
        let root_scope = HandlerScope {
            event_node: Some(event_node),
            ..HandlerScope::default()
        };
        if let Expr::MethodCall(method_call) = value {
            if let Some(stmt) = self.lower_callback_slot_method_call(method_call, &root_scope)? {
                return lower_handler_hir_to_client_lowering(vec![stmt]);
            }
        }
        if let Expr::Call(call) = value {
            if is_rsjs_debounce_call(&call.func) {
                return self.lower_debounce_handler(call, event_node);
            }
            if let Some(stmt) = self.lower_callback_slot_call(call, &root_scope)? {
                return lower_handler_hir_to_client_lowering(vec![stmt]);
            }
            if let Some(statements) = self.lower_handler_ref_call_hir(call, &root_scope)? {
                let source = handler_local_call_source(call, &self.handlers);
                let mut lowered = lower_handler_hir_to_client_lowering(statements)?;
                if let Some(handler) = source {
                    lowered.source_name = Some(handler.name.to_string());
                    lowered.source_anchor = Some(handler.source_anchor);
                }
                return Ok(lowered);
            }
        }

        let Expr::Closure(closure) = value else {
            return Err(syn::Error::new(
                value.span(),
                "`#[rsjs]` expects an event-handler attribute to receive a closure \
                 (e.g. `on_click={|_| count.update(|c| c + 1)}`) or a direct \
                 call to a local `rsjs::handler(...)` / `rsjs::event_handler(...)` ref \
                 (e.g. `on_click={on_save(())}`)",
            ));
        };
        if closure.inputs.len() > 1 {
            return Err(syn::Error::new(
                closure.span(),
                "`#[rsjs]` event handlers accept at most one parameter (the event); \
                 multi-arg handlers are not supported",
            ));
        }

        let event_param = event_handler_param(closure)?;
        let scope = HandlerScope {
            handler_param: None,
            event_param: event_param.as_deref(),
            handler_locals: Vec::new(),
            event_node: Some(event_node),
        };
        let statements = self.lower_handler_closure_hir_body(closure, &scope)?;
        lower_handler_hir_to_client_lowering(statements)
    }

    /// Lower an `on_*={rsjs::debounce(|event| ..., delay_ms)}` attribute
    /// value: the closure body is the ordinary event-handler statement
    /// grammar, and `ms` records the trailing-edge debounce window the
    /// JS emitter wraps the handler with.
    pub(super) fn lower_debounce_handler(
        &mut self,
        call: &ExprCall,
        event_node: NodeId,
    ) -> syn::Result<HandlerLowering> {
        expect_call_arg_count(call, 2, "rsjs::debounce(|event| ..., delay_ms)")?;
        let Expr::Closure(closure) = &call.args[0] else {
            return Err(syn::Error::new(
                call.args[0].span(),
                "`rsjs::debounce(handler, delay_ms)` expects a closure as its first argument, \
                 e.g. `on_input={rsjs::debounce(|event| query.set(event.value()), 300)}`",
            ));
        };
        let ms = timer_ms_literal(&call.args[1], "debounce")?;
        let event_param = event_handler_param(closure)?;
        let scope = HandlerScope {
            handler_param: None,
            event_param: event_param.as_deref(),
            handler_locals: Vec::new(),
            event_node: Some(event_node),
        };
        let statements = self.lower_handler_closure_hir_body(closure, &scope)?;
        let mut lowered = lower_handler_hir_to_client_lowering(statements)?;
        lowered.debounce_ms = Some(ms);
        Ok(lowered)
    }

    pub(super) fn lower_handler_closure_hir_body(
        &mut self,
        closure: &ExprClosure,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        let body_expr = closure.body.as_ref();
        let mut scope = scope.clone();
        let mut statements = Vec::new();
        match body_expr {
            Expr::Block(ExprBlock { block, .. }) => {
                for stmt in &block.stmts {
                    statements.extend(self.lower_handler_hir_stmt(stmt, &mut scope)?);
                }
            }
            other => statements.extend(self.lower_handler_hir_expr_stmt(other, &scope)?),
        }
        Ok(statements)
    }

    pub(super) fn lower_handler_hir_stmt(
        &mut self,
        stmt: &Stmt,
        scope: &mut HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        match stmt {
            Stmt::Expr(e, _) => self.lower_handler_hir_expr_stmt(e, scope),
            Stmt::Macro(m) => Err(syn::Error::new(
                m.mac.path.span(),
                "`#[rsjs]` event-handler bodies cannot contain macro statements in v0.1",
            )),
            Stmt::Local(l) => self.lower_handler_hir_let(l, scope).map(|stmt| vec![stmt]),
            Stmt::Item(i) => Err(syn::Error::new(
                i.span(),
                "`#[rsjs]` event-handler bodies cannot contain item declarations",
            )),
        }
    }

    pub(super) fn alloc_handler_local(&mut self) -> LocalId {
        let id = LocalId(self.next_handler_local_id);
        self.next_handler_local_id += 1;
        id
    }

    pub(super) fn lower_handler_hir_let(
        &mut self,
        local: &Local,
        scope: &mut HandlerScope<'_>,
    ) -> syn::Result<HandlerHirStmt> {
        let HandlerLocalDecl { name, mutable } = handler_local_binding(local)?;
        if scope.event_param == Some(name.as_str()) {
            return Err(syn::Error::new(
                local.pat.span(),
                "`#[rsjs]` handler-local `let` bindings cannot shadow the event parameter",
            ));
        }
        if let Some((param_name, _)) = scope.handler_param {
            if param_name == name {
                return Err(syn::Error::new(
                    local.pat.span(),
                    "`#[rsjs]` handler-local `let` bindings cannot shadow a handler payload parameter",
                ));
            }
        }
        if scope.handler_locals.iter().any(|local| local.name == name) {
            return Err(syn::Error::new(
                local.pat.span(),
                "`#[rsjs]` handler-local `let` bindings cannot shadow another handler local yet",
            ));
        }
        let init = local.init.as_ref().ok_or_else(|| {
            syn::Error::new(
                local.span(),
                "`#[rsjs]` handler-local `let` bindings require an initializer",
            )
        })?;
        if let Some((_, diverge_expr)) = &init.diverge {
            return Err(syn::Error::new(
                diverge_expr.span(),
                "`#[rsjs]` handler-local `let ... else` bindings are not supported",
            ));
        }
        let integer_source = handler_pat_integer_source(&local.pat);
        if let Some(IntegerSource::Unsupported(name)) = integer_source.as_ref() {
            return Err(unsupported_integer_domain_error(local.pat.span(), name));
        }
        let explicit_init_source =
            if let Some(IntegerSource::Supported(domain)) = integer_source.as_ref() {
                let source =
                    explicit_integer_source_in_expr(&init.expr, "typed handler-local initializer")?;
                validate_explicit_integer_source_against_domain(
                    source.as_ref(),
                    *domain,
                    init.expr.span(),
                    "typed handler-local initializer",
                )?;
                source
            } else {
                None
            };
        let mut value = if receiver_is_event_current_target(&init.expr, scope.event_param) {
            HandlerHirExpr::new(init.expr.span(), FrontendExpr::EventCurrentTarget)
        } else {
            self.lower_handler_payload_hir_expr(&init.expr, scope)?
        };
        if let Some(IntegerSource::Supported(domain)) = integer_source.as_ref() {
            let inferred_before = self.frontend_integer_domain(&value.expr, &scope.handler_locals);
            let literal_only = integer_literal_only(&value.expr);
            self.validate_integer_value_for_domain(
                &mut value.expr,
                explicit_init_source,
                *domain,
                &scope.handler_locals,
                local.span(),
                "typed handler-local initializer",
                true,
            )?;
            if inferred_before.is_none() && !literal_only {
                value.expr = self.anchor_integer_domain(value.expr, *domain);
            }
        }
        let inferred_domain = self.frontend_integer_domain(&value.expr, &scope.handler_locals);
        let integer_domain = match integer_source.as_ref() {
            Some(IntegerSource::Supported(domain)) => Some(*domain),
            _ => inferred_domain,
        };
        let unsupported_integer_domain = match integer_source.as_ref() {
            Some(IntegerSource::Unsupported(name)) => Some(name.clone()),
            _ => None,
        };
        let inferred_ty = self.frontend_expr_type(&value.expr, &scope.handler_locals);
        let ty = integer_domain
            .map(IntegerDomain::client_type)
            .unwrap_or(inferred_ty);
        let constant = (!mutable && integer_source.is_none() && integer_literal_only(&value.expr))
            .then(|| value.expr.clone());
        let id = self.alloc_handler_local();
        scope.handler_locals.push(HandlerLocalBinding {
            name: name.clone(),
            id,
            mutable,
            ty: ty.clone(),
            integer_domain,
            unsupported_integer_domain,
            constant,
        });
        Ok(HandlerHirStmt::new(
            local.span(),
            HandlerHirStmtKind::Let {
                local: id,
                name,
                ty,
                value,
            },
        ))
    }

    pub(super) fn validate_handler_match_binding(
        &self,
        name: &str,
        span: proc_macro2::Span,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<()> {
        if scope.event_param == Some(name) {
            return Err(syn::Error::new(
                span,
                "`#[rsjs]` handler `Some(value)` match bindings cannot shadow the event parameter",
            ));
        }
        if let Some((param_name, _)) = scope.handler_param {
            if param_name == name {
                return Err(syn::Error::new(
                    span,
                    "`#[rsjs]` handler `Some(value)` match bindings cannot shadow a handler payload parameter",
                ));
            }
        }
        if scope.handler_locals.iter().any(|local| local.name == name) {
            return Err(syn::Error::new(
                span,
                "`#[rsjs]` handler `Some(value)` match bindings cannot shadow another handler local yet",
            ));
        }
        Ok(())
    }

    pub(super) fn lower_handler_hir_expr_stmt(
        &mut self,
        expr: &Expr,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        // Patterns we recognise:
        //   signal.set(<expr>) -> HandlerHirStmtKind::SignalSet
        //   signal.update(|c| <expr>) -> HandlerHirStmtKind::SignalUpdate
        //   handler_ref(<expr>) -> inline local rsjs::handler body
        //   <expr> -> HandlerHirStmtKind::Expr
        //
        // Round-4 review #4: unsupported control-flow constructs
        // (`match`, `while`, `loop`, `for`) used to fall through to
        // the generic `lower_reactive_expr` rejection, which spelled
        // them as "this is not a reactive expression". The user's
        // mental model is "branching in handlers", not "this is a
        // reactive expr", so we surface a per-shape diagnostic that
        // names the construct and points at the current IR boundary.
        if let Expr::If(if_expr) = expr {
            return self.lower_handler_hir_if(if_expr, scope);
        }
        if let Expr::Match(match_expr) = expr {
            return self.lower_handler_hir_match(match_expr, scope);
        }
        if let Expr::ForLoop(for_loop) = expr {
            return self.lower_handler_hir_static_for_loop(for_loop, scope);
        }
        if let Expr::Assign(assign) = expr {
            return self
                .lower_handler_hir_assign(assign, scope)
                .map(|stmt| vec![stmt]);
        }
        if let Expr::Binary(binary) = expr {
            if is_compound_assignment_op(&binary.op) {
                return self
                    .lower_handler_hir_compound_assign(binary, scope)
                    .map(|stmt| vec![stmt]);
            }
        }
        if let Some(err) = unsupported_handler_control_flow(expr) {
            return Err(err);
        }
        if let Expr::Call(call) = expr {
            if is_rsjs_handler_constructor_call(&call.func, "batch") {
                if call.args.len() != 1 {
                    return Err(syn::Error::new(
                        call.span(),
                        "`rsjs::batch(...)` takes exactly one zero-argument closure",
                    ));
                }
                let Expr::Closure(closure) = &call.args[0] else {
                    return Err(syn::Error::new(
                        call.args[0].span(),
                        "`rsjs::batch(...)` expects a closure, e.g. `rsjs::batch(|| { first.set(1); second.set(2); })`",
                    ));
                };
                if !closure.inputs.is_empty() {
                    return Err(syn::Error::new(
                        closure.inputs.span(),
                        "`rsjs::batch(...)` closures take no parameters",
                    ));
                }
                return self.lower_handler_closure_hir_body(closure, scope);
            }
            if let Some(stmt) = self.lower_callback_slot_call(call, scope)? {
                return Ok(vec![stmt]);
            }
            if let Some(statements) = self.lower_handler_ref_call_hir(call, scope)? {
                return Ok(statements);
            }
            if let Some(stmt) = self.lower_console_call(call, scope)? {
                return Ok(vec![stmt]);
            }
            if let Some(stmt) = self.lower_browser_effect_call(call, scope)? {
                return Ok(vec![stmt]);
            }
        }
        if let Expr::MethodCall(mc) = expr {
            let method = mc.method.to_string();
            if let Some(stmt) = self.lower_callback_slot_method_call(mc, scope)? {
                return Ok(vec![stmt]);
            }
            if receiver_is_event_param(&mc.receiver, scope.event_param) {
                if let Some(effect) = self.lower_event_browser_effect(mc, &method, scope)? {
                    return Ok(vec![HandlerHirStmt::new(
                        expr.span(),
                        HandlerHirStmtKind::BrowserEffect(effect),
                    )]);
                }
                if mc.args.is_empty() {
                    match method.as_str() {
                        "prevent_default" => {
                            return Ok(vec![HandlerHirStmt::new(
                                expr.span(),
                                HandlerHirStmtKind::EventPreventDefault,
                            )])
                        }
                        "stop_propagation" => {
                            return Ok(vec![HandlerHirStmt::new(
                                expr.span(),
                                HandlerHirStmtKind::EventStopPropagation,
                            )]);
                        }
                        _ => {}
                    }
                }
                if matches!(method.as_str(), "prevent_default" | "stop_propagation") {
                    return Err(syn::Error::new(
                        mc.span(),
                        format!("`event.{method}()` takes no arguments"),
                    ));
                }
            }
            if method == "set" {
                let id = self.expect_signal_receiver(&mc.receiver, "set")?;
                self.reject_memo_write(id, mc.method.span(), "call `.set(...)` on it")?;
                if mc.args.len() != 1 {
                    return Err(syn::Error::new(
                        mc.span(),
                        "`signal.set(value)` takes exactly one argument",
                    ));
                }
                if let Some(name) = self.unsupported_integer_signals.get(&id) {
                    return Err(unsupported_integer_domain_error(mc.method.span(), name));
                }
                // Like `.update(...)` below, a shared field starts as `Dyn`
                // because its concrete Rust field type is not present in the
                // island tokens. An explicit suffix/cast in the value is still
                // a sound type boundary: Rust checks the call against the real
                // `SharedField<T>::set` signature. Establish that domain before
                // lowering so an expression such as
                // `count.set(count.get() + 1_i64)` takes the checked i64 path.
                let shared_receiver = self
                    .builder
                    .signal_decls()
                    .get(id.0 as usize)
                    .is_some_and(|decl| decl.shared.is_some());
                let explicit_value_source =
                    explicit_integer_source_in_expr(&mc.args[0], "signal.set(...) value")?;
                if shared_receiver && self.builder.signal_integer_domain(id).is_none() {
                    match explicit_value_source.as_ref() {
                        Some(IntegerSource::Supported(domain)) => {
                            self.builder.set_signal_integer_domain(id, Some(*domain));
                            self.builder.set_signal_ty(id, domain.client_type());
                        }
                        Some(IntegerSource::Unsupported(name)) => {
                            return Err(unsupported_integer_domain_error(mc.args[0].span(), name));
                        }
                        None => {}
                    }
                }
                let receiver_domain = self.builder.signal_integer_domain(id);
                if let Some(domain) = receiver_domain {
                    validate_explicit_integer_source_against_domain(
                        explicit_value_source.as_ref(),
                        domain,
                        mc.args[0].span(),
                        "signal.set(...) value",
                    )?;
                }
                if receiver_domain.is_some() && matches!(&mc.args[0], Expr::Array(_)) {
                    return Err(syn::Error::new(
                        mc.args[0].span(),
                        "rsjs/integer-domain-mismatch: a fixed-width integer signal cannot be set to an array",
                    ));
                }
                if let Some(elems) = vec_literal_array_value(&mc.args[0])? {
                    return Ok(vec![HandlerHirStmt::new(
                        expr.span(),
                        HandlerHirStmtKind::SignalSet {
                            id,
                            value: HandlerHirExpr::new(
                                mc.args[0].span(),
                                FrontendExpr::ArrayLit(elems),
                            ),
                        },
                    )]);
                }
                let mut value = self.lower_handler_payload_hir_expr(&mc.args[0], scope)?;
                // A typed handler local is another exact source boundary. It
                // is visible only after lowering, so specialize here if the
                // source expression itself carried no suffix/cast.
                if shared_receiver && self.builder.signal_integer_domain(id).is_none() {
                    match self.resolved_integer_source(
                        explicit_value_source.as_ref(),
                        &value.expr,
                        &scope.handler_locals,
                    ) {
                        Some(IntegerSource::Supported(domain)) => {
                            self.builder.set_signal_integer_domain(id, Some(domain));
                            self.builder.set_signal_ty(id, domain.client_type());
                        }
                        Some(IntegerSource::Unsupported(name)) => {
                            return Err(unsupported_integer_domain_error(mc.args[0].span(), &name));
                        }
                        None => {}
                    }
                }
                let receiver_domain = self.builder.signal_integer_domain(id);
                if let Some(domain) = receiver_domain {
                    self.validate_integer_value_for_domain(
                        &mut value.expr,
                        explicit_value_source,
                        domain,
                        &scope.handler_locals,
                        mc.args[0].span(),
                        "signal.set(...) value",
                        false,
                    )?;
                }
                // Proven fixed-domain receivers were fully validated and
                // contextualized above: i64/u64 writes are BigInts even for
                // small values, while i32/u32 writes are integer Numbers.
                //
                // The remaining representation rewrite is compatibility logic
                // for legacy type-only or schema-less non-arithmetic values.
                // A runtime-pinned shared-state receiver is left untouched;
                // shared fixed-width arithmetic must establish a domain before
                // this point.
                let receiver_dyn = self.builder.signal_ty(id).is_some_and(ClientType::is_dyn);
                let receiver_wide = self
                    .builder
                    .signal_ty(id)
                    .is_some_and(ClientType::is_wide_int);
                if receiver_domain.is_some() {
                    // Exact-domain validation above also contextualized literals.
                } else if receiver_dyn {
                    // Leave literals in source form.
                } else if receiver_wide {
                    promote_int_literals_to_wide(&mut value.expr);
                } else if !self
                    .frontend_numeric_anchors(&value.expr, &scope.handler_locals)
                    .wide
                {
                    demote_bigint_literals(&mut value.expr);
                }
                return Ok(vec![HandlerHirStmt::new(
                    expr.span(),
                    HandlerHirStmtKind::SignalSet { id, value },
                )]);
            }
            if method == "update" {
                let id = self.expect_signal_receiver(&mc.receiver, "update")?;
                self.reject_memo_write(id, mc.method.span(), "call `.update(...)` on it")?;
                if mc.args.len() != 1 {
                    return Err(syn::Error::new(
                        mc.span(),
                        "`signal.update(|c| ...)` takes exactly one closure argument",
                    ));
                }
                let closure = match &mc.args[0] {
                    Expr::Closure(c) => c,
                    other => {
                        return Err(syn::Error::new(
                            other.span(),
                            "`signal.update(...)` expects a one-arg closure: `|c| <body>`",
                        ));
                    }
                };
                if let Some(name) = self.unsupported_integer_signals.get(&id) {
                    return Err(unsupported_integer_domain_error(mc.method.span(), name));
                }
                // A shared field enters this macro as `Dyn`: its concrete Rust
                // type lives behind the generated state handle. An explicitly
                // typed update still gives us a sound source-level boundary,
                // either through `|value: u32| ...` or a suffixed result such
                // as `value + 1_u32`. Rust type-checks that boundary against
                // `SharedField<T>::update`, so specialize the shared cell before
                // lowering the closure body and use the ordinary checked path.
                let shared_receiver = self
                    .builder
                    .signal_decls()
                    .get(id.0 as usize)
                    .is_some_and(|decl| decl.shared.is_some());
                let update_result_source = simple_closure_explicit_integer_source(closure)?;
                if shared_receiver && self.builder.signal_integer_domain(id).is_none() {
                    let source = if let Some(source) =
                        closure.inputs.first().and_then(handler_pat_integer_source)
                    {
                        Some(source)
                    } else {
                        update_result_source.clone()
                    };
                    match source {
                        Some(IntegerSource::Supported(domain)) => {
                            self.builder.set_signal_integer_domain(id, Some(domain));
                            self.builder.set_signal_ty(id, domain.client_type());
                        }
                        Some(IntegerSource::Unsupported(name)) => {
                            return Err(unsupported_integer_domain_error(
                                closure.inputs.span(),
                                &name,
                            ));
                        }
                        None => {}
                    }
                }
                if let Some(domain) = self.builder.signal_integer_domain(id) {
                    let parameter_source =
                        closure.inputs.first().and_then(handler_pat_integer_source);
                    validate_explicit_integer_source_against_domain(
                        parameter_source.as_ref(),
                        domain,
                        closure.inputs.span(),
                        "signal.update(...) parameter",
                    )?;
                    validate_explicit_integer_source_against_domain(
                        update_result_source.as_ref(),
                        domain,
                        closure.body.span(),
                        "signal.update(...) result",
                    )?;
                }
                let mut simple = self.lower_simple_closure(closure, id, scope)?;
                let receiver_domain = self.builder.signal_integer_domain(id);
                if let Some(domain) = receiver_domain {
                    self.validate_integer_value_for_domain(
                        &mut simple.body.expr,
                        update_result_source,
                        domain,
                        &scope.handler_locals,
                        closure.body.span(),
                        "signal.update(...) result",
                        false,
                    )?;
                }
                // Same representation compatibility as `.set(...)`: the closure
                // body's value is stored back into the signal. Reads
                // of the closure parameter lower to SignalRead, so
                // binary expressions were already rewritten by the
                // typed numeric fixup in lower_binary; this covers
                // literal-only bodies.
                // See `.set(...)`: a representation-pinned receiver leaves
                // literals alone, because the cell coerces on write.
                let receiver_dyn = self.builder.signal_ty(id).is_some_and(ClientType::is_dyn);
                let receiver_wide = self
                    .builder
                    .signal_ty(id)
                    .is_some_and(ClientType::is_wide_int);
                if receiver_domain.is_some() {
                    // Exact-domain validation above also contextualized literals.
                } else if receiver_dyn {
                    // Leave literals in source form.
                } else if receiver_wide {
                    promote_int_literals_to_wide(&mut simple.body.expr);
                } else if !self
                    .frontend_numeric_anchors(&simple.body.expr, &scope.handler_locals)
                    .wide
                {
                    demote_bigint_literals(&mut simple.body.expr);
                }
                return Ok(vec![HandlerHirStmt::new(
                    expr.span(),
                    HandlerHirStmtKind::SignalUpdate {
                        id,
                        closure: simple,
                    },
                )]);
            }
            if let Some(id) =
                self.node_ref_receiver(&mc.receiver, node_ref_effect_method(&method).is_some())?
            {
                let effect = self.lower_node_ref_effect(mc, &method, scope)?;
                return Ok(vec![HandlerHirStmt::new(
                    expr.span(),
                    HandlerHirStmtKind::NodeRefEffect { id, effect },
                )]);
            }
            if method == "run" {
                if let Some(id) = self.mutation_receiver(&mc.receiver) {
                    expect_arg_count(mc, 1, "mutation.run(args)")?;
                    let arg = self.lower_handler_payload_hir_expr(&mc.args[0], scope)?;
                    return Ok(vec![HandlerHirStmt::new(
                        expr.span(),
                        HandlerHirStmtKind::RunMutation {
                            id,
                            args: vec![arg],
                        },
                    )]);
                }
            }
            if method == "refetch" {
                if let Some(id) = self.query_receiver(&mc.receiver) {
                    expect_arg_count(mc, 0, "query.refetch()")?;
                    return Ok(vec![HandlerHirStmt::new(
                        expr.span(),
                        HandlerHirStmtKind::QueryRefetch { id },
                    )]);
                }
            }
            if method == "invalidate" {
                if let Some(id) = self.query_receiver(&mc.receiver) {
                    expect_arg_count(mc, 0, "query.invalidate()")?;
                    return Ok(vec![HandlerHirStmt::new(
                        expr.span(),
                        HandlerHirStmtKind::QueryInvalidate { id },
                    )]);
                }
            }
        }

        // Fallback: arbitrary expression statement (rare in v0.1).
        let lowered = self.lower_handler_payload_hir_expr(expr, scope)?;
        Ok(vec![HandlerHirStmt::new(
            expr.span(),
            HandlerHirStmtKind::Expr(lowered),
        )])
    }

    pub(super) fn lower_event_browser_effect(
        &mut self,
        mc: &ExprMethodCall,
        method: &str,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Option<HandlerHirBrowserEffect>> {
        let (source, has_fallback, label) = match method {
            "target_first_file_text_into" => (
                EventFileSource::Target,
                false,
                "event.target_first_file_text_into(&signal)",
            ),
            "target_first_file_text_into_or" => (
                EventFileSource::Target,
                true,
                "event.target_first_file_text_into_or(&signal, fallback)",
            ),
            "data_transfer_first_file_text_into" => (
                EventFileSource::DataTransfer,
                false,
                "event.data_transfer_first_file_text_into(&signal)",
            ),
            "data_transfer_first_file_text_into_or" => (
                EventFileSource::DataTransfer,
                true,
                "event.data_transfer_first_file_text_into_or(&signal, fallback)",
            ),
            _ => return Ok(None),
        };
        expect_arg_count(mc, if has_fallback { 2 } else { 1 }, label)?;
        Ok(Some(HandlerHirBrowserEffect::EventFileTextInto {
            source,
            target: self.resolve_signal_target_arg(&mc.args[0], label)?,
            fallback: if has_fallback {
                Some(self.lower_handler_payload_hir_expr(&mc.args[1], scope)?)
            } else {
                None
            },
        }))
    }

    pub(super) fn lower_node_ref_effect(
        &mut self,
        mc: &ExprMethodCall,
        method: &str,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<HandlerHirNodeRefEffect> {
        use rsjs_ir::dom_registry::DomOperation;

        let Some(spec) = rsjs_ir::dom_registry::find_node_ref_effect(method) else {
            return Err(unknown_node_ref_method_error(mc.method.span(), method));
        };
        match spec.operation {
            DomOperation::Focus => {
                expect_arg_count(mc, 0, "node_ref.focus()")?;
                Ok(HandlerHirNodeRefEffect::Focus)
            }
            DomOperation::Blur => {
                expect_arg_count(mc, 0, "node_ref.blur()")?;
                Ok(HandlerHirNodeRefEffect::Blur)
            }
            DomOperation::ScrollIntoView => {
                expect_arg_count(mc, 0, "node_ref.scroll_into_view()")?;
                Ok(HandlerHirNodeRefEffect::ScrollIntoView)
            }
            DomOperation::Click => {
                expect_arg_count(mc, 0, "node_ref.click()")?;
                Ok(HandlerHirNodeRefEffect::Click)
            }
            DomOperation::RequestSubmit => {
                expect_arg_count(mc, 0, "node_ref.request_submit()")?;
                Ok(HandlerHirNodeRefEffect::RequestSubmit)
            }
            DomOperation::Reset => {
                expect_arg_count(mc, 0, "node_ref.reset()")?;
                Ok(HandlerHirNodeRefEffect::Reset)
            }
            DomOperation::ReportValidity => {
                expect_arg_count(mc, 0, "node_ref.report_validity()")?;
                Ok(HandlerHirNodeRefEffect::ReportValidity)
            }
            DomOperation::ShowModal => {
                expect_arg_count(mc, 0, "node_ref.show_modal()")?;
                Ok(HandlerHirNodeRefEffect::ShowModal)
            }
            DomOperation::Close => {
                expect_arg_count(mc, 0, "node_ref.close()")?;
                Ok(HandlerHirNodeRefEffect::Close)
            }
            DomOperation::ShowPopover => {
                expect_arg_count(mc, 0, "node_ref.show_popover()")?;
                Ok(HandlerHirNodeRefEffect::ShowPopover)
            }
            DomOperation::HidePopover => {
                expect_arg_count(mc, 0, "node_ref.hide_popover()")?;
                Ok(HandlerHirNodeRefEffect::HidePopover)
            }
            DomOperation::TogglePopover => {
                expect_arg_count(mc, 0, "node_ref.toggle_popover()")?;
                Ok(HandlerHirNodeRefEffect::TogglePopover)
            }
            DomOperation::ScrollTo => {
                let (x, y) = self.lower_two_node_ref_args(mc, scope, "node_ref.scroll_to(x, y)")?;
                Ok(HandlerHirNodeRefEffect::ScrollTo { x, y })
            }
            DomOperation::ScrollBy => {
                let (x, y) = self.lower_two_node_ref_args(mc, scope, "node_ref.scroll_by(x, y)")?;
                Ok(HandlerHirNodeRefEffect::ScrollBy { x, y })
            }
            DomOperation::SetPointerCapture => {
                let value =
                    self.lower_single_node_ref_arg(mc, scope, "node_ref.set_pointer_capture(id)")?;
                Ok(HandlerHirNodeRefEffect::SetPointerCapture(value))
            }
            DomOperation::ReleasePointerCapture => {
                let value = self.lower_single_node_ref_arg(
                    mc,
                    scope,
                    "node_ref.release_pointer_capture(id)",
                )?;
                Ok(HandlerHirNodeRefEffect::ReleasePointerCapture(value))
            }
            DomOperation::Select => {
                expect_arg_count(mc, 0, "node_ref.select()")?;
                Ok(HandlerHirNodeRefEffect::Select)
            }
            DomOperation::SetValue => {
                let value =
                    self.lower_single_node_ref_arg(mc, scope, "node_ref.set_value(value)")?;
                Ok(HandlerHirNodeRefEffect::SetValue(value))
            }
            DomOperation::SetChecked => {
                let value =
                    self.lower_single_node_ref_arg(mc, scope, "node_ref.set_checked(value)")?;
                Ok(HandlerHirNodeRefEffect::SetChecked(value))
            }
            DomOperation::SetCustomValidity => {
                let value = self.lower_single_node_ref_arg(
                    mc,
                    scope,
                    "node_ref.set_custom_validity(message)",
                )?;
                Ok(HandlerHirNodeRefEffect::SetCustomValidity(value))
            }
            DomOperation::SetSelectionRange => {
                let (start, end) = self.lower_two_node_ref_args(
                    mc,
                    scope,
                    "node_ref.set_selection_range(start, end)",
                )?;
                Ok(HandlerHirNodeRefEffect::SetSelectionRange(start, end))
            }
            DomOperation::SetTextContent => {
                let value =
                    self.lower_single_node_ref_arg(mc, scope, "node_ref.set_text_content(value)")?;
                Ok(HandlerHirNodeRefEffect::SetTextContent(value))
            }
            DomOperation::AddClass => {
                let value =
                    self.lower_single_node_ref_arg(mc, scope, "node_ref.add_class(class)")?;
                Ok(HandlerHirNodeRefEffect::AddClass(value))
            }
            DomOperation::RemoveClass => {
                let value =
                    self.lower_single_node_ref_arg(mc, scope, "node_ref.remove_class(class)")?;
                Ok(HandlerHirNodeRefEffect::RemoveClass(value))
            }
            DomOperation::ToggleClass => {
                let value =
                    self.lower_single_node_ref_arg(mc, scope, "node_ref.toggle_class(class)")?;
                Ok(HandlerHirNodeRefEffect::ToggleClass(value))
            }
            DomOperation::SetAttribute => {
                expect_arg_count(mc, 2, "node_ref.set_attribute(name, value)")?;
                let name = expect_static_node_ref_attribute_arg(
                    &mc.args[0],
                    "set_attribute",
                    "node_ref.set_attribute(\"aria-expanded\", value)",
                )?;
                let value = self.lower_handler_payload_hir_expr(&mc.args[1], scope)?;
                Ok(HandlerHirNodeRefEffect::SetAttribute { name, value })
            }
            DomOperation::RemoveAttribute => {
                expect_arg_count(mc, 1, "node_ref.remove_attribute(name)")?;
                let name = expect_static_node_ref_attribute_arg(
                    &mc.args[0],
                    "remove_attribute",
                    "node_ref.remove_attribute(\"hidden\")",
                )?;
                Ok(HandlerHirNodeRefEffect::RemoveAttribute { name })
            }
            DomOperation::SetStyleProperty => {
                expect_arg_count(mc, 2, "node_ref.set_style_property(name, value)")?;
                let name = expect_static_style_property_arg(&mc.args[0])?;
                let value = self.lower_handler_payload_hir_expr(&mc.args[1], scope)?;
                Ok(HandlerHirNodeRefEffect::SetStyleProperty { name, value })
            }
            _ => Err(unknown_node_ref_method_error(mc.method.span(), method)),
        }
    }

    pub(super) fn lower_single_node_ref_arg(
        &mut self,
        mc: &ExprMethodCall,
        scope: &HandlerScope<'_>,
        label: &str,
    ) -> syn::Result<HandlerHirExpr> {
        expect_arg_count(mc, 1, label)?;
        self.lower_handler_payload_hir_expr(&mc.args[0], scope)
    }

    pub(super) fn lower_two_node_ref_args(
        &mut self,
        mc: &ExprMethodCall,
        scope: &HandlerScope<'_>,
        label: &str,
    ) -> syn::Result<(HandlerHirExpr, HandlerHirExpr)> {
        expect_arg_count(mc, 2, label)?;
        Ok((
            self.lower_handler_payload_hir_expr(&mc.args[0], scope)?,
            self.lower_handler_payload_hir_expr(&mc.args[1], scope)?,
        ))
    }

    pub(super) fn lower_handler_hir_assign(
        &mut self,
        assign: &ExprAssign,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<HandlerHirStmt> {
        let target = handler_assignment_target(&assign.left)?;
        let local = self.resolve_handler_assignment_local(&target, assign.left.span(), scope)?;
        let binding = scope
            .handler_locals
            .iter()
            .rev()
            .find(|binding| binding.id == local)
            .expect("assignment target was resolved from handler locals");
        if let Some(name) = &binding.unsupported_integer_domain {
            return Err(unsupported_integer_domain_error(assign.left.span(), name));
        }
        let explicit_source =
            explicit_integer_source_in_expr(&assign.right, "handler-local assignment")?;
        if let Some(domain) = binding.integer_domain {
            validate_explicit_integer_source_against_domain(
                explicit_source.as_ref(),
                domain,
                assign.right.span(),
                "handler-local assignment",
            )?;
        }
        let mut value = self.lower_handler_payload_hir_expr(&assign.right, scope)?;
        if let Some(domain) = binding.integer_domain {
            self.validate_integer_value_for_domain(
                &mut value.expr,
                explicit_source,
                domain,
                &scope.handler_locals,
                assign.right.span(),
                "handler-local assignment",
                false,
            )?;
        }
        Ok(HandlerHirStmt::new(
            assign.span(),
            HandlerHirStmtKind::Assign { local, value },
        ))
    }

    pub(super) fn lower_handler_hir_compound_assign(
        &mut self,
        binary: &ExprBinary,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<HandlerHirStmt> {
        let target = handler_assignment_target(&binary.left)?;
        let local = self.resolve_handler_assignment_local(&target, binary.left.span(), scope)?;
        let op = compound_assignment_binary_op(&binary.op).ok_or_else(|| {
            syn::Error::new(
                binary.op.span(),
                "`#[rsjs]` handler compound assignments currently support only `+=`, `-=`, `*=`, `/=`, and `%=`",
                )
        })?;
        let binding = scope
            .handler_locals
            .iter()
            .rev()
            .find(|binding| binding.id == local)
            .expect("compound-assignment target was resolved from handler locals");
        if let Some(name) = &binding.unsupported_integer_domain {
            return Err(unsupported_integer_domain_error(binary.left.span(), name));
        }
        let rhs_source = explicit_integer_source_in_expr(
            &binary.right,
            "handler compound-assignment right operand",
        )?;
        if let Some(domain) = binding.integer_domain {
            validate_explicit_integer_source_against_domain(
                rhs_source.as_ref(),
                domain,
                binary.right.span(),
                "handler compound-assignment right operand",
            )?;
        }
        let rhs = self.lower_handler_payload_hir_expr(&binary.right, scope)?;
        // Compound assignment desugars to `local = local <op> rhs`;
        // run the same typed numeric fixup as an explicit binary so
        // `count /= 2` selects truncating division and wide locals
        // promote integer literals.
        let mut lhs_expr = FrontendExpr::Local(local);
        let mut rhs_expr = rhs.expr;
        let op = self.fixup_numeric_binary(
            op,
            &mut lhs_expr,
            &mut rhs_expr,
            None,
            rhs_source,
            &scope.handler_locals,
            binary.span(),
        )?;
        let value = HandlerHirExpr::new(
            binary.span(),
            FrontendExpr::BinaryOp {
                lhs: Box::new(lhs_expr),
                op,
                rhs: Box::new(rhs_expr),
            },
        );
        Ok(HandlerHirStmt::new(
            binary.span(),
            HandlerHirStmtKind::Assign { local, value },
        ))
    }

    pub(super) fn resolve_handler_assignment_local(
        &self,
        target: &str,
        span: proc_macro2::Span,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<LocalId> {
        if scope.event_param == Some(target) {
            return Err(syn::Error::new(
                span,
                "`#[rsjs]` handler assignments cannot assign to the event parameter",
            ));
        }
        if let Some((param_name, _)) = scope.handler_param {
            if param_name == target {
                return Err(syn::Error::new(
                    span,
                    "`#[rsjs]` handler assignments cannot assign to a handler payload parameter",
                ));
            }
        }
        let Some((local, mutable)) = scope
            .handler_locals
            .iter()
            .rev()
            .find(|local| local.name == target)
            .map(|local| (local.id, local.mutable))
        else {
            if self.signals.contains_key(target) {
                return Err(syn::Error::new(
                    span,
                    "`#[rsjs]` signals cannot be assigned directly inside handlers; use `signal.set(value)`",
                ));
            }
            return Err(syn::Error::new(
                span,
                "`#[rsjs]` handler assignments must target a handler-local `let mut` binding",
            ));
        };
        if !mutable {
            return Err(syn::Error::new(
                span,
                "`#[rsjs]` handler assignments must target a `let mut` handler-local binding",
            ));
        }
        Ok(local)
    }

    pub(super) fn lower_handler_hir_if(
        &mut self,
        if_expr: &ExprIf,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        let condition = self.lower_handler_payload_hir_expr(&if_expr.cond, scope)?;
        self.require_bool_condition(&condition.expr, &scope.handler_locals, if_expr.cond.span())?;
        let mut then_scope = scope.clone();
        let then_statements =
            self.lower_handler_hir_block(&if_expr.then_branch, &mut then_scope)?;
        let else_statements = match &if_expr.else_branch {
            None => Vec::new(),
            Some((_, else_expr)) => match else_expr.as_ref() {
                Expr::Block(block) => {
                    let mut else_scope = scope.clone();
                    self.lower_handler_hir_block(&block.block, &mut else_scope)?
                }
                Expr::If(nested) => {
                    // Preserve `else if` semantics as a single nested
                    // if inside the else block. The JS emitter can
                    // later pretty-print this as `else if`; keeping
                    // the structured form is enough for correctness.
                    self.lower_handler_hir_if(nested, scope)?
                }
                other => {
                    return Err(syn::Error::new(
                        other.span(),
                        "`#[rsjs]` handler `else` branches must be blocks or `else if` chains",
                    ));
                }
            },
        };

        Ok(vec![HandlerHirStmt::new(
            if_expr.if_token.span,
            HandlerHirStmtKind::If {
                condition,
                then_statements,
                else_statements,
            },
        )])
    }

    pub(super) fn lower_handler_hir_match(
        &mut self,
        match_expr: &ExprMatch,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        if handler_match_is_exact_bool(match_expr) {
            return self.lower_handler_hir_bool_match(match_expr, scope);
        }
        self.lower_handler_hir_literal_match(match_expr, scope)
    }

    pub(super) fn lower_handler_hir_bool_match(
        &mut self,
        match_expr: &ExprMatch,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        if match_expr.arms.len() != 2 {
            return Err(unsupported_handler_control_flow_error(
                match_expr.match_token.span,
                "match",
            ));
        }

        let mut true_arm: Option<&Expr> = None;
        let mut false_arm: Option<&Expr> = None;
        for arm in &match_expr.arms {
            if arm.guard.is_some() {
                return Err(unsupported_handler_control_flow_error(
                    match_expr.match_token.span,
                    "match",
                ));
            }
            match bool_match_arm(&arm.pat) {
                Some(true) if true_arm.is_none() => true_arm = Some(&arm.body),
                Some(false) if false_arm.is_none() => false_arm = Some(&arm.body),
                _ => {
                    return Err(unsupported_handler_control_flow_error(
                        match_expr.match_token.span,
                        "match",
                    ));
                }
            }
        }

        let Some(true_body) = true_arm else {
            return Err(unsupported_handler_control_flow_error(
                match_expr.match_token.span,
                "match",
            ));
        };
        let Some(false_body) = false_arm else {
            return Err(unsupported_handler_control_flow_error(
                match_expr.match_token.span,
                "match",
            ));
        };

        let condition = self.lower_handler_payload_hir_expr(&match_expr.expr, scope)?;
        let then_statements = self.lower_handler_hir_match_arm_body(true_body, scope)?;
        let else_statements = self.lower_handler_hir_match_arm_body(false_body, scope)?;

        Ok(vec![HandlerHirStmt::new(
            match_expr.match_token.span,
            HandlerHirStmtKind::If {
                condition,
                then_statements,
                else_statements,
            },
        )])
    }

    pub(super) fn lower_handler_hir_literal_match(
        &mut self,
        match_expr: &ExprMatch,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        if match_expr.arms.is_empty() {
            return Err(unsupported_handler_match_error(match_expr.match_token.span));
        }

        let match_value = self.lower_handler_payload_hir_expr(&match_expr.expr, scope)?;
        let match_ty = self.frontend_expr_type(&match_value.expr, &scope.handler_locals);
        let match_integer_domain =
            self.frontend_integer_domain(&match_value.expr, &scope.handler_locals);
        let match_local = self.alloc_handler_local();
        let mut match_scope = scope.clone();
        match_scope.handler_locals.push(HandlerLocalBinding {
            name: "__match".to_string(),
            id: match_local,
            mutable: false,
            ty: match_ty.clone(),
            integer_domain: match_integer_domain,
            unsupported_integer_domain: None,
            constant: None,
        });

        let mut statements = vec![HandlerHirStmt::new(
            match_expr.expr.span(),
            HandlerHirStmtKind::Let {
                local: match_local,
                name: "__match".to_string(),
                ty: match_ty.clone(),
                value: match_value,
            },
        )];

        let mut literal_arms = Vec::new();
        let mut default_body = None;
        for (idx, arm) in match_expr.arms.iter().enumerate() {
            if matches!(arm.pat, Pat::Wild(_)) {
                if let Some((if_token, _)) = &arm.guard {
                    return Err(syn::Error::new(
                        if_token.span,
                        "`#[rsjs]` handler literal `match` fallback `_` must be unguarded",
                    ));
                }
                if idx + 1 != match_expr.arms.len() {
                    return Err(syn::Error::new(
                        arm.pat.span(),
                        "`#[rsjs]` handler literal `match` fallback `_` must be the final arm",
                    ));
                }
                default_body =
                    Some(self.lower_handler_hir_match_arm_body(&arm.body, &match_scope)?);
                continue;
            }

            let pattern =
                handler_literal_match_pattern(match_local, &arm.pat, match_integer_domain)?;
            let mut condition = pattern.condition;
            let mut arm_scope = match_scope.clone();
            let mut guard_scope = match_scope.clone();
            let mut prelude_statements = Vec::new();
            if let Some(binding) = pattern.option_binding {
                let binding_ty = match &match_ty {
                    ClientType::Option(inner) => inner.as_ref().clone(),
                    _ => ClientType::Unknown,
                };
                let binding_integer_domain = match &binding_ty {
                    ClientType::I32 => Some(IntegerDomain::I32),
                    ClientType::U32 => Some(IntegerDomain::U32),
                    ClientType::I64 => Some(IntegerDomain::I64),
                    ClientType::U64 => Some(IntegerDomain::U64),
                    _ => None,
                };
                self.validate_handler_match_binding(&binding, arm.pat.span(), &match_scope)?;
                guard_scope.handler_locals.push(HandlerLocalBinding {
                    name: binding.clone(),
                    id: match_local,
                    mutable: false,
                    ty: binding_ty.clone(),
                    integer_domain: binding_integer_domain,
                    unsupported_integer_domain: None,
                    constant: None,
                });
                let id = self.alloc_handler_local();
                arm_scope.handler_locals.push(HandlerLocalBinding {
                    name: binding.clone(),
                    id,
                    mutable: false,
                    ty: binding_ty.clone(),
                    integer_domain: binding_integer_domain,
                    unsupported_integer_domain: None,
                    constant: None,
                });
                prelude_statements.push(HandlerHirStmt::new(
                    arm.pat.span(),
                    HandlerHirStmtKind::Let {
                        local: id,
                        name: binding,
                        ty: binding_ty,
                        value: HandlerHirExpr::new(
                            arm.pat.span(),
                            FrontendExpr::Local(match_local),
                        ),
                    },
                ));
            }
            if let Some((_, guard_expr)) = &arm.guard {
                let guard = self.lower_handler_payload_hir_expr(guard_expr, &guard_scope)?;
                self.require_bool_condition(
                    &guard.expr,
                    &guard_scope.handler_locals,
                    guard_expr.span(),
                )?;
                condition = FrontendExpr::BinaryOp {
                    op: BinOp::And,
                    lhs: Box::new(condition),
                    rhs: Box::new(guard.expr),
                };
            }
            let mut body = prelude_statements;
            body.extend(self.lower_handler_hir_match_arm_body(&arm.body, &arm_scope)?);
            literal_arms.push((arm.pat.span(), condition, body));
        }

        let Some(mut else_statements) = default_body else {
            return Err(syn::Error::new(
                match_expr.match_token.span,
                "`#[rsjs]` handler literal `match` needs a final `_` fallback arm so generated \
                 JavaScript has explicit unmatched-case behavior",
            ));
        };

        for (span, condition, then_statements) in literal_arms.into_iter().rev() {
            else_statements = vec![HandlerHirStmt::new(
                span,
                HandlerHirStmtKind::If {
                    condition: HandlerHirExpr::new(span, condition),
                    then_statements,
                    else_statements,
                },
            )];
        }

        statements.extend(else_statements);
        Ok(statements)
    }

    pub(super) fn lower_handler_hir_match_arm_body(
        &mut self,
        body: &Expr,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        match body {
            Expr::Block(block) => {
                let mut branch_scope = scope.clone();
                self.lower_handler_hir_block(&block.block, &mut branch_scope)
            }
            other => self.lower_handler_hir_expr_stmt(other, scope),
        }
    }

    pub(super) fn lower_handler_hir_static_for_loop(
        &mut self,
        for_loop: &ExprForLoop,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        if for_loop.label.is_some() {
            return Err(unsupported_handler_for_loop_error(for_loop.for_token.span));
        }
        let binding_source = handler_pat_integer_source(&for_loop.pat);
        if let Some(IntegerSource::Unsupported(name)) = binding_source.as_ref() {
            return Err(unsupported_integer_domain_error(for_loop.pat.span(), name));
        }
        let Expr::Array(array) = for_loop.expr.as_ref() else {
            if let Expr::Range(range) = for_loop.expr.as_ref() {
                let name = handler_for_loop_pat_name(&for_loop.pat)?;
                return self.lower_handler_hir_range_loop(
                    &name,
                    binding_source,
                    range,
                    &for_loop.body,
                    scope,
                );
            }
            return Err(unsupported_handler_for_loop_error(for_loop.for_token.span));
        };
        let name = handler_for_loop_pat_name(&for_loop.pat)?;
        self.lower_handler_hir_static_array_loop(
            &name,
            binding_source,
            array,
            &for_loop.body,
            scope,
        )
    }

    pub(super) fn lower_handler_hir_static_array_loop(
        &mut self,
        name: &str,
        binding_source: Option<IntegerSource>,
        array: &ExprArray,
        body: &Block,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        let mut statements = Vec::new();
        for elem in &array.elems {
            let elem_source = explicit_integer_source_in_expr(elem, "typed handler loop item")?;
            if let Some(IntegerSource::Supported(domain)) = binding_source.as_ref() {
                validate_explicit_integer_source_against_domain(
                    elem_source.as_ref(),
                    *domain,
                    elem.span(),
                    "typed handler loop item",
                )?;
            }
            let mut value = self.lower_handler_payload_hir_expr(elem, scope)?;
            let integer_source = merge_handler_loop_integer_sources(
                binding_source.clone(),
                self.resolved_integer_source(
                    elem_source.as_ref(),
                    &value.expr,
                    &scope.handler_locals,
                ),
                elem.span(),
            )?;
            if let Some(IntegerSource::Supported(domain)) = binding_source.as_ref() {
                self.validate_integer_value_for_domain(
                    &mut value.expr,
                    elem_source,
                    *domain,
                    &scope.handler_locals,
                    elem.span(),
                    "typed handler loop item",
                    false,
                )?;
            }
            if let Some(IntegerSource::Supported(domain)) = integer_source.as_ref() {
                coerce_integer_literals_to_domain(&mut value.expr, *domain, elem.span())?;
            }
            let integer_domain = match integer_source.as_ref() {
                Some(IntegerSource::Supported(domain)) => Some(*domain),
                _ => self.frontend_integer_domain(&value.expr, &scope.handler_locals),
            };
            let unsupported_integer_domain = match integer_source.as_ref() {
                Some(IntegerSource::Unsupported(name)) => Some(name.clone()),
                _ => None,
            };
            let ty = integer_domain
                .map(IntegerDomain::client_type)
                .unwrap_or_else(|| self.frontend_expr_type(&value.expr, &scope.handler_locals));
            let constant = (integer_source.is_none() && integer_literal_only(&value.expr))
                .then(|| value.expr.clone());
            let local = self.alloc_handler_local();
            let mut iteration_scope = scope.clone();
            iteration_scope.handler_locals.push(HandlerLocalBinding {
                name: name.to_string(),
                id: local,
                mutable: false,
                ty: ty.clone(),
                integer_domain,
                unsupported_integer_domain,
                constant,
            });
            statements.push(HandlerHirStmt::new(
                elem.span(),
                HandlerHirStmtKind::Let {
                    local,
                    name: name.to_string(),
                    ty,
                    value,
                },
            ));
            statements.extend(self.lower_handler_hir_block(body, &mut iteration_scope)?);
        }
        Ok(statements)
    }

    pub(super) fn lower_handler_hir_range_loop(
        &mut self,
        name: &str,
        binding_source: Option<IntegerSource>,
        range: &ExprRange,
        body: &Block,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        let Some(start) = &range.start else {
            return Err(unsupported_handler_for_loop_error(range.span()));
        };
        let Some(end) = &range.end else {
            return Err(unsupported_handler_for_loop_error(range.span()));
        };
        let start_source = integer_expr_source_hint(start);
        let end_source = integer_expr_source_hint(end);
        let range_source = merge_handler_loop_integer_sources(
            binding_source,
            merge_range_integer_sources(start_source.clone(), end_source.clone(), range.span())?,
            range.span(),
        )?;
        if let Some(IntegerSource::Unsupported(name)) = range_source.as_ref() {
            return Err(unsupported_integer_domain_error(range.span(), name));
        }

        if let (Some(start), Some(end)) =
            (static_i64_range_bound(start)?, static_i64_range_bound(end)?)
        {
            let mut end = end;
            if matches!(range.limits, RangeLimits::Closed(_)) {
                end = end.checked_add(1).ok_or_else(|| {
                    syn::Error::new(
                        range.span(),
                        "`#[rsjs]` static inclusive range end overflows i64",
                    )
                })?;
            }

            let mut statements = Vec::new();
            for value in start..end {
                statements.extend(self.lower_handler_hir_static_loop_iteration(
                    name,
                    HandlerHirExpr::new(range.span(), FrontendExpr::Literal(Value::Int(value))),
                    body,
                    scope,
                    range.span(),
                    range_source.clone(),
                )?);
            }
            return Ok(statements);
        }

        let mut start = self.lower_handler_payload_hir_expr(start, scope)?;
        let mut end = self.lower_handler_payload_hir_expr(end, scope)?;
        let resolved_start =
            self.resolved_integer_source(start_source.as_ref(), &start.expr, &scope.handler_locals);
        let resolved_end =
            self.resolved_integer_source(end_source.as_ref(), &end.expr, &scope.handler_locals);
        if let Some(name) = explicit_unsupported_integer_source(resolved_start.as_ref())
            .or_else(|| explicit_unsupported_integer_source(resolved_end.as_ref()))
        {
            return Err(unsupported_integer_domain_error(range.span(), &name));
        }
        let start_domain = explicit_supported_integer_source(resolved_start.as_ref());
        let end_domain = explicit_supported_integer_source(resolved_end.as_ref());
        let inferred_domain = match (start_domain, end_domain) {
            (Some(lhs), Some(rhs)) if lhs == rhs => lhs,
            (Some(lhs), Some(rhs)) => {
                return Err(integer_domain_mismatch_error(
                    range.span(),
                    lhs,
                    rhs,
                    "handler range bounds",
                ));
            }
            (Some(domain), None) if integer_literal_only(&end.expr) => domain,
            (None, Some(domain)) if integer_literal_only(&start.expr) => domain,
            _ => {
                return Err(integer_domain_proof_error(
                    range.span(),
                    "dynamic handler range bounds need one matching i32/u32/i64/u64 domain",
                ));
            }
        };
        let integer_domain = match range_source.as_ref() {
            Some(IntegerSource::Supported(expected)) if *expected != inferred_domain => {
                return Err(integer_domain_mismatch_error(
                    range.span(),
                    inferred_domain,
                    *expected,
                    "typed handler range binding",
                ));
            }
            Some(IntegerSource::Supported(expected)) => *expected,
            Some(IntegerSource::Unsupported(name)) => {
                return Err(unsupported_integer_domain_error(range.span(), name));
            }
            None => inferred_domain,
        };
        coerce_integer_literals_to_domain(&mut start.expr, integer_domain, range.span())?;
        coerce_integer_literals_to_domain(&mut end.expr, integer_domain, range.span())?;
        let local = self.alloc_handler_local();
        let mut loop_scope = scope.clone();
        loop_scope.handler_locals.push(HandlerLocalBinding {
            name: name.to_string(),
            id: local,
            mutable: false,
            ty: integer_domain.client_type(),
            integer_domain: Some(integer_domain),
            unsupported_integer_domain: None,
            constant: None,
        });
        let body = self.lower_handler_hir_block(body, &mut loop_scope)?;
        Ok(vec![HandlerHirStmt::new(
            range.span(),
            HandlerHirStmtKind::ForRange {
                local,
                name: name.to_string(),
                integer_domain,
                start,
                end,
                inclusive: matches!(range.limits, RangeLimits::Closed(_)),
                body,
            },
        )])
    }

    pub(super) fn lower_handler_hir_static_loop_iteration(
        &mut self,
        name: &str,
        value: HandlerHirExpr,
        body: &Block,
        scope: &HandlerScope<'_>,
        span: proc_macro2::Span,
        integer_source: Option<IntegerSource>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        let mut value = value;
        let integer_domain = match integer_source.as_ref() {
            Some(IntegerSource::Supported(domain)) => {
                coerce_integer_literals_to_domain(&mut value.expr, *domain, span)?;
                Some(*domain)
            }
            _ => self.frontend_integer_domain(&value.expr, &scope.handler_locals),
        };
        let unsupported_integer_domain = match integer_source {
            Some(IntegerSource::Unsupported(name)) => Some(name),
            _ => None,
        };
        let ty = integer_domain
            .map(IntegerDomain::client_type)
            .unwrap_or_else(|| self.frontend_expr_type(&value.expr, &scope.handler_locals));
        let local = self.alloc_handler_local();
        let mut iteration_scope = scope.clone();
        iteration_scope.handler_locals.push(HandlerLocalBinding {
            name: name.to_string(),
            id: local,
            mutable: false,
            ty: ty.clone(),
            integer_domain,
            unsupported_integer_domain: unsupported_integer_domain.clone(),
            constant: (integer_domain.is_none()
                && unsupported_integer_domain.is_none()
                && integer_literal_only(&value.expr))
            .then(|| value.expr.clone()),
        });
        let mut statements = vec![HandlerHirStmt::new(
            span,
            HandlerHirStmtKind::Let {
                local,
                name: name.to_string(),
                ty,
                value,
            },
        )];
        statements.extend(self.lower_handler_hir_block(body, &mut iteration_scope)?);
        Ok(statements)
    }

    pub(super) fn lower_handler_hir_block(
        &mut self,
        block: &Block,
        scope: &mut HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        let mut statements = Vec::new();
        for stmt in &block.stmts {
            statements.extend(self.lower_handler_hir_stmt(stmt, scope)?);
        }
        Ok(statements)
    }

    pub(super) fn lower_console_call(
        &mut self,
        call: &ExprCall,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Option<HandlerHirStmt>> {
        let Expr::Path(ExprPath {
            path, qself: None, ..
        }) = call.func.as_ref()
        else {
            return Ok(None);
        };
        let Some(level) = console_call_level(path) else {
            return Ok(None);
        };
        let mut args = Vec::with_capacity(call.args.len());
        for arg in &call.args {
            args.push(self.lower_handler_payload_hir_expr(arg, scope)?);
        }
        Ok(Some(HandlerHirStmt::new(
            call.span(),
            HandlerHirStmtKind::Console { level, args },
        )))
    }

    pub(super) fn lower_browser_effect_call(
        &mut self,
        call: &ExprCall,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Option<HandlerHirStmt>> {
        let Expr::Path(ExprPath {
            path, qself: None, ..
        }) = call.func.as_ref()
        else {
            return Ok(None);
        };
        if let Some(spec) = registry_effect_spec(path) {
            return Ok(Some(self.lower_registry_effect_call(call, spec, scope)?));
        }
        let Some(kind) = browser_effect_call_kind(path) else {
            return Ok(None);
        };
        let effect = match kind {
            BrowserEffectCallKind::LocationSetHash => {
                expect_call_arg_count(call, 1, "rsjs::location::set_hash(value)")?;
                HandlerHirBrowserEffect::LocationSetHash {
                    value: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                }
            }
            BrowserEffectCallKind::LocationAssign => {
                expect_call_arg_count(call, 1, "rsjs::location::assign(url)")?;
                HandlerHirBrowserEffect::LocationAssign {
                    url: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                }
            }
            BrowserEffectCallKind::LocationReplace => {
                expect_call_arg_count(call, 1, "rsjs::location::replace(url)")?;
                HandlerHirBrowserEffect::LocationReplace {
                    url: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                }
            }
            BrowserEffectCallKind::LocationReload => {
                expect_call_arg_count(call, 0, "rsjs::location::reload()")?;
                HandlerHirBrowserEffect::LocationReload
            }
            BrowserEffectCallKind::HistoryPushState => {
                expect_call_arg_count(call, 1, "rsjs::history::push_state(url)")?;
                HandlerHirBrowserEffect::HistoryPushState {
                    url: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                }
            }
            BrowserEffectCallKind::HistoryReplaceState => {
                expect_call_arg_count(call, 1, "rsjs::history::replace_state(url)")?;
                HandlerHirBrowserEffect::HistoryReplaceState {
                    url: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                }
            }
            BrowserEffectCallKind::HistoryBack => {
                expect_call_arg_count(call, 0, "rsjs::history::back()")?;
                HandlerHirBrowserEffect::HistoryBack
            }
            BrowserEffectCallKind::HistoryForward => {
                expect_call_arg_count(call, 0, "rsjs::history::forward()")?;
                HandlerHirBrowserEffect::HistoryForward
            }
            BrowserEffectCallKind::HistoryGo => {
                expect_call_arg_count(call, 1, "rsjs::history::go(delta)")?;
                HandlerHirBrowserEffect::HistoryGo {
                    delta: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                }
            }
            BrowserEffectCallKind::DocumentDispatchEvent => {
                expect_call_arg_count(call, 1, "rsjs::document::dispatch_event(name)")?;
                HandlerHirBrowserEffect::DocumentDispatchEvent {
                    name: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                    detail: None,
                }
            }
            BrowserEffectCallKind::DocumentDispatchEventWithDetail => {
                expect_call_arg_count(
                    call,
                    2,
                    "rsjs::document::dispatch_event_with_detail(name, detail)",
                )?;
                HandlerHirBrowserEffect::DocumentDispatchEvent {
                    name: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                    detail: Some(self.lower_handler_payload_hir_expr(&call.args[1], scope)?),
                }
            }
            BrowserEffectCallKind::DocumentSetTitle => {
                expect_call_arg_count(call, 1, "rsjs::document::set_title(title)")?;
                HandlerHirBrowserEffect::DocumentSetTitle {
                    title: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                }
            }
            BrowserEffectCallKind::WebAuthnCreateJsonInto => {
                expect_call_arg_count(
                    call,
                    2,
                    "rsjs::webauthn::create_json_into(options_json, &signal)",
                )?;
                HandlerHirBrowserEffect::WebAuthnCreateJsonInto {
                    options_json: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                    target: self.resolve_signal_target_arg(
                        &call.args[1],
                        "rsjs::webauthn::create_json_into(options_json, &signal)",
                    )?,
                    fallback: None,
                }
            }
            BrowserEffectCallKind::WebAuthnCreateJsonIntoOr => {
                expect_call_arg_count(
                    call,
                    3,
                    "rsjs::webauthn::create_json_into_or(options_json, &signal, fallback)",
                )?;
                HandlerHirBrowserEffect::WebAuthnCreateJsonInto {
                    options_json: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                    target: self.resolve_signal_target_arg(
                        &call.args[1],
                        "rsjs::webauthn::create_json_into_or(options_json, &signal, fallback)",
                    )?,
                    fallback: Some(self.lower_handler_payload_hir_expr(&call.args[2], scope)?),
                }
            }
            BrowserEffectCallKind::WebAuthnGetJsonInto => {
                expect_call_arg_count(
                    call,
                    2,
                    "rsjs::webauthn::get_json_into(options_json, &signal)",
                )?;
                HandlerHirBrowserEffect::WebAuthnGetJsonInto {
                    options_json: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                    target: self.resolve_signal_target_arg(
                        &call.args[1],
                        "rsjs::webauthn::get_json_into(options_json, &signal)",
                    )?,
                    fallback: None,
                }
            }
            BrowserEffectCallKind::WebAuthnGetJsonIntoOr => {
                expect_call_arg_count(
                    call,
                    3,
                    "rsjs::webauthn::get_json_into_or(options_json, &signal, fallback)",
                )?;
                HandlerHirBrowserEffect::WebAuthnGetJsonInto {
                    options_json: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                    target: self.resolve_signal_target_arg(
                        &call.args[1],
                        "rsjs::webauthn::get_json_into_or(options_json, &signal, fallback)",
                    )?,
                    fallback: Some(self.lower_handler_payload_hir_expr(&call.args[2], scope)?),
                }
            }
            BrowserEffectCallKind::UrlSetSearchParam(mode) => {
                expect_call_arg_count(call, 2, "rsjs::url::<set|push>_search_param(key, value)")?;
                HandlerHirBrowserEffect::UrlSetSearchParam {
                    mode,
                    key: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                    value: self.lower_handler_payload_hir_expr(&call.args[1], scope)?,
                }
            }
            BrowserEffectCallKind::UrlDeleteSearchParam(mode) => {
                expect_call_arg_count(
                    call,
                    1,
                    "rsjs::url::<delete|push_delete>_search_param(key)",
                )?;
                HandlerHirBrowserEffect::UrlDeleteSearchParam {
                    mode,
                    key: self.lower_handler_payload_hir_expr(&call.args[0], scope)?,
                }
            }
        };
        Ok(Some(HandlerHirStmt::new(
            call.span(),
            HandlerHirStmtKind::BrowserEffect(effect),
        )))
    }

    /// Lower a handler statement recognised by
    /// `rsjs_ir::browser_registry` in effect position: check arity
    /// against the entry, lower each argument per its declared kind
    /// (`Reactive` payload expression or `&signal` write target),
    /// then realise the entry's Client IR lowering.
    fn lower_registry_effect_call(
        &mut self,
        call: &ExprCall,
        spec: &'static BrowserHelperSpec,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<HandlerHirStmt> {
        use rsjs_ir::browser_registry::{ArgKind, HelperLowering};

        enum RegistryArg {
            Expr(HandlerHirExpr),
            Signal(SignalId),
        }

        expect_call_arg_count(call, spec.args.len(), spec.usage)?;
        let mut args = Vec::with_capacity(spec.args.len());
        for (arg, arg_spec) in call.args.iter().zip(spec.args) {
            args.push(match arg_spec.kind {
                ArgKind::Reactive => {
                    RegistryArg::Expr(self.lower_handler_payload_hir_expr(arg, scope)?)
                }
                ArgKind::SignalRef => {
                    RegistryArg::Signal(self.resolve_signal_target_arg(arg, spec.usage)?)
                }
            });
        }
        let mut args = args.into_iter();
        let mut next_expr = |what: &str| match args.next() {
            Some(RegistryArg::Expr(expr)) => expr,
            _ => unreachable!("arity and arg kinds checked against the registry: {what}"),
        };
        let effect = match spec.lowering {
            HelperLowering::WindowScrollTo => HandlerHirBrowserEffect::WindowScrollTo {
                x: next_expr("x"),
                y: next_expr("y"),
            },
            HelperLowering::WindowScrollBy => HandlerHirBrowserEffect::WindowScrollBy {
                x: next_expr("x"),
                y: next_expr("y"),
            },
            HelperLowering::WindowPrint => HandlerHirBrowserEffect::WindowPrint,
            HelperLowering::ClipboardWriteText => HandlerHirBrowserEffect::ClipboardWriteText {
                text: next_expr("text"),
            },
            HelperLowering::ClipboardReadTextInto => {
                let Some(RegistryArg::Signal(target)) = args.next() else {
                    unreachable!("arity and arg kinds checked against the registry: target");
                };
                HandlerHirBrowserEffect::ClipboardReadTextInto {
                    target,
                    fallback: match args.next() {
                        Some(RegistryArg::Expr(fallback)) => Some(fallback),
                        Some(RegistryArg::Signal(_)) => {
                            unreachable!("registry declares the fallback as reactive")
                        }
                        None => None,
                    },
                }
            }
            HelperLowering::StorageSetItem(area) => HandlerHirBrowserEffect::StorageSetItem {
                area,
                key: next_expr("key"),
                value: next_expr("value"),
            },
            HelperLowering::StorageRemoveItem(area) => HandlerHirBrowserEffect::StorageRemoveItem {
                area,
                key: next_expr("key"),
            },
            HelperLowering::DeclaredEffect { .. } => HandlerHirBrowserEffect::Declared {
                helper: spec.id,
                args: (0..spec.args.len())
                    .map(|_| next_expr("declared effect argument"))
                    .collect(),
            },
            HelperLowering::WindowField(_)
            | HelperLowering::StorageGet { .. }
            | HelperLowering::DeclaredRead { .. } => {
                unreachable!("registry_effect_spec only resolves HandlerEffect entries")
            }
        };
        Ok(HandlerHirStmt::new(
            call.span(),
            HandlerHirStmtKind::BrowserEffect(effect),
        ))
    }

    pub(super) fn lower_handler_payload_expr(
        &mut self,
        expr: &Expr,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        if let Some(error) = batch_in_value_position_error(expr) {
            return Err(error);
        }
        let expr_scope = scope.expr_scope();
        match self.lower_reactive_expr_in(expr, &expr_scope) {
            Ok(expr) => Ok(expr),
            Err(err) => {
                if !self.can_serialize_loop_payload(expr) {
                    return Err(err);
                }
                let Some(node) = scope.event_node else {
                    return Err(err);
                };
                let (attr, _) = self.add_payload_attr(node, expr);
                Ok(FrontendExpr::EventPayload { attr })
            }
        }
    }

    pub(super) fn lower_handler_payload_hir_expr(
        &mut self,
        expr: &Expr,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<HandlerHirExpr> {
        self.lower_handler_payload_expr(expr, scope)
            .map(|expr_ir| HandlerHirExpr::new(expr.span(), expr_ir))
    }

    pub(super) fn expr_mentions_loop_local(&self, expr: &Expr) -> bool {
        if self.loop_scopes.is_empty() {
            return false;
        }
        let mut visitor = LoopLocalVisitor {
            scopes: &self.loop_scopes,
            found: false,
        };
        visitor.visit_expr(expr);
        visitor.found
    }

    pub(super) fn add_payload_attr(&mut self, node: NodeId, expr: &Expr) -> (String, String) {
        let tokens = expr.to_token_stream().to_string();
        if payload_expression_is_cse_safe(expr) {
            if let Some(attr) = self.builder.payload_attr_for_tokens(node, &tokens) {
                return (attr, tokens);
            }
        }
        let attr = format!("data-rsjs-payload-p{}", self.payload_counter);
        self.payload_counter += 1;
        self.builder.payload_attr_with_source(
            node,
            &attr,
            &tokens,
            Some(payload_label(expr)),
            Some(source_anchor(expr.span())),
        );
        (attr, tokens)
    }

    pub(super) fn lower_callback_slot_method_call(
        &mut self,
        call: &ExprMethodCall,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Option<HandlerHirStmt>> {
        if call.method == "call_if_some" {
            let Some(field_name) = self_field_name(&call.receiver) else {
                return Ok(None);
            };
            if !field_name.starts_with("on_") {
                return Err(syn::Error::new(
                    call.method.span(),
                    "`#[rsjs]` only treats `self.on_*.call_if_some(payload)` calls as optional \
                     component callback slots",
                ));
            }
            return self
                .lower_callback_slot_payload(
                    &field_name,
                    call.span(),
                    call.args.iter(),
                    scope,
                    InboundCallbackShape::Optional,
                )
                .map(Some);
        }
        if call.method == "call" {
            let Some(field_name) = self_field_name(&call.receiver) else {
                return Ok(None);
            };
            if !field_name.starts_with("on_") {
                return Err(syn::Error::new(
                    call.method.span(),
                    "`#[rsjs]` only treats `self.on_*.call(payload)` calls as component callback slots",
                ));
            }
            return self
                .lower_callback_slot_payload(
                    &field_name,
                    call.span(),
                    call.args.iter(),
                    scope,
                    InboundCallbackShape::Required,
                )
                .map(Some);
        }
        if !expr_is_self_path(&call.receiver) {
            return Ok(None);
        }
        let field_name = call.method.to_string();
        if !field_name.starts_with("on_") {
            return Err(guided_boundary_error(
                call.method.span(),
                format!("Rust method `self.{field_name}(...)` cannot run inside a client handler"),
                "rsjs handler bodies compile to JavaScript Client IR. Arbitrary Rust methods are \
                 server-side code unless you expose equivalent client code explicitly.",
                [
                    "Use a component callback slot named `self.on_*` and call it as `self.on_change.call(payload)`.",
                    "Rewrite the handler with supported signal, event, callback, and control-flow operations.",
                    "For pure portable computation only, move the logic into a standalone helper annotated once with `#[rsjs::client]`; effects must use supported handler, browser-capability, query, or mutation operations.",
                ],
                Some("component.self_non_callback_method"),
            ));
        }
        Err(syn::Error::new(
            call.method.span(),
            format!("call callback fields with `self.{field_name}.call(payload)`"),
        ))
    }

    pub(super) fn lower_callback_slot_call(
        &mut self,
        call: &ExprCall,
        _scope: &HandlerScope<'_>,
    ) -> syn::Result<Option<HandlerHirStmt>> {
        let Some(field) = callback_call_field(call.func.as_ref()) else {
            return Ok(None);
        };
        if !expr_is_self_path(&field.base) {
            return Ok(None);
        }
        let syn::Member::Named(field_ident) = &field.member else {
            return Err(syn::Error::new(
                field.member.span(),
                "`#[rsjs]` callback slots must be named fields, e.g. `self.on_change.call(value)`",
            ));
        };
        let field_name = field_ident.to_string();
        if !field_name.starts_with("on_") {
            return Err(guided_boundary_error(
                field_ident.span(),
                format!(
                    "Rust field or function `self.{field_name}` cannot run inside a client handler"
                ),
                "rsjs handler bodies compile to JavaScript Client IR. Arbitrary Rust fields or \
                 function values are not serializable callback slots.",
                [
                    "Use a component callback slot named `self.on_*` and call it as `self.on_change.call(payload)`.",
                    "Rewrite the handler with supported signal, event, callback, and control-flow operations.",
                    "For pure portable computation only, move the logic into a standalone helper annotated once with `#[rsjs::client]`; effects must use supported handler, browser-capability, query, or mutation operations.",
                ],
                Some("component.self_non_callback_method"),
            ));
        }
        Err(syn::Error::new(
            field_ident.span(),
            format!("call callback fields with `self.{field_name}.call(payload)`"),
        ))
    }

    pub(super) fn lower_callback_slot_payload<'a>(
        &mut self,
        field_name: &str,
        call_span: proc_macro2::Span,
        mut args: impl Iterator<Item = &'a Expr>,
        scope: &HandlerScope<'_>,
        callback_shape: InboundCallbackShape,
    ) -> syn::Result<HandlerHirStmt> {
        let Some(payload_expr) = args.next() else {
            return Err(syn::Error::new(
                call_span,
                format!("`self.{field_name}.call(payload)` takes exactly one payload argument"),
            ));
        };
        if args.next().is_some() {
            return Err(syn::Error::new(
                call_span,
                format!("`self.{field_name}.call(payload)` takes exactly one payload argument"),
            ));
        }
        let payload = self.lower_handler_payload_hir_expr(payload_expr, scope)?;
        // Callback fields are live dispatch edges. Their browser invocation
        // resolves whichever parent callback is connected at that moment; it
        // never captures or serializes the SSR closure.
        self.record_inbound_callback(field_name, call_span, callback_shape)?;
        let slot = self.builder.callback_slot(field_name);
        Ok(HandlerHirStmt::new(
            call_span,
            HandlerHirStmtKind::CallbackInvoke { slot, payload },
        ))
    }

    pub(super) fn lower_handler_ref_call_hir(
        &mut self,
        call: &ExprCall,
        outer_scope: &HandlerScope<'_>,
    ) -> syn::Result<Option<Vec<HandlerHirStmt>>> {
        let Expr::Path(ExprPath {
            path, qself: None, ..
        }) = call.func.as_ref()
        else {
            return Ok(None);
        };
        let Some(ident) = path.get_ident() else {
            return Ok(None);
        };
        let Some(handler) = self.handlers.get(&ident.to_string()).cloned() else {
            return Ok(None);
        };

        match handler.kind {
            HandlerLocalKind::Payload => {
                let param = payload_handler_param(&handler.closure)?;
                if call.args.len() != param.arity {
                    return Err(syn::Error::new(
                        call.span(),
                        format!(
                            "`{}` expects {} payload argument{} based on its `rsjs::handler(...)` closure",
                            handler.name,
                            param.arity,
                            if param.arity == 1 { "" } else { "s" }
                        ),
                    ));
                }

                let Some(param_name) = param.name.as_deref() else {
                    return Ok(Some(
                        self.lower_captured_handler_body(&handler, outer_scope)?,
                    ));
                };

                let payload =
                    self.lower_typed_handler_payload(&call.args[0], &param, outer_scope)?;
                let scoped = HandlerScope {
                    handler_param: Some((param_name, &payload)),
                    event_param: outer_scope.event_param,
                    handler_locals: outer_scope.handler_locals.clone(),
                    event_node: outer_scope.event_node,
                };
                self.lower_captured_handler_body(&handler, &scoped)
                    .map(Some)
            }
            HandlerLocalKind::Event => {
                let Some(event_node) = outer_scope.event_node else {
                    return Err(syn::Error::new(
                        call.span(),
                        format!(
                            "`{}` is an `rsjs::event_handler(...)` value and can only be called from a DOM event handler",
                            handler.name
                        ),
                    ));
                };
                let params = event_handler_local_params(&handler.closure)?;
                if call.args.len() != params.payload.arity {
                    return Err(syn::Error::new(
                        call.span(),
                        format!(
                            "`{}` expects {} payload argument{} based on its `rsjs::event_handler(...)` closure",
                            handler.name,
                            params.payload.arity,
                            if params.payload.arity == 1 { "" } else { "s" }
                        ),
                    ));
                }

                let payload = if params.payload.arity == 1 {
                    Some(self.lower_typed_handler_payload(
                        &call.args[0],
                        &params.payload,
                        outer_scope,
                    )?)
                } else {
                    None
                };
                let scoped = HandlerScope {
                    handler_param: params.payload.name.as_deref().zip(payload.as_ref()),
                    event_param: params.event_param.as_deref(),
                    handler_locals: outer_scope.handler_locals.clone(),
                    event_node: Some(event_node),
                };
                self.lower_captured_handler_body(&handler, &scoped)
                    .map(Some)
            }
        }
    }

    fn lower_typed_handler_payload(
        &mut self,
        arg: &Expr,
        param: &HandlerParam,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        let explicit_source =
            explicit_integer_source_in_expr(arg, "typed handler payload argument")?;
        match &param.integer_source {
            Some(IntegerSource::Unsupported(name)) => {
                Err(unsupported_integer_domain_error(arg.span(), name))
            }
            Some(IntegerSource::Supported(domain)) => {
                validate_explicit_integer_source_against_domain(
                    explicit_source.as_ref(),
                    *domain,
                    arg.span(),
                    "typed handler payload argument",
                )?;
                let mut payload = self.lower_handler_payload_expr(arg, scope)?;
                self.validate_integer_value_for_domain(
                    &mut payload,
                    explicit_source,
                    *domain,
                    &scope.handler_locals,
                    arg.span(),
                    "typed handler payload argument",
                    true,
                )?;
                Ok(self.anchor_integer_domain(payload, *domain))
            }
            None => self.lower_handler_payload_expr(arg, scope),
        }
    }

    pub(super) fn lower_simple_closure(
        &mut self,
        closure: &ExprClosure,
        receiver: SignalId,
        outer_scope: &HandlerScope<'_>,
    ) -> syn::Result<HandlerHirSimpleClosure> {
        if closure.inputs.len() != 1 {
            return Err(syn::Error::new(
                closure.span(),
                "`signal.update` closure must take exactly one parameter",
            ));
        }
        let param_ident = match &closure.inputs[0] {
            Pat::Ident(pi) => pi.ident.clone(),
            Pat::Type(pt) => match pt.pat.as_ref() {
                Pat::Ident(pi) => pi.ident.clone(),
                other => {
                    return Err(syn::Error::new(
                        other.span(),
                        "`signal.update` closure parameter must be a plain ident",
                    ));
                }
            },
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "`signal.update` closure parameter must be a plain ident",
                ));
            }
        };
        let param_name = param_ident.to_string();
        if let Some(receiver_domain) = self.builder.signal_integer_domain(receiver) {
            match handler_pat_integer_source(&closure.inputs[0]) {
                Some(IntegerSource::Supported(param_domain)) if param_domain != receiver_domain => {
                    return Err(integer_domain_mismatch_error(
                        closure.inputs[0].span(),
                        param_domain,
                        receiver_domain,
                        "signal.update closure parameter",
                    ));
                }
                Some(IntegerSource::Unsupported(name)) => {
                    return Err(unsupported_integer_domain_error(
                        closure.inputs[0].span(),
                        &name,
                    ));
                }
                None if matches!(&closure.inputs[0], Pat::Type(_)) => {
                    return Err(syn::Error::new(
                        closure.inputs[0].span(),
                        format!(
                            "rsjs/integer-domain-mismatch: signal.update closure parameter must be `{}` or left unannotated",
                            receiver_domain.as_str()
                        ),
                    ));
                }
                _ => {}
            }
        }
        let scope = ExprScope {
            closure_param: Some((param_name.as_str(), receiver)),
            inline_param: None,
            handler_param: outer_scope.handler_param,
            event_param: outer_scope.event_param,
            handler_locals: &outer_scope.handler_locals,
            node_payload_target: None,
        };

        // Pre-check: the user is attempting to assign in the body via
        // `<closure_param>.set(...)` or a nested
        // `<closure_param>.update(...)`. This trips the generic
        // "unsupported method" branch downstream with a confusing
        // message. Surface a clearer diagnostic here.
        let body_expr = match closure.body.as_ref() {
            Expr::Block(ExprBlock { block, .. }) => match block.stmts.last() {
                Some(Stmt::Expr(e, _)) => Some(e),
                _ => None,
            },
            other => Some(other),
        };
        if let Some(e) = body_expr {
            if let Some(err) = detect_set_or_update_on_closure_param(e, &param_name) {
                return Err(err);
            }
        }

        let (body_span, body) = match closure.body.as_ref() {
            Expr::Block(ExprBlock { block, .. }) => {
                // Round-4 review #5: `signal.update(|c| { let x = 5; c
                // + x })` previously kept only `block.stmts.last()` and
                // silently dropped any leading `let`s, which then
                // produced a confusing "unknown signal `x`" further
                // down (or, worse, accepted a body the analyser
                // misanalysed). Reject explicitly so the user knows the
                // intermediate binding is the problem.
                let mut diag: Option<syn::Error> = None;
                let total = block.stmts.len();
                for (i, s) in block.stmts.iter().enumerate() {
                    let is_tail = i + 1 == total;
                    // The tail expression statement is the closure's
                    // value; everything before it is an "intermediate"
                    // statement (a `let`, an `Expr` whose value is
                    // discarded, an item, ...).
                    if is_tail && matches!(s, Stmt::Expr(_, None)) {
                        continue;
                    }
                    let span = s.span();
                    let err = match s {
                        Stmt::Local(_) => syn::Error::new(
                            span,
                            "v0.1 of `signal.update(|c| ...)` requires a single expression \
                             body. Found a `let` binding here; move the binding outside the \
                             closure or inline the value into the expression.",
                        ),
                        _ => syn::Error::new(
                            span,
                            "v0.1 of `signal.update(|c| ...)` requires a single expression \
                             body. Found an intermediate statement here; move it outside the \
                             closure or fold it into the tail expression.",
                        ),
                    };
                    match &mut diag {
                        Some(d) => d.combine(err),
                        None => diag = Some(err),
                    }
                }
                if let Some(d) = diag {
                    return Err(d);
                }
                match block.stmts.last() {
                    Some(Stmt::Expr(e, _)) => (e.span(), self.lower_reactive_expr_in(e, &scope)?),
                    _ => {
                        return Err(syn::Error::new(
                            closure.span(),
                            "`signal.update` closure body must end in an expression",
                        ));
                    }
                }
            }
            other => (other.span(), self.lower_reactive_expr_in(other, &scope)?),
        };
        Ok(HandlerHirSimpleClosure {
            param: param_ident.to_string(),
            body: Box::new(HandlerHirExpr::new(body_span, body)),
        })
    }
}

/// Find explicit fixed-width evidence anywhere in an update result.
///
/// `integer_expr_source_hint` deliberately describes only one expression node;
/// that is what binary lowering needs for contextual literal typing. A shared
/// field starts as `Dyn`, though, so `count.update(|value| value + 1_u64)` must
/// discover the suffix inside the result before lowering the receiver read.
/// Rust checks the complete closure against `SharedField<T>::update`, making
/// that explicit suffix a sound boundary. Conflicting suffixes are rejected
/// here instead of choosing one.
fn simple_closure_explicit_integer_source(
    closure: &ExprClosure,
) -> syn::Result<Option<IntegerSource>> {
    explicit_integer_source_in_expr(&closure.body, "signal.update closure result")
}

/// Find explicit fixed-width evidence anywhere in a value expression.
///
/// This deliberately ignores bare literals and opaque paths. Only source
/// evidence that Rust itself checks at the surrounding typed boundary may
/// specialize a schema-less shared field.
pub(super) fn explicit_integer_source_in_expr(
    expr: &Expr,
    context: &str,
) -> syn::Result<Option<IntegerSource>> {
    fn merge(
        lhs: Option<IntegerSource>,
        rhs: Option<IntegerSource>,
        span: proc_macro2::Span,
        context: &str,
    ) -> syn::Result<Option<IntegerSource>> {
        match (lhs, rhs) {
            (None, source) | (source, None) => Ok(source),
            (Some(lhs), Some(rhs)) if lhs == rhs => Ok(Some(lhs)),
            (Some(IntegerSource::Supported(lhs)), Some(IntegerSource::Supported(rhs))) => {
                Err(integer_domain_mismatch_error(span, rhs, lhs, context))
            }
            (Some(IntegerSource::Unsupported(name)), _)
            | (_, Some(IntegerSource::Unsupported(name))) => {
                Err(unsupported_integer_domain_error(span, &name))
            }
        }
    }

    fn visit(expr: &Expr, context: &str) -> syn::Result<Option<IntegerSource>> {
        if let Some(source) = integer_expr_source_hint(expr) {
            return Ok(Some(source));
        }
        match expr {
            Expr::Binary(binary) => merge(
                visit(&binary.left, context)?,
                visit(&binary.right, context)?,
                binary.span(),
                context,
            ),
            Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Neg(_)) => {
                visit(&unary.expr, context)
            }
            Expr::Paren(paren) => visit(&paren.expr, context),
            Expr::Group(group) => visit(&group.expr, context),
            Expr::If(if_expr) => {
                let then_source = if_expr
                    .then_branch
                    .stmts
                    .last()
                    .and_then(|stmt| match stmt {
                        Stmt::Expr(expr, _) => Some(expr),
                        _ => None,
                    })
                    .map(|expr| visit(expr, context))
                    .transpose()?
                    .flatten();
                let else_source = if_expr
                    .else_branch
                    .as_ref()
                    .map(|(_, expr)| visit(expr, context))
                    .transpose()?
                    .flatten();
                merge(then_source, else_source, if_expr.span(), context)
            }
            Expr::Block(block) => block
                .block
                .stmts
                .last()
                .and_then(|stmt| match stmt {
                    Stmt::Expr(expr, _) => Some(expr),
                    _ => None,
                })
                .map(|expr| visit(expr, context))
                .transpose()
                .map(Option::flatten),
            _ => Ok(None),
        }
    }

    visit(expr, context)
}

/// Whether evaluating the same payload expression once instead of once per
/// consumer preserves ordinary Rust behavior.
///
/// Keep this deliberately syntactic and conservative. Calls and method calls
/// can be effectful, so equal token text is not enough to intern them. Plain
/// paths and immutable projection chains are the common repeated-loop case and
/// have no author-visible call boundary to collapse.
fn payload_expression_is_cse_safe(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(_) | Expr::Path(_) => true,
        Expr::Field(field) => payload_expression_is_cse_safe(&field.base),
        Expr::Index(index) => {
            payload_expression_is_cse_safe(&index.expr)
                && payload_expression_is_cse_safe(&index.index)
        }
        Expr::Paren(paren) => payload_expression_is_cse_safe(&paren.expr),
        Expr::Group(group) => payload_expression_is_cse_safe(&group.expr),
        Expr::Reference(reference) => payload_expression_is_cse_safe(&reference.expr),
        Expr::Cast(cast) => payload_expression_is_cse_safe(&cast.expr),
        _ => false,
    }
}

fn payload_label(expr: &Expr) -> String {
    match expr {
        Expr::Path(path) => path
            .path
            .get_ident()
            .map(ToString::to_string)
            .unwrap_or_else(|| "handler payload".to_string()),
        Expr::Field(field) => field.member.to_token_stream().to_string().replace(' ', ""),
        Expr::Lit(_) => "literal payload".to_string(),
        _ => "handler payload".to_string(),
    }
}

fn handler_local_call_source<'a>(
    call: &ExprCall,
    handlers: &'a HashMap<String, HandlerLocal>,
) -> Option<&'a HandlerLocal> {
    let Expr::Path(ExprPath {
        path, qself: None, ..
    }) = call.func.as_ref()
    else {
        return None;
    };
    let ident = path.get_ident()?;
    handlers.get(&ident.to_string())
}

fn source_anchor(span: proc_macro2::Span) -> rsjs_ir::SourceAnchor {
    let start = span.start();
    rsjs_ir::SourceAnchor {
        start_line: start.line,
        start_column: start.column + 1,
    }
}

/// If `expr` is a method call shaped `<closure_param>.set(...)` or
/// `<closure_param>.update(...)` (where `closure_param` matches the
/// closure's parameter name), return a span-pointed error explaining
/// the user is conflating `update` with `set`.
///
/// `signal.update(|c| ...)` expects the closure body to compute the
/// new value; assigning via `c.set(...)` inside the body is a common
/// confusion. The downstream lowering would otherwise hit the generic
/// "unsupported method `.set`" branch, which is technically correct
/// but unhelpful.
/// Round-4 review #4: produce a per-shape diagnostic when a handler
/// body uses a control-flow construct (`match`, `while`, `loop`,
/// `for`) that the current IR doesn't lower. The previous behaviour was
/// to fall through to `lower_reactive_expr`, which rejected them with
/// the generic "this is not a reactive expression" message; that
/// misclassified the user's intent (handlers are imperative side
/// effects, not reactive reads).
///
/// Returns `None` when `expr` is not one of these constructs (so the
/// caller continues with the normal lowering path). Returns a
/// span-pointed `syn::Error` otherwise; the message names the
/// construct and recommends the v0.1 escape hatches.
pub(super) fn unsupported_handler_control_flow(expr: &Expr) -> Option<syn::Error> {
    let (span, construct) = match expr {
        Expr::Match(e) => return Some(unsupported_handler_match_error(e.match_token.span)),
        Expr::While(e) => (e.while_token.span, "while"),
        Expr::Loop(e) => (e.loop_token.span, "loop"),
        Expr::ForLoop(e) => return Some(unsupported_handler_for_loop_error(e.for_token.span)),
        _ => return None,
    };
    Some(unsupported_handler_control_flow_error(span, construct))
}

pub(super) fn unsupported_handler_for_loop_error(span: proc_macro2::Span) -> syn::Error {
    guided_boundary_error(
        span,
        "this handler `for` loop is outside the current Client IR loop surface",
        "Handler bodies compile to JavaScript without running Rust at hydration time. rsjs currently \
         lowers static array literals and numeric ranges, but dynamic non-range iterators and labeled \
         loops need richer iterator semantics.",
        [
            "Rewrite the loop as a supported static array literal, static range, or dynamic numeric range.",
            "Move dynamic iterator logic into a standalone helper annotated once with `#[rsjs::client]`; reachable calls are linked automatically.",
            "Normalize the data into a signal-friendly primitive before it reaches the handler.",
        ],
        Some("handler.for_loop"),
    )
}

pub(super) fn unsupported_handler_control_flow_error(
    span: proc_macro2::Span,
    construct: &str,
) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "`#[rsjs]` handler bodies do not support `{construct}` yet. \
             The current client IR supports statement blocks, `let`, \
             signal/event operations, callback calls, and `if`/`else`; \
             move this logic into a standalone `#[rsjs::client]` helper or rewrite it \
             using the supported handler surface."
        ),
    )
}

/// Reject `rsjs::batch(...)` used in value position, e.g.
/// `let next = rsjs::batch(|| count.get() + 1);`.
///
/// `batch` IS a recognised primitive one statement up: in statement
/// position it groups the signal writes in its closure so dependent DOM
/// updates flush once. The client IR does not capture the closure's
/// return value, so a value-position batch has nothing to evaluate to.
/// Without this guard the call falls through to the generic "not a
/// recognised IR primitive" rejection, whose suggestions (including
/// `rsjs::server_only(...)`) would move the writes out of the client
/// handler entirely.
pub(super) fn batch_in_value_position_error(expr: &Expr) -> Option<syn::Error> {
    struct BatchCallFinder {
        found: Option<proc_macro2::Span>,
    }
    impl<'ast> Visit<'ast> for BatchCallFinder {
        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if self.found.is_some() {
                return;
            }
            if is_rsjs_handler_constructor_call(&node.func, "batch") {
                self.found = Some(node.span());
                return;
            }
            visit::visit_expr_call(self, node);
        }
    }
    let mut finder = BatchCallFinder { found: None };
    finder.visit_expr(expr);
    let span = finder.found?;
    Some(syn::Error::new(
        span,
        "`rsjs::batch(...)` cannot be used as a value here: in rsjs v0.1 `batch` is \
         statement-only inside handlers. It groups the signal writes in its closure so \
         dependent DOM updates flush once, and the closure's result is not captured by \
         the client IR. Compute the value with ordinary handler statements first, then \
         perform the writes in a standalone batch statement, e.g. \
         `let next = count.get() + 1; rsjs::batch(|| { count.set(next); total.set(next); });`",
    ))
}

pub(super) fn and_optional(prefix: Option<FrontendExpr>, rhs: FrontendExpr) -> FrontendExpr {
    match prefix {
        Some(lhs) => FrontendExpr::BinaryOp {
            op: BinOp::And,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        None => rhs,
    }
}

pub(super) fn invert_condition(condition: FrontendExpr) -> FrontendExpr {
    FrontendExpr::BinaryOp {
        op: BinOp::Eq,
        lhs: Box::new(condition),
        rhs: Box::new(FrontendExpr::Literal(Value::Bool(false))),
    }
}

pub(super) fn unsupported_handler_match_error(span: proc_macro2::Span) -> syn::Error {
    guided_boundary_error(
        span,
        "this handler `match` pattern is outside the current Client IR match surface",
        "rsjs lowers handler matches only when the pattern space has a direct JavaScript shape: \
         exact booleans, `Some(value)` / `Some(_)` / `None`, literals, bounded ranges, optional \
         guards, and a final `_` fallback. Path patterns and nested destructuring need richer \
         type-aware pattern lowering.",
        [
            "Rewrite the match to supported boolean, Option, literal, range, guard, and `_` arms.",
            "Move path-pattern or destructuring logic into a standalone helper annotated once with `#[rsjs::client]`; reachable calls are linked automatically.",
            "Normalize the matched value into a signal-friendly primitive before the handler uses it.",
        ],
        Some("handler.match"),
    )
}

pub(super) fn bool_match_arm(pat: &Pat) -> Option<bool> {
    match pat.to_token_stream().to_string().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub(super) fn handler_match_is_exact_bool(match_expr: &ExprMatch) -> bool {
    if match_expr.arms.len() != 2 {
        return false;
    }
    let mut true_arm = false;
    let mut false_arm = false;
    for arm in &match_expr.arms {
        if arm.guard.is_some() {
            return false;
        }
        match bool_match_arm(&arm.pat) {
            Some(true) if !true_arm => true_arm = true,
            Some(false) if !false_arm => false_arm = true,
            _ => return false,
        }
    }
    true_arm && false_arm
}

pub(super) struct HandlerLiteralMatchPattern {
    pub(super) condition: FrontendExpr,
    pub(super) option_binding: Option<String>,
}

pub(super) fn handler_literal_match_pattern(
    local: LocalId,
    pat: &Pat,
    integer_domain: Option<IntegerDomain>,
) -> syn::Result<HandlerLiteralMatchPattern> {
    match pat {
        Pat::Paren(paren) => {
            return handler_literal_match_pattern(local, &paren.pat, integer_domain)
        }
        Pat::Type(typed) => {
            return handler_literal_match_pattern(local, &typed.pat, integer_domain)
        }
        Pat::Ident(ident) if ident.ident == "None" => {
            return Ok(HandlerLiteralMatchPattern {
                condition: handler_option_none_condition(local),
                option_binding: None,
            });
        }
        Pat::Path(path) if path.path.is_ident("None") => {
            return Ok(HandlerLiteralMatchPattern {
                condition: handler_option_none_condition(local),
                option_binding: None,
            });
        }
        Pat::TupleStruct(tuple) if tuple.path.is_ident("Some") => {
            return handler_option_some_pattern(local, tuple);
        }
        Pat::Range(range) => {
            return Ok(HandlerLiteralMatchPattern {
                condition: handler_range_match_condition(local, range, integer_domain)?,
                option_binding: None,
            });
        }
        _ => {}
    }

    validate_handler_pattern_integer_source(
        handler_literal_pattern_integer_source(pat),
        integer_domain,
        pat.span(),
    )?;
    let mut value = handler_literal_match_value(pat)?;
    if let Some(domain) = integer_domain {
        coerce_integer_literals_to_domain(&mut value, domain, pat.span())?;
    }
    Ok(HandlerLiteralMatchPattern {
        condition: FrontendExpr::BinaryOp {
            op: BinOp::Eq,
            lhs: Box::new(FrontendExpr::Local(local)),
            rhs: Box::new(value),
        },
        option_binding: None,
    })
}

pub(super) fn handler_option_some_pattern(
    local: LocalId,
    tuple: &syn::PatTupleStruct,
) -> syn::Result<HandlerLiteralMatchPattern> {
    if tuple.elems.len() != 1 {
        return Err(syn::Error::new(
            tuple.span(),
            "`#[rsjs]` handler `Some(...)` match arms support only `Some(_)` or `Some(value)`",
        ));
    }
    let elem = tuple
        .elems
        .first()
        .expect("tuple length checked before reading Some payload pattern");
    Ok(HandlerLiteralMatchPattern {
        condition: handler_option_present_condition(local),
        option_binding: handler_option_payload_binding(elem)?,
    })
}

pub(super) fn handler_option_payload_binding(pat: &Pat) -> syn::Result<Option<String>> {
    match pat {
        Pat::Wild(_) => Ok(None),
        Pat::Paren(paren) => handler_option_payload_binding(&paren.pat),
        Pat::Type(typed) => handler_option_payload_binding(&typed.pat),
        Pat::Ident(ident) => handler_option_payload_binding_name(ident).map(Some),
        other => Err(syn::Error::new(
            other.span(),
            "`#[rsjs]` handler `Some(...)` match arms support only `Some(_)` or `Some(value)`; nested destructuring is future Client IR work",
        )),
    }
}

pub(super) fn handler_option_payload_binding_name(ident: &syn::PatIdent) -> syn::Result<String> {
    if ident.by_ref.is_some() {
        return Err(syn::Error::new(
            ident.span(),
            "`#[rsjs]` handler `Some(ref value)` match bindings are not supported; option payload bindings are immutable values",
        ));
    }
    if ident.mutability.is_some() {
        return Err(syn::Error::new(
            ident.span(),
            "`#[rsjs]` handler `Some(mut value)` match bindings are not supported; option payload bindings are immutable values",
        ));
    }
    if ident.subpat.is_some() {
        return Err(syn::Error::new(
            ident.span(),
            "`#[rsjs]` handler `Some(value @ ...)` match bindings are future Client IR work",
        ));
    }
    if ident.ident == "None" || ident.ident == "Some" {
        return Err(syn::Error::new(
            ident.span(),
            "`#[rsjs]` handler `Some(...)` match arms support only `Some(_)` or `Some(value)`; nested option patterns are future Client IR work",
        ));
    }
    Ok(ident.ident.to_string())
}

pub(super) fn handler_option_present_condition(local: LocalId) -> FrontendExpr {
    FrontendExpr::IsPresent {
        target: Box::new(FrontendExpr::Local(local)),
    }
}

pub(super) fn handler_option_none_condition(local: LocalId) -> FrontendExpr {
    FrontendExpr::BinaryOp {
        op: BinOp::Eq,
        lhs: Box::new(handler_option_present_condition(local)),
        rhs: Box::new(FrontendExpr::Literal(Value::Bool(false))),
    }
}

pub(super) fn handler_range_match_condition(
    local: LocalId,
    range: &syn::PatRange,
    integer_domain: Option<IntegerDomain>,
) -> syn::Result<FrontendExpr> {
    let Some(start) = &range.start else {
        return Err(unsupported_handler_match_error(range.span()));
    };
    let Some(end) = &range.end else {
        return Err(unsupported_handler_match_error(range.span()));
    };

    validate_handler_pattern_integer_source(
        integer_expr_source_hint(start),
        integer_domain,
        start.span(),
    )?;
    validate_handler_pattern_integer_source(
        integer_expr_source_hint(end),
        integer_domain,
        end.span(),
    )?;
    let mut lower_bound = handler_range_match_bound(start)?;
    let mut upper_bound = handler_range_match_bound(end)?;
    if let Some(domain) = integer_domain {
        coerce_integer_literals_to_domain(&mut lower_bound, domain, start.span())?;
        coerce_integer_literals_to_domain(&mut upper_bound, domain, end.span())?;
    }
    let lower = FrontendExpr::BinaryOp {
        op: BinOp::Ge,
        lhs: Box::new(FrontendExpr::Local(local)),
        rhs: Box::new(lower_bound),
    };
    let upper = FrontendExpr::BinaryOp {
        op: match range.limits {
            RangeLimits::HalfOpen(_) => BinOp::Lt,
            RangeLimits::Closed(_) => BinOp::Le,
        },
        lhs: Box::new(FrontendExpr::Local(local)),
        rhs: Box::new(upper_bound),
    };
    Ok(FrontendExpr::BinaryOp {
        op: BinOp::And,
        lhs: Box::new(lower),
        rhs: Box::new(upper),
    })
}

fn handler_literal_pattern_integer_source(pat: &Pat) -> Option<IntegerSource> {
    match pat {
        Pat::Lit(lit) => match &lit.lit {
            Lit::Int(value) if !value.suffix().is_empty() => {
                let name = value.suffix().to_string();
                Some(
                    IntegerDomain::from_name(&name)
                        .map(IntegerSource::Supported)
                        .unwrap_or(IntegerSource::Unsupported(name)),
                )
            }
            _ => None,
        },
        Pat::Paren(paren) => handler_literal_pattern_integer_source(&paren.pat),
        Pat::Type(typed) => handler_literal_pattern_integer_source(&typed.pat),
        _ => None,
    }
}

fn validate_handler_pattern_integer_source(
    source: Option<IntegerSource>,
    expected: Option<IntegerDomain>,
    span: proc_macro2::Span,
) -> syn::Result<()> {
    match (source, expected) {
        (Some(IntegerSource::Unsupported(name)), _) => {
            Err(unsupported_integer_domain_error(span, &name))
        }
        (Some(IntegerSource::Supported(actual)), Some(expected)) if actual != expected => Err(
            integer_domain_mismatch_error(span, actual, expected, "handler match pattern"),
        ),
        (Some(IntegerSource::Supported(_)), None) => Err(syn::Error::new(
            span,
            "rsjs/integer-domain-unproven: a suffixed integer match pattern needs a scrutinee with a proven matching i32/u32/i64/u64 domain",
        )),
        _ => Ok(()),
    }
}

pub(super) fn handler_range_match_bound(expr: &Expr) -> syn::Result<FrontendExpr> {
    match expr {
        Expr::Lit(ExprLit { lit, .. }) => {
            Ok(FrontendExpr::Literal(lit_to_value(lit, expr.span())?))
        }
        Expr::Paren(paren) => handler_range_match_bound(&paren.expr),
        Expr::Group(group) => handler_range_match_bound(&group.expr),
        other => Err(unsupported_handler_match_error(other.span())),
    }
}

pub(super) fn handler_literal_match_value(pat: &Pat) -> syn::Result<FrontendExpr> {
    match pat {
        Pat::Lit(lit) => Ok(FrontendExpr::Literal(lit_to_value(&lit.lit, lit.span())?)),
        Pat::Paren(paren) => handler_literal_match_value(&paren.pat),
        Pat::Type(typed) => handler_literal_match_value(&typed.pat),
        other => Err(unsupported_handler_match_error(other.span())),
    }
}

pub(super) fn inline_reactive_closure_param_name(closure: &ExprClosure) -> syn::Result<String> {
    if closure.inputs.len() != 1 {
        return Err(syn::Error::new(
            closure.inputs.span(),
            "`#[rsjs]` reactive inline closure calls currently support exactly one parameter",
        ));
    }
    let pat = closure
        .inputs
        .first()
        .expect("len checked before reading first closure input");
    inline_reactive_closure_pat_name(pat)
}

pub(super) fn inline_reactive_closure_pat_name(pat: &Pat) -> syn::Result<String> {
    match pat {
        Pat::Ident(pi) => {
            if pi.mutability.is_some() {
                return Err(syn::Error::new(
                    pi.mutability.span(),
                    "`#[rsjs]` reactive inline closure parameters cannot be `mut`",
                ));
            }
            Ok(pi.ident.to_string())
        }
        Pat::Type(pt) => inline_reactive_closure_pat_name(&pt.pat),
        other => Err(syn::Error::new(
            other.span(),
            "`#[rsjs]` reactive inline closure calls currently support only a plain parameter \
             identifier",
        )),
    }
}

pub(super) fn inline_closure_func(expr: &Expr) -> Option<&ExprClosure> {
    match expr {
        Expr::Closure(closure) => Some(closure),
        Expr::Paren(paren) => inline_closure_func(&paren.expr),
        _ => None,
    }
}

pub(super) fn static_i64_range_bound(expr: &Expr) -> syn::Result<Option<i64>> {
    match expr {
        Expr::Lit(ExprLit { lit, .. }) => match lit_to_value(lit, expr.span())? {
            Value::Int(value) => Ok(Some(value)),
            Value::BigInt(value) => i64::try_from(value)
                .map(Some)
                .map_err(|_| {
                    syn::Error::new(
                        expr.span(),
                        "rsjs/integer-range-static-carrier: static handler range expansion currently requires bounds within i64; use a dynamic typed u64 signal range or reduce the literal bounds",
                    )
                }),
            _ => Err(unsupported_handler_for_loop_error(expr.span())),
        },
        Expr::Paren(paren) => static_i64_range_bound(&paren.expr),
        _ => Ok(None),
    }
}

pub(super) fn handler_for_loop_pat_name(pat: &Pat) -> syn::Result<String> {
    match pat {
        Pat::Ident(pi) => {
            if pi.mutability.is_some() {
                return Err(syn::Error::new(
                    pi.mutability.span(),
                    "`#[rsjs]` handler `for` loop bindings cannot be `mut`; loop items are immutable reactive expressions",
                ));
            }
            Ok(pi.ident.to_string())
        }
        Pat::Type(pt) => handler_for_loop_pat_name(&pt.pat),
        other => Err(syn::Error::new(
            other.span(),
            "`#[rsjs]` handler `for` loops currently support only a plain item identifier over static array literals or numeric ranges",
        )),
    }
}

pub(super) fn detect_set_or_update_on_closure_param(
    expr: &Expr,
    param_name: &str,
) -> Option<syn::Error> {
    let mc = match expr {
        Expr::MethodCall(mc) => mc,
        // Walk through a Paren since `(c.set(x))` is the same shape.
        Expr::Paren(p) => return detect_set_or_update_on_closure_param(&p.expr, param_name),
        _ => return None,
    };
    let method = mc.method.to_string();
    if method != "set" && method != "update" {
        return None;
    }
    // The receiver must be a path resolving to the closure parameter.
    let recv_path = match mc.receiver.as_ref() {
        Expr::Path(p) => &p.path,
        _ => return None,
    };
    let recv_ident = recv_path.get_ident()?;
    if recv_ident != param_name {
        return None;
    }
    Some(syn::Error::new(
        mc.span(),
        "`signal.update(|c| ...)` expects the closure body to compute the new value. \
         To assign, use `signal.set(<value>)` at the handler-statement level",
    ))
}
