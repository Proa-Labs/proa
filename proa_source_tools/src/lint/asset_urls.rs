// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn lint_markup(
    path: &Path,
    inv: &MacroInvocation,
    root: &HtmlRoot,
    suppressions: &SuppressionComments,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut lint = AssetUrls {
        path,
        origin: SpanOrigin::Macro(inv),
        suppressions,
        diagnostics,
    };
    lint.visit_nodes(&root.nodes);
}

pub(super) fn lint_render_block(
    path: &Path,
    block: &Block,
    suppressions: &SuppressionComments,
    diagnostics: &mut Vec<Diagnostic>,
) {
    AssetUrls {
        path,
        origin: SpanOrigin::File,
        suppressions,
        diagnostics,
    }
    .visit_block(block);
}

struct AssetUrls<'a> {
    path: &'a Path,
    origin: SpanOrigin<'a>,
    suppressions: &'a SuppressionComments,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl AssetUrls<'_> {
    fn visit_nodes(&mut self, nodes: &[Node]) {
        for node in nodes {
            match node {
                Node::Element(element) => {
                    for attr in &element.attrs {
                        let name = attr_name(attr).as_markup_str();
                        let srcset = matches!(name, "srcset" | "imagesrcset");
                        let is_url = srcset || proa_dsl::consolidate::is_url_attr_name(name);
                        match attr {
                            Attribute::Static { value, .. } if is_url => {
                                self.literal(value, srcset)
                            }
                            Attribute::Dynamic { value, .. } => {
                                if is_url {
                                    self.url_expr(value, srcset);
                                }
                                self.visit_expr(value);
                            }
                            _ => {}
                        }
                    }
                    self.visit_nodes(&element.children);
                }
                Node::Fragment(children) => self.visit_nodes(children),
                Node::Block(expr) => self.visit_expr(expr),
                _ => {}
            }
        }
    }

    fn literal(&mut self, value: &syn::LitStr, srcset: bool) {
        let text = value.value();
        let kind = if srcset {
            srcset_urls(&text).find_map(asset_url)
        } else {
            asset_url(&text)
        };
        let Some(kind) = kind else { return };
        let (code, message) = match kind {
            AssetUrl::Logical => (
                "asset/public-url",
                "public asset path bypasses URL resolution",
            ),
            AssetUrl::Generated => ("asset/generated-url", "generated asset URL is hard-coded"),
        };
        self.warn(
            value.span(),
            code,
            message,
            if srcset {
                "resolve each managed candidate with `cx.url(\"public/…\")` before composing the srcset"
            } else {
                "pass the logical `public/…` path to `cx.url(...)` so it follows the active asset catalog"
            },
        );
    }

    fn url_expr(&mut self, expr: &Expr, srcset: bool) {
        match expr {
            Expr::Lit(syn::ExprLit {
                lit: Lit::Str(value),
                ..
            }) => self.literal(value, srcset),
            Expr::Paren(expr) => self.url_expr(&expr.expr, srcset),
            Expr::Group(expr) => self.url_expr(&expr.expr, srcset),
            Expr::Reference(expr) => self.url_expr(&expr.expr, srcset),
            Expr::Block(expr) => {
                if let Some(syn::Stmt::Expr(value, None)) = expr.block.stmts.last() {
                    self.url_expr(value, srcset);
                }
            }
            Expr::MethodCall(call)
                if call.args.is_empty()
                    && matches!(
                        call.method.to_string().as_str(),
                        "into" | "to_owned" | "to_string"
                    ) =>
            {
                self.url_expr(&call.receiver, srcset);
            }
            Expr::Call(call) if call.args.len() == 1 => {
                if let Expr::Path(path) = call.func.as_ref() {
                    let segments = &path.path.segments;
                    if segments.last().is_some_and(|last| last.ident == "Some")
                        || (segments.len() == 2
                            && segments[0].ident == "String"
                            && segments[1].ident == "from")
                    {
                        self.url_expr(&call.args[0], srcset);
                    }
                }
            }
            Expr::MethodCall(call) if call.method == "url" && call.args.len() == 1 => {
                if let Expr::Lit(syn::ExprLit {
                    lit: Lit::Str(value),
                    ..
                }) = &call.args[0]
                {
                    let text = value.value();
                    if text.starts_with("/public/") || text.starts_with("./public/") {
                        self.warn(
                            value.span(),
                            "asset/logical-path",
                            "managed asset paths start with `public/`",
                            "remove the leading slash or dot-slash to resolve the path in the asset catalog",
                        );
                    } else if asset_url(&text) == Some(AssetUrl::Generated) {
                        self.literal(value, false);
                    }
                }
            }
            _ => {}
        }
    }

    fn warn(&mut self, span: proc_macro2::Span, code: &'static str, message: &str, help: &str) {
        push_expr_diag(
            self.path,
            self.origin,
            self.diagnostics,
            self.suppressions,
            span,
            code,
            message,
            help,
        );
    }
}

impl<'ast> Visit<'ast> for AssetUrls<'_> {
    fn visit_field_value(&mut self, field: &'ast syn::FieldValue) {
        if let syn::Member::Named(name) = &field.member {
            let name = name.to_string();
            if matches!(
                name.as_str(),
                "src" | "href" | "poster" | "url" | "srcset" | "src_set" | "image_url"
            ) {
                self.url_expr(&field.expr, matches!(name.as_str(), "srcset" | "src_set"));
            }
        }
        visit::visit_field_value(self, field);
    }
}

#[derive(PartialEq, Eq)]
enum AssetUrl {
    Logical,
    Generated,
}

fn asset_url(value: &str) -> Option<AssetUrl> {
    let value = value.trim();
    let path = value.split(['?', '#']).next()?;
    if ["public/", "./public/", "/public/"].iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty())
    }) {
        Some(AssetUrl::Logical)
    } else if path.starts_with('/') && !path.starts_with("//") && path.contains("/_proa/assets/") {
        Some(AssetUrl::Generated)
    } else {
        None
    }
}

fn srcset_urls(mut input: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        input = input.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
        let end = input
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(input.len());
        let (url, rest) = input.split_at(end);
        input = if url.ends_with(',') {
            rest
        } else {
            rest.split_once(',').map_or("", |(_, rest)| rest)
        };
        (!url.is_empty()).then(|| url.trim_end_matches(','))
    })
}
