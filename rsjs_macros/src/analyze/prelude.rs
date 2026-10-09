// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Prelude `let` recognition (signal/memo/handler/node_ref/query/mutation declarations) and the Analyzer collection passes.
use super::*;

impl Analyzer {
    /// Ordinary server statements remain byte-for-byte Rust, but they may not
    /// become a hidden execution surface for compiler-owned browser values.
    ///
    /// This validation is intentionally lexical. It does not interpret normal
    /// Rust calls; it only rejects references to declarations the analyzer
    /// owns plus visibly named RSJS/browser primitives. Everything else is
    /// preserved by the lossless server-program rewriter.
    pub(super) fn validate_opaque_server_statements(&self, statements: &[Stmt]) -> syn::Result<()> {
        let mut client_names = HashSet::new();
        let mut local_index = 0_usize;
        for statement in statements {
            if let Stmt::Local(local) = statement {
                let index = local_index;
                local_index += 1;
                let compiler_owned = self.prelude_signal_ids.contains_key(&index)
                    || self.prelude_shared_store_ids.contains_key(&index)
                    || self.prelude_query_ids.contains_key(&index)
                    || self.prelude_mutation_ids.contains_key(&index)
                    || is_handler_local(local)
                    || is_node_ref_local(local)
                    || self.client_prelude_local_indices.contains(&index);

                // An ordinary `let` initializer runs before its pattern comes
                // into scope. Validate it against the client bindings visible
                // at that exact point; otherwise `let _ = count.get();` would
                // evade the same rule enforced for an `if count.get()`.
                if !compiler_owned {
                    if let Some(init) = &local.init {
                        let mut visitor = OpaqueServerStatementValidator {
                            client_names: &client_names,
                            shadowed_names: HashSet::new(),
                            error: None,
                        };
                        visitor.visit_expr(&init.expr);
                        if let Some(error) = visitor.error {
                            return Err(error);
                        }
                    }
                }

                // Every `let` shadows the preceding binding before the next
                // statement. Reinsert the name only when this exact lexical
                // declaration is compiler-owned client state.
                for name in pattern_binding_names(&local.pat) {
                    client_names.remove(&name);
                }
                if compiler_owned {
                    if let Some(name) = local_ident(local).map(|ident| ident.to_string()) {
                        client_names.insert(name);
                    }
                }
                continue;
            }
            if matches!(
                statement,
                Stmt::Expr(Expr::Call(call), _)
                    if crate::parse::is_interval_call_expr(call)
            ) {
                continue;
            }

            let mut visitor = OpaqueServerStatementValidator {
                client_names: &client_names,
                shadowed_names: HashSet::new(),
                error: None,
            };
            visitor.visit_stmt(statement);
            if let Some(error) = visitor.error {
                return Err(error);
            }
        }
        Ok(())
    }

    /// Collect every compiler-owned prelude declaration in authored order.
    ///
    /// Rust puts a `let` pattern in scope only after its initializer. Each
    /// initializer is therefore lowered against the exact client environment
    /// left by preceding statements, then its pattern replaces every
    /// same-named binding before the next statement. Timers are lowered at
    /// their statement position instead of after the complete prelude.
    // proa-perf-ok(scope): compiler analysis builds diagnostics and IR; it never renders request HTML
    pub(super) fn collect_prelude_bindings(
        &mut self,
        statements: &[Stmt],
        lets: &[Local],
    ) -> syn::Result<()> {
        let mut all_shared_store_names = HashSet::new();
        for local in lets {
            if let Some(shared) = shared_store_local_decl(local)? {
                all_shared_store_names.insert(shared.name.to_string());
            }
        }
        let replayed_names = server_replayed_prelude_names(lets);
        let mut bound_names = HashSet::new();
        let mut prior_lets = Vec::new();
        let mut local_index = 0_usize;

        for statement in statements {
            match statement {
                Stmt::Local(local) => {
                    let index = local_index;
                    local_index += 1;
                    let claimed = self.collect_shared_or_signal_local(
                        index,
                        local,
                        &all_shared_store_names,
                        &bound_names,
                        &replayed_names,
                    )? || self.collect_node_ref_local(index, local)?
                        || self.collect_handler_local(index, local)?
                        || self.collect_query_local(index, local, &prior_lets)?
                        || self.collect_mutation_local(index, local)?;

                    if !claimed {
                        self.collect_client_prelude_local(index, local)?;
                    }
                    for name in pattern_binding_names(&local.pat) {
                        bound_names.insert(name);
                    }
                    prior_lets.push(local.clone());
                }
                Stmt::Expr(Expr::Call(call), _) if crate::parse::is_interval_call_expr(call) => {
                    self.collect_interval(call)?;
                }
                _ => {}
            }
        }
        debug_assert_eq!(local_index, lets.len());
        Ok(())
    }

    /// Remove the currently visible binding for one source name from every
    /// client namespace. The caller inserts the new declaration afterwards.
    fn shadow_prelude_binding(&mut self, name: &str) {
        self.shared_stores.remove(name);
        self.signals.remove(name);
        self.queries.remove(name);
        self.mutations.remove(name);
        self.node_refs.remove(name);
        self.handlers.remove(name);
        self.client_prelude_locals.remove(name);
        self.unsupported_client_prelude_locals.remove(name);
    }

    fn prelude_client_binding_env(&self) -> PreludeClientBindingEnv {
        PreludeClientBindingEnv {
            signals: self.signals.clone(),
            queries: self.queries.clone(),
            mutations: self.mutations.clone(),
            node_refs: self.node_refs.clone(),
            handlers: self.handlers.clone(),
            client_prelude_locals: self.client_prelude_locals.clone(),
            unsupported_client_prelude_locals: self.unsupported_client_prelude_locals.clone(),
        }
    }

    fn with_prelude_client_binding_env<T>(
        &mut self,
        env: &PreludeClientBindingEnv,
        lower: impl FnOnce(&mut Self) -> syn::Result<T>,
    ) -> syn::Result<T> {
        let saved = self.prelude_client_binding_env();
        self.signals = env.signals.clone();
        self.queries = env.queries.clone();
        self.mutations = env.mutations.clone();
        self.node_refs = env.node_refs.clone();
        self.handlers = env.handlers.clone();
        self.client_prelude_locals = env.client_prelude_locals.clone();
        self.unsupported_client_prelude_locals = env.unsupported_client_prelude_locals.clone();
        let result = lower(self);
        self.signals = saved.signals;
        self.queries = saved.queries;
        self.mutations = saved.mutations;
        self.node_refs = saved.node_refs;
        self.handlers = saved.handlers;
        self.client_prelude_locals = saved.client_prelude_locals;
        self.unsupported_client_prelude_locals = saved.unsupported_client_prelude_locals;
        result
    }

    pub(super) fn lower_captured_handler_body(
        &mut self,
        handler: &HandlerLocal,
        scope: &HandlerScope<'_>,
    ) -> syn::Result<Vec<HandlerHirStmt>> {
        let Some(env) = handler.capture_env.clone() else {
            return self.lower_handler_closure_hir_body(&handler.closure, scope);
        };
        self.with_prelude_client_binding_env(&env, |analyzer| {
            analyzer.lower_handler_closure_hir_body(&handler.closure, scope)
        })
    }

    /// Pass 1: scan shared-store, shared-field, signal, and memo bindings
    /// in lexical order. This keeps source shadowing and runtime initializer
    /// identity exact while preventing a field from seeing a store declared
    /// later in the Rust prelude.
    ///
    /// The Rust state type itself may live in any module; the macro preserves
    /// its syntax and lets rustc/`SharedState` resolve it.
    ///
    /// `let <name> = signal(<lit>)` / `signal(<runtime expr>)` and
    /// `let <name> = memo(|| <expr>)`, allocating both in source
    /// order so a memo cell's id is greater than the ids of every
    /// signal its body reads (Rust scoping guarantees the names it
    /// references were declared earlier in the prelude).
    // proa-perf-ok(scope): compiler analysis builds diagnostics and IR; it never renders request HTML
    #[cfg(test)]
    pub(super) fn collect_shared_stores_and_signals(&mut self, lets: &[Local]) -> syn::Result<()> {
        let mut all_shared_store_names = HashSet::new();
        for local in lets {
            if let Some(shared) = shared_store_local_decl(local)? {
                all_shared_store_names.insert(shared.name.to_string());
            }
        }

        let replayed_names = server_replayed_prelude_names(lets);
        // Real lexical scope for the prelude: `bound_names` holds every name
        // an *earlier* `let` already bound, whatever it was bound to. A
        // reference resolves against the binding in scope at that point, so a
        // plain server local that shares a name with a shared store declared
        // further down is never mistaken for the store.
        let mut bound_names: HashSet<String> = HashSet::new();
        for (index, local) in lets.iter().enumerate() {
            self.collect_shared_or_signal_local(
                index,
                local,
                &all_shared_store_names,
                &bound_names,
                &replayed_names,
            )?;

            let compiler_owned = self.prelude_signal_ids.contains_key(&index)
                || self.prelude_shared_store_ids.contains_key(&index);
            for name in pattern_binding_names(&local.pat) {
                bound_names.insert(name.clone());
                if !compiler_owned {
                    // The pattern enters scope only after its initializer.
                    // From the next declaration onward, an ordinary Rust
                    // binding with this name hides any earlier signal,
                    // memo cell, shared field, or SharedState provider.
                    // Keeping the old entries here made later memo/shared
                    // declarations bind to a value Rust itself could no
                    // longer see.
                    self.signals.remove(&name);
                    self.shared_stores.remove(&name);
                }
            }
        }
        Ok(())
    }

    /// Classify one prelude `let` at lexical position `index`.
    ///
    /// `bound_names` is the set of names bound by the `let`s before this one;
    /// `all_shared_store_names` is every name bound to an `rsjs::shared::<T>()`
    /// acquisition anywhere in the prelude, used only to tell a genuine
    /// use-before-acquire apart from an unrelated local. `replayed_names` is
    /// every name a verbatim-replayed server `let` still binds in the
    /// generated render body.
    // proa-perf-ok(scope): compiler analysis builds diagnostics and IR; it never renders request HTML
    fn collect_shared_or_signal_local(
        &mut self,
        index: usize,
        local: &Local,
        all_shared_store_names: &HashSet<String>,
        bound_names: &HashSet<String>,
        replayed_names: &HashSet<String>,
    ) -> syn::Result<bool> {
        if let Some(shared) = shared_store_local_decl(local)? {
            self.require_client_origin(shared.name.span(), ClientOriginKind::SharedState)?;
            let source_name = shared.name.to_string();
            // The generated render binds the store handle ahead of the
            // replayed prelude and reads its fields after it, so a server
            // local that reuses the source name would shadow the handle
            // in between. Emit the handle under a generated name in that
            // case, the way component inlining already does.
            let emitted_name = if replayed_names.contains(&source_name) {
                quote::format_ident!(
                    "__rsjs_shared_store_{}",
                    self.shared_state_initializers.len()
                )
            } else {
                shared.name.clone()
            };
            let id = self.builder.shared_store(
                emitted_name.to_string(),
                shared.ty.to_token_stream().to_string(),
                Some(source_anchor(shared.name.span())),
            );
            self.shadow_prelude_binding(&source_name);
            self.shared_stores.insert(source_name.clone(), id);
            self.prelude_shared_store_ids.insert(index, id);
            self.shared_prelude_local_indices.insert(index);
            self.shared_state_initializers.push(SharedStateInitializer {
                id,
                source_name: emitted_name,
                ty: shared.ty,
                expected_type_key: None,
            });
            return Ok(true);
        }
        if let Some(error) = chained_shared_acquire_error(local) {
            return Err(error);
        }
        // Any other shape that still hides an `rsjs::shared::<T>()` call has no
        // store identity the analyzer can bind, and replaying it into the
        // server render would reach the panicking stub at request time.
        if let Some(error) = unrecognized_shared_acquire_error(local) {
            return Err(error);
        }
        if let Some(decl) = signal_local_decl(local)? {
            self.require_client_origin(decl.name.span(), ClientOriginKind::Signal)?;
            if let Some(expr) = decl.runtime_expr.as_ref() {
                self.validate_signal_seed_expr(expr)?;
            }
            let runtime_expr = decl.runtime_expr.clone();
            let explicit_ty = decl.explicit_ty.clone();
            let ty = signal_decl_client_type(&decl);
            let integer_domain = match &decl.integer_source {
                Some(IntegerSource::Supported(domain)) => Some(*domain),
                _ => None,
            };
            let id = self.builder.signal_typed(decl.fallback, ty);
            self.builder.set_signal_integer_domain(id, integer_domain);
            if let Some(IntegerSource::Unsupported(name)) = &decl.integer_source {
                self.unsupported_integer_signals.insert(id, name.clone());
            }
            self.builder.set_signal_source(
                id,
                Some(decl.name.to_string()),
                Some(decl.source_anchor),
            );
            let name = decl.name.to_string();
            self.shadow_prelude_binding(&name);
            self.signals.insert(name, id);
            self.prelude_signal_ids.insert(index, id);
            if let Some((field_name, field_span)) =
                runtime_expr.as_ref().and_then(inbound_signal_self_field)
            {
                // This is a local-state constructor, not a live alias to the
                // parent value. The linked child samples the then-current
                // inbound value once for each mounted instance.
                self.record_inbound_signal_seed(field_name, field_span, id)?;
            }
            if let Some(expr) = runtime_expr {
                self.signal_initializers.push(RuntimeSignalInitializer {
                    id,
                    expr,
                    explicit_ty,
                });
            }
            return Ok(true);
        }
        if let Some(field) = shared_field_local_decl(local, &self.shared_stores)? {
            let field_name = field.field.to_string();
            let id = if let Some(id) = self
                .shared_fields
                .get(&(field.store, field_name.clone()))
                .copied()
            {
                id
            } else {
                let ty =
                    self.shared_field_client_type(field.store, &field_name, field.field.span())?;
                let id = self
                    .builder
                    .shared_field(field.store, field_name.clone(), ty);
                self.builder.set_signal_source(
                    id,
                    Some(field.name.to_string()),
                    Some(source_anchor(field.field.span())),
                );
                self.shared_fields.insert((field.store, field_name), id);
                id
            };
            let name = field.name.to_string();
            self.shadow_prelude_binding(&name);
            self.signals.insert(name, id);
            self.prelude_signal_ids.insert(index, id);
            self.shared_prelude_local_indices.insert(index);
            return Ok(true);
        }
        if let Some(receiver) = shared_field_receiver_name(local) {
            // Only a receiver that is not bound to anything yet can be a
            // forward reference to a store declared later. A receiver already
            // bound at this point is an ordinary local (a `String`, a `Vec`, a
            // config struct) that merely shares its name with a later store,
            // and its zero-argument method call is plain server Rust.
            if !bound_names.contains(&receiver) && all_shared_store_names.contains(&receiver) {
                return Err(syn::Error::new(
                    local.span(),
                    format!(
                        "rsjs/shared-state-use-before-acquire: `{receiver}` is used before its `rsjs::shared::<T>()` declaration; move the shared-state acquisition above this field binding"
                    ),
                ));
            }
        }
        if let Some((name, call)) = memo_local_call(local) {
            let body = self.lower_memo_body(call)?;
            let body_id = self.alloc_expr(body, call.span())?;
            let id = self.builder.memo(body_id);
            // Backfill the derived cell's declared type from its
            // body. Rust scoping guarantees the body only reads
            // earlier prelude declarations, so the typing pass
            // already has everything it needs.
            let ty = self.type_env(&[]).signal_type(id);
            self.builder.set_signal_ty(id, ty);
            let name = name.to_string();
            self.shadow_prelude_binding(&name);
            self.signals.insert(name, id);
            self.prelude_signal_ids.insert(index, id);
            self.memo_cells.insert(id);
            return Ok(true);
        }
        Ok(false)
    }

