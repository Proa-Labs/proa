// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Reactive expression lowering: calls, method-call table, binary/unary ops, typed numeric fixups, and literal conversion.
use super::*;

impl Analyzer {
    /// Lower a Rust expression to the constrained [`FrontendExpr`] shape.
    ///
    /// Recognised shapes: literal, signal-name `.get()`,
    /// `signal_name.is_non_empty()`, binary op, unary `!`.
    pub(super) fn lower_reactive_expr(&mut self, expr: &Expr) -> syn::Result<FrontendExpr> {
        self.lower_reactive_expr_in(expr, &ExprScope::default())
    }

    /// Lower a Rust expression in a scope: a closure parameter is in
    /// scope iff `scope.closure_param == Some((name, signal_id))`.
    /// References to that name inside the body lower to a SignalRead
    /// of the receiver signal (the `signal.update(|c| c + 1)` shape:
    /// the closure parameter `c` *is* the current value of `signal`).
    pub(super) fn lower_reactive_expr_in(
        &mut self,
        expr: &Expr,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        if let Some(elements) = vec_literal_array_value(expr)? {
            return Ok(FrontendExpr::ArrayLit(elements));
        }
        if let Expr::Field(field) = expr {
            if expr_is_self_path(&field.base) {
                return self.lower_inbound_live_field(field);
            }
        }
        if expr_is_self_path(expr) {
            return Err(server_owned_self_error(expr));
        }

        match expr {
            Expr::Macro(m) if crate::is_text_intrinsic_path(&m.mac.path) => {
                self.lower_text_intrinsic(expr, scope)
            }
            Expr::Lit(ExprLit { lit, .. }) => {
                Ok(FrontendExpr::Literal(lit_to_value(lit, expr.span())?))
            }
            Expr::Tuple(t) if t.elems.is_empty() => Ok(FrontendExpr::Literal(Value::Null)),
            Expr::Path(ExprPath {
                path, qself: None, ..
            }) if is_none_value_path(path) => Ok(FrontendExpr::Literal(Value::Null)),
            // `String::new()` and `<lit>.to_string()` shorthands.
            Expr::Call(c) if path_ends_with(&c.func, "new") && is_string_path(&c.func) => {
                Ok(FrontendExpr::Literal(Value::String(String::new())))
            }
            Expr::Call(c) if Self::is_some_constructor(c) => {
                let Some(inner) = c.args.first() else {
                    return Err(syn::Error::new(
                        c.span(),
                        "`Some(...)` in an rsjs prop expression needs exactly one value",
                    ));
                };
                self.lower_reactive_expr_in(inner, scope)
            }
            Expr::MethodCall(mc) if mc.method == "to_string" => {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = &*mc.receiver
                {
                    Ok(FrontendExpr::Literal(Value::String(s.value())))
                } else {
                    self.lower_method_call(mc, scope)
                }
            }
            Expr::Path(ExprPath { path, .. }) if path.get_ident().is_some() => {
                let ident = path.get_ident().unwrap().to_string();
                if let Some((param_name, receiver)) = scope.closure_param {
                    if ident == param_name {
                        return Ok(FrontendExpr::SignalRead(receiver));
                    }
                }
                if let Some((param_name, value)) = scope.inline_param {
                    if ident == param_name {
                        return Ok(value.clone());
                    }
                }
                if let Some((param_name, value)) = scope.handler_param {
                    if ident == param_name {
                        return Ok(value.clone());
                    }
                }
                if let Some(local) = scope
                    .handler_locals
                    .iter()
                    .rev()
                    .find(|local| local.name == ident)
                {
                    if local.ty == ClientType::DomElement {
                        return Err(syn::Error::new(
                            expr.span(),
                            "typed DOM handles may only be used as the receiver of a supported \
                             scalar DOM read inside the current event handler",
                        ));
                    }
                    if let Some(constant) = &local.constant {
                        return Ok(constant.clone());
                    }
                    return Ok(FrontendExpr::Local(local.id));
                }
                if scope.node_payload_target.is_some()
                    && self.is_loop_local_name(path.get_ident().unwrap())
                {
                    return self.lower_node_payload_or_error(expr, scope);
                }
                if let Some(expr) = self.client_prelude_locals.get(&ident) {
                    return Ok(expr.clone());
                }
                if let Some(local) = self.unsupported_client_prelude_locals.get(&ident) {
                    return Err(self.unsupported_client_prelude_alias_error(
                        path.span(),
                        &ident,
                        &local.dependency_name,
                    ));
                }
                match self.lookup_signal(&ident, path.span()) {
                    Ok(id) => Ok(FrontendExpr::SignalRead(id)),
                    Err(err) => {
                        if scope.node_payload_target.is_some()
                            && self.expr_mentions_loop_local(expr)
                        {
                            self.lower_node_payload_or_error(expr, scope)
                        } else {
                            Err(err)
                        }
                    }
                }
            }
            Expr::MethodCall(mc) => self.lower_method_call(mc, scope),
            Expr::Index(_) if self.can_lower_rooted_node_payload(expr, scope) => {
                self.lower_node_payload_or_error(expr, scope)
            }
            Expr::Index(index) => match self.lower_index_access(index, scope) {
                Ok(expr) => Ok(expr),
                Err(err) => {
                    if scope.node_payload_target.is_some() && self.expr_mentions_loop_local(expr) {
                        self.lower_node_payload_or_error(expr, scope)
                    } else {
                        Err(err)
                    }
                }
            },
            Expr::Field(_) if self.can_lower_rooted_node_payload(expr, scope) => {
                self.lower_node_payload_or_error(expr, scope)
            }
            Expr::Field(field) => match self.lower_field_access(field, scope) {
                Ok(expr) => Ok(expr),
                Err(err) => {
                    if scope.node_payload_target.is_some() && self.expr_mentions_loop_local(expr) {
                        self.lower_node_payload_or_error(expr, scope)
                    } else {
                        Err(err)
                    }
                }
            },
            Expr::Binary(bin) => self.lower_binary(bin, scope),
            Expr::Unary(u) => self.lower_unary(u, scope),
            Expr::Paren(p) => self.lower_reactive_expr_in(&p.expr, scope),
            Expr::Cast(cast) => self.lower_node_payload_integer_cast(cast, scope),
            Expr::If(if_expr) => self.lower_conditional_expr(if_expr, scope),
            Expr::Call(c) => self.lower_call(c, scope),
            Expr::Struct(s) => Err(self.struct_literal_reactive_error(s)),
            other => self.lower_node_payload_or_error(other, scope),
        }
    }

    fn lower_node_payload_integer_cast(
        &mut self,
        cast: &syn::ExprCast,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        let Some(name) = rust_type_leaf_name(&cast.ty) else {
            return Err(syn::Error::new(
                cast.ty.span(),
                "`#[rsjs]` only accepts an explicit node-payload integer proof cast to i32, u32, i64, or u64",
            ));
        };
        let Some(domain) = IntegerDomain::from_name(&name) else {
            if is_rust_integer_type(&name) {
                return Err(unsupported_integer_domain_error(cast.ty.span(), &name));
            }
            return Err(syn::Error::new(
                cast.ty.span(),
                "`#[rsjs]` client casts are limited to explicit node-payload proofs for i32, u32, i64, or u64",
            ));
        };
        if !self.can_lower_node_payload(&cast.expr, scope) {
            return Err(syn::Error::new(
                cast.span(),
                format!(
                    "`#[rsjs]` does not implement general Rust `as {}` conversion semantics; \
                     this cast is accepted only around an opaque loop-derived node payload to \
                     declare its already-valid client integer domain",
                    domain.as_str()
                ),
            ));
        }
        // Preserve the cast in the server-side payload replay as well as in
        // the client-domain proof. Lowering only `cast.expr` here would emit
        // (for example) a `u8` payload followed by a checked `u32` identity,
        // which correctly fails Rust type checking even though the author
        // explicitly wrote `as u32`.
        let cast_expr = Expr::Cast(cast.clone());
        let mut value = self.lower_node_payload_or_error(&cast_expr, scope)?;
        self.validate_integer_value_for_domain(
            &mut value,
            None,
            domain,
            scope.handler_locals,
            cast.span(),
            "explicit node-payload proof",
            true,
        )?;
        Ok(self.anchor_integer_domain(value, domain))
    }

    pub(super) fn lower_conditional_expr(
        &mut self,
        if_expr: &ExprIf,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        let condition = self.lower_reactive_expr_in(&if_expr.cond, scope)?;
        self.require_bool_condition(&condition, scope.handler_locals, if_expr.cond.span())?;
        let then_expr = self.lower_block_tail_expr(&if_expr.then_branch, scope, "then")?;
        let else_expr = match &if_expr.else_branch {
            Some((_, else_expr)) => match else_expr.as_ref() {
                Expr::Block(ExprBlock { block, .. }) => {
                    self.lower_block_tail_expr(block, scope, "else")?
                }
                Expr::If(nested) => self.lower_conditional_expr(nested, scope)?,
                other => {
                    return Err(syn::Error::new(
                        other.span(),
                        "`#[rsjs]` value-level `if` expressions need an `else` branch that is \
                         either a block expression or an `else if` chain",
                    ));
                }
            },
            None => {
                return Err(syn::Error::new(
                    if_expr.if_token.span,
                    "`#[rsjs]` value-level `if` expressions need an `else` branch so the \
                     generated JavaScript expression has a value in both cases",
                ));
            }
        };
        Ok(FrontendExpr::Conditional {
            condition: Box::new(condition),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
        })
    }

    pub(super) fn lower_block_tail_expr(
        &mut self,
        block: &Block,
        scope: &ExprScope<'_>,
        label: &str,
    ) -> syn::Result<FrontendExpr> {
        match block.stmts.as_slice() {
            [Stmt::Expr(expr, None)] => self.lower_reactive_expr_in(expr, scope),
            [Stmt::Expr(expr, Some(_))] => Err(syn::Error::new(
                expr.span(),
                format!(
                    "`#[rsjs]` value-level `if` {label} branches must end in an expression, \
                     not a semicolon-terminated statement"
                ),
            )),
            [] => Err(syn::Error::new(
                block.brace_token.span.open(),
                format!("`#[rsjs]` value-level `if` {label} branches cannot be empty"),
            )),
            [stmt] => Err(syn::Error::new(
                stmt.span(),
                format!(
                    "`#[rsjs]` value-level `if` {label} branches must contain exactly one \
                     supported reactive expression"
                ),
            )),
            [first, ..] => Err(syn::Error::new(
                first.span(),
                format!(
                    "`#[rsjs]` value-level `if` {label} branches cannot contain local \
                     statements yet; move the logic into a standalone `#[rsjs::client]` helper \
                     or return one supported expression directly"
                ),
            )),
        }
    }

    pub(super) fn is_some_constructor(c: &syn::ExprCall) -> bool {
        let Expr::Path(path) = c.func.as_ref() else {
            return false;
        };
        path.path.is_ident("Some") && c.args.len() == 1
    }

