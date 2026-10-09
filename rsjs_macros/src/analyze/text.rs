// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Direct `text!` intrinsics share Proa's parser and keep streaming SSR.

use super::*;
use proa_dsl::text::{parse_expr, Segment};

impl Analyzer {
    pub(super) fn text_intrinsic_is_server_only(&self, expr: &Expr) -> syn::Result<bool> {
        let Some(parsed) = parse_expr(expr) else {
            return Ok(false);
        };
        let parsed = parsed?;
        Ok(parsed
            .args
            .iter()
            .all(|arg| self.can_infer_opaque_server_expr(arg) && !mentions_self_field(arg)))
    }

    pub(super) fn lower_text_intrinsic(
        &mut self,
        expr: &Expr,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        let parsed = parse_expr(expr).expect("caller recognized a text! intrinsic")?;
        // Opaque arguments can fall back to SSR if their format is unsupported.
        // Such speculative lowering must not leave signals, prop-use records,
        // or helper edges in the descriptor. Reactive failures are diagnostics
        // and never need a snapshot; static text bypasses lowering entirely.
        let checkpoint = parsed
            .args
            .iter()
            .all(|arg| self.can_infer_opaque_server_expr(arg))
            .then(|| self.clone());
        let result = self.lower_text_intrinsic_parsed(parsed, scope);
        if result.is_err() {
            if let Some(checkpoint) = checkpoint {
                *self = checkpoint;
            }
        }
        result
    }

    fn lower_text_intrinsic_parsed(
        &mut self,
        parsed: proa_dsl::text::ParsedText,
        scope: &ExprScope<'_>,
    ) -> syn::Result<FrontendExpr> {
        let args = parsed
            .args
            .iter()
            .map(|arg| {
                let lowered = self.lower_reactive_expr_in(arg, scope)?;
                let mut literal = arg;
                loop {
                    literal = match literal {
                        Expr::Paren(paren) => &paren.expr,
                        Expr::Group(group) => &group.expr,
                        _ => break,
                    };
                }
                // Value literals retain their representation, not their Rust
                // suffix. Keep the authored width for formatting (especially
                // signed radix output) before that proof is lost.
                Ok(
                    match prelude::numeric_literal_suffix(literal)
                        .and_then(IntegerDomain::from_name)
                    {
                        Some(domain) => self.anchor_integer_domain(lowered, domain),
                        None => lowered,
                    },
                )
            })
            .collect::<syn::Result<Vec<_>>>()?;
        let query_initializers: Vec<_> = self
            .query_initializers
            .iter()
            .map(|init| crate::emit_rust::QueryInitializer {
                id: init.id,
                expr: init.expr.clone(),
                key_expr: init.key_expr.clone(),
            })
            .collect();
        let mut parts = Vec::new();
        for segment in parsed.segments {
            match segment {
                Segment::Static(text) => parts.push(json!({"text": text})),
                Segment::Bare { index } => {
                    let ty = self.frontend_expr_type(&args[index], scope.handler_locals);
                    let client_arg =
                        lower_handler_ir_expr_to_client_expr(&args[index], parsed.span)?;
                    let attr_bool = crate::emit_rust::text_arg_can_be_native_bool(
                        &client_arg,
                        &query_initializers,
                    );
                    parts.push(json!({"arg": index, "string": ty == ClientType::String,
                        "attr_bool": attr_bool,
                        "float": matches!(ty, ClientType::F32 | ClientType::F64)}));
                }
                Segment::Formatted {
                    index,
                    integer_plan,
                    ..
                } => {
                    let ty = self.frontend_expr_type(&args[index], scope.handler_locals);
                    let domain = self.frontend_integer_domain(&args[index], scope.handler_locals);
                    let Some(plan) = integer_plan.filter(|plan| {
                        plan.kind <= 4
                            && (domain.is_some() || (ty == ClientType::String && plan.kind == 0))
                    }) else {
                        return Err(syn::Error::new(parsed.span,
                            "rsjs/text-format-unsupported: reactive text! formatting supports integer display/binary/octal/hex and string display, with literal width, alignment and precision; other Rust format traits remain server-only"));
                    };
                    parts.push(json!({"arg": index, "string": ty == ClientType::String, "domain": domain.map(IntegerDomain::as_str),
                        "fill": plan.fill.to_string(), "align": plan.align, "plus": plan.sign_plus,
                        "alternate": plan.alternate, "zero": plan.zero_pad, "width": plan.width,
                        "precision": plan.precision, "kind": plan.kind}));
                }
            }
        }
        // Workspace builds can enable serde_json/preserve_order through
        // another crate. Keep the embedded plan (and descriptor hashes)
        // identical across that feature graph and standalone consumers.
        let plan = rsjs_transport::canonical_json_bytes(&serde_json::Value::Array(parts))
            .expect("text format plan serializes");
        Ok(FrontendExpr::Text {
            template: parsed.template.value(),
            plan: String::from_utf8(plan).expect("JSON is UTF-8"),
            args,
            attribute: false,
        })
    }
}

fn mentions_self_field(expr: &Expr) -> bool {
    struct Fields(bool);
    impl<'ast> Visit<'ast> for Fields {
        fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
            if expr_is_self_path(&field.base) {
                self.0 = true;
            }
            visit::visit_expr_field(self, field);
        }
    }
    let mut visitor = Fields(false);
    visitor.visit_expr(expr);
    visitor.0
}

/// Attribute values use AttrValue instead of Display for bare arguments.
/// Specialize a copy of the expression so a prelude alias can be used in both
/// text and attribute positions without changing its text-slot behavior.
pub(super) fn set_attribute_context(expr: &mut FrontendExpr) {
    match expr {
        FrontendExpr::Text {
            attribute, args, ..
        } => {
            *attribute = true;
            for arg in args {
                set_attribute_context(arg);
            }
        }
        FrontendExpr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            set_attribute_context(then_expr);
            set_attribute_context(else_expr);
        }
        _ => {}
    }
}