    /// Signal initializers execute once as ordinary server seed expressions.
    /// They cannot read an earlier compiler-owned cell: the authored signal
    /// handle is intentionally replaced by a generated initial-value local in
    /// the lossless server program, and silently treating a live read as a
    /// server snapshot would also give the wrong temporal semantics.
    fn validate_signal_seed_expr(&self, expr: &Expr) -> syn::Result<()> {
        let client_names: HashSet<String> = self
            .signals
            .keys()
            .chain(self.queries.keys())
            .chain(self.mutations.keys())
            .chain(self.node_refs.keys())
            .chain(self.handlers.keys())
            .chain(self.client_prelude_locals.keys())
            .chain(self.unsupported_client_prelude_locals.keys())
            .cloned()
            .collect();
        let mut visitor = OpaqueServerStatementValidator {
            client_names: &client_names,
            shadowed_names: HashSet::new(),
            error: None,
        };
        visitor.visit_expr(expr);
        if visitor.error.is_none() {
            return Ok(());
        }
        Err(syn::Error::new(
            expr.span(),
            "rsjs/signal-seed-client-dependency: a signal initializer is a \
             one-time server seed and cannot read an earlier RSJS client cell \
             or browser operation\nhelp: compute an ordinary server value \
             before declaring client state, or use `rsjs::memo(|| ...)` \
             when the value should stay derived from another signal",
        ))
    }

    /// Client value shape of a page-shared field.
    ///
    /// Always [`ClientType::Dyn`], and deliberately so. A shared field's Rust
    /// width decides whether the browser holds the value as a BigInt or a
    /// Number, and a proc macro cannot resolve a Rust type: the island macro
    /// expanding `rsjs::shared::<CartState>()` sees only tokens. Every
    /// mechanism that tried to recover the width outside the macro invocation
    /// (an on-disk descriptor sidecar keyed on the unqualified struct name, an
    /// in-process registry populated by the derive) was build-order dependent,
    /// stale-artifact sensitive, and collided across crates and modules.
    ///
    /// So the compiler stops asking. The width still reaches the browser, by
    /// the one channel that is type-checked rather than name-matched:
    /// `#[derive(SharedState)]` emits `__rsjs_wide_paths_json()`, which
    /// references `__rsjs_codec_meta_<Ident>` by path from inside its own
    /// expansion, walks the real `ClientPropsMeta`, and ships the true wide
    /// paths in the page seed. `install_page_shared_state` pins each field cell
    /// to that representation. Bare shared-field arithmetic is rejected because
    /// representation alone cannot reproduce Rust's width-specific overflow.
    /// An explicitly typed update boundary (for example `|n| n + 1_u64` or
    /// `|n: u64| n + 1`) may specialize the field to the ordinary checked
    /// domain: Rust type-checks that closure against `SharedField<T>`, so no
    /// name-based side channel is involved.
    ///
    /// `store`, `field`, and `span` are kept in the signature so callers do not
    /// have to know that the answer is constant, and so a future design that
    /// recovers a width for some fields has somewhere to put it.
    pub(super) fn shared_field_client_type(
        &mut self,
        store: SharedStoreId,
        field: &str,
        span: Span,
    ) -> syn::Result<ClientType> {
        let _ = (store, field, span);
        Ok(ClientType::Dyn)
    }

    /// Validate and lower the closure body of a `memo(...)`
    /// prelude declaration into a reactive [`FrontendExpr`].
    pub(super) fn lower_memo_body(&mut self, call: &ExprCall) -> syn::Result<FrontendExpr> {
        if call.args.len() != 1 {
            return Err(syn::Error::new(
                call.span(),
                "`memo(...)` takes exactly one zero-argument closure, \
                 e.g. `let total = rsjs::memo(|| price.get() * qty.get());`",
            ));
        }
        let closure = match &call.args[0] {
            Expr::Closure(closure) => closure,
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "`memo(...)` expects a closure argument; the closure body is the \
                     derived expression that re-evaluates when its signals change, \
                     e.g. `let total = rsjs::memo(|| price.get() * qty.get());`",
                ));
            }
        };
        if !closure.inputs.is_empty() {
            return Err(syn::Error::new(
                closure.inputs.span(),
                "`memo(...)` closures take no parameters; they read signals directly, \
                 e.g. `rsjs::memo(|| price.get() * qty.get())`",
            ));
        }
        let body = match closure.body.as_ref() {
            Expr::Block(ExprBlock { block, .. }) => match block.stmts.as_slice() {
                [Stmt::Expr(expr, None)] => expr,
                _ => return Err(memo_body_error(block.span())),
            },
            expr => expr,
        };
        self.lower_reactive_expr(body)
    }

    fn collect_node_ref_local(&mut self, index: usize, local: &Local) -> syn::Result<bool> {
        let mut locals = collect_node_ref_locals(std::slice::from_ref(local))?;
        let Some(local) = locals.pop() else {
            return Ok(false);
        };
        self.require_client_origin(local.name.span(), ClientOriginKind::NodeRef)?;
        let id = self.builder.node_ref();
        self.builder.set_node_ref_source(
            id,
            Some(local.name.to_string()),
            Some(local.source_anchor),
        );
        let name = local.name.to_string();
        self.shadow_prelude_binding(&name);
        self.node_refs.insert(name, id);
        self.prelude_node_ref_ids.insert(index, id);
        Ok(true)
    }

    // Capture environments are immutable compiler data shared by handlers in
    // one macro expansion. They never cross a thread boundary, while the IR's
    // shared pointer type is intentionally `Arc` throughout.
    #[allow(clippy::arc_with_non_send_sync)]
    fn collect_handler_local(&mut self, index: usize, local: &Local) -> syn::Result<bool> {
        let mut handlers = collect_handler_locals(std::slice::from_ref(local))?;
        let Some(mut handler) = handlers.pop() else {
            return Ok(false);
        };
        self.require_client_origin(handler.name.span(), ClientOriginKind::Handler)?;
        handler.prelude_local_index = u32::try_from(index).map_err(|_| {
            syn::Error::new(
                handler.name.span(),
                "an analyzed RSJS component cannot contain more than 2^32 prelude bindings",
            )
        })?;
        handler.capture_env = Some(std::sync::Arc::new(self.prelude_client_binding_env()));
        let name = handler.name.to_string();
        self.shadow_prelude_binding(&name);
        self.handlers.insert(name, handler);
        Ok(true)
    }

    /// Pass 1e: lower prelude `rsjs::interval(ms, || ...);` statements.
    ///
    /// The closure body uses the event-handler statement grammar minus
    /// the event parameter: a timer tick has no event, so closures with
    /// parameters are rejected. The interval is client-only; nothing is
    /// replayed into the server render body (nothing ticks during SSR).
    ///
    /// Each call is lowered at its source position, so it sees declarations
    /// above it and cannot bind to a same-named declaration below it.
    fn collect_interval(&mut self, call: &ExprCall) -> syn::Result<()> {
        self.require_client_origin(call.span(), ClientOriginKind::Timer)?;
        expect_call_arg_count(call, 2, "rsjs::interval(ms, || ...)")?;
        let ms = timer_ms_literal(&call.args[0], "interval")?;
        let Expr::Closure(closure) = &call.args[1] else {
            return Err(syn::Error::new(
                call.args[1].span(),
                "`rsjs::interval(ms, ...)` expects a closure as its second argument, \
                 e.g. `rsjs::interval(1000, || tick.update(|t| t + 1));`",
            ));
        };
        if let Some(input) = closure.inputs.first() {
            return Err(syn::Error::new(
                input.span(),
                "`rsjs::interval(...)` closures take no parameters: a timer tick has \
                 no event to receive. Read signals directly instead, e.g. \
                 `rsjs::interval(1000, || tick.update(|t| t + 1));`",
            ));
        }
        let scope = HandlerScope::default();
        let statements = self.lower_handler_closure_hir_body(closure, &scope)?;
        let lowered = lower_handler_hir_to_client_lowering(statements)?;
        self.hir_sidecar.push_interval(ms, lowered.body_hir);
        self.builder.interval(ms, lowered.body_ir);
        Ok(())
    }

    fn collect_query_local(
        &mut self,
        index: usize,
        local: &Local,
        prior_lets: &[Local],
    ) -> syn::Result<bool> {
        let Some((name, call, kind)) = query_local_call(local) else {
            return Ok(false);
        };
        self.require_client_origin(call.span(), ClientOriginKind::Query)?;
        if let Some(removed_name) = kind.removed_name() {
            return Err(syn::Error::new(
                call.func.span(),
                format!(
                    "`rsjs::{removed_name}(...)` has been removed; load the initial value \
                         through the render `cx` or ordinary server Rust, then call \
                         `rsjs::query_with_initial(key, initial, client_fetcher, options)`"
                ),
            ));
        }
        let expected = kind.arg_count();
        expect_call_arg_count(call, expected, kind.signature_label())?;
        if is_server_fetcher_call(&call.args[1]) {
            return Err(syn::Error::new(
                call.args[1].span(),
                "`rsjs::server_fetcher(...)` is no longer supported; load through the \
                     render `cx` before declaring the query and pass that value to \
                     `rsjs::query_with_initial(...)`",
            ));
        }
        let key_expr = match self.lower_query_key_expr(&call.args[0]) {
            Ok(key_expr) => {
                validate_live_query_key_expr(&key_expr).map_err(|reason| {
                    syn::Error::new(
                        call.args[0].span(),
                        format!(
                            "rsjs/query-key-not-synchronous: query keys may depend only on \
                             literals, signals, memo values, SharedState fields, and pure \
                             client helpers; {reason}\nhelp: move the value into the structural \
                             key's signal/memo dependency closure"
                        ),
                    )
                })?;
                key_expr
            }
            Err(lowering_error) => {
                if let Some((span, name)) =
                    self.first_client_prelude_dependency_mention(&call.args[0])
                {
                    return Err(syn::Error::new(
                        span,
                        format!(
                            "rsjs/query-key-client-expression-unsupported: query key depends on \
                             client value `{name}`, but that expression cannot be lowered into \
                             the synchronous live-key graph\nhelp: use the signal/memo/shared \
                             value directly or a supported pure `#[rsjs::client]` helper\n\
                             caused by: {lowering_error}"
                        ),
                    ));
                }
                // An ordinary server local has no client evaluator. Its exact
                // SSR key is carried by adjacent dehydration metadata instead.
                FrontendExpr::Literal(Value::Null)
            }
        };
        let server_initial = FrontendExpr::Literal(Value::Null);
        let client_fetcher = match request_config_expr(
            &call.args[2],
            "GET",
            "`query_with_initial(...)` client_fetcher must be a literal URL string, \
                 signal-backed URL expression, or `rsjs::FetchRequest { ... }`",
        ) {
            Ok(expr) => expr,
            Err(static_err) => self
                .lower_reactive_expr(&call.args[2])
                .map_err(|_| static_err)?,
        };
        validate_live_query_key_expr(&client_fetcher).map_err(|reason| {
            syn::Error::new(
                call.args[2].span(),
                format!(
                    "rsjs/query-request-not-synchronous: query request descriptors may depend \
                     only on literals, signals, memo values, SharedState fields, and pure \
                     client helpers; {reason}\nhelp: snapshot browser, DOM, query, or mutation \
                     state into an explicit signal and include that signal in both the \
                     structural key and request descriptor"
                ),
            )
        })?;
        let key_signal_deps = frontend_expr_signal_deps(&key_expr, call.args[0].span())?;
        let fetcher_signal_deps = frontend_expr_signal_deps(&client_fetcher, call.args[2].span())?;
        if let Some(dependency) = fetcher_signal_deps
            .iter()
            .find(|dependency| !key_signal_deps.contains(dependency))
        {
            let source_name = self
                .signals
                .iter()
                .find_map(|(name, id)| (id == dependency).then_some(name.as_str()))
                .unwrap_or("<unknown signal>");
            return Err(syn::Error::new(
                call.args[2].span(),
                format!(
                    "rsjs/query-fetcher-dependency-not-in-key: request descriptor reads \
                     signal `{source_name}`, but the structural query key does not\n\
                     help: include every reactive request input in `QueryKey::new(...)` so an \
                     old cache identity keeps its original request snapshot"
                ),
            ));
        }
        let options = parse_query_options(&call.args[3])?;
        if options.stream_text && !request_config_response_is_text(&client_fetcher) {
            return Err(syn::Error::new(
                call.args[3].span(),
                "`rsjs::QueryOptions.stream_text` requires a text response decoder; set \
                     `rsjs::FetchRequest { response: Some(rsjs::ResponseKind::Text), ... }`",
            ));
        }

        let key_expr = self.alloc_expr(key_expr, call.span())?;
        let server_initial = self.alloc_expr(server_initial, call.span())?;
        let client_fetcher = self.alloc_expr(client_fetcher, call.span())?;
        let data_ty = query_initial_data_type(&call.args[1], prior_lets)
            .filter(|ty| !ty.is_unknown())
            .or_else(|| query_local_data_type(local))
            .unwrap_or(ClientType::Unknown);
        let id =
            self.builder
                .query_typed(key_expr, server_initial, client_fetcher, data_ty, options);
        self.builder
            .set_query_source(id, Some(name.to_string()), Some(source_anchor(call.span())));
        let name = name.to_string();
        self.shadow_prelude_binding(&name);
        self.queries.insert(name, id);
        self.prelude_query_ids.insert(index, id);
        Ok(true)
    }

    fn collect_mutation_local(&mut self, index: usize, local: &Local) -> syn::Result<bool> {
        let Some((name, call, with_options)) = mutation_local_call(local) else {
            return Ok(false);
        };
        self.require_client_origin(call.span(), ClientOriginKind::Mutation)?;
        if !self.is_async {
            return Err(syn::Error::new(
                call.func.span(),
                "`mutation(...)` requires an async `WebRender` island; switch this impl from \
                     `WebRenderSync` to `WebRender` and use `html!` in the render body",
            ));
        }
        let expected = if with_options { 4 } else { 3 };
        expect_call_arg_count(
            call,
            expected,
            if with_options {
                "rsjs::mutation_with_options(key, client_runner, invalidates, options)"
            } else {
                "rsjs::mutation(key, client_runner, invalidates)"
            },
        )?;
        let mut runtime_key_expr = None;
        let key_expr = match self.lower_mutation_key_expr(&call.args[0]) {
            Ok(key_expr) => key_expr,
            Err(_) => {
                runtime_key_expr = Some(runtime_query_key_expr(&call.args[0]));
                FrontendExpr::Literal(Value::Null)
            }
        };
        let server_runner = FrontendExpr::Literal(Value::Null);
        let client_runner = match request_config_expr(
            &call.args[1],
            "POST",
            "`mutation(...)` client_runner must be a literal URL string, signal-backed URL \
                 expression, JSON runner config, or `rsjs::FetchRequest { ... }`",
        ) {
            Ok(expr) => expr,
            Err(static_err) => self
                .lower_reactive_expr(&call.args[1])
                .map_err(|_| static_err)?,
        };
        let mut runtime_invalidates_expr = None;
        let invalidates = match self.lower_invalidation_exprs(&call.args[2]) {
            Ok(invalidates) => invalidates,
            Err(_) => {
                runtime_invalidates_expr = Some(runtime_invalidation_expr(&call.args[2]));
                Vec::new()
            }
        };
        let options = if with_options {
            parse_mutation_options(&call.args[3])?
        } else {
            MutationOptions::default()
        };

        let key_expr = self.alloc_expr(key_expr, call.span())?;
        let server_runner = self.alloc_expr(server_runner, call.span())?;
        let client_runner = self.alloc_expr(client_runner, call.span())?;
        let invalidates = invalidates
            .into_iter()
            .map(|expr| self.alloc_expr(expr, call.span()))
            .collect::<syn::Result<Vec<_>>>()?;
        let id =
            self.builder
                .mutation(key_expr, server_runner, client_runner, invalidates, options);
        self.builder.set_mutation_source(
            id,
            Some(name.to_string()),
            Some(source_anchor(call.span())),
        );
        if runtime_key_expr.is_some() || runtime_invalidates_expr.is_some() {
            self.mutation_initializers.push(MutationInitializer {
                id,
                key_expr: runtime_key_expr.unwrap_or_else(|| runtime_query_key_expr(&call.args[0])),
                invalidates_expr: runtime_invalidates_expr,
            });
        }
        let name = name.to_string();
        self.shadow_prelude_binding(&name);
        self.mutations.insert(name, id);
        self.prelude_mutation_ids.insert(index, id);
        Ok(true)
    }

    /// Pass 1f: scan ordinary prelude `let` bindings for aliases of
    /// client-reachable expressions.
    ///
    /// This keeps source like `let display = count.get(); <span>{display}</span>`
    /// on the same lowering path as `<span>{count.get()}</span>`. Locals whose
    /// initializer is pure server Rust stay out of this map and continue to be
    /// replayed into the generated server render body.
    ///
    /// Production analysis calls [`Self::collect_client_prelude_local`] during
    /// the single source-ordered traversal. This plural wrapper remains useful
    /// to focused analyzer tests that seed an already-visible client binding.
    #[cfg(test)]
    pub(super) fn collect_client_prelude_locals(&mut self, lets: &[Local]) -> syn::Result<()> {
        for (index, local) in lets.iter().enumerate() {
            let compiler_owned = is_signal_local(local)
                || is_memo_local(local)
                || shared_store_local_decl(local)?.is_some()
                || shared_field_local_decl(local, &self.shared_stores)?.is_some()
                || is_handler_local(local)
                || is_node_ref_local(local)
                || is_query_local(local)
                || is_mutation_local(local);
            if compiler_owned {
                continue;
            }
            self.collect_client_prelude_local(index, local)?;
        }
        Ok(())
    }

    fn collect_client_prelude_local(&mut self, index: usize, local: &Local) -> syn::Result<()> {
        let lowered = local.init.as_ref().and_then(|init| {
            let opaque_call_candidate = match init.expr.as_ref() {
                Expr::Call(_) => true,
                Expr::MethodCall(call) => {
                    !prelude_method_can_participate_in_client_graph(&call.method.to_string())
                }
                _ => false,
            };
            if opaque_call_candidate && self.can_infer_opaque_server_expr(&init.expr) {
                // Ordinary server helper and method calls remain in the
                // lossless prelude, even when their receiver or one argument
                // is a direct `self.field`. An exact field alias
                // (`let value = self.value`) and closed Client-IR methods
                // still participate in live lowering.
                None
            } else {
                Some(self.lower_reactive_expr(&init.expr))
            }
        });
        let dependency = match &lowered {
            Some(Ok(_)) | None => None,
            Some(Err(_)) => local
                .init
                .as_ref()
                .and_then(|init| self.first_client_prelude_dependency_mention(&init.expr)),
        };
        let names = pattern_binding_names(&local.pat);
        for name in &names {
            self.shadow_prelude_binding(name);
        }

        let Some(name) = local_ident(local).map(|name| name.to_string()) else {
            return Ok(());
        };
        let Some(lowered) = lowered else {
            return Ok(());
        };
        let lowered = match lowered {
            Ok(lowered) => lowered,
            Err(_) => {
                if let Some((span, dependency_name)) = dependency {
                    self.unsupported_client_prelude_locals.insert(
                        name.clone(),
                        UnsupportedClientPreludeLocal {
                            dependency_name: dependency_name.clone(),
                        },
                    );
                    self.client_prelude_local_indices.insert(index);
                    return Err(self.unsupported_client_prelude_alias_error(
                        span,
                        &name,
                        &dependency_name,
                    ));
                }
                return Ok(());
            }
        };
        if frontend_expr_has_client_dependency(&lowered) {
            self.client_prelude_locals.insert(name, lowered);
            self.client_prelude_local_indices.insert(index);
        }
        Ok(())
    }

    fn first_client_prelude_dependency_mention(
        &self,
        expr: &Expr,
    ) -> Option<(proc_macro2::Span, String)> {
        let mut visitor = ClientPreludeDependencyMention {
            signals: &self.signals,
            queries: &self.queries,
            mutations: &self.mutations,
            node_refs: &self.node_refs,
            handlers: &self.handlers,
            client_prelude_locals: &self.client_prelude_locals,
            unsupported_client_prelude_locals: &self.unsupported_client_prelude_locals,
            capture: None,
        };
        visitor.visit_expr(expr);
        visitor.capture
    }
}