    pub(super) fn struct_literal_reactive_error(&self, expr: &ExprStruct) -> syn::Error {
        let component = expr.path.to_token_stream().to_string();
        let handler_field = expr.fields.iter().find(|field| {
            let field_is_on_callback = match &field.member {
                syn::Member::Named(ident) => ident.to_string().starts_with("on_"),
                syn::Member::Unnamed(_) => false,
            };
            let value_is_handler_ref = match &field.expr {
                Expr::Path(path) => path
                    .path
                    .get_ident()
                    .map(|ident| self.handlers.contains_key(&ident.to_string()))
                    .unwrap_or(false),
                _ => false,
            };
            field_is_on_callback || value_is_handler_ref
        });

        if let Some(field) = handler_field {
            let descriptor_context =
                match crate::component_descriptor::find_by_component_path(&expr.path) {
                    crate::component_descriptor::ComponentDescriptorLookup::Found(summary) => {
                        if summary.handler_fields.is_empty() {
                            format!(
                                "found rsjs descriptor `{}` for `{}` at `{}`, but it does not \
                                 declare handler fields",
                                summary.asset_id,
                                summary.self_ty,
                                summary.path.display()
                            )
                        } else {
                            format!(
                                "found rsjs descriptor `{}` for `{}` with handler field(s): {}",
                                summary.asset_id,
                                summary.self_ty,
                                summary.handler_fields.join(", ")
                            )
                        }
                    }
                    crate::component_descriptor::ComponentDescriptorLookup::Missing => {
                        "no rsjs component descriptor was found for this type".to_string()
                    }
                    crate::component_descriptor::ComponentDescriptorLookup::Ambiguous(matches) => {
                        let ids = matches
                            .iter()
                            .map(|summary| summary.asset_id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!(
                            "found multiple matching rsjs component descriptors ({ids}); use a \
                             less ambiguous component path once inlining is enabled"
                        )
                    }
                    crate::component_descriptor::ComponentDescriptorLookup::Invalid {
                        path,
                        reason,
                    } => {
                        format!(
                            "found an invalid rsjs component descriptor at `{}`: {reason}",
                            path.display()
                        )
                    }
                };
            return guided_boundary_error(
                field.span(),
                format!(
                    "`{component} {{ ... }}` receives rsjs handler props but cannot be inlined"
                ),
                format!(
                    "Handler props are client IR callbacks, not ordinary renderable Rust values. \
                     rsjs needs a component descriptor to map the prop onto child DOM handlers. \
                     Descriptor lookup: {descriptor_context}."
                ),
                [
                    "Make the child a `#[rsjs]` component that emits a descriptor and declares the handler field.",
                    "Move the DOM event handler into the parent island at this call site.",
                    "Keep the child server-rendered with `rsjs::server_only(...)` and do not pass rsjs handler props through it.",
                ],
                Some("component.handler_prop_without_inlining"),
            );
        }

        guided_boundary_error(
            expr.span(),
            format!(
                "`{component} {{ ... }}` is server-rendered Rust content, not a client-reactive expression"
            ),
            "`#[rsjs]` only lowers explicit client-reactive expressions into this slot. Ordinary \
             Proa component construction stays on the server unless you mark it as an rsjs \
             component boundary.",
            [
                "Wrap server-rendered content in `rsjs::server_only(...)` so it stays outside the client IR.",
                "Make the child a descriptor-backed `#[rsjs]` component if it needs client-side callbacks or reactive props.",
            ],
            None,
        )
    }

    pub(super) fn lower_node_payload_or_error(
        &mut self,
        expr: &Expr,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        if let Some(node) = scope.node_payload_target {
            if self.can_lower_node_payload(expr, scope) {
                let (attr, tokens) = self.add_payload_attr(node, expr);
                let payload = FrontendExpr::NodePayload { attr, tokens };
                return Ok(match self.loop_local_integer_domain(expr) {
                    Some(domain) => self.anchor_integer_domain(payload, domain),
                    None => payload,
                });
            }
        }
        Err(unsupported_reactive_expr_error(expr))
    }

    /// Fixed-width domain of a direct repeated-loop binding, when the static
    /// iterator supplied an explicit source proof.
    ///
    /// Stop at the first scope containing the name even when that scope has no
    /// domain entry: an inner opaque binding shadows an outer proven one.
    fn loop_local_integer_domain(&self, expr: &Expr) -> Option<IntegerDomain> {
        let expr = match expr {
            Expr::Paren(paren) => return self.loop_local_integer_domain(&paren.expr),
            Expr::Group(group) => return self.loop_local_integer_domain(&group.expr),
            expr => expr,
        };
        let Expr::Path(path) = expr else {
            return None;
        };
        let name = path.path.get_ident()?.to_string();
        for scope in self.loop_scopes.iter().rev() {
            if scope.locals.contains(&name) {
                return scope.integer_domains.get(&name).copied();
            }
        }
        None
    }

    /// Lower a function call inside a reactive expression.
    ///
    /// A statically resolved function path must identify one standalone
    /// `#[rsjs::client]` helper. The linker follows reachable helper edges
    /// automatically, using rustc's resolution for ordinary imports, aliases,
    /// and re-exports. Dynamic dispatch remains unsupported.
    pub(super) fn lower_call(
        &mut self,
        c: &syn::ExprCall,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        if let Some(closure) = inline_closure_func(c.func.as_ref()) {
            return self.lower_inline_closure_call(c, closure, scope);
        }

        let path = match &*c.func {
            Expr::Path(ExprPath {
                path, qself: None, ..
            }) => path,
            other => {
                let expr = Expr::Call(c.clone());
                if self.can_lower_node_payload(&expr, scope) {
                    return self.lower_node_payload_or_error(&expr, scope);
                }
                let message = if expr_is_inline_closure(other) {
                    "`#[rsjs]` reactive expressions cannot call inline closures or lambdas yet. \
                     The current client IR does not lower arbitrary Rust closure bodies; move \
                     this logic into a standalone helper annotated once with `#[rsjs::client]`, \
                     or rewrite it using supported signal/event expressions."
                } else {
                    "`#[rsjs]` reactive expressions cannot use dynamic call expressions; \
                     client helper calls must use a statically resolved function path whose \
                     definition is annotated with `#[rsjs::client]`"
                };
                return Err(syn::Error::new(other.span(), message));
            }
        };

        // `rsjs::div_int(a, b)` / `rsjs::div_f64(a, b)`: explicit forms of
        // division at a source-typed Rust boundary. Integer division must
        // preserve the complete fixed-width contract, including MIN / -1
        // overflow, so it lowers only after proving (or explicitly receiving)
        // one of the four supported domains. Float division deliberately
        // chooses Number semantics and remains representation-agnostic.
        if let Some(helper) = rsjs_division_helper(path) {
            if c.args.len() != 2 {
                return Err(syn::Error::new(
                    c.span(),
                    "`rsjs::div_int(a, b)` and `rsjs::div_f64(a, b)` take exactly two arguments",
                ));
            }
            match helper {
                RsjsDivisionHelper::Float => {
                    reject_div_f64_turbofish(path)?;
                    let lhs = self.lower_reactive_expr_in(&c.args[0], scope)?;
                    let rhs = self.lower_reactive_expr_in(&c.args[1], scope)?;
                    return Ok(FrontendExpr::BinaryOp {
                        op: BinOp::DynDiv,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                }
                RsjsDivisionHelper::Integer => {
                    let explicit_source = div_int_explicit_integer_source(path)?;
                    if let Some(IntegerSource::Unsupported(name)) = explicit_source.as_ref() {
                        return Err(unsupported_integer_domain_error(c.span(), name));
                    }
                    let declared_domain =
                        explicit_supported_integer_source(explicit_source.as_ref());
                    let lhs_source =
                        explicit_integer_source_in_expr(&c.args[0], "rsjs::div_int left operand")?;
                    let rhs_source =
                        explicit_integer_source_in_expr(&c.args[1], "rsjs::div_int right operand")?;
                    if let Some(domain) = declared_domain {
                        validate_explicit_integer_source_against_domain(
                            lhs_source.as_ref(),
                            domain,
                            c.args[0].span(),
                            "rsjs::div_int left operand",
                        )?;
                        validate_explicit_integer_source_against_domain(
                            rhs_source.as_ref(),
                            domain,
                            c.args[1].span(),
                            "rsjs::div_int right operand",
                        )?;
                    } else if let Some(name) =
                        explicit_unsupported_integer_source(lhs_source.as_ref())
                            .or_else(|| explicit_unsupported_integer_source(rhs_source.as_ref()))
                    {
                        return Err(unsupported_integer_domain_error(c.span(), &name));
                    }
                    let mut lhs_expr = c.args[0].clone();
                    let mut rhs_expr = c.args[1].clone();
                    if let Some(domain) = declared_domain {
                        contextualize_unsuffixed_integer_literals(&mut lhs_expr, domain);
                        contextualize_unsuffixed_integer_literals(&mut rhs_expr, domain);
                        self.specialize_shared_reads_in_explicit_integer_expr(
                            &lhs_expr,
                            domain,
                            "rsjs::div_int left operand",
                        )?;
                        self.specialize_shared_reads_in_explicit_integer_expr(
                            &rhs_expr,
                            domain,
                            "rsjs::div_int right operand",
                        )?;
                    }

                    let mut lhs = self.lower_reactive_expr_in(&lhs_expr, scope)?;
                    let mut rhs = self.lower_reactive_expr_in(&rhs_expr, scope)?;
                    if declared_domain.is_none()
                        && (self.frontend_expr_contains_shared_signal(&lhs)
                            || self.frontend_expr_contains_shared_signal(&rhs))
                    {
                        // A prior explicit helper use may have specialized the
                        // shared declaration, but that contextual fact must not
                        // make a later bare call source-order dependent. Shared
                        // schema fields therefore require their width at every
                        // `div_int` call boundary.
                        return Err(div_int_domain_proof_error(c.span()));
                    }
                    let resolved_lhs = self.resolved_integer_source(
                        lhs_source.as_ref(),
                        &lhs,
                        scope.handler_locals,
                    );
                    let resolved_rhs = self.resolved_integer_source(
                        rhs_source.as_ref(),
                        &rhs,
                        scope.handler_locals,
                    );
                    if let Some(name) = explicit_unsupported_integer_source(resolved_lhs.as_ref())
                        .or_else(|| explicit_unsupported_integer_source(resolved_rhs.as_ref()))
                    {
                        return Err(unsupported_integer_domain_error(c.span(), &name));
                    }

                    let lhs_domain = explicit_supported_integer_source(resolved_lhs.as_ref());
                    let rhs_domain = explicit_supported_integer_source(resolved_rhs.as_ref());
                    let explicit_domain = declared_domain;
                    let domain = match explicit_domain {
                        Some(domain) => domain,
                        None => match (lhs_domain, rhs_domain) {
                            (Some(lhs), Some(rhs)) if lhs == rhs => lhs,
                            (Some(lhs), Some(rhs)) => {
                                return Err(integer_domain_mismatch_error(
                                    c.span(),
                                    rhs,
                                    lhs,
                                    "rsjs::div_int operands",
                                ));
                            }
                            (Some(domain), None) if integer_literal_only(&rhs) => domain,
                            (None, Some(domain)) if integer_literal_only(&lhs) => domain,
                            (Some(_), None) | (None, Some(_)) => {
                                // An unsuffixed literal can inherit the
                                // other operand's domain. A Dyn/Unknown
                                // value cannot: handler bodies are lowered,
                                // not preserved verbatim, so require the
                                // explicit generic boundary that also
                                // produces a generated Rust type witness.
                                return Err(div_int_domain_proof_error(c.span()));
                            }
                            (None, None) => {
                                return Err(div_int_domain_proof_error(c.span()));
                            }
                        },
                    };

                    if explicit_domain.is_some() {
                        self.specialize_explicit_shared_integer_operand(
                            &lhs,
                            domain,
                            lhs_expr.span(),
                            "rsjs::div_int left operand",
                        )?;
                        self.specialize_explicit_shared_integer_operand(
                            &rhs,
                            domain,
                            rhs_expr.span(),
                            "rsjs::div_int right operand",
                        )?;
                    }
                    self.validate_integer_value_for_domain(
                        &mut lhs,
                        lhs_source,
                        domain,
                        scope.handler_locals,
                        lhs_expr.span(),
                        "rsjs::div_int left operand",
                        false,
                    )?;
                    self.validate_integer_value_for_domain(
                        &mut rhs,
                        rhs_source,
                        domain,
                        scope.handler_locals,
                        rhs_expr.span(),
                        "rsjs::div_int right operand",
                        false,
                    )?;
                    return Ok(FrontendExpr::BinaryOp {
                        op: BinOp::Checked {
                            op: CheckedIntegerOp::Div,
                            domain,
                        },
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                }
            }
        }

        // Preserve the ordinary Rust conversion used to combine Web-IDL
        // integer dimensions with floating-point coordinates. Rust implements
        // `From<i32>` and `From<u32>` for `f64`, and both domains are exactly
        // representable as a JavaScript Number. Wider fixed-width domains do
        // not implement this conversion in Rust and must not be silently
        // rounded on the client.
        if is_f64_from_path(path) {
            expect_call_arg_count(c, 1, "f64::from(i32_or_u32)")?;
            let arg = &c.args[0];
            let source = explicit_integer_source_in_expr(arg, "f64::from argument")?;
            match source.as_ref() {
                Some(IntegerSource::Unsupported(name)) => {
                    return Err(unsupported_integer_domain_error(arg.span(), name));
                }
                Some(IntegerSource::Supported(
                    domain @ (IntegerDomain::I64 | IntegerDomain::U64),
                )) => {
                    return Err(syn::Error::new(
                        arg.span(),
                        format!(
                            "`f64::from(...)` inside `#[rsjs]` accepts `i32` or `u32`, not `{}`; Rust does not implement this potentially lossy conversion",
                            domain.as_str()
                        ),
                    ));
                }
                _ => {}
            }
            let mut target = self.lower_reactive_expr_in(arg, scope)?;
            let mut domain = explicit_supported_integer_source(source.as_ref())
                .or_else(|| self.frontend_integer_domain(&target, scope.handler_locals));
            if domain.is_none() && source.is_none() && integer_literal_only(&target) {
                // `f64::from(1)` is ordinary Rust and contextually infers the
                // default integer literal as i32.
                coerce_integer_literals_to_domain(&mut target, IntegerDomain::I32, arg.span())?;
                domain = Some(IntegerDomain::I32);
            }
            match domain {
                Some(IntegerDomain::I32 | IntegerDomain::U32) => {
                    return Ok(FrontendExpr::AsType {
                        target: Box::new(target),
                        ty: AsType::F64FromInt,
                    });
                }
                Some(domain) => {
                    return Err(syn::Error::new(
                        arg.span(),
                        format!(
                            "`f64::from(...)` client lowering only accepts Rust's exact \
                             `From<i32>` and `From<u32>` forms; `{}` has no lossless \
                             ordinary-Rust `From` conversion to `f64`",
                            domain.as_str()
                        ),
                    ));
                }
                None => {
                    return Err(integer_domain_proof_error(
                        arg.span(),
                        "`f64::from(...)` needs a statically proven `i32` or `u32` operand",
                    ));
                }
            }
        }

        if let Some(spec) = registry_read_spec(path) {
            return self.lower_registry_read_call(c, spec, scope);
        }

        if let Some(kind) = browser_read_call_kind(path) {
            return self.lower_browser_read_call(c, kind, scope);
        }

        if let Some(op) = encoding_call_op(path) {
            expect_call_arg_count(c, 1, "rsjs::encoding::<helper>(value)")?;
            let value = self.lower_reactive_expr_in(&c.args[0], scope)?;
            return Ok(FrontendExpr::Encoding {
                op,
                value: Box::new(value),
            });
        }

        let helper_seg = path.segments.last().ok_or_else(|| {
            syn::Error::new(
                path.span(),
                "client helper calls require a statically resolved Rust function path",
            )
        })?;
        let helper_name = helper_seg.ident.to_string();
        let mut args = Vec::with_capacity(c.args.len());
        for a in &c.args {
            match self.lower_reactive_expr_in(a, scope) {
                Ok(mut arg) => {
                    // A direct helper signature is intentionally unavailable
                    // in this proc-macro phase. Preserve an explicitly
                    // suffixed source domain structurally so final linking can
                    // distinguish it from an unsuffixed literal whose domain
                    // legitimately comes from the resolved parameter. This
                    // also keeps SSR replay honest when it re-emits generic
                    // literals without suffixes for rustc inference.
                    match integer_expr_source_hint(a) {
                        Some(IntegerSource::Supported(domain)) => {
                            coerce_integer_literals_to_domain(&mut arg, domain, a.span())?;
                            arg = self.anchor_integer_domain(arg, domain);
                        }
                        Some(IntegerSource::Unsupported(name)) => {
                            return Err(unsupported_integer_domain_error(a.span(), &name));
                        }
                        None => {}
                    }
                    args.push(arg);
                }
                Err(error) => {
                    let expr = Expr::Call(c.clone());
                    if self.can_lower_node_payload(&expr, scope) {
                        return self.lower_node_payload_or_error(&expr, scope);
                    }
                    return Err(error);
                }
            }
        }
        let edge = self.record_direct_helper_call(path, c.args.len())?;

        Ok(FrontendExpr::ClientCall {
            edge,
            authored_path: crate::parse::path_to_canonical_string(path),
            name: helper_name,
            args,
        })
    }

    /// Retain one statically spelled helper call as a Rust-resolved source
    /// edge. The local id is occurrence-based so repeated calls remain
    /// distinct and descriptor output is deterministic.
    fn record_direct_helper_call(
        &mut self,
        helper_path: &syn::Path,
        argument_count: usize,
    ) -> syn::Result<HelperCallEdgeId> {
        let local_id = u32::try_from(self.direct_helper_edges.len()).map_err(|_| {
            syn::Error::new(
                helper_path.span(),
                "an analyzed RSJS component cannot contain more than 2^32 client-helper calls",
            )
        })?;
        self.direct_helper_edges.push(DirectHelperSourceEdge {
            local_id,
            helper_path: helper_path.clone(),
            argument_count,
            expected_return_domain: None,
        });
        Ok(HelperCallEdgeId::new(local_id))
    }

    pub(super) fn lower_inline_closure_call(
        &mut self,
        call: &ExprCall,
        closure: &ExprClosure,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        if closure.asyncness.is_some() {
            return Err(syn::Error::new(
                closure.asyncness.span(),
                "`#[rsjs]` reactive inline closure calls cannot be `async`; move asynchronous \
                 client work into a query or mutation (`#[rsjs::client]` helpers are \
                 origin-free and synchronous)",
            ));
        }
        if call.args.len() != 1 {
            return Err(syn::Error::new(
                call.span(),
                "`#[rsjs]` reactive inline closure calls currently support exactly one argument",
            ));
        }
        let param_name = inline_reactive_closure_param_name(closure)?;
        let arg = self.lower_reactive_expr_in(&call.args[0], scope)?;
        let closure_scope = ExprScope {
            inline_param: Some((param_name.as_str(), &arg)),
            ..*scope
        };
        match closure.body.as_ref() {
            Expr::Block(block) => {
                self.lower_block_tail_expr(&block.block, &closure_scope, "inline closure")
            }
            body => self.lower_reactive_expr_in(body, &closure_scope),
        }
    }

    /// Lower a call recognised by `rsjs_ir::browser_registry` in
    /// read position: check arity against the entry, lower each
    /// argument per its declared kind, then realise the entry's
    /// Client IR lowering.
    pub(super) fn lower_registry_read_call(
        &mut self,
        call: &ExprCall,
        spec: &'static BrowserHelperSpec,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        use rsjs_ir::browser_registry::HelperLowering;

        self.require_client_origin(call.span(), ClientOriginKind::BrowserRead)?;
        expect_call_arg_count(call, spec.args.len(), spec.usage)?;
        if matches!(
            &spec.lowering,
            HelperLowering::StorageGet {
                value_type: rsjs_ir::StorageValueType::I64,
                ..
            }
        ) {
            let fallback_source =
                explicit_integer_source_in_expr(&call.args[1], "storage i64 fallback")?;
            validate_explicit_integer_source_against_domain(
                fallback_source.as_ref(),
                IntegerDomain::I64,
                call.args[1].span(),
                "storage i64 fallback",
            )?;
        }
        let mut args = Vec::with_capacity(spec.args.len());
        for (arg, arg_spec) in call.args.iter().zip(spec.args) {
            debug_assert_eq!(
                arg_spec.kind,
                rsjs_ir::browser_registry::ArgKind::Reactive,
                "registry read helpers take only reactive args (enforced by registry tests)"
            );
            args.push(self.lower_reactive_expr_in(arg, scope)?);
        }
        let mut args = args.into_iter();
        Ok(match spec.lowering {
            HelperLowering::WindowField(field) => {
                FrontendExpr::BrowserRead(BrowserRead::Window(field))
            }
            HelperLowering::StorageGet { area, value_type } => {
                let key = args.next().expect("arity checked: storage key");
                let mut fallback = args.next();
                if value_type == rsjs_ir::StorageValueType::I64 {
                    if let Some(value) = fallback.as_mut() {
                        self.validate_integer_value_for_domain(
                            value,
                            explicit_integer_source_in_expr(&call.args[1], "storage i64 fallback")?,
                            IntegerDomain::I64,
                            scope.handler_locals,
                            call.args[1].span(),
                            "storage i64 fallback",
                            false,
                        )?;
                    }
                }
                FrontendExpr::BrowserRead(BrowserRead::Storage {
                    area,
                    value_type,
                    key: Box::new(key),
                    fallback: fallback.map(Box::new),
                })
            }
            HelperLowering::DeclaredRead { .. } => {
                FrontendExpr::BrowserRead(BrowserRead::Declared {
                    helper: spec.id,
                    args: args.collect(),
                })
            }
            _ => {
                return Err(syn::Error::new(
                    call.span(),
                    format!(
                        "`{}` is a browser side effect; call it as a handler statement, \
                         not inside a value expression",
                        spec.usage
                    ),
                ));
            }
        })
    }

    pub(super) fn lower_browser_read_call(
        &mut self,
        call: &ExprCall,
        kind: BrowserReadCallKind,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        self.require_client_origin(call.span(), ClientOriginKind::BrowserRead)?;
        match kind {
            BrowserReadCallKind::Location(field) => {
                expect_call_arg_count(call, 0, "rsjs::location::<field>()")?;
                Ok(FrontendExpr::BrowserRead(BrowserRead::Location(field)))
            }
            BrowserReadCallKind::UrlSearchParam => {
                expect_call_arg_count(call, 1, "rsjs::url::search_param(key)")?;
                let key = self.lower_reactive_expr_in(&call.args[0], scope)?;
                Ok(FrontendExpr::BrowserRead(BrowserRead::UrlSearchParam {
                    key: Box::new(key),
                    fallback: None,
                }))
            }
            BrowserReadCallKind::UrlSearchParamOr => {
                expect_call_arg_count(call, 2, "rsjs::url::search_param_or(key, fallback)")?;
                let key = self.lower_reactive_expr_in(&call.args[0], scope)?;
                let fallback = self.lower_reactive_expr_in(&call.args[1], scope)?;
                Ok(FrontendExpr::BrowserRead(BrowserRead::UrlSearchParam {
                    key: Box::new(key),
                    fallback: Some(Box::new(fallback)),
                }))
            }
        }
    }

    pub(super) fn unsupported_client_prelude_alias_error(
        &self,
        span: proc_macro2::Span,
        local_name: &str,
        dependency_name: &str,
    ) -> syn::Error {
        let mut err = syn::Error::new(
            span,
            format!(
                "`let {local_name} = ...` depends on rsjs client state `{dependency_name}` but \
                 does not lower to supported Client IR"
            ),
        );
        err.combine(syn::Error::new(
            span,
            "move this logic into a standalone helper annotated once with `#[rsjs::client]`, \
             render the supported client expression directly, or keep it in a server-only \
             position before touching rsjs client primitives",
        ));
        err
    }

    pub(super) fn lower_method_call(
        &mut self,
        mc: &ExprMethodCall,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        let method = mc.method.to_string();
        if let Some(id) = self.query_receiver(&mc.receiver) {
            if let Some(accessor) = query_accessor_method(&method) {
                expect_arg_count(mc, 0, &format!("query.{method}()"))?;
                return Ok(FrontendExpr::QueryRead { id, accessor });
            }
            if matches!(method.as_str(), "refetch" | "invalidate") {
                return Err(syn::Error::new(
                    mc.method.span(),
                    format!(
                        "`query.{method}()` is a handler statement, not a value; call it as a \
                         standalone statement inside an event handler"
                    ),
                ));
            }
            return Err(unknown_query_method_error(mc.method.span(), &method));
        }
        if let Some(id) = self.mutation_receiver(&mc.receiver) {
            if let Some(accessor) = mutation_accessor_method(&method) {
                expect_arg_count(mc, 0, &format!("mutation.{method}()"))?;
                return Ok(FrontendExpr::MutationRead { id, accessor });
            }
            if method == "run" {
                return Err(syn::Error::new(
                    mc.method.span(),
                    "`mutation.run(args)` is a handler statement, not a value; call it as a \
                     standalone statement inside an event handler",
                ));
            }
            return Err(unknown_mutation_method_error(mc.method.span(), &method));
        }
        if receiver_is_event_current_target(&mc.receiver, scope.event_param) {
            if let Some(value) = current_target_value_method(&method) {
                expect_arg_count(mc, 0, &format!("event.current_target().{method}()"))?;
                return Ok(FrontendExpr::EventValue(value));
            }
            if let Some(spec) = rsjs_ir::dom_registry::find_current_target_read(&method) {
                let accessor = dom_read_accessor_for_call(mc, spec, "event.current_target()")?;
                return Ok(FrontendExpr::DomRead {
                    target: Box::new(FrontendExpr::EventCurrentTarget),
                    accessor,
                });
            }
            return Err(unknown_current_target_method_error(
                mc.method.span(),
                &method,
            ));
        }
        if let Some(id) = handler_dom_local_receiver(&mc.receiver, scope.handler_locals) {
            if let Some(spec) = rsjs_ir::dom_registry::find_current_target_read(&method) {
                let accessor = dom_read_accessor_for_call(mc, spec, "current-target handle")?;
                return Ok(FrontendExpr::DomRead {
                    target: Box::new(FrontendExpr::Local(id)),
                    accessor,
                });
            }
            return Err(unknown_current_target_method_error(
                mc.method.span(),
                &method,
            ));
        }
        if method == "at" {
            expect_arg_count(mc, 1, "value.at(json_pointer)")?;
            let path = expect_static_json_pointer_arg(&mc.args[0])?;
            let target = self.lower_reactive_expr_in(&mc.receiver, scope)?;
            return Ok(FrontendExpr::Project {
                target: Box::new(target),
                path,
            });
        }
        if let Some(ty) = as_type_method(&method) {
            expect_arg_count(mc, 0, &format!("value.{method}()"))?;
            let target = self.lower_reactive_expr_in(&mc.receiver, scope)?;
            return Ok(FrontendExpr::AsType {
                target: Box::new(target),
                ty,
            });
        }
        if method == "is_present" {
            expect_arg_count(mc, 0, "value.is_present()")?;
            let target = self.lower_reactive_expr_in(&mc.receiver, scope)?;
            return Ok(FrontendExpr::IsPresent {
                target: Box::new(target),
            });
        }
        if method == "eq" || method == "neq" {
            expect_arg_count(mc, 1, "value.eq(literal)")?;
            let target = self.lower_reactive_expr_in(&mc.receiver, scope)?;
            let value = projection_literal_arg(&mc.args[0])?;
            return if method == "eq" {
                Ok(FrontendExpr::Eq {
                    target: Box::new(target),
                    value,
                })
            } else {
                Ok(FrontendExpr::Neq {
                    target: Box::new(target),
                    value,
                })
            };
        }
        if method == "coalesce" {
            expect_arg_count(mc, 1, "value.coalesce(literal)")?;
            let target = self.lower_reactive_expr_in(&mc.receiver, scope)?;
            let fallback = projection_literal_arg(&mc.args[0])?;
            return Ok(FrontendExpr::Coalesce {
                target: Box::new(target),
                fallback,
            });
        }
        if receiver_is_event_param(&mc.receiver, scope.event_param) {
            if method == "current_target" {
                expect_arg_count(mc, 0, "event.current_target()")?;
                return Err(syn::Error::new(
                    mc.span(),
                    "`event.current_target()` is an opaque handler-local DOM handle; bind it \
                     first with `let element = event.current_target();`, then call a supported \
                     scalar read such as `element.value()`",
                ));
            }
            if let Some(source) = event_attribute_method(&method) {
                let attr = expect_static_event_attribute_arg(mc, &method)?;
                return Ok(FrontendExpr::EventAttribute { source, attr });
            }
            if matches!(method.as_str(), "target_inside" | "target_outside") {
                let id = self.expect_node_ref_event_target_arg(mc, &method)?;
                return Ok(FrontendExpr::EventTargetWithinNodeRef {
                    id,
                    inside: method == "target_inside",
                });
            }
            if let Some(collection) = event_collection_len_method(&method) {
                expect_arg_count(mc, 0, &format!("event.{method}()"))?;
                return Ok(FrontendExpr::EventCollectionLen(collection));
            }
            if let Some((collection, field)) = event_collection_read_method(&method) {
                expect_arg_count(mc, 1, &format!("event.{method}(index)"))?;
                let index = self.lower_reactive_expr_in(&mc.args[0], scope)?;
                return Ok(FrontendExpr::EventCollectionRead {
                    collection,
                    index: Box::new(index),
                    field,
                });
            }
            if let Some(expr) = event_cmd_or_ctrl_shortcut_expr(mc, &method)? {
                return Ok(expr);
            }
            if let Some(value) = event_value_method(&method) {
                if !mc.args.is_empty() {
                    return Err(syn::Error::new(
                        mc.span(),
                        format!("`event.{method}()` takes no arguments"),
                    ));
                }
                return Ok(FrontendExpr::EventValue(value));
            }
            if event_effect_method(&method) {
                return Err(syn::Error::new(
                    mc.method.span(),
                    format!(
                        "`event.{method}()` is a handler statement, not a value; \
                        use it as a standalone statement inside the handler block"
                    ),
                ));
            }
            if matches!(method.as_str(), "prevent_default" | "stop_propagation") {
                return Err(syn::Error::new(
                    mc.method.span(),
                    format!(
                        "`event.{method}()` is a handler statement, not a value; \
                        use it as a standalone statement inside the handler block"
                    ),
                ));
            }
            return Err(syn::Error::new(
                mc.method.span(),
                format!(
                    "`#[rsjs]` does not recognise event method `.{method}()`; \
                     supported event value accessors include `type_()`, `value()`, \
                     `checked()`, `target_value()`, `target_checked()`, \
                     `target_date()`, `target_id()`, `current_target_id()`, \
                     `current_value()`, `current_checked()`, `key()`, \
                     `code()`, `repeat()`, modifier keys, `is_cmd_or_ctrl(\"k\")`, mouse/wheel coordinates, \
                     button fields, pointer fields, scroll fields, input-event/toggle-state fields, storage-event fields, clipboard/data-transfer/file fields, changed-touch fields, indexed touch/file/data-transfer item fields, target containment \
                     via `target_inside(ref)` / `target_outside(ref)`, and static attribute reads \
                     like `target_attr(\"data-id\")`, `closest_attr(\"data-id\")`, \
                     and `current_target_attr(\"data-id\")`. \
                     Event statement methods are `prevent_default()`, \
                     `stop_propagation()`, `target_first_file_text_into(&signal)`, \
                     and `data_transfer_first_file_text_into(&signal)`."
                ),
            ));
        }
        let recognized_node_ref_method = rsjs_ir::dom_registry::find_node_ref_read(&method)
            .is_some()
            || rsjs_ir::dom_registry::find_node_ref_effect(&method).is_some();
        if let Some(id) = self.node_ref_receiver(&mc.receiver, recognized_node_ref_method)? {
            if let Some(spec) = rsjs_ir::dom_registry::find_node_ref_read(&method) {
                let accessor = dom_read_accessor_for_call(mc, spec, "node_ref")?;
                return Ok(FrontendExpr::NodeRefRead { id, accessor });
            }
            if rsjs_ir::dom_registry::find_node_ref_effect(&method).is_some() {
                return Err(syn::Error::new(
                    mc.method.span(),
                    format!(
                        "`node_ref.{method}()` is a handler statement, not a value; \
                         use it as a standalone statement inside the handler block"
                    ),
                ));
            }
            return Err(unknown_node_ref_method_error(mc.method.span(), &method));
        }
        match method.as_str() {
            "get" => {
                let id = self.expect_signal_receiver(&mc.receiver, "get")?;
                if !mc.args.is_empty() {
                    return Err(syn::Error::new(
                        mc.span(),
                        "`signal.get()` takes no arguments",
                    ));
                }
                Ok(FrontendExpr::SignalRead(id))
            }
            "is_non_empty" => {
                if !mc.args.is_empty() {
                    return Err(syn::Error::new(
                        mc.span(),
                        "`is_non_empty()` takes no arguments",
                    ));
                }
                let inner = self.lower_reactive_expr_in(&mc.receiver, scope)?;
                Ok(FrontendExpr::IsNonEmpty {
                    target: Box::new(inner),
                })
            }
            "to_lowercase" => {
                if !mc.args.is_empty() {
                    return Err(syn::Error::new(
                        mc.span(),
                        "`to_lowercase()` takes no arguments",
                    ));
                }
                let inner = self.lower_reactive_expr_in(&mc.receiver, scope)?;
                Ok(FrontendExpr::StringToLowercase {
                    target: Box::new(inner),
                })
            }
            "contains" => {
                expect_arg_count(mc, 1, "value.contains(needle)")?;
                let haystack = self.lower_reactive_expr_in(&mc.receiver, scope)?;
                let needle = self.lower_reactive_expr_in(&mc.args[0], scope)?;
                Ok(FrontendExpr::StringContains {
                    haystack: Box::new(haystack),
                    needle: Box::new(needle),
                })
            }
            "to_uppercase" => {
                if !mc.args.is_empty() {
                    return Err(syn::Error::new(
                        mc.span(),
                        "`to_uppercase()` takes no arguments",
                    ));
                }
                let inner = self.lower_reactive_expr_in(&mc.receiver, scope)?;
                Ok(FrontendExpr::StringTransform {
                    op: StringTransformOp::ToUppercase,
                    target: Box::new(inner),
                })
            }
            "trim" => {
                if !mc.args.is_empty() {
                    return Err(syn::Error::new(mc.span(), "`trim()` takes no arguments"));
                }
                let inner = self.lower_reactive_expr_in(&mc.receiver, scope)?;
                Ok(FrontendExpr::StringTransform {
                    op: StringTransformOp::Trim,
                    target: Box::new(inner),
                })
            }
            "starts_with" => {
                expect_arg_count(mc, 1, "value.starts_with(prefix)")?;
                let haystack = self.lower_reactive_expr_in(&mc.receiver, scope)?;
                let needle = self.lower_reactive_expr_in(&mc.args[0], scope)?;
                Ok(FrontendExpr::StringPredicate {
                    op: StringPredicateOp::StartsWith,
                    haystack: Box::new(haystack),
                    needle: Box::new(needle),
                })
            }
            "ends_with" => {
                expect_arg_count(mc, 1, "value.ends_with(suffix)")?;
                let haystack = self.lower_reactive_expr_in(&mc.receiver, scope)?;
                let needle = self.lower_reactive_expr_in(&mc.args[0], scope)?;
                Ok(FrontendExpr::StringPredicate {
                    op: StringPredicateOp::EndsWith,
                    haystack: Box::new(haystack),
                    needle: Box::new(needle),
                })
            }
            "is_empty" => {
                if !mc.args.is_empty() {
                    return Err(syn::Error::new(
                        mc.span(),
                        "`is_empty()` takes no arguments",
                    ));
                }
                // No dedicated IR variant: lower exactly like
                // `!x.is_non_empty()`, i.e. `<IsNonEmpty> == false`,
                // so the IR stays in the existing BinaryOp surface.
                let inner = self.lower_reactive_expr_in(&mc.receiver, scope)?;
                Ok(FrontendExpr::BinaryOp {
                    op: BinOp::Eq,
                    lhs: Box::new(FrontendExpr::IsNonEmpty {
                        target: Box::new(inner),
                    }),
                    rhs: Box::new(FrontendExpr::Literal(Value::Bool(false))),
                })
            }
            _ => {
                let expr = Expr::MethodCall(mc.clone());
                if self.can_lower_node_payload(&expr, scope) {
                    self.lower_node_payload_or_error(&expr, scope)
                } else if is_iterator_like_method(&method) {
                    Err(guided_boundary_error(
                        mc.method.span(),
                        format!(
                            "iterator or collection method `.{method}` cannot be lowered to client JS yet"
                        ),
                        "Iterator adapters combine Rust closure semantics, collection ownership, and dynamic \
                         iteration. The current Client IR keeps those semantics explicit instead of guessing a \
                         JavaScript rewrite.",
                        [
                            "Rewrite the handler using supported static array/range or numeric-range `for` loops when that matches the shape.",
                            "Move the collection transform into a standalone helper annotated once with `#[rsjs::client]`; reachable calls are linked automatically.",
                            "If it does not need client updates, precompute it on the server and render it through `rsjs::server_only(...)`.",
                        ],
                        Some("iterator.map"),
                    ))
                } else {
                    Err(syn::Error::new(
                        mc.method.span(),
                        format!(
                            "`#[rsjs]` does not recognise the method `.{method}` in a reactive expression; \
                             v0.1 supports `.get()`, `.is_non_empty()`, `.is_empty()`, `.to_lowercase()`, \
                             `.to_uppercase()`, `.trim()`, `.contains(...)`, `.starts_with(...)`, \
                             and `.ends_with(...)`"
                        ),
                    ))
                }
            }
        }
    }

    pub(super) fn lower_field_access(
        &mut self,
        field: &syn::ExprField,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        let target = self.lower_reactive_expr_in(&field.base, scope)?;
        if !is_projection_field_base(&target) {
            return Err(syn::Error::new(
                field.member.span(),
                "`#[rsjs]` only supports `.field` projection on typed client objects \
                 (shared fields, handler locals, or query/mutation data); this value has \
                 no structural client schema",
            ));
        }
        let segment = projection_field_segment(&field.member)?;
        Ok(project_field_segment(target, segment))
    }

    pub(super) fn lower_index_access(
        &mut self,
        index: &syn::ExprIndex,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        let target = self.lower_reactive_expr_in(&index.expr, scope)?;
        if !is_projection_field_base(&target) {
            return Err(syn::Error::new(
                index.bracket_token.span.join(),
                "`#[rsjs]` only supports `[index]` projection on typed client arrays \
                 (shared fields, handler locals, or query/mutation data); this value has \
                 no structural client schema",
            ));
        }
        let segment = projection_index_segment(&index.index)?;
        Ok(project_field_segment(target, segment))
    }

    pub(super) fn can_lower_node_payload(&self, expr: &Expr, scope: &ExprScope<'_>) -> bool {
        scope.node_payload_target.is_some() && self.can_serialize_loop_payload(expr)
    }

    pub(super) fn can_serialize_loop_payload(&self, expr: &Expr) -> bool {
        self.expr_mentions_loop_local(expr)
            && self
                .reject_reactive_captures_in_opaque_boundary(expr, OpaqueBoundary::ServerOnly)
                .is_ok()
    }

    pub(super) fn can_lower_rooted_node_payload(&self, expr: &Expr, scope: &ExprScope<'_>) -> bool {
        self.can_lower_node_payload(expr, scope) && self.is_projection_rooted_in_loop_local(expr)
    }

    fn is_projection_rooted_in_loop_local(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Path(ExprPath { path, .. }) => path
                .get_ident()
                .is_some_and(|ident| self.is_loop_local_name(ident)),
            Expr::Field(field) => self.is_projection_rooted_in_loop_local(&field.base),
            Expr::Index(index) => self.is_projection_rooted_in_loop_local(&index.expr),
            Expr::Paren(paren) => self.is_projection_rooted_in_loop_local(&paren.expr),
            Expr::Group(group) => self.is_projection_rooted_in_loop_local(&group.expr),
            Expr::Reference(reference) => self.is_projection_rooted_in_loop_local(&reference.expr),
            Expr::Cast(cast) => self.is_projection_rooted_in_loop_local(&cast.expr),
            _ => false,
        }
    }

    pub(super) fn lower_binary(
        &mut self,
        bin: &ExprBinary,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        let op = match bin.op {
            syn::BinOp::Add(_) => BinOp::Add,
            syn::BinOp::Sub(_) => BinOp::Sub,
            syn::BinOp::Mul(_) => BinOp::Mul,
            syn::BinOp::Div(_) => BinOp::Div,
            syn::BinOp::Rem(_) => BinOp::Rem,
            syn::BinOp::Eq(_) => BinOp::Eq,
            syn::BinOp::Ne(_) => BinOp::Neq,
            syn::BinOp::Lt(_) => BinOp::Lt,
            syn::BinOp::Gt(_) => BinOp::Gt,
            syn::BinOp::Le(_) => BinOp::Le,
            syn::BinOp::Ge(_) => BinOp::Ge,
            syn::BinOp::And(_) => BinOp::And,
            syn::BinOp::Or(_) => BinOp::Or,
            other => {
                return Err(syn::Error::new(
                    bin.span(),
                    format!(
                        "`#[rsjs]` does not support binary operator `{:?}` in a reactive expression",
                        other
                    ),
                ));
            }
        };
        let mut lhs = self.lower_reactive_expr_in(&bin.left, scope)?;
        let mut rhs = self.lower_reactive_expr_in(&bin.right, scope)?;
        if matches!(op, BinOp::And | BinOp::Or) {
            self.require_bool_condition(&lhs, scope.handler_locals, bin.left.span())?;
            self.require_bool_condition(&rhs, scope.handler_locals, bin.right.span())?;
        }
        let op = if binop_takes_numeric_operands(op) {
            self.fixup_numeric_binary(
                op,
                &mut lhs,
                &mut rhs,
                integer_expr_source_hint(&bin.left),
                integer_expr_source_hint(&bin.right),
                scope.handler_locals,
                bin.span(),
            )?
        } else {
            op
        };
        Ok(FrontendExpr::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }

    /// Typing context for the island as built so far, with the given
    /// handler locals bound. This is the macro frontend's view of the
    /// whole-island typing pass (`rsjs_ir::analysis::typing`).
    pub(super) fn type_env<'s>(&'s self, locals: &[HandlerLocalBinding]) -> TypeEnv<'s> {
        let mut env = TypeEnv::new_with_queries(
            self.builder.signal_decls(),
            self.builder.query_decls(),
            Some(self.builder.expr_arena()),
        );
        for local in locals {
            env.bind_local(ClientLocalId(local.id.0), local.ty.clone());
            if let Some(domain) = local.integer_domain {
                env.bind_local_integer_domain(ClientLocalId(local.id.0), domain);
            }
        }
        env
    }

    pub(super) fn frontend_integer_domain(
        &self,
        expr: &FrontendExpr,
        locals: &[HandlerLocalBinding],
    ) -> Option<IntegerDomain> {
        let client =
            lower_handler_ir_expr_to_client_expr(expr, proc_macro2::Span::call_site()).ok()?;
        self.type_env(locals).integer_domain_of(&client)
    }

    /// Client value shape of a frontend expression under the
    /// whole-island typing pass. `Unknown` when the shape has no
    /// Client IR form (which cannot happen for analyzer-produced
    /// frontend expressions, but stays honest rather than panicking).
    pub(super) fn frontend_expr_type(
        &self,
        expr: &FrontendExpr,
        locals: &[HandlerLocalBinding],
    ) -> ClientType {
        match lower_handler_ir_expr_to_client_expr(expr, proc_macro2::Span::call_site()) {
            Ok(client) => self.type_env(locals).type_of(&client),
            Err(_) => ClientType::Unknown,
        }
    }

    /// Reject a condition when whole-island typing proves that it is not a
    /// Rust `bool`. Unknown and dynamically represented values remain
    /// accepted because the macro cannot soundly prove their Rust type.
    pub(super) fn require_bool_condition(
        &self,
        expr: &FrontendExpr,
        locals: &[HandlerLocalBinding],
        span: proc_macro2::Span,
    ) -> syn::Result<()> {
        let ty = self.frontend_expr_type(expr, locals);
        if matches!(ty, ClientType::Bool | ClientType::Unknown | ClientType::Dyn) {
            return Ok(());
        }
        Err(syn::Error::new(
            span,
            format!(
                "`#[rsjs]` conditions with a statically known type must be Rust `bool`, \
                 but this expression is `{ty:?}`; compare it explicitly"
            ),
        ))
    }

    /// Runtime numeric anchors of a frontend expression under the
    /// whole-island typing pass; see
    /// `rsjs_ir::analysis::typing::TypeEnv::numeric_anchors`.
    pub(super) fn frontend_numeric_anchors(
        &self,
        expr: &FrontendExpr,
        locals: &[HandlerLocalBinding],
    ) -> NumericAnchors {
        match lower_handler_ir_expr_to_client_expr(expr, proc_macro2::Span::call_site()) {
            Ok(client) => self.type_env(locals).numeric_anchors(&client),
            Err(_) => NumericAnchors::default(),
        }
    }

    /// Typed numeric fixups for one binary operation, driven by the
    /// whole-island typing pass. Used by [`Self::lower_binary`] and by
    /// compound-assignment desugaring.
    ///
    /// 1. **Exact integer-domain validation.** Fixed-width arithmetic and
    ///    comparisons first require one matching Rust domain (`i32`, `u32`,
    ///    `i64`, or `u64`). Unsuffixed literals inherit that domain; explicitly
    ///    different domains are rejected.
    /// 2. **Legacy representation rejection.** For schema-less/dynamic numeric
    ///    paths that do not carry a fixed-width domain, when one arithmetic operand is
    ///    anchored by a BigInt-backed runtime value and the other by a
    ///    Number-backed runtime value, no literal rewrite can make the
    ///    expression sound (JS throws `TypeError` on BigInt/Number
    ///    arithmetic), so it is rejected at compile time.
    /// 3. **Legacy literal representation coercion.** When exactly one
    ///    schema-less side is wide,
    ///    plain integer literals on the other side are promoted to
    ///    BigInt literals. Proven fixed-width expressions never use this
    ///    heuristic: their checked operator already records the exact domain.
    /// 4. **Truncating-division selection for non-fixed legacy values.** Rust integer `/`
    ///    truncates toward zero while JS `/` produces a fractional
    ///    Number, so `/` over two Number-backed integers lowers to
    ///    [`BinOp::DivInt`] (the JS emitter wraps it in
    ///    `Math.trunc`). BigInt operands keep the bare `/`: BigInt
    ///    division truncates natively and `Math.trunc` throws on a
    ///    BigInt argument. Anything `Unknown` keeps the plain `/`.
    pub(super) fn fixup_numeric_binary(
        &mut self,
        op: BinOp,
        lhs: &mut FrontendExpr,
        rhs: &mut FrontendExpr,
        lhs_source: Option<IntegerSource>,
        rhs_source: Option<IntegerSource>,
        locals: &[HandlerLocalBinding],
        span: proc_macro2::Span,
    ) -> syn::Result<BinOp> {
        if matches!(
            op,
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
        ) {
            self.validate_integer_comparison_domains(
                lhs,
                rhs,
                lhs_source.as_ref(),
                rhs_source.as_ref(),
                locals,
                span,
            )?;
        }

        if let Some(checked_op) = checked_integer_op(op) {
            let lhs_ty = self.frontend_expr_type(lhs, locals);
            let rhs_ty = self.frontend_expr_type(rhs, locals);
            let lhs_width = numeric_width(&lhs_ty);
            let rhs_width = numeric_width(&rhs_ty);

            if let Some(name) = explicit_unsupported_integer_source(lhs_source.as_ref())
                .or_else(|| self.frontend_unsupported_integer_source(lhs, locals))
                .or_else(|| explicit_unsupported_integer_source(rhs_source.as_ref()))
                .or_else(|| self.frontend_unsupported_integer_source(rhs, locals))
            {
                return Err(unsupported_integer_domain_error(span, &name));
            }

            let lhs_domain = explicit_supported_integer_source(lhs_source.as_ref())
                .or_else(|| self.frontend_integer_domain(lhs, locals));
            let rhs_domain = explicit_supported_integer_source(rhs_source.as_ref())
                .or_else(|| self.frontend_integer_domain(rhs, locals));
            let lhs_literal = integer_literal_only(lhs);
            let rhs_literal = integer_literal_only(rhs);
            let integer_shaped = lhs_domain.is_some()
                || rhs_domain.is_some()
                || lhs_literal
                || rhs_literal
                || matches!(
                    lhs_width,
                    Some(NumericWidth::NumberInt | NumericWidth::BigInt | NumericWidth::Dynamic)
                )
                || matches!(
                    rhs_width,
                    Some(NumericWidth::NumberInt | NumericWidth::BigInt | NumericWidth::Dynamic)
                );

            // Float arithmetic remains ordinary IEEE-754 arithmetic. Known
            // non-integer `+` (the existing String surface) is also outside
            // fixed-width integer semantics.
            let known_float = matches!(lhs_width, Some(NumericWidth::NumberFloat))
                && matches!(rhs_width, Some(NumericWidth::NumberFloat));
            let known_string_add = matches!(op, BinOp::Add)
                && matches!(lhs_ty, ClientType::String)
                && matches!(rhs_ty, ClientType::String);

            if integer_shaped && !known_float {
                if matches!(lhs_width, Some(NumericWidth::Dynamic))
                    || matches!(rhs_width, Some(NumericWidth::Dynamic))
                {
                    return Err(integer_domain_proof_error(
                        span,
                        "page-shared state does not expose its Rust integer width to this macro",
                    ));
                }

                let domain = match (lhs_domain, rhs_domain) {
                    (Some(lhs), Some(rhs)) if lhs == rhs => lhs,
                    (Some(lhs), Some(rhs)) => {
                        return Err(syn::Error::new(
                            span,
                            format!(
                                "rsjs/integer-domain-mismatch: this arithmetic mixes `{}` and `{}`; \
                                 use one Rust integer type on both operands",
                                lhs.as_str(),
                                rhs.as_str()
                            ),
                        ));
                    }
                    (Some(domain), None) if rhs_literal => domain,
                    (None, Some(domain)) if lhs_literal => domain,
                    (None, None) if lhs_literal && rhs_literal => IntegerDomain::I32,
                    _ => {
                        return Err(integer_domain_proof_error(
                            span,
                            "one or both operands have no proven fixed-width Rust integer domain",
                        ));
                    }
                };
                coerce_integer_literals_to_domain(lhs, domain, span)?;
                coerce_integer_literals_to_domain(rhs, domain, span)?;
                return Ok(BinOp::Checked {
                    op: checked_op,
                    domain,
                });
            }

            if !known_float && !known_string_add && (lhs_ty.is_unknown() || rhs_ty.is_unknown()) {
                return Err(integer_domain_proof_error(
                    span,
                    "the operand type is runtime-initialized or otherwise unknown",
                ));
            }
        }

        let lhs_anchors = self.frontend_numeric_anchors(lhs, locals);
        let rhs_anchors = self.frontend_numeric_anchors(rhs, locals);

        // Representation-pinned operands reach this compatibility path only
        // for comparisons. Fixed-width arithmetic over `Dyn` was rejected
        // above because BigInt-versus-Number does not prove Rust width,
        // signedness, or overflow behavior. Equality still needs `DynEq` /
        // `DynNe` to compare a seed-pinned BigInt and Number consistently.
        if lhs_anchors.dynamic || rhs_anchors.dynamic {
            return self.dyn_numeric_binary(op, lhs, rhs, lhs_anchors, rhs_anchors, locals, span);
        }

        let arithmetic = matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::DivInt | BinOp::Rem
        );
        if arithmetic
            && ((lhs_anchors.wide && rhs_anchors.number)
                || (lhs_anchors.number && rhs_anchors.wide))
        {
            return Err(self.mixed_width_error(lhs, rhs, lhs_anchors, locals, span));
        }

        // Literal width coercion. "Wide" here means the subtree
        // mentions BigInt at all (a BigInt literal or a BigInt-backed
        // read), matching the old conservative rewrite trigger.
        let lhs_wide = lhs_anchors.wide || contains_bigint_literal(lhs);
        let rhs_wide = rhs_anchors.wide || contains_bigint_literal(rhs);
        if lhs_wide != rhs_wide {
            let (wide_side, other, other_anchors) = if lhs_wide {
                (&mut *lhs, &mut *rhs, rhs_anchors)
            } else {
                (&mut *rhs, &mut *lhs, lhs_anchors)
            };
            if other_anchors.number {
                // The anchored-vs-anchored case was rejected above,
                // so a wide side meeting a Number anchor is
                // literal-only here.
                demote_bigint_literals(wide_side);
            } else {
                promote_int_literals_to_wide(other);
            }
        }

        if matches!(op, BinOp::Div) {
            let lhs_width = numeric_width(&self.frontend_expr_type(lhs, locals));
            let rhs_width = numeric_width(&self.frontend_expr_type(rhs, locals));
            if lhs_width == Some(NumericWidth::NumberInt)
                && rhs_width == Some(NumericWidth::NumberInt)
            {
                return Ok(BinOp::DivInt);
            }
        }
        Ok(op)
    }

    /// Preserve Rust's exact integer typing for comparisons as well as
    /// arithmetic. Handler source expressions are not executed by generated
    /// Rust, so allowing `i32_signal.get() < u32_signal.get()` through merely
    /// because both are JavaScript Numbers would silently accept invalid Rust.
    fn validate_integer_comparison_domains(
        &mut self,
        lhs: &mut FrontendExpr,
        rhs: &mut FrontendExpr,
        lhs_source: Option<&IntegerSource>,
        rhs_source: Option<&IntegerSource>,
        locals: &[HandlerLocalBinding],
        span: proc_macro2::Span,
    ) -> syn::Result<()> {
        let lhs_source = self.resolved_integer_source(lhs_source, lhs, locals);
        let rhs_source = self.resolved_integer_source(rhs_source, rhs, locals);

        if let Some(name) = explicit_unsupported_integer_source(lhs_source.as_ref())
            .or_else(|| explicit_unsupported_integer_source(rhs_source.as_ref()))
        {
            return Err(unsupported_integer_domain_error(span, &name));
        }

        let lhs_domain = match lhs_source {
            Some(IntegerSource::Supported(domain)) => Some(domain),
            _ => None,
        };
        let rhs_domain = match rhs_source {
            Some(IntegerSource::Supported(domain)) => Some(domain),
            _ => None,
        };
        let lhs_literal = integer_literal_only(lhs);
        let rhs_literal = integer_literal_only(rhs);

        let domain = match (lhs_domain, rhs_domain) {
            (Some(lhs), Some(rhs)) if lhs == rhs => Some(lhs),
            (Some(lhs), Some(rhs)) => {
                return Err(integer_domain_mismatch_error(span, lhs, rhs, "comparison"));
            }
            (Some(domain), None) if rhs_literal => Some(domain),
            (None, Some(domain)) if lhs_literal => Some(domain),
            (Some(domain), None) => {
                if self.contextualize_direct_client_helper_return(
                    rhs,
                    domain,
                    span,
                    "comparison right operand",
                )? {
                    Some(domain)
                } else {
                    return Err(integer_domain_proof_error(
                        span,
                        "a comparison operand has no matching fixed-width Rust integer domain",
                    ));
                }
            }
            (None, Some(domain)) => {
                if self.contextualize_direct_client_helper_return(
                    lhs,
                    domain,
                    span,
                    "comparison left operand",
                )? {
                    Some(domain)
                } else {
                    return Err(integer_domain_proof_error(
                        span,
                        "a comparison operand has no matching fixed-width Rust integer domain",
                    ));
                }
            }
            (None, None) => None,
        };
        if let Some(domain) = domain {
            coerce_integer_literals_to_domain(lhs, domain, span)?;
            coerce_integer_literals_to_domain(rhs, domain, span)?;
        }
        Ok(())
    }

    pub(super) fn resolved_integer_source(
        &self,
        explicit: Option<&IntegerSource>,
        expr: &FrontendExpr,
        locals: &[HandlerLocalBinding],
    ) -> Option<IntegerSource> {
        explicit.cloned().or_else(|| {
            self.frontend_unsupported_integer_source(expr, locals)
                .map(IntegerSource::Unsupported)
                .or_else(|| {
                    self.frontend_integer_domain(expr, locals)
                        .map(IntegerSource::Supported)
                })
        })
    }

    /// Apply an explicit fixed-width helper boundary to a direct shared-field
    /// read. Shared fields enter the analyzer as `Dyn`; recording the domain on
    /// the declaration both drives checked Client IR and makes the Rust emitter
    /// generate a zero-cost `&T` type witness against the derive-generated
    /// replay accessor. That witness is essential for handler-only expressions,
    /// whose original source body is not retained in the expanded impl.
    fn specialize_explicit_shared_integer_operand(
        &mut self,
        value: &FrontendExpr,
        domain: IntegerDomain,
        span: proc_macro2::Span,
        context: &str,
    ) -> syn::Result<()> {
        let FrontendExpr::SignalRead(id) = value else {
            return Ok(());
        };
        let is_shared = self
            .builder
            .signal_decls()
            .get(id.0 as usize)
            .is_some_and(|decl| decl.shared.is_some());
        if !is_shared {
            return Ok(());
        }
        if let Some(existing) = self.builder.signal_integer_domain(*id) {
            if existing != domain {
                return Err(integer_domain_mismatch_error(
                    span, domain, existing, context,
                ));
            }
            return Ok(());
        }
        self.builder.set_signal_integer_domain(*id, Some(domain));
        self.builder.set_signal_ty(*id, domain.client_type());
        Ok(())
    }

    /// Whether an already-lowered expression reads a page-scoped shared cell.
    ///
    /// Shared fields enter macro analysis without their derive-time Rust field
    /// type. An explicit integer boundary can safely specialize them because
    /// generated Rust emits a matching type witness, but that specialization
    /// is contextual—not an independent proof that a later bare `div_int`
    /// call may inherit. Walking the full expression keeps this rule stable
    /// for nested arithmetic and conditionals as well as direct reads.
    fn frontend_expr_contains_shared_signal(&self, expr: &FrontendExpr) -> bool {
        match expr {
            FrontendExpr::SignalRead(id) => self
                .builder
                .signal_decls()
                .get(id.0 as usize)
                .is_some_and(|decl| decl.shared.is_some()),
            FrontendExpr::BinaryOp { lhs, rhs, .. } => {
                self.frontend_expr_contains_shared_signal(lhs)
                    || self.frontend_expr_contains_shared_signal(rhs)
            }
            FrontendExpr::UnaryNeg { expr, .. }
            | FrontendExpr::IsNonEmpty { target: expr }
            | FrontendExpr::StringToLowercase { target: expr }
            | FrontendExpr::StringTransform { target: expr, .. }
            | FrontendExpr::Encoding { value: expr, .. }
            | FrontendExpr::Project { target: expr, .. }
            | FrontendExpr::AsType { target: expr, .. }
            | FrontendExpr::IsPresent { target: expr }
            | FrontendExpr::Eq { target: expr, .. }
            | FrontendExpr::Neq { target: expr, .. }
            | FrontendExpr::Coalesce { target: expr, .. } => {
                self.frontend_expr_contains_shared_signal(expr)
            }
            FrontendExpr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.frontend_expr_contains_shared_signal(condition)
                    || self.frontend_expr_contains_shared_signal(then_expr)
                    || self.frontend_expr_contains_shared_signal(else_expr)
            }
            FrontendExpr::StringContains { haystack, needle }
            | FrontendExpr::StringPredicate {
                haystack, needle, ..
            } => {
                self.frontend_expr_contains_shared_signal(haystack)
                    || self.frontend_expr_contains_shared_signal(needle)
            }
            FrontendExpr::EventCollectionRead { index, .. } => {
                self.frontend_expr_contains_shared_signal(index)
            }
            FrontendExpr::QueryKey(parts) => parts
                .iter()
                .any(|part| self.frontend_expr_contains_shared_signal(part)),
            FrontendExpr::BrowserRead(BrowserRead::Storage { key, fallback, .. })
            | FrontendExpr::BrowserRead(BrowserRead::UrlSearchParam { key, fallback }) => {
                self.frontend_expr_contains_shared_signal(key)
                    || fallback
                        .as_deref()
                        .is_some_and(|value| self.frontend_expr_contains_shared_signal(value))
            }
            FrontendExpr::BrowserRead(BrowserRead::Declared { args, .. })
            | FrontendExpr::ClientCall { args, .. }
            | FrontendExpr::Text { args, .. } => args
                .iter()
                .any(|arg| self.frontend_expr_contains_shared_signal(arg)),
            _ => false,
        }
    }

    /// Propagate an explicit helper domain into shared reads inside one numeric
    /// operand before ordinary recursive lowering. Without this contextual
    /// pass, `div_int::<i64>(shared.get() + 1_i64, rhs.get())` reaches the
    /// inner `+` while the shared read is still `Dyn` and is rejected before
    /// the outer boundary can validate it.
    ///
    /// We recurse only through value-producing numeric structure. Conditions
    /// are boolean, and casts/calls establish their own boundaries. Every
    /// specialized shared declaration receives a generated Rust `&T` witness,
    /// so a wrong explicit domain still fails even in a handler-only body.
    fn specialize_shared_reads_in_explicit_integer_expr(
        &mut self,
        expr: &Expr,
        domain: IntegerDomain,
        context: &str,
    ) -> syn::Result<()> {
        match expr {
            Expr::MethodCall(method) if method.method == "get" && method.args.is_empty() => {
                let id = self.expect_signal_receiver(&method.receiver, "get")?;
                self.specialize_explicit_shared_integer_operand(
                    &FrontendExpr::SignalRead(id),
                    domain,
                    method.span(),
                    context,
                )
            }
            Expr::Binary(binary) => {
                self.specialize_shared_reads_in_explicit_integer_expr(
                    &binary.left,
                    domain,
                    context,
                )?;
                self.specialize_shared_reads_in_explicit_integer_expr(
                    &binary.right,
                    domain,
                    context,
                )
            }
            Expr::Unary(unary) => {
                self.specialize_shared_reads_in_explicit_integer_expr(&unary.expr, domain, context)
            }
            Expr::Paren(paren) => {
                self.specialize_shared_reads_in_explicit_integer_expr(&paren.expr, domain, context)
            }
            Expr::Group(group) => {
                self.specialize_shared_reads_in_explicit_integer_expr(&group.expr, domain, context)
            }
            Expr::If(if_expr) => {
                if let Some(Stmt::Expr(then_expr, _)) = if_expr.then_branch.stmts.last() {
                    self.specialize_shared_reads_in_explicit_integer_expr(
                        then_expr, domain, context,
                    )?;
                }
                if let Some((_, else_expr)) = &if_expr.else_branch {
                    self.specialize_shared_reads_in_explicit_integer_expr(
                        else_expr, domain, context,
                    )?;
                }
                Ok(())
            }
            Expr::Block(block) => {
                if let Some(Stmt::Expr(tail, _)) = block.block.stmts.last() {
                    self.specialize_shared_reads_in_explicit_integer_expr(tail, domain, context)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Validate a value written into an already-typed fixed-width integer
    /// location. Unsuffixed literal-only values inherit the target domain;
    /// explicit suffixes and anchored expressions must match exactly.
    pub(super) fn validate_integer_value_for_domain(
        &mut self,
        value: &mut FrontendExpr,
        explicit_source: Option<IntegerSource>,
        expected: IntegerDomain,
        locals: &[HandlerLocalBinding],
        span: proc_macro2::Span,
        context: &str,
        allow_unproven_typed_boundary: bool,
    ) -> syn::Result<()> {
        validate_explicit_integer_source_against_domain(
            explicit_source.as_ref(),
            expected,
            span,
            context,
        )?;
        if let Some(name) = self.frontend_unsupported_integer_source(value, locals) {
            return Err(unsupported_integer_domain_error(span, &name));
        }
        let explicit_domain = explicit_supported_integer_source(explicit_source.as_ref());
        let inferred_domain = self.frontend_integer_domain(value, locals);
        if let (Some(explicit), Some(inferred)) = (explicit_domain, inferred_domain) {
            if explicit != inferred {
                return Err(integer_domain_mismatch_error(
                    span, explicit, inferred, context,
                ));
            }
        }
        let actual = explicit_domain.or(inferred_domain);
        match actual {
            Some(actual) if actual != expected => {
                return Err(integer_domain_mismatch_error(
                    span, actual, expected, context,
                ));
            }
            Some(_) => {
                coerce_integer_literals_to_domain(value, expected, span)?;
                return Ok(());
            }
            None if integer_literal_only(value) && explicit_source.is_none() => {
                coerce_integer_literals_to_domain(value, expected, span)?;
                return Ok(());
            }
            None => {}
        }

        if self.contextualize_direct_client_helper_return(value, expected, span, context)? {
            return Ok(());
        }

        let ty = self.frontend_expr_type(value, locals);
        if allow_unproven_typed_boundary && matches!(ty, ClientType::Unknown | ClientType::Dyn) {
            if frontend_expr_contains_client_call(value) {
                return Err(integer_domain_proof_error(
                    span,
                    &format!(
                        "{context} uses an imported client-helper result whose Rust return \
                         domain is not present in the island descriptor; keep the typed integer \
                         composition inside one standalone `#[rsjs::client]` helper"
                    ),
                ));
            }
            return Ok(());
        }
        if matches!(ty, ClientType::Unknown | ClientType::Dyn) {
            return Err(integer_domain_proof_error(
                span,
                &format!("{context} has an unproven runtime value"),
            ));
        }
        Err(syn::Error::new(
            span,
            format!(
                "rsjs/integer-domain-mismatch: {context} expects `{}` but the value has client type `{ty:?}`",
                expected.as_str()
            ),
        ))
    }

    /// Attach a checked identity operation to a value whose exact integer
    /// domain is declared at a typed boundary but whose payload/client-call
    /// metadata is otherwise opaque. This does not weaken unknown-domain
    /// arithmetic: the anchor exists only after explicit source typing and
    /// validates representation and bounds at runtime.
    pub(super) fn anchor_integer_domain(
        &self,
        value: FrontendExpr,
        domain: IntegerDomain,
    ) -> FrontendExpr {
        if matches!(
            &value,
            FrontendExpr::BinaryOp {
                op: BinOp::Checked {
                    op: CheckedIntegerOp::Add,
                    domain: existing,
                },
                rhs,
                ..
            } if *existing == domain
                && matches!(rhs.as_ref(),
                    FrontendExpr::Literal(Value::Int(0) | Value::BigInt(0)))
        ) {
            return value;
        }
        let zero = if domain.is_wide() {
            Value::BigInt(0)
        } else {
            Value::Int(0)
        };
        FrontendExpr::BinaryOp {
            op: BinOp::Checked {
                op: CheckedIntegerOp::Add,
                domain,
            },
            lhs: Box::new(value),
            rhs: Box::new(FrontendExpr::Literal(zero)),
        }
    }

    /// Give a direct standalone helper result the fixed-width domain supplied
    /// by an enclosing Rust-typed boundary.
    ///
    /// The proc macro cannot resolve an imported helper's signature. It may,
    /// however, use ordinary contextual typing exactly where Rust would: a
    /// signal write, typed handler local, callback payload, or comparison with
    /// a proven fixed-width operand. The checked identity preserves that
    /// domain in Client IR, while the relocation emission produces a Rust
    /// function-pointer witness against the actual imported function item.
    /// Unknown helper results remain rejected when no such boundary exists.
    fn contextualize_direct_client_helper_return(
        &mut self,
        value: &mut FrontendExpr,
        expected: IntegerDomain,
        span: proc_macro2::Span,
        context: &str,
    ) -> syn::Result<bool> {
        let FrontendExpr::ClientCall { edge, .. } = value else {
            return Ok(false);
        };
        let source = self
            .direct_helper_edges
            .get_mut(edge.index() as usize)
            .ok_or_else(|| {
                syn::Error::new(
                    span,
                    "RSJS internal error: client-helper call lost its source relocation edge",
                )
            })?;
        if let Some(actual) = source.expected_return_domain {
            if actual != expected {
                return Err(integer_domain_mismatch_error(
                    span, actual, expected, context,
                ));
            }
        } else {
            source.expected_return_domain = Some(expected);
        }

        let unanchored = core::mem::replace(value, FrontendExpr::Literal(Value::Null));
        *value = self.anchor_integer_domain(unanchored, expected);
        Ok(true)
    }

    /// Select the representation-aware comparison for an expression with at
    /// least one shared-state (runtime-pinned) operand.
    ///
    /// Ordering comparisons keep the bare JS operator: JavaScript already
    /// defines mixed BigInt/Number ordering exactly (`5n > 4` is `true`), so
    /// there is nothing for a helper to fix. Equality does *not*: `5n === 5` is
    /// `false` while Rust's `5 == 5` is `true`, so `==` and `!=` route through
    /// `num_eq` / `num_ne`.
    #[allow(clippy::too_many_arguments)]
    fn dyn_numeric_binary(
        &self,
        op: BinOp,
        _lhs: &FrontendExpr,
        _rhs: &FrontendExpr,
        _lhs_anchors: NumericAnchors,
        _rhs_anchors: NumericAnchors,
        _locals: &[HandlerLocalBinding],
        span: proc_macro2::Span,
    ) -> syn::Result<BinOp> {
        Ok(match op {
            BinOp::Eq => BinOp::DynEq,
            BinOp::Neq => BinOp::DynNe,
            // Ordering comparisons and the boolean connectives need no help.
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::And | BinOp::Or => op,
            _ => {
                return Err(integer_domain_proof_error(
                    span,
                    "page-shared state has no proven Rust integer width",
                ));
            }
        })
    }

    /// The rsjs diagnostic for an arithmetic expression that mixes a
    /// BigInt-backed operand with a Number-backed operand.
    fn mixed_width_error(
        &self,
        lhs: &FrontendExpr,
        rhs: &FrontendExpr,
        lhs_anchors: NumericAnchors,
        locals: &[HandlerLocalBinding],
        span: proc_macro2::Span,
    ) -> syn::Error {
        let (wide_expr, number_expr) = if lhs_anchors.wide {
            (lhs, rhs)
        } else {
            (rhs, lhs)
        };
        let wide_desc = self
            .find_numeric_anchor_description(wide_expr, true, locals)
            .unwrap_or_else(|| "a BigInt-backed value".to_string());
        let number_desc = self
            .find_numeric_anchor_description(number_expr, false, locals)
            .unwrap_or_else(|| "a Number-backed value".to_string());
        let mut message = String::new();
        message.push_str(
            "`#[rsjs]` cannot lower this arithmetic: it mixes a BigInt-backed wide integer \
             with a Number-backed value.\n\n",
        );
        message.push_str(&format!(
            "{wide_desc} is BigInt-backed on the client (it keeps exact integer semantics \
             past 2^53, hydrating as a JS BigInt), while {number_desc} is a plain JS Number. \
             JavaScript throws `TypeError: Cannot mix BigInt and other types` when an \
             arithmetic operator receives one BigInt and one Number, so this expression \
             would crash in the browser.\n\n",
        ));
        message.push_str(
            "help: keep both operands in one wide domain, for example \
             `signal::<i64>(0)` on both sides; i64/u64 are always BigInt-backed\n",
        );
        message.push_str(
            "help: keep both operands in one Number-backed domain such as i32, u32, or f64\n",
        );
        message.push_str(
            "help: or convert explicitly in a standalone `#[rsjs::client]` helper and call \
             it from this expression",
        );
        syn::Error::new(span, message)
    }

    /// First non-literal numeric leaf of the requested width inside a
    /// numeric subtree, described for diagnostics ("the signal
    /// `total`", "the handler local `count`", ...).
    fn find_numeric_anchor_description(
        &self,
        expr: &FrontendExpr,
        wide: bool,
        locals: &[HandlerLocalBinding],
    ) -> Option<String> {
        match expr {
            FrontendExpr::Literal(_) => None,
            FrontendExpr::BinaryOp {
                op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::DivInt | BinOp::Rem,
                lhs,
                rhs,
            } => self
                .find_numeric_anchor_description(lhs, wide, locals)
                .or_else(|| self.find_numeric_anchor_description(rhs, wide, locals)),
            FrontendExpr::Conditional {
                then_expr,
                else_expr,
                ..
            } => self
                .find_numeric_anchor_description(then_expr, wide, locals)
                .or_else(|| self.find_numeric_anchor_description(else_expr, wide, locals)),
            leaf => {
                let width = numeric_width(&self.frontend_expr_type(leaf, locals))?;
                // A runtime-pinned leaf is neither the BigInt side nor the
                // Number side of a mixed-width diagnostic; it is the side the
                // compiler declined to guess.
                if matches!(width, NumericWidth::Dynamic) {
                    return None;
                }
                let leaf_is_wide = matches!(width, NumericWidth::BigInt);
                if leaf_is_wide != wide {
                    return None;
                }
                Some(match leaf {
                    FrontendExpr::SignalRead(id) => match self.signal_name(*id) {
                        Some(name) => format!("the signal `{name}`"),
                        None => "this signal read".to_string(),
                    },
                    FrontendExpr::Local(id) => {
                        match locals.iter().rev().find(|local| local.id == *id) {
                            Some(local) => format!("the handler local `{}`", local.name),
                            None => "this handler local".to_string(),
                        }
                    }
                    FrontendExpr::EventValue(_)
                    | FrontendExpr::EventCollectionLen(_)
                    | FrontendExpr::EventCollectionRead { .. } => "this event read".to_string(),
                    FrontendExpr::QueryRead { .. } => "this query read".to_string(),
                    FrontendExpr::MutationRead { .. } => "this mutation read".to_string(),
                    FrontendExpr::NodeRefRead { .. } => "this node-ref read".to_string(),
                    FrontendExpr::BrowserRead(_) => "this browser read".to_string(),
                    _ => "this value".to_string(),
                })
            }
        }
    }

    /// Prelude name of a signal, for diagnostics.
    fn signal_name(&self, id: SignalId) -> Option<&str> {
        self.signals
            .iter()
            .find(|(_, signal_id)| **signal_id == id)
            .map(|(name, _)| name.as_str())
    }

    fn frontend_unsupported_integer_source(
        &self,
        expr: &FrontendExpr,
        locals: &[HandlerLocalBinding],
    ) -> Option<String> {
        match expr {
            FrontendExpr::SignalRead(id) => self.unsupported_integer_signals.get(id).cloned(),
            FrontendExpr::Local(id) => locals
                .iter()
                .rev()
                .find(|local| local.id == *id)
                .and_then(|local| local.unsupported_integer_domain.clone()),
            FrontendExpr::BinaryOp { lhs, rhs, .. } => self
                .frontend_unsupported_integer_source(lhs, locals)
                .or_else(|| self.frontend_unsupported_integer_source(rhs, locals)),
            FrontendExpr::UnaryNeg { expr, .. } => {
                self.frontend_unsupported_integer_source(expr, locals)
            }
            FrontendExpr::Conditional {
                then_expr,
                else_expr,
                ..
            } => self
                .frontend_unsupported_integer_source(then_expr, locals)
                .or_else(|| self.frontend_unsupported_integer_source(else_expr, locals)),
            _ => None,
        }
    }

    pub(super) fn lower_unary(
        &mut self,
        u: &ExprUnary,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        if let syn::UnOp::Not(_) = u.op {
            let inner = self.lower_reactive_expr_in(&u.expr, scope)?;
            self.require_bool_condition(&inner, scope.handler_locals, u.expr.span())?;
            return Ok(FrontendExpr::BinaryOp {
                op: BinOp::Eq,
                lhs: Box::new(inner),
                rhs: Box::new(FrontendExpr::Literal(Value::Bool(false))),
            });
        }
        if let syn::UnOp::Neg(_) = u.op {
            // Signed numeric literals are constants, not runtime arithmetic.
            // Parse them as one token-level value so `i64::MIN` remains
            // representable even though its positive magnitude exceeds MAX.
            if matches!(u.expr.as_ref(), Expr::Lit(ExprLit { .. })) {
                return literal_to_value(&Expr::Unary(u.clone())).map(FrontendExpr::Literal);
            }
            let inner = self.lower_reactive_expr_in(&u.expr, scope)?;
            if let Some(name) =
                self.frontend_unsupported_integer_source(&inner, scope.handler_locals)
            {
                return Err(unsupported_integer_domain_error(u.span(), &name));
            }
            let Some(domain) = self.frontend_integer_domain(&inner, scope.handler_locals) else {
                return Err(integer_domain_proof_error(
                    u.span(),
                    "unary negation has no proven signed Rust integer domain",
                ));
            };
            if !domain.is_signed() {
                return Err(syn::Error::new(
                    u.span(),
                    format!(
                        "rsjs/integer-negation-unsigned: unary `-` is not defined for `{}`",
                        domain.as_str()
                    ),
                ));
            }
            return Ok(FrontendExpr::UnaryNeg {
                domain,
                expr: Box::new(inner),
            });
        }
        Err(syn::Error::new(
            u.span(),
            "`#[rsjs]` supports unary `!` and checked signed integer `-` in reactive expressions",
        ))
    }
}

fn handler_dom_local_receiver(receiver: &Expr, locals: &[HandlerLocalBinding]) -> Option<LocalId> {
    match receiver {
        Expr::Path(path) => {
            let name = path.path.get_ident()?.to_string();
            locals
                .iter()
                .rev()
                .find(|local| local.name == name && local.ty == ClientType::DomElement)
                .map(|local| local.id)
        }
        Expr::Paren(paren) => handler_dom_local_receiver(&paren.expr, locals),
        Expr::Group(group) => handler_dom_local_receiver(&group.expr, locals),
        _ => None,
    }
}

fn frontend_expr_contains_client_call(expr: &FrontendExpr) -> bool {
    match expr {
        FrontendExpr::ClientCall { .. } => true,
        FrontendExpr::BinaryOp { lhs, rhs, .. } => {
            frontend_expr_contains_client_call(lhs) || frontend_expr_contains_client_call(rhs)
        }
        FrontendExpr::UnaryNeg { expr, .. }
        | FrontendExpr::IsNonEmpty { target: expr }
        | FrontendExpr::StringToLowercase { target: expr }
        | FrontendExpr::StringTransform { target: expr, .. }
        | FrontendExpr::Encoding { value: expr, .. }
        | FrontendExpr::Project { target: expr, .. }
        | FrontendExpr::AsType { target: expr, .. }
        | FrontendExpr::IsPresent { target: expr }
        | FrontendExpr::Eq { target: expr, .. }
        | FrontendExpr::Neq { target: expr, .. }
        | FrontendExpr::Coalesce { target: expr, .. } => frontend_expr_contains_client_call(expr),
        FrontendExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            frontend_expr_contains_client_call(condition)
                || frontend_expr_contains_client_call(then_expr)
                || frontend_expr_contains_client_call(else_expr)
        }
        FrontendExpr::StringContains { haystack, needle }
        | FrontendExpr::StringPredicate {
            haystack, needle, ..
        } => {
            frontend_expr_contains_client_call(haystack)
                || frontend_expr_contains_client_call(needle)
        }
        FrontendExpr::EventCollectionRead { index, .. } => {
            frontend_expr_contains_client_call(index)
        }
        FrontendExpr::QueryKey(parts) | FrontendExpr::Text { args: parts, .. } => {
            parts.iter().any(frontend_expr_contains_client_call)
        }
        FrontendExpr::BrowserRead(BrowserRead::Storage { key, fallback, .. })
        | FrontendExpr::BrowserRead(BrowserRead::UrlSearchParam { key, fallback }) => {
            frontend_expr_contains_client_call(key)
                || fallback
                    .as_deref()
                    .is_some_and(frontend_expr_contains_client_call)
        }
        FrontendExpr::BrowserRead(BrowserRead::Declared { args, .. }) => {
            args.iter().any(frontend_expr_contains_client_call)
        }
        _ => false,
    }
}

pub(super) fn integer_expr_source_hint(expr: &Expr) -> Option<IntegerSource> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(value),
            ..
        }) if !value.suffix().is_empty() => {
            let name = value.suffix().to_string();
            Some(
                IntegerDomain::from_name(&name)
                    .map(IntegerSource::Supported)
                    .unwrap_or(IntegerSource::Unsupported(name)),
            )
        }
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Neg(_)) => {
            integer_expr_source_hint(&unary.expr)
        }
        Expr::Paren(paren) => integer_expr_source_hint(&paren.expr),
        Expr::Group(group) => integer_expr_source_hint(&group.expr),
        Expr::Cast(cast) => rust_type_leaf_name(&cast.ty).and_then(|name| {
            is_rust_integer_type(&name).then(|| {
                IntegerDomain::from_name(&name)
                    .map(IntegerSource::Supported)
                    .unwrap_or(IntegerSource::Unsupported(name))
            })
        }),
        _ => None,
    }
}

