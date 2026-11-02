// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Format-independent lowering of Rust render slots.
//!
//! Statements retain normal Rust semantics. Only a block's tail is rendered;
//! control flow directs that render into the selected branch or loop body.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Block, Expr, Stmt};

pub(crate) fn lower(expr: &Expr, render: &impl Fn(&Expr) -> TokenStream) -> TokenStream {
    match expr {
        Expr::If(e) => {
            let attrs = &e.attrs;
            let cond = &e.cond;
            let body = lower_block(&e.then_branch, render);
            let otherwise = e.else_branch.as_ref().map(|(_, expr)| {
                let body = lower(expr, render);
                // An else block preserves attributes and labels through lower.
                quote! { else { #body } }
            });
            quote! { #(#attrs)* if #cond { #body } #otherwise }
        }
        Expr::Match(e) => {
            let attrs = &e.attrs;
            let value = &e.expr;
            let arms = e.arms.iter().map(|arm| {
                let attrs = &arm.attrs;
                let pat = &arm.pat;
                let guard = arm
                    .guard
                    .as_ref()
                    .map(|(token, expr)| quote! { #token #expr });
                let body = lower(&arm.body, render);
                quote! { #(#attrs)* #pat #guard => { #body } }
            });
            quote! { #(#attrs)* match #value { #(#arms),* } }
        }
        Expr::ForLoop(e) => {
            let attrs = &e.attrs;
            let label = &e.label;
            let pat = &e.pat;
            let iter = &e.expr;
            let body = lower_block(&e.body, render);
            quote! { #(#attrs)* #label for #pat in #iter { #body } }
        }
        Expr::While(e) => {
            let attrs = &e.attrs;
            let label = &e.label;
            let cond = &e.cond;
            let body = lower_block(&e.body, render);
            quote! { #(#attrs)* #label while #cond { #body } }
        }
        Expr::Loop(e) => {
            let attrs = &e.attrs;
            let label = &e.label;
            let body = lower_block(&e.body, render);
            if !loop_can_break(e) {
                // A diverging loop has no child value. Do not impose a render
                // trait on `!` or rely on the pre-2024 fallback to unit.
                return quote! { #(#attrs)* #label loop { #body } };
            }
            // Unlike `for` and `while`, a loop can return a value through
            // `break`. Its body still streams each tail; render the final
            // break value once after the loop finishes (unit emits nothing).
            render(&Expr::Verbatim(quote! { #(#attrs)* #label loop { #body } }))
        }
        // A labeled block may produce its value through `break 'label value`.
        // Render that value once, rather than turning only its tail into output.
        Expr::Block(e) if e.label.is_none() => {
            let attrs = &e.attrs;
            let body = lower_block(&e.block, render);
            quote! { #(#attrs)* { #body } }
        }
        // These transfer control; their never type is not a renderable value.
        Expr::Break(_) | Expr::Continue(_) | Expr::Return(_) => quote! { #expr },
        // None contributes no output, without requiring an inferred child type.
        Expr::Path(e) if e.path.is_ident("None") => quote! {},
        _ => render(expr),
    }
}

fn loop_can_break(expr: &syn::ExprLoop) -> bool {
    use syn::visit::{self, Visit};
    struct Exits<'a> {
        label: Option<&'a syn::Lifetime>,
        depth: usize,
        found: bool,
    }
    impl<'ast> Visit<'ast> for Exits<'_> {
        fn visit_expr_break(&mut self, expr: &'ast syn::ExprBreak) {
            self.found |= match &expr.label {
                Some(label) => self.label == Some(label),
                None => self.depth == 0,
            };
            visit::visit_expr_break(self, expr);
        }
        fn visit_expr_loop(&mut self, expr: &'ast syn::ExprLoop) {
            if self.label.is_some() && expr.label.as_ref().map(|label| &label.name) == self.label {
                return;
            }
            self.depth += 1;
            visit::visit_expr_loop(self, expr);
            self.depth -= 1;
        }
        fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
            // The iterator expression is evaluated outside the new loop's
            // scope, including when its label shadows the enclosing loop.
            self.visit_expr(&expr.expr);
            if self.label.is_some() && expr.label.as_ref().map(|label| &label.name) == self.label {
                return;
            }
            self.depth += 1;
            self.visit_block(&expr.body);
            self.depth -= 1;
        }
        fn visit_expr_while(&mut self, expr: &'ast syn::ExprWhile) {
            if self.label.is_some() && expr.label.as_ref().map(|label| &label.name) == self.label {
                return;
            }
            self.depth += 1;
            visit::visit_expr_while(self, expr);
            self.depth -= 1;
        }
        fn visit_expr_closure(&mut self, _: &'ast syn::ExprClosure) {}
        fn visit_expr_block(&mut self, expr: &'ast syn::ExprBlock) {
            if self.label.is_some() && expr.label.as_ref().map(|label| &label.name) == self.label {
                let label = self.label.take();
                visit::visit_expr_block(self, expr);
                self.label = label;
            } else {
                visit::visit_expr_block(self, expr);
            }
        }
        fn visit_expr_async(&mut self, _: &'ast syn::ExprAsync) {}
        fn visit_item(&mut self, _: &'ast syn::Item) {}
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            // Proa render macros evaluate their slots inside a new closure, so
            // their control flow cannot exit this loop. Other macro expansions
            // are opaque and may contain a break targeting it.
            let proa_path = mac.path.leading_colon.is_some()
                && mac.path.segments.len() == 2
                && mac.path.segments[0].ident == "proa_macros";
            let render_macro = proa_path
                && mac.path.segments.last().is_some_and(|segment| {
                    matches!(
                        segment.ident.to_string().as_str(),
                        "html" | "html_sync" | "xml" | "xml_sync" | "md" | "md_sync"
                    )
                });
            self.found |= !render_macro;
        }
    }
    let mut exits = Exits {
        label: expr.label.as_ref().map(|label| &label.name),
        depth: 0,
        found: false,
    };
    exits.visit_block(&expr.body);
    exits.found
}

fn lower_block(block: &Block, render: &impl Fn(&Expr) -> TokenStream) -> TokenStream {
    let Some((last, init)) = block.stmts.split_last() else {
        return quote! {};
    };
    match last {
        Stmt::Expr(expr, None) => {
            let tail = lower(expr, render);
            quote! { #(#init)* #tail }
        }
        Stmt::Macro(stmt) if stmt.semi_token.is_none() => {
            let attrs = &stmt.attrs;
            let expr = Expr::Macro(syn::ExprMacro {
                attrs: Vec::new(),
                mac: stmt.mac.clone(),
            });
            let tail = render(&expr);
            quote! { #(#init)* #(#attrs)* { #tail } }
        }
        _ => {
            let stmts = &block.stmts;
            quote! { #(#stmts)* }
        }
    }
}