struct ClientPreludeDependencyMention<'a> {
    signals: &'a HashMap<String, SignalId>,
    queries: &'a HashMap<String, QueryId>,
    mutations: &'a HashMap<String, MutationId>,
    node_refs: &'a HashMap<String, NodeRefId>,
    handlers: &'a HashMap<String, HandlerLocal>,
    client_prelude_locals: &'a HashMap<String, FrontendExpr>,
    unsupported_client_prelude_locals: &'a HashMap<String, UnsupportedClientPreludeLocal>,
    capture: Option<(proc_macro2::Span, String)>,
}

impl ClientPreludeDependencyMention<'_> {
    fn contains_client_local(&self, name: &str) -> bool {
        self.signals.contains_key(name)
            || self.queries.contains_key(name)
            || self.mutations.contains_key(name)
            || self.node_refs.contains_key(name)
            || self.handlers.contains_key(name)
            || self.client_prelude_locals.contains_key(name)
            || self.unsupported_client_prelude_locals.contains_key(name)
    }
}

impl<'ast> Visit<'ast> for ClientPreludeDependencyMention<'_> {
    fn visit_expr_path(&mut self, node: &'ast ExprPath) {
        if self.capture.is_some() {
            return;
        }
        if let Some(ident) = node.path.get_ident() {
            let name = ident.to_string();
            if self.contains_client_local(&name) {
                self.capture = Some((ident.span(), name));
                return;
            }
        }
        visit::visit_expr_path(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.capture.is_some() {
            return;
        }
        if let Expr::Path(ExprPath {
            path, qself: None, ..
        }) = node.func.as_ref()
        {
            if registry_read_spec(path).is_some()
                || registry_effect_spec(path).is_some()
                || browser_read_call_kind(path).is_some()
                || browser_effect_call_kind(path).is_some()
            {
                let name = path
                    .segments
                    .last()
                    .map(|segment| segment.ident.to_string())
                    .unwrap_or_else(|| path.to_token_stream().to_string());
                self.capture = Some((path.span(), name));
                return;
            }
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        if self.capture.is_some() {
            return;
        }
        let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
        if let Ok(args) = syn::parse::Parser::parse2(parser, node.mac.tokens.clone()) {
            for arg in args {
                self.visit_expr(&arg);
                if self.capture.is_some() {
                    return;
                }
            }
        }
    }
}

/// Methods whose receiver may introduce or propagate a client-graph edge even
/// when the receiver is a type-ambiguous `self.<field>` projection.
///
/// Every other method call that passes the opaque-server capture scan remains
/// ordinary Rust. Keeping this list closed prevents speculative Client IR
/// lowering from allocating a bogus inbound value before it discovers that a
/// method such as `.as_ref()`, `.len()`, or `.iter()` is server-only.
fn prelude_method_can_participate_in_client_graph(method: &str) -> bool {
    matches!(
        method,
        "at" | "is_present"
            | "eq"
            | "neq"
            | "coalesce"
            | "is_non_empty"
            | "is_empty"
            | "to_lowercase"
            | "to_uppercase"
            | "trim"
            | "contains"
            | "starts_with"
            | "ends_with"
            | "get_attribute"
            | "has_attribute"
    ) || as_type_method(method).is_some()
        || node_ref_accessor_method(method).is_some()
        || node_ref_effect_method(method).is_some()
}

pub(super) fn frontend_expr_has_client_dependency(expr: &FrontendExpr) -> bool {
    match expr {
        FrontendExpr::Literal(_) | FrontendExpr::ArrayLit(_) => false,
        FrontendExpr::SignalRead(_)
        | FrontendExpr::Local(_)
        | FrontendExpr::EventValue(_)
        | FrontendExpr::EventCurrentTarget
        | FrontendExpr::EventCollectionLen(_)
        | FrontendExpr::EventCollectionRead { .. }
        | FrontendExpr::NodeRefRead { .. }
        | FrontendExpr::EventTargetWithinNodeRef { .. }
        | FrontendExpr::EventAttribute { .. }
        | FrontendExpr::BrowserRead(_)
        | FrontendExpr::EventPayload { .. }
        | FrontendExpr::CallbackPayload
        | FrontendExpr::NodePayload { .. }
        | FrontendExpr::QueryRead { .. }
        | FrontendExpr::MutationRead { .. } => true,
        FrontendExpr::DomRead { target, .. } => frontend_expr_has_client_dependency(target),
        FrontendExpr::BinaryOp { lhs, rhs, .. } => {
            frontend_expr_has_client_dependency(lhs) || frontend_expr_has_client_dependency(rhs)
        }
        FrontendExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            frontend_expr_has_client_dependency(condition)
                || frontend_expr_has_client_dependency(then_expr)
                || frontend_expr_has_client_dependency(else_expr)
        }
        FrontendExpr::IsNonEmpty { target }
        | FrontendExpr::UnaryNeg { expr: target, .. }
        | FrontendExpr::StringToLowercase { target }
        | FrontendExpr::StringTransform { target, .. }
        | FrontendExpr::Encoding { value: target, .. }
        | FrontendExpr::Project { target, .. }
        | FrontendExpr::AsType { target, .. }
        | FrontendExpr::IsPresent { target }
        | FrontendExpr::Eq { target, .. }
        | FrontendExpr::Neq { target, .. }
        | FrontendExpr::Coalesce { target, .. } => frontend_expr_has_client_dependency(target),
        FrontendExpr::StringContains { haystack, needle }
        | FrontendExpr::StringPredicate {
            haystack, needle, ..
        } => {
            frontend_expr_has_client_dependency(haystack)
                || frontend_expr_has_client_dependency(needle)
        }
        FrontendExpr::QueryKey(parts) => parts.iter().any(frontend_expr_has_client_dependency),
        FrontendExpr::ClientCall { args, .. } | FrontendExpr::Text { args, .. } => {
            args.iter().any(frontend_expr_has_client_dependency)
        }
    }
}