pub(super) fn explicit_supported_integer_source(
    source: Option<&IntegerSource>,
) -> Option<IntegerDomain> {
    match source {
        Some(IntegerSource::Supported(domain)) => Some(*domain),
        _ => None,
    }
}

pub(super) fn explicit_unsupported_integer_source(
    source: Option<&IntegerSource>,
) -> Option<String> {
    match source {
        Some(IntegerSource::Unsupported(name)) => Some(name.clone()),
        _ => None,
    }
}

fn checked_integer_op(op: BinOp) -> Option<CheckedIntegerOp> {
    Some(match op {
        BinOp::Add => CheckedIntegerOp::Add,
        BinOp::Sub => CheckedIntegerOp::Sub,
        BinOp::Mul => CheckedIntegerOp::Mul,
        BinOp::Div | BinOp::DivInt => CheckedIntegerOp::Div,
        BinOp::Rem => CheckedIntegerOp::Rem,
        _ => return None,
    })
}

pub(super) fn integer_literal_only(expr: &FrontendExpr) -> bool {
    match expr {
        FrontendExpr::Literal(Value::Int(_) | Value::BigInt(_)) => true,
        FrontendExpr::Conditional {
            then_expr,
            else_expr,
            ..
        } => integer_literal_only(then_expr) && integer_literal_only(else_expr),
        _ => false,
    }
}

