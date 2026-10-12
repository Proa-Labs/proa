// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Resolve generated runtime paths when callers use the `proa` facade or aliases.

use proc_macro2::TokenStream;
use proc_macro_crate::{crate_name, FoundCrate};
use quote::{quote, ToTokens};
use syn::visit_mut::VisitMut;

pub(crate) fn resolve(tokens: TokenStream) -> TokenStream {
    let facade = crate_name("proa").ok();
    let mut replacements = Vec::new();
    for (package, module) in [
        ("proa_core", None),
        ("proa_actions", Some("actions")),
        ("proa_ir", Some("ir")),
    ] {
        let path = match crate_name(package) {
            Ok(FoundCrate::Name(name)) if name == package => continue,
            // Keep self aliases intact: integration tests have their own crate root.
            Ok(FoundCrate::Itself) => continue,
            Ok(FoundCrate::Name(name)) => {
                let ident = proc_macro2::Ident::new(&name, proc_macro2::Span::call_site());
                quote!(#ident)
            }
            Err(_) => match &facade {
                Some(found) => {
                    let root = match found {
                        FoundCrate::Itself => quote!(crate),
                        FoundCrate::Name(name) => {
                            let ident =
                                proc_macro2::Ident::new(name, proc_macro2::Span::call_site());
                            quote!(#ident)
                        }
                    };
                    match module {
                        Some(module) => {
                            let module =
                                proc_macro2::Ident::new(module, proc_macro2::Span::call_site());
                            quote!(#root::#module)
                        }
                        None => root,
                    }
                }
                None => continue,
            },
        };
        replacements.push((package, path));
    }
    if replacements.is_empty() {
        tokens
    } else {
        rewrite(tokens, &replacements)
    }
}

fn rewrite(tokens: TokenStream, replacements: &[(&str, TokenStream)]) -> TokenStream {
    struct RuntimePaths<'a>(&'a [(&'a str, TokenStream)]);
    impl RuntimePaths<'_> {
        fn visit_runtime_use(
            &self,
            tree: &mut syn::UseTree,
            leading_colon: &mut Option<syn::Token![::]>,
        ) {
            match tree {
                syn::UseTree::Path(path) => {
                    let Some((_, replacement)) = self.0.iter().find(|(name, _)| path.ident == name)
                    else {
                        return;
                    };
                    let prefix: syn::Path =
                        syn::parse2(replacement.clone()).expect("runtime crate path");
                    let mut rewritten = *path.tree.clone();
                    if prefix.segments[0].ident == "crate" {
                        *leading_colon = None;
                    }
                    for segment in prefix.segments.into_iter().rev() {
                        rewritten = syn::UseTree::Path(syn::UsePath {
                            ident: segment.ident,
                            colon2_token: Default::default(),
                            tree: Box::new(rewritten),
                        });
                    }
                    *tree = rewritten;
                }
                syn::UseTree::Group(group) => {
                    for item in &mut group.items {
                        self.visit_runtime_use(item, leading_colon);
                    }
                }
                _ => {}
            }
        }

        fn visit_associated_path(
            &mut self,
            qself: &mut Option<syn::QSelf>,
            path: &mut syn::Path,
        ) -> bool {
            let Some(qself) = qself.as_mut().filter(|qself| qself.position == 0) else {
                return false;
            };
            self.visit_type_mut(&mut qself.ty);
            for segment in &mut path.segments {
                self.visit_path_arguments_mut(&mut segment.arguments);
            }
            true
        }
    }
    impl VisitMut for RuntimePaths<'_> {
        fn visit_item_use_mut(&mut self, node: &mut syn::ItemUse) {
            syn::visit_mut::visit_item_use_mut(self, node);
            self.visit_runtime_use(&mut node.tree, &mut node.leading_colon);
        }

        fn visit_type_path_mut(&mut self, node: &mut syn::TypePath) {
            if !self.visit_associated_path(&mut node.qself, &mut node.path) {
                syn::visit_mut::visit_type_path_mut(self, node);
            }
        }

        fn visit_expr_path_mut(&mut self, node: &mut syn::ExprPath) {
            if self.visit_associated_path(&mut node.qself, &mut node.path) {
                for attribute in &mut node.attrs {
                    self.visit_attribute_mut(attribute);
                }
            } else {
                syn::visit_mut::visit_expr_path_mut(self, node);
            }
        }

        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);
            // A bare identifier may be a local value rather than a crate path.
            if path.segments.len() < 2 {
                return;
            }
            let first = &path.segments[0];
            if !matches!(first.arguments, syn::PathArguments::None) {
                return;
            }
            let Some((_, replacement)) = self.0.iter().find(|(name, _)| first.ident == name) else {
                return;
            };
            let prefix: syn::Path = syn::parse2(replacement.clone()).expect("runtime crate path");
            let suffix = path.segments.iter().skip(1).cloned().collect::<Vec<_>>();
            if prefix.segments[0].ident == "crate" {
                path.leading_colon = None;
            }
            path.segments = prefix.segments;
            path.segments.extend(suffix);
        }
    }
    let mut visitor = RuntimePaths(replacements);
    if let Ok(mut expression) = syn::parse2::<syn::Expr>(tokens.clone()) {
        visitor.visit_expr_mut(&mut expression);
        expression.into_token_stream()
    } else if let Ok(mut file) = syn::parse2::<syn::File>(tokens.clone()) {
        visitor.visit_file_mut(&mut file);
        file.into_token_stream()
    } else {
        // Preserve diagnostics and token-only outputs that are not Rust ASTs.
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_paths_and_preserves_literals_plain_identifiers_and_other_modules() {
        let input = quote!({
            ::proa_core::WebContext::new();
            let proa_core = "proa_core::text";
            other::proa_core::Value;
            use ::proa_core::{MdRenderSyncExt, WebContext};
        });
        assert_eq!(
            rewrite(input, &[("proa_core", quote!(my_proa))]).to_string(),
            quote!({
                ::my_proa::WebContext::new();
                let proa_core = "proa_core::text";
                other::proa_core::Value;
                use ::my_proa::{MdRenderSyncExt, WebContext};
            })
            .to_string()
        );
    }

    #[test]
    fn current_crate_and_facade_modules_keep_valid_path_syntax() {
        let output = rewrite(
            quote!({
                ::proa_core::WriteError;
                proa_actions::ActionData;
            }),
            &[
                ("proa_core", quote!(crate)),
                ("proa_actions", quote!(facade::actions)),
            ],
        );
        assert_eq!(
            output.to_string(),
            quote!({
                crate::WriteError;
                facade::actions::ActionData;
            })
            .to_string()
        );
        assert_eq!(
            rewrite(
                quote!(::proa_core::WriteError),
                &[("proa_core", quote!(crate_ui))]
            )
            .to_string(),
            quote!(::crate_ui::WriteError).to_string()
        );
    }

    #[test]
    fn generic_implementations_and_qualified_associated_paths_are_distinguished() {
        let input = quote! {
            impl<T> ::proa_core::WebRenderSync<T> for Wrapper<T> {}
            type Value<T> = <T as other::Trait>::proa_core::Value;
            type Direct<T> = <T>::proa_core::Value;
        };
        assert_eq!(
            rewrite(input, &[("proa_core", quote!(facade))]).to_string(),
            quote! {
                impl<T> ::facade::WebRenderSync<T> for Wrapper<T> {}
                type Value<T> = <T as other::Trait>::proa_core::Value;
                type Direct<T> = <T>::proa_core::Value;
            }
            .to_string()
        );
    }
}