fn frontend_expr_signal_deps(
    expr: &FrontendExpr,
    span: proc_macro2::Span,
) -> syn::Result<Vec<SignalId>> {
    let client = super::client_lowering::lower_handler_ir_expr_to_client_expr(expr, span)?;
    Ok(rsjs_ir::analysis::collect_client_expr_reactive_deps(&client).signals)
}

fn validate_live_query_key_expr(expr: &FrontendExpr) -> Result<(), &'static str> {
    match expr {
        FrontendExpr::Literal(_) | FrontendExpr::ArrayLit(_) | FrontendExpr::SignalRead(_) => {
            Ok(())
        }
        FrontendExpr::BinaryOp { lhs, rhs, .. } => {
            validate_live_query_key_expr(lhs)?;
            validate_live_query_key_expr(rhs)
        }
        FrontendExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            validate_live_query_key_expr(condition)?;
            validate_live_query_key_expr(then_expr)?;
            validate_live_query_key_expr(else_expr)
        }
        FrontendExpr::IsNonEmpty { target }
        | FrontendExpr::UnaryNeg { expr: target, .. }
        | FrontendExpr::StringToLowercase { target }
        | FrontendExpr::StringTransform { target, .. }
        | FrontendExpr::Encoding { value: target, .. }
        | FrontendExpr::Project { target, .. }
        | FrontendExpr::AsType { target, .. }
        | FrontendExpr::IsPresent { target }
        | FrontendExpr::Eq { target, .. }
        | FrontendExpr::Neq { target, .. }
        | FrontendExpr::Coalesce { target, .. } => validate_live_query_key_expr(target),
        FrontendExpr::StringContains { haystack, needle }
        | FrontendExpr::StringPredicate {
            haystack, needle, ..
        } => {
            validate_live_query_key_expr(haystack)?;
            validate_live_query_key_expr(needle)
        }
        FrontendExpr::QueryKey(parts) => {
            for part in parts {
                validate_live_query_key_expr(part)?;
            }
            Ok(())
        }
        FrontendExpr::ClientCall { args, .. } | FrontendExpr::Text { args, .. } => {
            for arg in args {
                validate_live_query_key_expr(arg)?;
            }
            Ok(())
        }
        FrontendExpr::QueryRead { .. } => Err(
            "reading another query would make cache identity depend on asynchronous query state",
        ),
        FrontendExpr::MutationRead { .. } => Err(
            "reading mutation state would make cache identity depend on asynchronous mutation state",
        ),
        FrontendExpr::BrowserRead(_) => {
            Err("browser reads are not deterministic synchronous query-key inputs")
        }
        FrontendExpr::Local(_)
        | FrontendExpr::EventValue(_)
        | FrontendExpr::EventCurrentTarget
        | FrontendExpr::EventCollectionLen(_)
        | FrontendExpr::EventCollectionRead { .. }
        | FrontendExpr::NodeRefRead { .. }
        | FrontendExpr::EventTargetWithinNodeRef { .. }
        | FrontendExpr::EventAttribute { .. }
        | FrontendExpr::EventPayload { .. }
        | FrontendExpr::CallbackPayload
        | FrontendExpr::NodePayload { .. } => {
            Err("event, DOM, callback, and handler-local values are not valid query-key inputs")
        }
        FrontendExpr::DomRead { .. } => {
            Err("event, DOM, callback, and handler-local values are not valid query-key inputs")
        }
    }
}

struct OpaqueServerStatementValidator<'a> {
    client_names: &'a HashSet<String>,
    shadowed_names: HashSet<String>,
    error: Option<syn::Error>,
}

impl OpaqueServerStatementValidator<'_> {
    fn is_visible_client_name(&self, name: &str) -> bool {
        self.client_names.contains(name) && !self.shadowed_names.contains(name)
    }

    fn with_pattern_scope(&mut self, pat: &Pat, visit: impl FnOnce(&mut Self)) {
        let saved = self.shadowed_names.clone();
        self.shadowed_names.extend(pattern_binding_names(pat));
        visit(self);
        self.shadowed_names = saved;
    }

    fn reject_client_name(&mut self, span: proc_macro2::Span, name: &str) {
        if self.error.is_none() {
            self.error = Some(syn::Error::new(
                span,
                format!(
                    "rsjs/template-client-state-in-opaque-server-statement: ordinary \
                     server code cannot read compiler-owned client local `{name}`\n\
                     help: move the read into the analyzed template, an event handler, \
                     or another RSJS expression the compiler can lower"
                ),
            ));
        }
    }

    fn reject_browser_operation(&mut self, span: proc_macro2::Span, name: &str) {
        if self.error.is_none() {
            self.error = Some(syn::Error::new(
                span,
                format!(
                    "rsjs/template-browser-operation-in-opaque-server-statement: \
                     `{name}` is browser behavior and cannot execute as an ordinary \
                     server statement\nhelp: move it into a reachable handler, effect, \
                     binding, or supported client expression"
                ),
            ));
        }
    }

    fn scan_macro_tokens(&mut self, stream: proc_macro2::TokenStream) {
        for token in stream {
            match token {
                proc_macro2::TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    if self.is_visible_client_name(&name) {
                        self.reject_client_name(ident.span(), &name);
                        return;
                    }
                }
                proc_macro2::TokenTree::Group(group) => {
                    self.scan_macro_tokens(group.stream());
                    if self.error.is_some() {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for OpaqueServerStatementValidator<'_> {
    fn visit_field_value(&mut self, field: &'ast syn::FieldValue) {
        // A struct field/member name lives in a different namespace from
        // local variables. Only the value expression can read a client local;
        // visiting `url:` as though it were the outer signal `url` is a false
        // capture.
        self.visit_expr(&field.expr);
    }

    fn visit_expr_path(&mut self, path: &'ast ExprPath) {
        if let Some(ident) = path.path.get_ident() {
            let name = ident.to_string();
            if self.is_visible_client_name(&name) {
                self.reject_client_name(ident.span(), &name);
                return;
            }
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(path) = call.func.as_ref() {
            if let Some(last) = path.path.segments.last() {
                let name = last.ident.to_string();
                if matches!(
                    name.as_str(),
                    "signal"
                        | "memo"
                        | "handler"
                        | "event_handler"
                        | "interval"
                        | "node_ref"
                        | "shared"
                        | "query"
                        | "query_with_options"
                        | "query_with_initial"
                        | "mutation"
                        | "mutation_with_options"
                ) && is_rsjs_root_path(&path.path, &name)
                {
                    self.reject_browser_operation(last.ident.span(), &name);
                    return;
                }
            }
            let browser_helper = rsjs_browser_helper_call_label(&path.path).or_else(|| {
                (registry_read_spec(&path.path).is_some()
                    || registry_effect_spec(&path.path).is_some()
                    || browser_read_call_kind(&path.path).is_some()
                    || browser_effect_call_kind(&path.path).is_some())
                .then(|| path.path.to_token_stream().to_string().replace(' ', ""))
            });
            if let Some(helper) = browser_helper {
                self.reject_browser_operation(path.span(), &helper);
                return;
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_block(&mut self, block: &'ast Block) {
        let saved = self.shadowed_names.clone();
        for statement in &block.stmts {
            if self.error.is_some() {
                break;
            }
            if let Stmt::Local(local) = statement {
                // Rust evaluates the initializer and `let ... else` diverging
                // branch before the new pattern bindings enter scope.
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

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
        if let Ok(args) = syn::parse::Parser::parse2(parser, mac.tokens.clone()) {
            for arg in &args {
                self.visit_expr(arg);
                if self.error.is_some() {
                    return;
                }
            }
            return;
        }
        // Unknown macro grammars stay conservative because the proc macro
        // cannot inspect their expansion.
        self.scan_macro_tokens(mac.tokens.clone());
        if self.error.is_none() {
            visit::visit_macro(self, mac);
        }
    }
}

pub(super) fn pattern_binding_names(pat: &Pat) -> Vec<String> {
    struct Names(Vec<String>);

    impl<'ast> Visit<'ast> for Names {
        fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
            self.0.push(pat.ident.to_string());
            visit::visit_pat_ident(self, pat);
        }
    }

    let mut names = Names(Vec::new());
    names.visit_pat(pat);
    names.0
}

pub(super) fn collect_condition_pattern_bindings(expr: &Expr, names: &mut Vec<String>) {
    match expr {
        Expr::Let(let_expr) => names.extend(pattern_binding_names(&let_expr.pat)),
        Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
            collect_condition_pattern_bindings(&binary.left, names);
            collect_condition_pattern_bindings(&binary.right, names);
        }
        Expr::Paren(paren) => collect_condition_pattern_bindings(&paren.expr, names),
        Expr::Group(group) => collect_condition_pattern_bindings(&group.expr, names),
        _ => {}
    }
}

/// Return true if the function-position expression is a path whose
/// last segment matches `name`.
/// Returns `true` if `local` matches the shape recognised by
/// [`Analyzer::collect_signals`] as a signal declaration
/// (`let <name> = signal(<lit>);`).
///
/// Used by the proc-macro entry point to partition prelude `let`
/// bindings into "the analyser already lowered this into the IR"
/// (signal declarations) and "needs to be replayed verbatim into the
/// generated render body" (everything else, e.g.
/// `let user = cx.load(...).await?;`). See round-3 review #1 for the
/// motivation.
pub(crate) fn is_signal_local(local: &Local) -> bool {
    let init = match &local.init {
        Some(li) => li,
        None => return false,
    };
    match init.expr.as_ref() {
        Expr::Call(c) => is_rsjs_root_call(&c.func, "signal"),
        _ => false,
    }
}

/// Returns `true` if `local` matches the shape recognised by
/// [`Analyzer::collect_signals`] as a memo (derived) cell
/// declaration (`let <name> = memo(|| ...);`).
///
/// Used alongside [`is_signal_local`] to keep analyser-lowered prelude
/// `let` bindings out of the verbatim server-render replay: the
/// generated render body declares its own `__rsjs_initial_signal_<N>`
/// local for the derived value instead.
pub(crate) fn is_memo_local(local: &Local) -> bool {
    memo_local_call(local).is_some()
}

/// Returns `true` if `local` matches the rsjs callback-ref shape
/// `let <name> = rsjs::handler(|payload| ...);`.
///
/// The analyser lowers these locals into inline handler IR. They must
/// not be replayed into the generated server render body because they
/// usually reference `signal(...)` locals that have already been
/// replaced by generated `__rsjs_initial_signal_*` locals.
pub(crate) fn is_handler_local(local: &Local) -> bool {
    let init = match &local.init {
        Some(li) => li,
        None => return false,
    };
    match init.expr.as_ref() {
        Expr::Call(c) => is_rsjs_handler_call(&c.func) || is_rsjs_event_handler_call(&c.func),
        _ => false,
    }
}

/// Returns `true` if `local` matches `let <name> = node_ref::<T>();`.
pub(crate) fn is_node_ref_local(local: &Local) -> bool {
    let init = match &local.init {
        Some(li) => li,
        None => return false,
    };
    match init.expr.as_ref() {
        Expr::Call(c) => is_rsjs_root_call(&c.func, "node_ref"),
        _ => false,
    }
}

/// Returns `true` if `local` matches a query declaration. Legacy
/// `query(...)` / `query_with_options(...)` calls are still detected so the
/// analyzer can produce a guided removal diagnostic.
pub(crate) fn is_query_local(local: &Local) -> bool {
    query_local_call(local).is_some()
}

pub(crate) fn is_query_runtime_initial_local(local: &Local) -> bool {
    let Some((_, _call, kind)) = query_local_call(local) else {
        return false;
    };
    kind.uses_server_initial_value()
}

/// Returns `true` if `local` matches `let <name> = mutation(...);` or
/// `let <name> = rsjs::mutation_with_options(...);`.
pub(crate) fn is_mutation_local(local: &Local) -> bool {
    mutation_local_call(local).is_some()
}

#[cfg(test)]
pub(crate) fn signal_initializers(
    lets: &[Local],
) -> syn::Result<Vec<(SignalId, Expr, Option<syn::Type>)>> {
    let mut out = Vec::new();
    // Memo declarations consume signal ids too (they share the id
    // space, allocated in source order), so the running id must count
    // both shapes to keep runtime-initializer ids aligned with the ids
    // the analyser allocates.
    let mut next_id = 0_u32;
    let mut shared_stores = HashSet::new();
    for local in lets {
        if let Some(shared) = shared_store_local_decl(local)? {
            shared_stores.insert(shared.name.to_string());
        } else if let Some(local) = signal_local_decl(local)? {
            if let Some(expr) = local.runtime_expr {
                out.push((SignalId(next_id), expr, local.explicit_ty));
            }
            next_id += 1;
        } else if is_memo_local(local)
            || shared_field_receiver_name(local)
                .is_some_and(|receiver| shared_stores.contains(&receiver))
        {
            next_id += 1;
        }
    }
    Ok(out)
}

#[derive(Debug)]
pub(super) struct SharedStoreLocal {
    name: syn::Ident,
    ty: syn::Type,
}

struct SharedFieldLocal {
    name: syn::Ident,
    store: SharedStoreId,
    field: syn::Ident,
}

/// Returns `true` only for the path shapes rsjs claims as shared-state
/// acquisition: bare `shared` (after `use rsjs::shared;`) or
/// `rsjs::shared`, with or without a leading `::`, mirroring
/// [`is_rsjs_handler_constructor_call`]. Foreign functions that happen
/// to be named `shared`, such as `features::shared()` or
/// `ecs::shared::<Position>()`, belong to user code: they must fall
/// through untouched and be replayed as ordinary server locals.
fn is_rsjs_shared_acquire_path(path: &syn::Path) -> bool {
    let Some(last) = path.segments.last() else {
        return false;
    };
    if last.ident != "shared" {
        return false;
    }
    match path.segments.len() {
        1 => true,
        2 => path
            .segments
            .first()
            .map(|seg| seg.ident == "rsjs")
            .unwrap_or(false),
        _ => false,
    }
}

pub(super) fn shared_store_local_decl(local: &Local) -> syn::Result<Option<SharedStoreLocal>> {
    let Some(name) = local_ident(local) else {
        return Ok(None);
    };
    let Some(init) = &local.init else {
        return Ok(None);
    };
    let Expr::Call(call) = init.expr.as_ref() else {
        return Ok(None);
    };
    let Expr::Path(path) = call.func.as_ref() else {
        return Ok(None);
    };
    if !is_rsjs_shared_acquire_path(&path.path) {
        return Ok(None);
    }
    let segment = path
        .path
        .segments
        .last()
        .expect("is_rsjs_shared_acquire_path requires a last segment");
    if !call.args.is_empty() {
        return Err(syn::Error::new(
            call.args.span(),
            "rsjs/shared-state-acquire-arguments: `shared::<T>()` takes no value arguments",
        ));
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new(
            segment.span(),
            "rsjs/shared-state-missing-type: write `rsjs::shared::<YourState>()`; the explicit type makes the page-store dependency auditable",
        ));
    };
    let mut types = arguments.args.iter().filter_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    });
    let Some(ty) = types.next() else {
        return Err(syn::Error::new(
            arguments.span(),
            "rsjs/shared-state-missing-type: `shared` requires exactly one state type, e.g. `rsjs::shared::<WorkbenchState>()`",
        ));
    };
    if types.next().is_some() || arguments.args.len() != 1 {
        return Err(syn::Error::new(
            arguments.span(),
            "rsjs/shared-state-type-arity: `shared` requires exactly one state type",
        ));
    }
    Ok(Some(SharedStoreLocal { name, ty }))
}

fn shared_field_local_decl(
    local: &Local,
    stores: &HashMap<String, SharedStoreId>,
) -> syn::Result<Option<SharedFieldLocal>> {
    let Some(name) = local_ident(local) else {
        return Ok(None);
    };
    let Some(init) = &local.init else {
        return Ok(None);
    };
    let Expr::MethodCall(call) = init.expr.as_ref() else {
        return Ok(None);
    };
    let Expr::Path(receiver) = call.receiver.as_ref() else {
        return Ok(None);
    };
    let Some(receiver_name) = receiver.path.get_ident() else {
        return Ok(None);
    };
    let Some(store) = stores.get(&receiver_name.to_string()).copied() else {
        return Ok(None);
    };
    if !call.args.is_empty() || call.turbofish.is_some() {
        return Err(syn::Error::new(
            call.span(),
            "rsjs/shared-state-field-arguments: shared-state field accessors take no arguments; use `let field = state.field();`",
        ));
    }
    Ok(Some(SharedFieldLocal {
        name,
        store,
        field: call.method.clone(),
    }))
}

/// Reject the one-liner `let mode = rsjs::shared::<T>().mode();` with a
/// targeted diagnostic spanned at the chained call.
///
/// Field bindings resolve against a named store local: the store handle
/// is what later field bindings, handler receivers, and the generated
/// SSR provider acquisition all key on, so an anonymous chained
/// acquisition has no identity the analyzer can track. Without this
/// guard the let falls through as an ordinary server local and the
/// first template read of the field produces a misleading "declare it
/// with `signal(...)`" error at the slot instead of the acquisition.
fn chained_shared_acquire_error(local: &Local) -> Option<syn::Error> {
    let init = local.init.as_ref()?;
    let Expr::MethodCall(outer) = init.expr.as_ref() else {
        return None;
    };
    // Walk to the innermost method call so `rsjs::shared::<T>().a().b()`
    // reports `.a()` as the field being chained onto the acquisition.
    let mut innermost = outer;
    while let Expr::MethodCall(inner) = innermost.receiver.as_ref() {
        innermost = inner;
    }
    let Expr::Call(acquire) = innermost.receiver.as_ref() else {
        return None;
    };
    let Expr::Path(path) = acquire.func.as_ref() else {
        return None;
    };
    if !is_rsjs_shared_acquire_path(&path.path) {
        return None;
    }
    let state_ty = path
        .path
        .segments
        .last()
        .and_then(|segment| match &segment.arguments {
            syn::PathArguments::AngleBracketed(arguments) => Some(
                arguments
                    .args
                    .to_token_stream()
                    .to_string()
                    .replace(' ', ""),
            ),
            _ => None,
        })
        .unwrap_or_else(|| "YourState".to_string());
    let field = innermost.method.to_string();
    let local_name = local_ident(local)
        .map(|ident| ident.to_string())
        .unwrap_or_else(|| field.clone());
    Some(syn::Error::new(
        init.expr.span(),
        format!(
            "rsjs/shared-state-chained-acquire: `rsjs::shared::<{state_ty}>()` cannot be \
             chained straight into `.{field}()` in one `let`. Field bindings resolve \
             against a named store local, and the SSR provider acquisition is emitted \
             once per named handle, so an anonymous chained acquisition has no identity \
             to bind to. Split it into two lets: \
             `let state = rsjs::shared::<{state_ty}>();` then \
             `let {local_name} = state.{field}();`"
        ),
    ))
}

/// Names still bound by a `let` in the generated server render body, i.e. the
/// prelude `let`s the replay filter keeps.
///
/// Deliberately conservative: a shape the analyser might still claim later
/// (a shared-field alias, a client-prelude alias) is counted as replayed. The
/// only consumer renames a generated store handle out of the way, so an extra
/// name here costs nothing but a hygienic local name.
fn server_replayed_prelude_names(lets: &[Local]) -> HashSet<String> {
    let mut names = HashSet::new();
    for local in lets {
        if is_signal_local(local)
            || is_memo_local(local)
            || is_handler_local(local)
            || is_node_ref_local(local)
            || is_query_local(local)
            || is_mutation_local(local)
            || matches!(shared_store_local_decl(local), Ok(Some(_)))
        {
            continue;
        }
        if let Some(name) = local_ident(local) {
            names.insert(name.to_string());
        }
    }
    names
}

/// Returns `true` for `rsjs::shared::<T>()` / `shared::<T>()` paths, the
/// turbofish form rsjs claims as a shared-state acquisition. The explicit type
/// argument is what separates this from a user function that merely happens to
/// be called `shared`.
fn is_rsjs_shared_acquire_turbofish(path: &syn::Path) -> bool {
    is_rsjs_shared_acquire_path(path)
        && path.segments.last().is_some_and(|segment| {
            matches!(segment.arguments, syn::PathArguments::AngleBracketed(_))
        })
}

fn shared_acquire_state_type(path: &syn::Path) -> Option<String> {
    let segment = path.segments.last()?;
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    Some(
        arguments
            .args
            .to_token_stream()
            .to_string()
            .replace(' ', ""),
    )
}

/// Find the first `rsjs::shared::<T>()` call nested anywhere inside an
/// expression.
struct NestedSharedAcquire<'ast> {
    found: Option<&'ast ExprCall>,
}

impl<'ast> Visit<'ast> for NestedSharedAcquire<'ast> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.found.is_some() {
            return;
        }
        if let Expr::Path(ExprPath {
            path, qself: None, ..
        }) = node.func.as_ref()
        {
            if is_rsjs_shared_acquire_turbofish(path) {
                self.found = Some(node);
                return;
            }
        }
        visit::visit_expr_call(self, node);
    }
}