pub(super) fn integer_domain_proof_error(span: proc_macro2::Span, detail: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "rsjs/integer-domain-unproven: cannot lower this fixed-width integer arithmetic: \
             {detail}. Add an explicit supported type (`i32`, `u32`, `i64`, or `u64`) to the \
             signal/local, or move the operation to server Rust"
        ),
    )
}

pub(super) fn integer_domain_mismatch_error(
    span: proc_macro2::Span,
    actual: IntegerDomain,
    expected: IntegerDomain,
    context: &str,
) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "rsjs/integer-domain-mismatch: {context} mixes `{}` and `{}`; use one exact Rust integer type",
            actual.as_str(),
            expected.as_str()
        ),
    )
}

/// Validate source-level integer evidence before lowering can erase it.
///
/// Typed boundaries call this on the recursively scanned `syn::Expr` before
/// lowering. They call [`Analyzer::validate_integer_value_for_domain`] again
/// afterward for inferred Client IR domains and literal coercion.
pub(super) fn validate_explicit_integer_source_against_domain(
    source: Option<&IntegerSource>,
    expected: IntegerDomain,
    span: proc_macro2::Span,
    context: &str,
) -> syn::Result<()> {
    match source {
        Some(IntegerSource::Supported(actual)) if *actual != expected => Err(
            integer_domain_mismatch_error(span, *actual, expected, context),
        ),
        Some(IntegerSource::Unsupported(name)) => Err(unsupported_integer_domain_error(span, name)),
        _ => Ok(()),
    }
}

