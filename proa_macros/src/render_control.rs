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