/// Reject an `rsjs::shared::<T>()` acquisition written in a shape the analyzer
/// cannot claim: wrapped in a block, produced by an `if`/`match`, or passed as
/// an argument to something else.
///
/// Callers run this only after the exact `let <name> = rsjs::shared::<T>();`
/// form and the chained `rsjs::shared::<T>().field()` form have been handled,
/// so anything reaching here would otherwise be replayed verbatim into the
/// generated server render, where `rsjs::shared` is a stub that panics on
/// every request. Failing at compile time is the whole point of this check.
fn unrecognized_shared_acquire_error(local: &Local) -> Option<syn::Error> {
    let init = local.init.as_ref()?;
    let mut finder = NestedSharedAcquire { found: None };
    finder.visit_expr(&init.expr);
    let call = finder.found?;
    let Expr::Path(ExprPath { path, .. }) = call.func.as_ref() else {
        return None;
    };
    let state_ty = shared_acquire_state_type(path).unwrap_or_else(|| "YourState".to_string());
    let local_name = local_ident(local)
        .map(|ident| ident.to_string())
        .unwrap_or_else(|| "state".to_string());
    Some(syn::Error::new(
        call.span(),
        format!(
            "rsjs/shared-state-unrecognized-acquire: `rsjs::shared::<{state_ty}>()` must be the \
             entire initializer of its own `let`. Here it is nested inside another expression (a \
             block, an `if`/`match` arm, or a call argument), so the analyzer has no named store \
             handle to bind the acquisition to, and the call would be replayed into the generated \
             server render where `rsjs::shared` is a stub that panics on every request. Write \
             `let {local_name} = rsjs::shared::<{state_ty}>();` as a top-level prelude statement, \
             then read fields from that local, e.g. `let field = {local_name}.field();`"
        ),
    ))
}

fn shared_field_receiver_name(local: &Local) -> Option<String> {
    let init = local.init.as_ref()?;
    let Expr::MethodCall(call) = init.expr.as_ref() else {
        return None;
    };
    if !call.args.is_empty() || call.turbofish.is_some() {
        return None;
    }
    let Expr::Path(receiver) = call.receiver.as_ref() else {
        return None;
    };
    receiver.path.get_ident().map(ToString::to_string)
}

pub(crate) fn query_initializers(
    lets: &[Local],
) -> syn::Result<Vec<(QueryId, Expr, Option<Expr>)>> {
    let mut out = Vec::new();
    let mut query_index = 0_u32;
    for local in lets {
        let Some((_name, call, kind)) = query_local_call(local) else {
            continue;
        };
        if kind.uses_server_initial_value() && call.args.len() > 1 {
            let key_expr = runtime_query_key_expr(&call.args[0]);
            out.push((QueryId(query_index), call.args[1].clone(), Some(key_expr)));
        }
        query_index += 1;
    }
    Ok(out)
}