pub(super) fn unsupported_integer_domain_error(span: proc_macro2::Span, name: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "rsjs/integer-domain-unsupported: `{name}` arithmetic is not yet supported by \
             checked client lowering; use `i32`, `u32`, `i64`, or `u64`, or perform this \
             operation in server Rust"
        ),
    )
}

pub(super) fn coerce_integer_literals_to_domain(
    expr: &mut FrontendExpr,
    domain: IntegerDomain,
    span: proc_macro2::Span,
) -> syn::Result<()> {
    match expr {
        FrontendExpr::Literal(value @ (Value::Int(_) | Value::BigInt(_))) => {
            let integer = match value {
                Value::Int(value) => i128::from(*value),
                Value::BigInt(value) => *value,
                _ => unreachable!("pattern restricts integer values"),
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
                        "rsjs/integer-literal-out-of-range: `{integer}` is outside the `{}` domain",
                        domain.as_str()
                    ),
                ));
            }
            *value = if domain.is_wide() {
                Value::BigInt(integer)
            } else {
                Value::Int(i64::try_from(integer).expect("i32/u32 integer always fits Value::Int"))
            };
        }
        FrontendExpr::BinaryOp { lhs, rhs, .. } => {
            coerce_integer_literals_to_domain(lhs, domain, span)?;
            coerce_integer_literals_to_domain(rhs, domain, span)?;
        }
        FrontendExpr::UnaryNeg { expr, .. } => {
            coerce_integer_literals_to_domain(expr, domain, span)?;
        }
        FrontendExpr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            coerce_integer_literals_to_domain(then_expr, domain, span)?;
            coerce_integer_literals_to_domain(else_expr, domain, span)?;
        }
        _ => {}
    }
    Ok(())
}