pub(super) fn runtime_query_key_expr(expr: &Expr) -> Expr {
    if let Expr::Call(call) = expr {
        if is_query_key_new_call(&call.func) && call.args.len() == 1 {
            return call.args[0].clone();
        }
    }
    syn::parse_quote! { (#expr,) }
}

pub(super) fn runtime_invalidation_expr(expr: &Expr) -> Expr {
    match expr {
        Expr::Array(array) => {
            let keys = array.elems.iter().map(runtime_query_key_expr);
            syn::parse_quote! { [#(#keys),*] }
        }
        Expr::Tuple(tuple) if tuple.elems.is_empty() => syn::parse_quote! { [] },
        Expr::Tuple(tuple) => {
            let keys = tuple.elems.iter().map(runtime_query_key_expr);
            syn::parse_quote! { (#(#keys),*,) }
        }
        Expr::Reference(reference) => runtime_invalidation_expr(&reference.expr),
        Expr::Paren(paren) => runtime_invalidation_expr(&paren.expr),
        Expr::Group(group) => runtime_invalidation_expr(&group.expr),
        other => {
            let key = runtime_query_key_expr(other);
            syn::parse_quote! { (#key,) }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueryLocalKind {
    RemovedQuery,
    RemovedQueryWithOptions,
    Initial,
}

impl QueryLocalKind {
    pub(super) const fn arg_count(self) -> usize {
        match self {
            Self::RemovedQuery => 3,
            Self::RemovedQueryWithOptions | Self::Initial => 4,
        }
    }

    pub(super) const fn uses_server_initial_value(self) -> bool {
        matches!(self, Self::Initial)
    }

    pub(super) const fn removed_name(self) -> Option<&'static str> {
        match self {
            Self::RemovedQuery => Some("query"),
            Self::RemovedQueryWithOptions => Some("query_with_options"),
            Self::Initial => None,
        }
    }

    pub(super) const fn signature_label(self) -> &'static str {
        match self {
            Self::RemovedQuery => "rsjs::query(key, initial_json, client_fetcher)",
            Self::RemovedQueryWithOptions => {
                "rsjs::query_with_options(key, initial_json, client_fetcher, options)"
            }
            Self::Initial => "rsjs::query_with_initial(key, initial, client_fetcher, options)",
        }
    }
}

pub(super) fn query_local_call(local: &Local) -> Option<(syn::Ident, &ExprCall, QueryLocalKind)> {
    let name = local_ident(local)?;
    let init = local.init.as_ref()?;
    let Expr::Call(call) = init.expr.as_ref() else {
        return None;
    };
    let name_str = path_leaf(&call.func)?;
    if !is_rsjs_root_call(&call.func, &name_str) {
        return None;
    }
    match name_str.as_str() {
        "query" => Some((name, call, QueryLocalKind::RemovedQuery)),
        "query_with_options" => Some((name, call, QueryLocalKind::RemovedQueryWithOptions)),
        "query_with_initial" => Some((name, call, QueryLocalKind::Initial)),
        _ => None,
    }
}

pub(super) fn query_local_data_type(local: &Local) -> Option<ClientType> {
    let Pat::Type(pt) = &local.pat else {
        return None;
    };
    query_handle_inner_type(&pt.ty).map(rust_query_data_type)
}

pub(super) fn query_initial_data_type(expr: &Expr, lets: &[Local]) -> Option<ClientType> {
    let ty = query_expr_data_type(expr, lets, 0);
    (!ty.is_unknown()).then_some(ty)
}

fn query_expr_data_type(expr: &Expr, lets: &[Local], depth: usize) -> ClientType {
    if depth > 8 {
        return ClientType::Unknown;
    }
    match expr {
        Expr::Reference(reference) => query_expr_data_type(&reference.expr, lets, depth + 1),
        Expr::Paren(paren) => query_expr_data_type(&paren.expr, lets, depth + 1),
        Expr::Group(group) => query_expr_data_type(&group.expr, lets, depth + 1),
        Expr::Lit(lit) => literal_to_value(expr)
            .map(|value| ClientType::of_value(&value))
            .unwrap_or_else(|_| {
                if matches!(lit.lit, Lit::Str(_)) {
                    ClientType::String
                } else {
                    ClientType::Unknown
                }
            }),
        Expr::Path(path) if path.path.get_ident().is_some() => {
            let name = path.path.get_ident().expect("checked").to_string();
            lets.iter()
                .rev()
                .find(|local| local_ident(local).is_some_and(|ident| ident == name))
                .and_then(|local| local.init.as_ref())
                .map(|init| query_expr_data_type(&init.expr, lets, depth + 1))
                .unwrap_or(ClientType::Unknown)
        }
        Expr::Struct(strukt) => {
            let mut fields = Vec::with_capacity(strukt.fields.len());
            for field in &strukt.fields {
                let syn::Member::Named(name) = &field.member else {
                    return ClientType::Unknown;
                };
                fields.push(rsjs_ir::ClientObjectField {
                    name: name.to_string(),
                    ty: query_expr_data_type(&field.expr, lets, depth + 1),
                });
            }
            ClientType::object(fields)
        }
        Expr::Array(array) => {
            let mut elem_ty = None;
            for elem in &array.elems {
                elem_ty = Some(unify_query_expr_types(
                    elem_ty.unwrap_or(ClientType::Unknown),
                    query_expr_data_type(elem, lets, depth + 1),
                ));
            }
            ClientType::vec(elem_ty.unwrap_or(ClientType::Unknown))
        }
        Expr::Repeat(repeat) => {
            ClientType::vec(query_expr_data_type(&repeat.expr, lets, depth + 1))
        }
        Expr::Call(call) => query_call_data_type(call, lets, depth + 1),
        Expr::MethodCall(method) if method.method == "to_string" && method.args.is_empty() => {
            ClientType::String
        }
        _ => ClientType::Unknown,
    }
}

fn query_call_data_type(call: &ExprCall, lets: &[Local], depth: usize) -> ClientType {
    let Expr::Path(path) = call.func.as_ref() else {
        return ClientType::Unknown;
    };
    let mut segments = path.path.segments.iter().rev();
    let Some(last) = segments.next() else {
        return ClientType::Unknown;
    };
    let leaf = last.ident.to_string();
    if leaf == "new" {
        return match segments.next().map(|seg| seg.ident.to_string()).as_deref() {
            Some("String") => ClientType::String,
            Some("Vec") => ClientType::vec(ClientType::Unknown),
            _ => ClientType::Unknown,
        };
    }
    if leaf == "from"
        && segments.next().is_some_and(|seg| seg.ident == "String")
        && call.args.len() == 1
    {
        return ClientType::String;
    }
    if leaf == "Some" && call.args.len() == 1 {
        return ClientType::option(query_expr_data_type(
            call.args.first().expect("len checked"),
            lets,
            depth + 1,
        ));
    }
    ClientType::Unknown
}

fn unify_query_expr_types(left: ClientType, right: ClientType) -> ClientType {
    if left.is_unknown() {
        return right;
    }
    if right.is_unknown() {
        return left;
    }
    if left == right {
        left
    } else {
        ClientType::Unknown
    }
}

fn query_handle_inner_type(ty: &syn::Type) -> Option<&syn::Type> {
    match ty {
        syn::Type::Path(tp) if tp.qself.is_none() => {
            let last = tp.path.segments.last()?;
            if last.ident != "Query" {
                return None;
            }
            let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
                return None;
            };
            args.args.iter().find_map(|arg| match arg {
                syn::GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
        }
        syn::Type::Reference(reference) => query_handle_inner_type(&reference.elem),
        syn::Type::Paren(paren) => query_handle_inner_type(&paren.elem),
        syn::Type::Group(group) => query_handle_inner_type(&group.elem),
        _ => None,
    }
}

fn rust_query_data_type(ty: &syn::Type) -> ClientType {
    match ty {
        syn::Type::Reference(reference) => rust_query_data_type(&reference.elem),
        syn::Type::Paren(paren) => rust_query_data_type(&paren.elem),
        syn::Type::Group(group) => rust_query_data_type(&group.elem),
        syn::Type::Path(tp) if tp.qself.is_none() => rust_query_path_type(&tp.path),
        _ => ClientType::Unknown,
    }
}

fn rust_query_path_type(path: &syn::Path) -> ClientType {
    let Some(last) = path.segments.last() else {
        return ClientType::Unknown;
    };
    let leaf = last.ident.to_string();
    if matches!(last.arguments, syn::PathArguments::None) {
        return match leaf.as_str() {
            "bool" => ClientType::Bool,
            "i8" | "i16" | "i32" => ClientType::I32,
            "u8" | "u16" | "u32" => ClientType::U32,
            "f32" => ClientType::F32,
            "f64" => ClientType::F64,
            "String" | "str" => ClientType::String,
            // `fetch().json()` and serde_json hydrate wide integers as JSON
            // numbers unless a richer codec says otherwise, so do not claim a
            // BigInt-backed shape for query payloads from Rust syntax alone.
            "i64" | "i128" | "isize" | "u64" | "u128" | "usize" => ClientType::Unknown,
            _ => ClientType::Struct(leaf),
        };
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return ClientType::Unknown;
    };
    let inner = args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    match (leaf.as_str(), inner) {
        ("Option", Some(inner)) => ClientType::option(rust_query_data_type(inner)),
        ("Vec", Some(inner)) => ClientType::vec(rust_query_data_type(inner)),
        _ => ClientType::Unknown,
    }
}

pub(super) fn is_query_key_new_call(func: &Expr) -> bool {
    let Expr::Path(ExprPath {
        path, qself: None, ..
    }) = func
    else {
        return false;
    };
    let mut segments = path.segments.iter().rev();
    matches!(segments.next(), Some(seg) if seg.ident == "new")
        && matches!(segments.next(), Some(seg) if seg.ident == "QueryKey")
}

pub(super) fn is_server_fetcher_call(expr: &Expr) -> bool {
    matches!(expr, Expr::Call(call) if path_ends_with(&call.func, "server_fetcher"))
}

pub(super) fn mutation_local_call(local: &Local) -> Option<(syn::Ident, &ExprCall, bool)> {
    let name = local_ident(local)?;
    let init = local.init.as_ref()?;
    let Expr::Call(call) = init.expr.as_ref() else {
        return None;
    };
    let name_str = path_leaf(&call.func)?;
    if !is_rsjs_root_call(&call.func, &name_str) {
        return None;
    }
    match name_str.as_str() {
        "mutation" => Some((name, call, false)),
        "mutation_with_options" => Some((name, call, true)),
        _ => None,
    }
}

pub(super) fn local_ident(local: &Local) -> Option<syn::Ident> {
    match &local.pat {
        Pat::Ident(pi) => Some(pi.ident.clone()),
        Pat::Type(pt) => match pt.pat.as_ref() {
            Pat::Ident(pi) => Some(pi.ident.clone()),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn path_leaf(expr: &Expr) -> Option<String> {
    let Expr::Path(ExprPath {
        path, qself: None, ..
    }) = expr
    else {
        return None;
    };
    path.segments.last().map(|seg| seg.ident.to_string())
}

pub(super) fn collect_handler_locals(lets: &[Local]) -> syn::Result<Vec<HandlerLocal>> {
    let mut out = Vec::new();
    for local in lets {
        let name = match &local.pat {
            Pat::Ident(pi) => pi.ident.clone(),
            Pat::Type(pt) => match pt.pat.as_ref() {
                Pat::Ident(pi) => pi.ident.clone(),
                _ => continue,
            },
            _ => continue,
        };

        let init = match &local.init {
            Some(li) => li,
            None => continue,
        };

        let call = match init.expr.as_ref() {
            Expr::Call(c) => c,
            _ => continue,
        };

        let kind = if is_rsjs_handler_call(&call.func) {
            HandlerLocalKind::Payload
        } else if is_rsjs_event_handler_call(&call.func) {
            HandlerLocalKind::Event
        } else {
            continue;
        };

        if call.args.len() != 1 {
            return Err(syn::Error::new(
                call.span(),
                format!(
                    "`{}` takes exactly one closure argument",
                    handler_constructor_name(kind)
                ),
            ));
        }

        let closure = match &call.args[0] {
            Expr::Closure(c) => c.clone(),
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    match kind {
                        HandlerLocalKind::Payload => {
                            "`rsjs::handler(...)` expects a closure, e.g. \
                         `let on_save = rsjs::handler(|_| saved.set(true));`"
                        }
                        HandlerLocalKind::Event => {
                            "`rsjs::event_handler(...)` expects a closure, e.g. \
                         `let on_pick = rsjs::event_handler(|event: rsjs::Event, id: u8| { ... });`"
                        }
                    },
                ));
            }
        };

        match kind {
            HandlerLocalKind::Payload => {
                payload_handler_param(&closure)?;
            }
            HandlerLocalKind::Event => {
                event_handler_local_params(&closure)?;
            }
        }

        out.push(HandlerLocal {
            name,
            closure,
            kind,
            // `collect_handler_local` replaces this parser-local placeholder
            // with the exact component prelude position before the handler is
            // made visible to template analysis.
            prelude_local_index: 0,
            source_anchor: source_anchor(call.span()),
            capture_env: None,
        });
    }
    Ok(out)
}

pub(super) struct NodeRefLocal {
    name: syn::Ident,
    source_anchor: rsjs_ir::SourceAnchor,
}

pub(super) fn collect_node_ref_locals(lets: &[Local]) -> syn::Result<Vec<NodeRefLocal>> {
    let mut out = Vec::new();
    for local in lets {
        let name = match &local.pat {
            Pat::Ident(pi) => pi.ident.clone(),
            Pat::Type(pt) => match pt.pat.as_ref() {
                Pat::Ident(pi) => pi.ident.clone(),
                _ => continue,
            },
            _ => continue,
        };

        let Some(init) = &local.init else {
            continue;
        };
        let Expr::Call(call) = init.expr.as_ref() else {
            continue;
        };
        if !is_rsjs_root_call(&call.func, "node_ref") {
            continue;
        }
        if !call.args.is_empty() {
            return Err(syn::Error::new(
                call.span(),
                "`node_ref::<T>()` takes no runtime arguments",
            ));
        }
        out.push(NodeRefLocal {
            name,
            source_anchor: source_anchor(call.span()),
        });
    }
    Ok(out)
}

pub(super) struct HandlerParam {
    pub(super) name: Option<String>,
    pub(super) arity: usize,
    pub(super) integer_source: Option<IntegerSource>,
}

pub(super) struct EventHandlerLocalParams {
    pub(super) event_param: Option<String>,
    pub(super) payload: HandlerParam,
}

pub(super) fn handler_constructor_name(kind: HandlerLocalKind) -> &'static str {
    match kind {
        HandlerLocalKind::Payload => "rsjs::handler(...)",
        HandlerLocalKind::Event => "rsjs::event_handler(...)",
    }
}

pub(super) struct HandlerLocalDecl {
    pub(super) name: String,
    pub(super) mutable: bool,
}

pub(super) fn handler_local_binding(local: &Local) -> syn::Result<HandlerLocalDecl> {
    fn name_from_pat(pat: &Pat) -> syn::Result<HandlerLocalDecl> {
        match pat {
            Pat::Ident(pi) => Ok(HandlerLocalDecl {
                name: pi.ident.to_string(),
                mutable: pi.mutability.is_some(),
            }),
            Pat::Type(pt) => name_from_pat(&pt.pat),
            other => Err(syn::Error::new(
                other.span(),
                "`#[rsjs]` handler-local `let` bindings must use a plain identifier",
            )),
        }
    }
    name_from_pat(&local.pat)
}

pub(super) fn handler_pat_integer_source(pat: &Pat) -> Option<IntegerSource> {
    let Pat::Type(typed) = pat else {
        return None;
    };
    let name = rust_type_leaf_name(&typed.ty)?;
    if !is_rust_integer_type(&name) {
        return None;
    }
    Some(
        IntegerDomain::from_name(&name)
            .map(IntegerSource::Supported)
            .unwrap_or(IntegerSource::Unsupported(name)),
    )
}

pub(super) fn handler_assignment_target(expr: &Expr) -> syn::Result<String> {
    match expr {
        Expr::Path(path) if path.path.get_ident().is_some() => {
            Ok(path.path.get_ident().expect("checked").to_string())
        }
        Expr::Paren(paren) => handler_assignment_target(&paren.expr),
        Expr::Group(group) => handler_assignment_target(&group.expr),
        other => Err(syn::Error::new(
            other.span(),
            "`#[rsjs]` handler assignments currently support only a plain local identifier on the left-hand side",
        )),
    }
}

pub(super) fn is_compound_assignment_op(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::AddAssign(_)
            | syn::BinOp::SubAssign(_)
            | syn::BinOp::MulAssign(_)
            | syn::BinOp::DivAssign(_)
            | syn::BinOp::RemAssign(_)
            | syn::BinOp::BitXorAssign(_)
            | syn::BinOp::BitAndAssign(_)
            | syn::BinOp::BitOrAssign(_)
            | syn::BinOp::ShlAssign(_)
            | syn::BinOp::ShrAssign(_)
    )
}

pub(super) fn compound_assignment_binary_op(op: &syn::BinOp) -> Option<BinOp> {
    match op {
        syn::BinOp::AddAssign(_) => Some(BinOp::Add),
        syn::BinOp::SubAssign(_) => Some(BinOp::Sub),
        syn::BinOp::MulAssign(_) => Some(BinOp::Mul),
        syn::BinOp::DivAssign(_) => Some(BinOp::Div),
        syn::BinOp::RemAssign(_) => Some(BinOp::Rem),
        _ => None,
    }
}

pub(super) fn payload_handler_param(closure: &ExprClosure) -> syn::Result<HandlerParam> {
    match closure.inputs.len() {
        0 => Ok(HandlerParam {
            name: None,
            arity: 0,
            integer_source: None,
        }),
        1 => {
            let input = closure.inputs.first().expect("len checked above");
            let integer_source = handler_pat_integer_source(input);
            let name = match input {
                Pat::Ident(pi) => Some(pi.ident.to_string()),
                Pat::Type(pt) => match pt.pat.as_ref() {
                    Pat::Ident(pi) => Some(pi.ident.to_string()),
                    Pat::Wild(_) => None,
                    other => {
                        return Err(syn::Error::new(
                            other.span(),
                            "`rsjs::handler(...)` payload parameter must be a plain ident or `_`",
                        ));
                    }
                },
                Pat::Wild(_) => None,
                other => {
                    return Err(syn::Error::new(
                        other.span(),
                        "`rsjs::handler(...)` payload parameter must be a plain ident or `_`",
                    ));
                }
            };
            Ok(HandlerParam {
                name,
                arity: 1,
                integer_source,
            })
        }
        _ => Err(syn::Error::new(
            closure.span(),
            "`rsjs::handler(...)` closures accept at most one payload argument",
        )),
    }
}

pub(super) fn event_handler_local_params(
    closure: &ExprClosure,
) -> syn::Result<EventHandlerLocalParams> {
    let mut inputs = closure.inputs.iter();
    match closure.inputs.len() {
        1 => {
            let event = inputs.next().expect("len checked above");
            Ok(EventHandlerLocalParams {
                event_param: event_param_name(
                    event,
                    "`rsjs::event_handler(...)` event parameter must be a plain ident, typed plain ident, or `_`",
                )?,
                payload: HandlerParam {
                    name: None,
                    arity: 0,
                    integer_source: None,
                },
            })
        }
        2 => {
            let event = inputs.next().expect("len checked above");
            let payload = inputs.next().expect("len checked above");
            Ok(EventHandlerLocalParams {
                event_param: event_param_name(
                    event,
                    "`rsjs::event_handler(...)` event parameter must be a plain ident, typed plain ident, or `_`",
                )?,
                payload: HandlerParam {
                    name: handler_payload_param_name(
                        payload,
                        "`rsjs::event_handler(...)` payload parameter must be a plain ident, typed plain ident, or `_`",
                    )?,
                    arity: 1,
                    integer_source: handler_pat_integer_source(payload),
                },
            })
        }
        _ => Err(syn::Error::new(
            closure.span(),
            "`rsjs::event_handler(...)` closures must take an event parameter and may take one payload parameter",
        )),
    }
}

pub(super) fn event_handler_param(closure: &ExprClosure) -> syn::Result<Option<String>> {
    match closure.inputs.len() {
        0 => Ok(None),
        1 => {
            let input = closure.inputs.first().expect("len checked above");
            event_param_name(
                input,
                "`#[rsjs]` event-handler parameter must be a plain ident, typed plain ident, or `_`",
            )
        }
        _ => Err(syn::Error::new(
            closure.span(),
            "`#[rsjs]` event handlers accept at most one parameter (the event)",
        )),
    }
}

pub(super) fn event_param_name(pat: &Pat, error: &'static str) -> syn::Result<Option<String>> {
    match pat {
        Pat::Ident(pi) => Ok(Some(pi.ident.to_string())),
        Pat::Type(pt) => event_param_name(pt.pat.as_ref(), error),
        Pat::Wild(_) => Ok(None),
        other => Err(syn::Error::new(other.span(), error)),
    }
}

pub(super) fn handler_payload_param_name(
    pat: &Pat,
    error: &'static str,
) -> syn::Result<Option<String>> {
    match pat {
        Pat::Ident(pi) => Ok(Some(pi.ident.to_string())),
        Pat::Type(pt) => handler_payload_param_name(pt.pat.as_ref(), error),
        Pat::Wild(_) => Ok(None),
        other => Err(syn::Error::new(other.span(), error)),
    }
}

/// Parse one prelude `let` binding as a `signal(...)` declaration.
///
/// Returns `Ok(None)` when the local is not a signal declaration
/// (e.g. `memo(...)`, `cx.load(...).await?`); only signal
/// declarations contribute entries.
pub(super) fn signal_local_decl(local: &Local) -> syn::Result<Option<SignalLocal>> {
    // Pattern: must be a plain ident (no destructuring).
    let name = match &local.pat {
        Pat::Ident(pi) => pi.ident.clone(),
        Pat::Type(pt) => match pt.pat.as_ref() {
            Pat::Ident(pi) => pi.ident.clone(),
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };

    let init = match &local.init {
        Some(li) => li,
        None => return Ok(None),
    };

    // Recognise `signal(...)`. Anything else (e.g.
    // `cx.load(...).await?`) is left alone; only signal
    // declarations contribute IR.
    let call = match init.expr.as_ref() {
        Expr::Call(c) => c,
        _ => return Ok(None),
    };

    if !is_rsjs_root_call(&call.func, "signal") {
        return Ok(None);
    }

    if call.args.len() != 1 {
        return Err(syn::Error::new(
            call.span(),
            "`signal(...)` takes exactly one initial-value argument; \
             use `signal(<literal>)` or `signal(<runtime expr>)`",
        ));
    }

    let arg = &call.args[0];
    let call_ty = signal_call_explicit_type(call);
    let pattern_ty = signal_pattern_explicit_type(&local.pat)?;
    validate_signal_explicit_type_agreement(call_ty.as_ref(), pattern_ty.as_ref(), call.span())?;
    // Explicit Option constructors are ordinary-Rust type boundaries.
    // Preserve `Option::<T>::None` / `Option::<T>::Some(...)`, and the exact
    // integer suffix in `Some(1_i64)`, when the outer signal constructor has
    // no turbofish. Opaque `Some(variable)` still requires
    // `signal::<Option<T>>(...)`; no variable type is guessed.
    let initializer_ty = option_initializer_explicit_type(arg);
    let explicit_ty = call_ty.or(pattern_ty).or(initializer_ty);
    validate_signal_integer_source(explicit_ty.as_ref(), arg)?;
    let integer_source = signal_integer_source(explicit_ty.as_ref(), arg);
    let (mut fallback, runtime_expr) = signal_initial_value(arg)?;
    if let Some(IntegerSource::Supported(domain)) = &integer_source {
        fallback = coerce_signal_integer_fallback(fallback, *domain, arg.span())?;
    }
    let runtime_expr = runtime_expr.or_else(|| {
        should_preserve_signal_initializer_expr(arg, explicit_ty.as_ref()).then(|| arg.clone())
    });
    Ok(Some(SignalLocal {
        name,
        source_anchor: source_anchor(call.span()),
        fallback,
        runtime_expr,
        explicit_ty,
        integer_source,
    }))
}

fn option_path_explicit_type(path: &syn::Path, variant: &str) -> Option<syn::Type> {
    if path.segments.last()?.ident != variant {
        return None;
    }
    let option = path
        .segments
        .iter()
        .rev()
        .skip(1)
        .find(|segment| segment.ident == "Option")?;
    let syn::PathArguments::AngleBracketed(arguments) = &option.arguments else {
        return None;
    };
    let inner = arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    })?;
    Some(syn::parse_quote!(::core::option::Option<#inner>))
}

fn option_initializer_explicit_type(expr: &Expr) -> Option<syn::Type> {
    match expr {
        Expr::Path(ExprPath {
            path, qself: None, ..
        }) => option_path_explicit_type(path, "None"),
        Expr::Call(call) => {
            let Expr::Path(ExprPath {
                path, qself: None, ..
            }) = call.func.as_ref()
            else {
                return None;
            };
            if let Some(ty) = option_path_explicit_type(path, "Some") {
                return Some(ty);
            }
            if !path.is_ident("Some") || call.args.len() != 1 {
                return None;
            }
            let source = integer_expr_source_hint(call.args.first()?)?;
            let name = match source {
                IntegerSource::Supported(domain) => domain.as_str().to_owned(),
                IntegerSource::Unsupported(name) => name,
            };
            let inner = syn::parse_str::<syn::Type>(&name).ok()?;
            Some(syn::parse_quote!(::core::option::Option<#inner>))
        }
        _ => None,
    }
}

/// Extract `T` from an explicitly typed signal binding such as
/// `let count: Signal<i64> = signal(0);` or
/// `let count: rsjs::Signal<i64> = rsjs::signal(0);`.
fn signal_pattern_explicit_type(pat: &Pat) -> syn::Result<Option<syn::Type>> {
    let Pat::Type(typed) = pat else {
        return Ok(None);
    };
    let syn::Type::Path(path) = typed.ty.as_ref() else {
        return Ok(None);
    };
    let Some(signal) = path.path.segments.last() else {
        return Ok(None);
    };
    if signal.ident != "Signal" {
        return Ok(None);
    }
    let syn::PathArguments::AngleBracketed(arguments) = &signal.arguments else {
        return Err(syn::Error::new(
            signal.span(),
            "`Signal` bindings must specify their value type, for example `Signal<u32>`",
        ));
    };
    let mut types = arguments.args.iter().filter_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    });
    let Some(value_ty) = types.next() else {
        return Err(syn::Error::new(
            arguments.span(),
            "`Signal` bindings must contain exactly one value type",
        ));
    };
    if types.next().is_some() || arguments.args.len() != 1 {
        return Err(syn::Error::new(
            arguments.span(),
            "`Signal` bindings must contain exactly one value type",
        ));
    }
    Ok(Some(value_ty))
}

fn validate_signal_explicit_type_agreement(
    call_ty: Option<&syn::Type>,
    pattern_ty: Option<&syn::Type>,
    span: proc_macro2::Span,
) -> syn::Result<()> {
    let (Some(call_ty), Some(pattern_ty)) = (call_ty, pattern_ty) else {
        return Ok(());
    };
    let call_name = rust_type_leaf_name(call_ty);
    let pattern_name = rust_type_leaf_name(pattern_ty);
    let (Some(call_name), Some(pattern_name)) = (call_name, pattern_name) else {
        return Ok(());
    };
    if call_name == pattern_name {
        return Ok(());
    }
    if is_rust_integer_type(&call_name) || is_rust_integer_type(&pattern_name) {
        if IntegerDomain::from_name(&call_name).is_none()
            || IntegerDomain::from_name(&pattern_name).is_none()
        {
            let unsupported = [&call_name, &pattern_name]
                .into_iter()
                .find(|name| is_rust_integer_type(name) && IntegerDomain::from_name(name).is_none())
                .expect("one unsupported integer was established");
            return Err(unsupported_integer_domain_error(span, unsupported));
        }
        return Err(syn::Error::new(
            span,
            format!(
                "rsjs/integer-domain-mismatch: signal binding declares `Signal<{pattern_name}>` \
                 but the constructor uses `signal::<{call_name}>(...)`"
            ),
        ));
    }
    Ok(())
}

fn source_anchor(span: proc_macro2::Span) -> rsjs_ir::SourceAnchor {
    let start = span.start();
    rsjs_ir::SourceAnchor {
        start_line: start.line,
        start_column: start.column + 1,
    }
}

/// Declared client-side value shape for a prelude `signal(...)`
/// declaration; fills [`rsjs_ir::reactive::SignalDecl::ty`].
///
/// Supported fixed-width integer signals carry their Rust domain separately
/// from this representation type: `i32`/`u32` are Number-backed and
/// `i64`/`u64` are BigInt-backed from the seed, including small wide values.
/// Runtime initializers require an explicit supported type; an unannotated
/// runtime expression remains [`ClientType::Unknown`] and cannot enter checked
/// integer arithmetic. Unsupported widths retain their source name solely for
/// a targeted diagnostic.
pub(super) fn signal_decl_client_type(local: &SignalLocal) -> ClientType {
    if let Some(integer_source) = &local.integer_source {
        return match integer_source {
            IntegerSource::Supported(domain) => domain.client_type(),
            IntegerSource::Unsupported(name) => rsjs_ir::client_type_from_name(name),
        };
    }
    if local.runtime_expr.is_none() {
        return ClientType::of_value(&local.fallback);
    }
    if let Some(ty) = &local.explicit_ty {
        return rust_signal_value_type(ty);
    }
    if local
        .runtime_expr
        .as_ref()
        .is_some_and(is_owned_string_initializer)
    {
        return ClientType::String;
    }
    let name = local
        .runtime_expr
        .as_ref()
        .and_then(numeric_literal_suffix)
        .map(str::to_string);
    let Some(name) = name else {
        return ClientType::Unknown;
    };
    rsjs_ir::client_type_from_name(&name)
}

fn rust_signal_value_type(ty: &syn::Type) -> ClientType {
    match ty {
        syn::Type::Reference(reference) => rust_signal_value_type(&reference.elem),
        syn::Type::Paren(paren) => rust_signal_value_type(&paren.elem),
        syn::Type::Group(group) => rust_signal_value_type(&group.elem),
        syn::Type::Path(path) if path.qself.is_none() => {
            let Some(last) = path.path.segments.last() else {
                return ClientType::Unknown;
            };
            let name = last.ident.to_string();
            if matches!(last.arguments, syn::PathArguments::None) {
                let primitive = rsjs_ir::client_type_from_name(&name);
                return if primitive.is_unknown() && name != "unknown" {
                    // An explicit nominal signal type is a real authoring
                    // boundary. Final Rust emission still requires
                    // `ClientEncode` for the seed, so treating a named type as
                    // a client struct does not let an unencodable value pass
                    // compilation. Keeping the nominal shape here lets
                    // `signal::<Vec<MyClientProps>>(runtime_value)` retain the
                    // type information that descriptor linking needs.
                    ClientType::Struct(name)
                } else {
                    primitive
                };
            }
            let syn::PathArguments::AngleBracketed(arguments) = &last.arguments else {
                return ClientType::Unknown;
            };
            let inner = arguments.args.iter().find_map(|argument| match argument {
                syn::GenericArgument::Type(ty) => Some(ty),
                _ => None,
            });
            match (name.as_str(), inner) {
                ("Option", Some(inner)) => ClientType::option(rust_signal_value_type(inner)),
                ("Vec", Some(inner)) => ClientType::vec(rust_signal_value_type(inner)),
                _ => ClientType::Unknown,
            }
        }
        _ => ClientType::Unknown,
    }
}

fn signal_integer_source(explicit_ty: Option<&syn::Type>, arg: &Expr) -> Option<IntegerSource> {
    let name = explicit_ty
        .and_then(rust_type_leaf_name)
        .filter(|name| is_rust_integer_type(name))
        .or_else(|| integer_literal_suffix(arg).map(ToOwned::to_owned))?;
    if let Some(domain) = IntegerDomain::from_name(&name) {
        Some(IntegerSource::Supported(domain))
    } else {
        Some(IntegerSource::Unsupported(name))
    }
}

fn validate_signal_integer_source(explicit_ty: Option<&syn::Type>, arg: &Expr) -> syn::Result<()> {
    if let Some(name) = explicit_ty.and_then(unsupported_signal_integer_leaf) {
        return Err(unsupported_integer_domain_error(arg.span(), &name));
    }
    let literal_source = integer_expr_source_hint(arg);
    if let Some(IntegerSource::Unsupported(name)) = literal_source.as_ref() {
        return Err(unsupported_integer_domain_error(arg.span(), name));
    }
    let Some(explicit_name) = explicit_ty
        .and_then(rust_type_leaf_name)
        .filter(|name| is_rust_integer_type(name))
    else {
        return Ok(());
    };
    if IntegerDomain::from_name(&explicit_name).is_none() {
        return Err(unsupported_integer_domain_error(arg.span(), &explicit_name));
    }
    let Some(literal_source) = literal_source else {
        // Unsuffixed literals inherit the explicit signal type. Runtime
        // expressions are guarded by the explicit domain and strict cell.
        return Ok(());
    };
    let explicit_source = IntegerSource::Supported(
        IntegerDomain::from_name(&explicit_name).expect("unsupported domains returned above"),
    );
    match (explicit_source, literal_source) {
        (IntegerSource::Supported(expected), IntegerSource::Supported(actual))
            if expected == actual =>
        {
            Ok(())
        }
        (IntegerSource::Supported(expected), IntegerSource::Supported(actual)) => {
            Err(syn::Error::new(
                arg.span(),
                format!(
                    "rsjs/integer-domain-mismatch: `signal::<{}>(...)` cannot be initialized by an explicitly `{}` literal",
                    expected.as_str(),
                    actual.as_str()
                ),
            ))
        }
        (_, IntegerSource::Unsupported(name))
        | (IntegerSource::Unsupported(name), IntegerSource::Supported(_)) => {
            Err(unsupported_integer_domain_error(arg.span(), &name))
        }
    }
}

fn unsupported_signal_integer_leaf(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Reference(reference) => unsupported_signal_integer_leaf(&reference.elem),
        syn::Type::Slice(slice) => unsupported_signal_integer_leaf(&slice.elem),
        syn::Type::Array(array) => unsupported_signal_integer_leaf(&array.elem),
        syn::Type::Tuple(tuple) => tuple.elems.iter().find_map(unsupported_signal_integer_leaf),
        syn::Type::Paren(paren) => unsupported_signal_integer_leaf(&paren.elem),
        syn::Type::Group(group) => unsupported_signal_integer_leaf(&group.elem),
        syn::Type::Path(path) if path.qself.is_none() => {
            let last = path.path.segments.last()?;
            let name = last.ident.to_string();
            if is_rust_integer_type(&name) && IntegerDomain::from_name(&name).is_none() {
                return Some(name);
            }
            let syn::PathArguments::AngleBracketed(arguments) = &last.arguments else {
                return None;
            };
            arguments.args.iter().find_map(|argument| match argument {
                syn::GenericArgument::Type(ty) => unsupported_signal_integer_leaf(ty),
                syn::GenericArgument::AssocType(assoc) => {
                    unsupported_signal_integer_leaf(&assoc.ty)
                }
                _ => None,
            })
        }
        _ => None,
    }
}

pub(super) fn is_rust_integer_type(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
    )
}

fn integer_literal_suffix(arg: &Expr) -> Option<&str> {
    match arg {
        Expr::Lit(ExprLit {
            lit: Lit::Int(value),
            ..
        }) => Some(if value.suffix().is_empty() {
            "i32"
        } else {
            value.suffix()
        }),
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Neg(_)) => {
            integer_literal_suffix(&unary.expr)
        }
        _ => None,
    }
}

fn coerce_signal_integer_fallback(
    fallback: Value,
    domain: IntegerDomain,
    span: proc_macro2::Span,
) -> syn::Result<Value> {
    let integer = match fallback {
        Value::Int(value) => i128::from(value),
        Value::BigInt(value) => value,
        // Runtime initializers intentionally use `Null` as their compile-time
        // placeholder; the generated Rust initializer supplies the real value.
        Value::Null => return Ok(Value::Null),
        _ => {
            return Err(syn::Error::new(
                span,
                format!(
                    "`signal::<{}>(...)` requires an integer initializer",
                    domain.as_str()
                ),
            ));
        }
    };
    let in_range = match domain {
        IntegerDomain::I32 => (i32::MIN as i128..=i32::MAX as i128).contains(&integer),
        IntegerDomain::U32 => (0..=u32::MAX as i128).contains(&integer),
        IntegerDomain::I64 => (i64::MIN as i128..=i64::MAX as i128).contains(&integer),
        IntegerDomain::U64 => (0..=u64::MAX as i128).contains(&integer),
    };
    if !in_range {
        return Err(syn::Error::new(
            span,
            format!(
                "rsjs/integer-initializer-out-of-range: `{integer}` is outside the `{}` domain",
                domain.as_str()
            ),
        ));
    }
    Ok(if domain.is_wide() {
        Value::BigInt(integer)
    } else {
        Value::Int(i64::try_from(integer).expect("i32/u32 always fit Value::Int"))
    })
}

/// Leaf identifier of a Rust type for [`signal_decl_client_type`]:
/// `std::string::String` yields `String`, `&str` yields `str`.
pub(super) fn rust_type_leaf_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path.path.segments.last().map(|seg| seg.ident.to_string()),
        syn::Type::Reference(reference) => rust_type_leaf_name(&reference.elem),
        syn::Type::Paren(paren) => rust_type_leaf_name(&paren.elem),
        syn::Type::Group(group) => rust_type_leaf_name(&group.elem),
        _ => None,
    }
}

/// Match `let <name> = memo(|| ...);` (bare or `rsjs::`-qualified,
/// mirroring how `signal(...)` is matched by path leaf).
pub(super) fn memo_local_call(local: &Local) -> Option<(syn::Ident, &ExprCall)> {
    let name = local_ident(local)?;
    let init = local.init.as_ref()?;
    let Expr::Call(call) = init.expr.as_ref() else {
        return None;
    };
    if !is_rsjs_root_call(&call.func, "memo") {
        return None;
    }
    Some((name, call))
}

pub(super) fn memo_body_error(span: proc_macro2::Span) -> syn::Error {
    syn::Error::new(
        span,
        "`memo(...)` closures must be a single reactive expression (signal reads, \
         literals, arithmetic and comparisons, the supported string methods, and \
         `if`/`else` expressions). Statements such as `let` bindings or signal writes \
         cannot run inside a derived cell: it re-evaluates automatically whenever its \
         inputs change, so side effects would fire on every recomputation. Move the \
         statements into an event handler and keep the closure to one expression, \
         e.g. `rsjs::memo(|| price.get() * qty.get())`",
    )
}

pub(super) fn signal_initial_value(arg: &Expr) -> syn::Result<(Value, Option<Expr>)> {
    Ok(match literal_to_value(arg) {
        Ok(value) => (value, None),
        Err(_) => (Value::Null, Some(arg.clone())),
    })
}

pub(crate) fn signal_call_explicit_type(call: &ExprCall) -> Option<syn::Type> {
    let Expr::Path(ExprPath { path, .. }) = call.func.as_ref() else {
        return None;
    };
    let last = path.segments.last()?;
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    })
}

pub(super) fn should_preserve_signal_initializer_expr(
    arg: &Expr,
    explicit_ty: Option<&syn::Type>,
) -> bool {
    // These shorthands have an unambiguous owned Rust type even without a
    // turbofish. Keeping only their JSON fallback would replay them as `&str`
    // during SSR, which is observably wrong when the signal snapshot crosses
    // a direct component field typed as `String`.
    if is_owned_string_initializer(arg) {
        return true;
    }
    let numeric_literal = numeric_literal_suffix(arg);
    if explicit_ty.is_some() {
        return numeric_literal.is_some();
    }
    numeric_literal.is_some_and(|suffix| !suffix.is_empty() && suffix != "i64" && suffix != "f64")
}

fn is_owned_string_initializer(arg: &Expr) -> bool {
    match arg {
        Expr::Call(call)
            if is_std_string_associated_call(&call.func, "new")
                || (call.args.len() == 1 && is_std_string_associated_call(&call.func, "from")) =>
        {
            true
        }
        Expr::MethodCall(method)
            if method.method == "to_string"
                && method.args.is_empty()
                && matches!(
                    method.receiver.as_ref(),
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(_),
                        ..
                    })
                ) =>
        {
            true
        }
        _ => false,
    }
}

fn is_std_string_associated_call(function: &Expr, method: &str) -> bool {
    let Expr::Path(ExprPath {
        path, qself: None, ..
    }) = function
    else {
        return false;
    };
    let segments = path.segments.iter().collect::<Vec<_>>();
    segments.len() >= 2
        && segments[segments.len() - 1].ident == method
        && segments[segments.len() - 2].ident == "String"
        && is_std_rooted_method_path(path)
}

pub(super) fn numeric_literal_suffix(arg: &Expr) -> Option<&str> {
    match arg {
        Expr::Lit(ExprLit {
            lit: Lit::Int(li), ..
        }) => Some(li.suffix()),
        Expr::Lit(ExprLit {
            lit: Lit::Float(lf),
            ..
        }) => Some(lf.suffix()),
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => numeric_literal_suffix(&u.expr),
        _ => None,
    }
}

pub(crate) fn path_ends_with(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Path(ExprPath { path, .. }) => path
            .segments
            .last()
            .map(|s| s.ident == name)
            .unwrap_or(false),
        _ => false,
    }
}

/// Match the public primitive spellings claimed by the RSJS frontend:
/// `signal(...)` after an explicit import, or `rsjs::signal(...)`.
///
/// A longer path such as `telemetry::signal(...)` belongs to ordinary Rust and
/// must remain untouched by the tolerant server-program rewrite.
pub(crate) fn is_rsjs_root_call(expr: &Expr, expected: &str) -> bool {
    let Expr::Path(ExprPath {
        path, qself: None, ..
    }) = expr
    else {
        return false;
    };
    is_rsjs_root_path(path, expected)
}