fn is_none_value_path(path: &syn::Path) -> bool {
    let segments = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    matches!(segments.as_slice(), [none] if none == "None")
        || matches!(segments.as_slice(), [option, none] if option == "Option" && none == "None")
        || matches!(
            segments.as_slice(),
            [root, option_module, option, none]
                if (root == "std" || root == "core")
                    && option_module == "option"
                    && option == "Option"
                    && none == "None"
        )
}

pub(super) fn literal_to_value(arg: &Expr) -> syn::Result<Value> {
    match arg {
        Expr::Lit(ExprLit { lit, .. }) => lit_to_value(lit, arg.span()),
        // Allow `String::new()` and `"".to_string()` as ergonomic
        // shorthands for `signal::<String>("")`. The runtime
        // representation is the same.
        Expr::Call(c) if path_ends_with(&c.func, "new") && is_string_path(&c.func) => {
            Ok(Value::String(String::new()))
        }
        Expr::MethodCall(mc) if mc.method == "to_string" => {
            if let Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            }) = &*mc.receiver
            {
                Ok(Value::String(s.value()))
            } else {
                Err(syn::Error::new(
                    mc.span(),
                    "`signal(...)` initial values must be literal; \
                     only `<lit>.to_string()` and `String::new()` shorthands are recognised",
                ))
            }
        }
        Expr::Unary(u) => {
            if let syn::UnOp::Neg(_) = u.op {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Int(li), ..
                }) = u.expr.as_ref()
                {
                    let n: i128 = li.base10_parse().map_err(|e| {
                        syn::Error::new(
                            u.span(),
                            format!("`signal(...)` integer literal out of range: {e}"),
                        )
                    })?;
                    return checked_int_literal_value(-n, li.suffix(), u.span());
                }
                if let Expr::Lit(ExprLit {
                    lit: Lit::Float(lf),
                    ..
                }) = u.expr.as_ref()
                {
                    let n: f64 = lf.base10_parse().map_err(|e| {
                        syn::Error::new(
                            u.span(),
                            format!("`signal(...)` float literal out of range: {e}"),
                        )
                    })?;
                    return Ok(Value::Float(-n));
                }
            }
            Err(syn::Error::new(
                u.span(),
                "`signal(...)` initial value must be a literal",
            ))
        }
        other => Err(syn::Error::new(
            other.span(),
            "`signal(...)` initial value must be a literal \
             (`0`, `\"\"`, `false`, etc.) or `self.<field>`",
        )),
    }
}