pub(super) fn is_rsjs_root_path(path: &syn::Path, expected: &str) -> bool {
    let Some(last) = path.segments.last() else {
        return false;
    };
    if last.ident != expected {
        return false;
    }
    match path.segments.len() {
        1 => true,
        2 => path
            .segments
            .first()
            .is_some_and(|segment| segment.ident == "rsjs"),
        _ => false,
    }
}

pub(super) fn is_rsjs_handler_call(expr: &Expr) -> bool {
    is_rsjs_handler_constructor_call(expr, "handler")
}

/// Returns `true` if `expr` is a `debounce` / `rsjs::debounce` path,
/// the wrapper recognised inside event-handler attribute values:
/// `on_input={rsjs::debounce(|event| ..., 300)}`.
pub(super) fn is_rsjs_debounce_call(expr: &Expr) -> bool {
    path_ends_with(expr, "debounce")
}

/// Parse the delay argument of `rsjs::interval(ms, ...)` /
/// `rsjs::debounce(..., delay_ms)`. The window must be an integer literal: the
/// emitted JS bakes the value into the island module, so there is no
/// place for a runtime expression to flow into it.
pub(super) fn timer_ms_literal(expr: &Expr, what: &str) -> syn::Result<u64> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(int), ..
        }) => int.base10_parse::<u64>(),
        other => {
            let (signature, parameter, example) = if what == "debounce" {
                (
                    "rsjs::debounce(handler, delay_ms)",
                    "delay_ms",
                    "rsjs::debounce(|event| ..., 300)",
                )
            } else {
                ("rsjs::interval(ms, ...)", "ms", "rsjs::interval(300, ...)")
            };
            Err(syn::Error::new(
                other.span(),
                format!(
                    "`{signature}` requires an integer literal for `{parameter}` \
                     (e.g. `{example}`); expressions are not supported \
                     because the window is baked into the island module at compile time"
                ),
            ))
        }
    }
}

pub(super) fn is_rsjs_event_handler_call(expr: &Expr) -> bool {
    is_rsjs_handler_constructor_call(expr, "event_handler")
}

pub(super) fn is_rsjs_handler_constructor_call(expr: &Expr, expected: &str) -> bool {
    let Expr::Path(ExprPath { path, .. }) = expr else {
        return false;
    };
    let Some(last) = path.segments.last() else {
        return false;
    };
    if last.ident != expected {
        return false;
    }
    match path.segments.len() {
        // `handler(...)` / `event_handler(...)` after `use rsjs::...;`.
        1 => true,
        // `rsjs::handler(...)` / `rsjs::event_handler(...)`, with or without a leading `::`.
        2 => path
            .segments
            .first()
            .map(|seg| seg.ident == "rsjs")
            .unwrap_or(false),
        _ => false,
    }
}

pub(super) fn is_rsjs_noop_handler_call(expr: &Expr) -> bool {
    let Expr::Path(ExprPath { path, .. }) = expr else {
        return false;
    };
    let Some(last) = path.segments.last() else {
        return false;
    };
    if last.ident != "noop_handler" {
        return false;
    }
    match path.segments.len() {
        1 => true,
        2 => path
            .segments
            .first()
            .map(|seg| seg.ident == "rsjs")
            .unwrap_or(false),
        _ => false,
    }
}

pub(super) fn is_some_call(expr: &Expr) -> bool {
    let Expr::Path(ExprPath {
        path, qself: None, ..
    }) = expr
    else {
        return false;
    };
    path.is_ident("Some")
}

pub(super) fn is_none_expr(expr: &Expr) -> bool {
    let Expr::Path(ExprPath {
        path, qself: None, ..
    }) = expr
    else {
        return false;
    };
    path.is_ident("None")
}