/// Returns `true` for the supported deterministic BigInt domains.
///
/// Other wide-looking Rust types (`i128`, `u128`, `isize`, `usize`) are not
/// silently treated as client BigInts: they are outside the accepted checked
/// domain set and produce targeted diagnostics when used client-side.
pub(super) fn is_wide_int_suffix(suffix: &str) -> bool {
    matches!(suffix, "i64" | "u64")
}

/// Choose the browser literal representation for a Rust integer token.
///
/// Explicit `i64`/`u64` literals are BigInts from the first lowering step,
/// including zero and values inside the JS safe-integer range. Unsuffixed and
/// narrow supported literals stay Numbers. Unsupported widths may use the
/// `BigInt` variant merely as an internal magnitude carrier when they do not
/// fit `i64`; source-domain validation still rejects them from client
/// arithmetic instead of granting them a supported wide domain.
pub(super) fn int_literal_value(n: i128, suffix: &str) -> Value {
    if is_wide_int_suffix(suffix) || i64::try_from(n).is_err() {
        Value::BigInt(n)
    } else {
        Value::Int(i64::try_from(n).expect("range checked above"))
    }
}

fn checked_int_literal_value(n: i128, suffix: &str, span: proc_macro2::Span) -> syn::Result<Value> {
    let in_range = match suffix {
        "" => true,
        "i8" => (i8::MIN as i128..=i8::MAX as i128).contains(&n),
        "i16" => (i16::MIN as i128..=i16::MAX as i128).contains(&n),
        "i32" => (i32::MIN as i128..=i32::MAX as i128).contains(&n),
        "i64" | "isize" => (i64::MIN as i128..=i64::MAX as i128).contains(&n),
        "u8" => (0..=u8::MAX as i128).contains(&n),
        "u16" => (0..=u16::MAX as i128).contains(&n),
        "u32" => (0..=u32::MAX as i128).contains(&n),
        "u64" | "usize" => (0..=u64::MAX as i128).contains(&n),
        // i128/u128 are outside the accepted arithmetic-domain set, but
        // literals still need deterministic parsing for diagnostics and
        // representation-only uses.
        "i128" => true,
        "u128" => n >= 0,
        _ => true,
    };
    if !in_range {
        return Err(syn::Error::new(
            span,
            format!("integer literal `{n}` is outside the `{suffix}` domain"),
        ));
    }
    Ok(int_literal_value(n, suffix))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RsjsDivisionHelper {
    Integer,
    Float,
}

/// Recognize `rsjs::div_int(a, b)` / `rsjs::div_f64(a, b)`.
///
/// The path must be qualified with `rsjs` so a linked user helper named
/// `div_int` remains distinct from the framework primitive.
fn rsjs_division_helper(path: &syn::Path) -> Option<RsjsDivisionHelper> {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let borrowed: Vec<&str> = segments.iter().map(String::as_str).collect();
    let [.., "rsjs", name] = borrowed.as_slice() else {
        return None;
    };
    match *name {
        "div_int" => Some(RsjsDivisionHelper::Integer),
        "div_f64" => Some(RsjsDivisionHelper::Float),
        _ => None,
    }
}

fn div_int_explicit_integer_source(path: &syn::Path) -> syn::Result<Option<IntegerSource>> {
    let Some(segment) = path.segments.last() else {
        return Ok(None);
    };
    match &segment.arguments {
        syn::PathArguments::None => Ok(None),
        syn::PathArguments::AngleBracketed(arguments) => {
            let mut args = arguments.args.iter();
            let Some(syn::GenericArgument::Type(ty)) = args.next() else {
                return Err(syn::Error::new(
                    arguments.span(),
                    "rsjs/integer-domain-unproven: `rsjs::div_int::<T>(a, b)` requires exactly one Rust integer type argument (`i32`, `u32`, `i64`, or `u64`)",
                ));
            };
            if args.next().is_some() {
                return Err(syn::Error::new(
                    arguments.span(),
                    "rsjs/integer-domain-unproven: `rsjs::div_int::<T>(a, b)` takes exactly one Rust integer type argument",
                ));
            }
            let Some(name) = exact_div_int_primitive_type_name(ty) else {
                return Err(syn::Error::new(
                    ty.span(),
                    "rsjs/integer-domain-unproven: `rsjs::div_int::<T>(a, b)` requires `T` to be the primitive type `i32`, `u32`, `i64`, or `u64`; references, wrappers, aliases, and lookalike paths are not accepted",
                ));
            };
            if !is_rust_integer_type(&name) {
                return Err(syn::Error::new(
                    ty.span(),
                    format!(
                        "rsjs/integer-domain-mismatch: `rsjs::div_int::<T>(a, b)` requires a fixed-width Rust integer type, not `{name}`"
                    ),
                ));
            }
            Ok(Some(
                IntegerDomain::from_name(&name)
                    .map(IntegerSource::Supported)
                    .unwrap_or(IntegerSource::Unsupported(name)),
            ))
        }
        syn::PathArguments::Parenthesized(arguments) => Err(syn::Error::new(
            arguments.span(),
            "rsjs/integer-domain-unproven: write `rsjs::div_int::<i64>(a, b)` with an angle-bracketed integer type",
        )),
    }
}

/// Return the leaf of an exact Rust primitive type spelling accepted by
/// `rsjs::div_int::<T>`.
///
/// `rust_type_leaf_name` intentionally peels references and grouping for
/// signal declarations, but a helper turbofish is a source-level semantic
/// boundary: `&i32`, `Option<i32>`, and `some_module::i32` are not the same
/// type as the primitive `i32`. Accept only the unqualified primitive spelling
/// or Rust's canonical `core::primitive::*` / `std::primitive::*` paths.
fn exact_div_int_primitive_type_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some()
        || type_path
            .path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
    {
        return None;
    }

    let segments = type_path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [name] => Some(name.clone()),
        [root, primitive, name]
            if (root == "core" || root == "std") && primitive == "primitive" =>
        {
            Some(name.clone())
        }
        _ => None,
    }
}

/// Apply the expected `div_int::<T>` domain to integer literals for which Rust
/// would infer `T` from the helper argument position.
///
/// Ordinary lowering sees a standalone `1 + 2` first and therefore defaults
/// that subtree to `i32`. At a real Rust call site, however,
/// `div_int::<i64>(1 + 2, 3)` supplies an `i64` expectation to both operands.
/// Clone-local suffixing lets recursive lowering observe the same context,
/// including values outside the i32 range, without weakening explicit suffix
/// mismatches. Calls, casts, and conditions establish their own type contexts
/// and are deliberately not traversed.
fn contextualize_unsuffixed_integer_literals(expr: &mut Expr, domain: IntegerDomain) {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(value),
            ..
        }) if value.suffix().is_empty() => {
            let span = value.span();
            *value = syn::LitInt::new(
                &format!("{}_{}", value.base10_digits(), domain.as_str()),
                span,
            );
        }
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Neg(_)) => {
            contextualize_unsuffixed_integer_literals(&mut unary.expr, domain);
        }
        Expr::Binary(binary)
            if matches!(
                binary.op,
                syn::BinOp::Add(_)
                    | syn::BinOp::Sub(_)
                    | syn::BinOp::Mul(_)
                    | syn::BinOp::Div(_)
                    | syn::BinOp::Rem(_)
            ) =>
        {
            contextualize_unsuffixed_integer_literals(&mut binary.left, domain);
            contextualize_unsuffixed_integer_literals(&mut binary.right, domain);
        }
        Expr::Paren(paren) => {
            contextualize_unsuffixed_integer_literals(&mut paren.expr, domain);
        }
        Expr::Group(group) => {
            contextualize_unsuffixed_integer_literals(&mut group.expr, domain);
        }
        Expr::If(if_expr) => {
            contextualize_unsuffixed_integer_block_tail(&mut if_expr.then_branch, domain);
            if let Some((_, else_expr)) = &mut if_expr.else_branch {
                contextualize_unsuffixed_integer_literals(else_expr, domain);
            }
        }
        Expr::Block(block) => {
            contextualize_unsuffixed_integer_block_tail(&mut block.block, domain);
        }
        _ => {}
    }
}

fn contextualize_unsuffixed_integer_block_tail(block: &mut syn::Block, domain: IntegerDomain) {
    let Some(syn::Stmt::Expr(expr, None)) = block.stmts.last_mut() else {
        return;
    };
    contextualize_unsuffixed_integer_literals(expr, domain);
}

fn reject_div_f64_turbofish(path: &syn::Path) -> syn::Result<()> {
    let Some(segment) = path.segments.last() else {
        return Ok(());
    };
    if matches!(segment.arguments, syn::PathArguments::None) {
        return Ok(());
    }
    Err(syn::Error::new(
        segment.arguments.span(),
        "`rsjs::div_f64(a, b)` does not take an explicit type argument",
    ))
}

fn div_int_domain_proof_error(span: proc_macro2::Span) -> syn::Error {
    syn::Error::new(
        span,
        "rsjs/integer-domain-unproven: `rsjs::div_int(a, b)` cannot prove one fixed-width Rust integer domain for these operands; write `rsjs::div_int::<i32>(a, b)`, `::<u32>`, `::<i64>`, or `::<u64>` so division-by-zero and signed MIN / -1 overflow match Rust",
    )
}

/// Recognize the primitive's ordinary `From` constructor without accepting a
/// user module that merely happens to end in `f64::from`.
fn is_f64_from_path(path: &syn::Path) -> bool {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let borrowed: Vec<&str> = segments.iter().map(String::as_str).collect();
    matches!(
        borrowed.as_slice(),
        ["f64", "from"]
            | ["std", "primitive", "f64", "from"]
            | ["core", "primitive", "f64", "from"]
    )
}

/// Returns `true` for binary operators whose operands are numeric
/// when the expression is numeric: arithmetic and ordering /
/// equality comparisons. `&&` and `||` take booleans, so wide-int
/// literal promotion never applies to them.
pub(super) fn binop_takes_numeric_operands(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::DivInt
            | BinOp::Rem
            | BinOp::Eq
            | BinOp::Neq
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::DynDiv
            | BinOp::DynEq
            | BinOp::DynNe
    )
}

/// Promote plain integer literals to BigInt literals throughout a
/// numeric subtree. Applied to the non-wide side of a binary
/// operation whose other side is provably wide: in well-typed Rust
/// the literals already have the wide type, and in JS the promotion
/// prevents both the BigInt/Number mixing TypeError and Number
/// rounding past 2^53. Recursion stays inside numeric positions
/// (arithmetic operands, conditional arms); anything else is left
/// untouched.
pub(super) fn promote_int_literals_to_wide(expr: &mut FrontendExpr) {
    match expr {
        FrontendExpr::Literal(value) => {
            if let Value::Int(n) = value {
                *value = Value::BigInt(i128::from(*n));
            }
        }
        FrontendExpr::BinaryOp {
            op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::DivInt | BinOp::Rem,
            lhs,
            rhs,
        } => {
            promote_int_literals_to_wide(lhs);
            promote_int_literals_to_wide(rhs);
        }
        FrontendExpr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            promote_int_literals_to_wide(then_expr);
            promote_int_literals_to_wide(else_expr);
        }
        _ => {}
    }
}

/// Returns `true` when a numeric subtree contains a BigInt literal,
/// recursing through arithmetic operands and conditional arms like
/// [`promote_int_literals_to_wide`]. Together with the typed wide
/// anchors this reconstructs the rewrite trigger ("this side mentions
/// BigInt at all") for [`Analyzer::fixup_numeric_binary`].
pub(super) fn contains_bigint_literal(expr: &FrontendExpr) -> bool {
    match expr {
        FrontendExpr::Literal(Value::BigInt(_)) => true,
        FrontendExpr::BinaryOp {
            op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::DivInt | BinOp::Rem,
            lhs,
            rhs,
        } => contains_bigint_literal(lhs) || contains_bigint_literal(rhs),
        FrontendExpr::Conditional {
            then_expr,
            else_expr,
            ..
        } => contains_bigint_literal(then_expr) || contains_bigint_literal(else_expr),
        _ => false,
    }
}

/// The inverse of [`promote_int_literals_to_wide`]: turn BigInt literals back
/// into plain Int literals throughout a representation-only numeric subtree.
/// This exists solely for legacy/schema-less comparison paths where a literal
/// must match a proven Number representation. Proven fixed-width expressions
/// are contextualized by their [`IntegerDomain`] and never use this rewrite.
pub(super) fn demote_bigint_literals(expr: &mut FrontendExpr) {
    match expr {
        FrontendExpr::Literal(value) => {
            if let Value::BigInt(n) = value {
                if let Ok(n) = i64::try_from(*n) {
                    *value = Value::Int(n);
                }
            }
        }
        FrontendExpr::BinaryOp {
            op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::DivInt | BinOp::Rem,
            lhs,
            rhs,
        } => {
            demote_bigint_literals(lhs);
            demote_bigint_literals(rhs);
        }
        FrontendExpr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            demote_bigint_literals(then_expr);
            demote_bigint_literals(else_expr);
        }
        _ => {}
    }
}

pub(super) fn lit_to_value(lit: &Lit, span: proc_macro2::Span) -> syn::Result<Value> {
    match lit {
        Lit::Int(li) => {
            let n: i128 = li
                .base10_parse()
                .map_err(|e| syn::Error::new(span, format!("integer literal out of range: {e}")))?;
            checked_int_literal_value(n, li.suffix(), span)
        }
        Lit::Float(lf) => {
            let n: f64 = lf
                .base10_parse()
                .map_err(|e| syn::Error::new(span, format!("float literal out of range: {e}")))?;
            Ok(Value::Float(n))
        }
        Lit::Bool(b) => Ok(Value::Bool(b.value)),
        Lit::Str(s) => Ok(Value::String(s.value())),
        other => Err(syn::Error::new(
            span,
            format!(
                "`#[rsjs]` does not support this literal type ({:?}) in v0.1",
                other
            ),
        )),
    }
}

pub(super) fn is_string_path(expr: &Expr) -> bool {
    if let Expr::Path(ExprPath { path, .. }) = expr {
        // Look for `String::new`. The last segment is `new`; the
        // penultimate must be `String`.
        let segs: Vec<_> = path.segments.iter().collect();
        if segs.len() >= 2
            && segs[segs.len() - 1].ident == "new"
            && segs[segs.len() - 2].ident == "String"
        {
            // Round-3 review #5: reject a user crate's
            // `my_crate::String::new()` so it does not silently lower
            // as a `signal(<empty string>)` initializer. The path is
            // accepted if it is `String::new` (bare), or rooted at
            // `core` / `std` / `alloc` (with `String` as the
            // penultimate segment), mirroring the std-rooted check
            // `client_props_derive` and `client_attr` apply to
            // primitive-type names. A leading `::` is preserved by
            // `syn::Path::leading_colon` and does not affect segment
            // iteration.
            return is_std_rooted_method_path(path);
        }
    }
    false
}

/// True iff `path` is a method-style call path (`<TypeName>::<method>`)
/// whose `<TypeName>` is std-rooted: either the bare 2-segment form
/// `String::new` or a fully qualified `core/std/alloc::...::String::new`.
pub(super) fn is_std_rooted_method_path(path: &syn::Path) -> bool {
    if path.segments.len() == 2 {
        // `String::new` — the type ident is the penultimate (and only
        // non-method) segment, which is `String` by the caller's
        // pre-check. Accept it.
        return true;
    }
    // `core::...::String::new` etc. The leading segment must be one
    // of the std crate roots; this prevents a user crate's
    // `my_crate::String::new` from lowering as the std `String`.
    let first = match path.segments.first() {
        Some(s) => &s.ident,
        None => return false,
    };
    first == "core" || first == "std" || first == "alloc"
}
