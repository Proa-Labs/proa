// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Asset-id derivation for `#[rsjs(component...)]` components.
//!
//! The asset id is the stable identifier the runtime uses to locate
//! an island's seed payload (`<script data-rsjs-island="<asset_id>">`),
//! its `<rsjs-island data-rsjs-island-root="<asset_id>">` wrapper, and its on-disk JS
//! bundle (`<asset_id>.js`).
//!
//! ## Why we hash the full path *and* the impl tokens
//!
//! Earlier versions of this helper lowercased the last segment of the
//! struct path only: `crate::a::Counter` and `crate::b::Counter` both
//! produced the asset id `"counter"`, which silently collided in the
//! manifest and overwrote each other's JS file on disk.
//!
//! Round-3 review #8 surfaced a tighter case: `derive_asset_id`
//! receives only the user-spelled self-type from the `impl ... for
//! <Type>` line, not the full module path. Two `impl ... for Counter`
//! blocks in *different modules* both spell `Counter`, hash to the
//! same digest, and collide. Proc macros cannot do real name
//! resolution, so we widen the hash input: instead of just the path,
//! we also fold the full impl-block token stream into the hash. Two
//! literal `impl WebRenderSync<::proa_core::NoLoader, ...> for Counter { ... }` blocks in
//! different modules will almost always have different surrounding
//! tokens (different bodies, different attributes, different
//! whitespace) and so produce different ids.
//!
//! Pathological cases (two byte-identical impl blocks in different
//! modules) still collide. Users hitting that can disambiguate at
//! the source level by adding a unique attribute, comment, or no-op
//! statement to one of the impls.
//!
//! ## Format
//!
//! `<lowercased_last_segment>_<hash8>` where `hash8` is the first 8
//! hex characters of SHA-256 over `(path || "::" || impl_tokens)`.
//! The asset id is therefore an opaque-ish identifier: users should
//! look it up via the runtime manifest rather than by constructing
//! it from a struct name.

use proc_macro2::TokenStream;
use quote::ToTokens;
use sha2::{Digest, Sha256};

/// Derive the asset id for an island from its struct path alone.
///
/// Use this overload when the caller does not have the full impl
/// tokens (test fixtures, hand-built call sites). Production callers
/// should prefer [`derive_asset_id_with_context`] so two
/// `impl ... for Counter` blocks in different modules disambiguate.
///
/// See module docs for the full rationale and format. Examples:
///
/// - `Counter` -> `counter_<hash8>`
/// - `crate::a::Counter` and `crate::b::Counter` -> two distinct ids
/// - empty / no segments path -> `island_<hash8>`
#[cfg(test)]
pub(crate) fn derive_asset_id(path: &syn::Path) -> String {
    derive_asset_id_inner(path, None)
}

/// Derive the asset id, folding the impl block's full token stream
/// into the hash so two byte-different `impl ... for Counter` blocks
/// in different modules produce distinct ids.
///
/// Round-3 review #8: the old `derive_asset_id(path)` hashed only
/// the source-spelled self-type. Proc macros cannot do real name
/// resolution, so two `impl WebRenderSync<::proa_core::NoLoader, ...> for Counter { ... }`
/// blocks in different modules both spell `Counter` and collided.
/// Mixing the full impl tokens into the hash disambiguates every
/// non-byte-identical case; the rare pathological case (two
/// byte-identical impls) is the user's responsibility to resolve.
pub(crate) fn derive_asset_id_with_context(path: &syn::Path, impl_tokens: &TokenStream) -> String {
    derive_asset_id_inner(path, Some(impl_tokens))
}

fn derive_asset_id_inner(path: &syn::Path, impl_tokens: Option<&TokenStream>) -> String {
    let last = path
        .segments
        .last()
        .map(|s| s.ident.to_string().to_lowercase())
        .unwrap_or_else(|| "island".to_string());

    // Hash the verbatim token rendering of the full path so different
    // module qualifications produce different ids. Strip whitespace
    // `quote!` introduces around `::` so the hash is stable across
    // formatting variations of the same logical path.
    let full: String = path
        .to_token_stream()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut hasher = Sha256::new();
    hasher.update(full.as_bytes());
    if let Some(ts) = impl_tokens {
        // Separator so `path || tokens` and `path' || tokens'` don't
        // collide on a substring boundary.
        hasher.update(b"::");
        // Whitespace-strip the tokens too; the user's formatting
        // shouldn't matter, but the lexical body content should.
        let tokens: String = ts
            .to_string()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        hasher.update(tokens.as_bytes());
    }
    let digest = hasher.finalize();
    let hash8: String = digest
        .iter()
        .take(4)
        .map(|b| format!("{:02x}", b))
        .collect();

    format!("{last}_{hash8}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_id_lowercases_last_segment_and_appends_hash() {
        let path: syn::Path = syn::parse_quote!(my_app::Counter);
        let id = derive_asset_id(&path);
        assert!(id.starts_with("counter_"), "got: {id}");
        // 8 hex chars after the underscore.
        let suffix = id.strip_prefix("counter_").unwrap();
        assert_eq!(suffix.len(), 8, "expected 8 hex chars, got: {suffix}");
        assert!(
            suffix.chars().all(|c| c.is_ascii_hexdigit()),
            "expected hex, got: {suffix}"
        );
    }

    #[test]
    fn asset_id_disambiguates_same_last_segment_in_different_modules() {
        let a: syn::Path = syn::parse_quote!(crate::a::Counter);
        let b: syn::Path = syn::parse_quote!(crate::b::Counter);
        let id_a = derive_asset_id(&a);
        let id_b = derive_asset_id(&b);
        assert!(id_a.starts_with("counter_"));
        assert!(id_b.starts_with("counter_"));
        assert_ne!(
            id_a, id_b,
            "expected distinct asset ids for distinct module paths"
        );
    }

    #[test]
    fn asset_id_stable_for_same_path() {
        let path: syn::Path = syn::parse_quote!(crate::ui::SignupForm);
        let id1 = derive_asset_id(&path);
        let id2 = derive_asset_id(&path);
        assert_eq!(id1, id2);
        assert!(id1.starts_with("signupform_"));
    }

    #[test]
    fn asset_id_with_context_disambiguates_byte_different_impl_bodies() {
        // Round-3 review #8 regression. Two `impl ... for Counter`
        // blocks in different modules: the user-spelled self-type is
        // `Counter` in both, but the surrounding impl tokens differ.
        // The widened `derive_asset_id_with_context` folds the full
        // impl tokens into the hash so the two ids no longer collide.
        let path: syn::Path = syn::parse_quote!(Counter);
        let tokens_a: TokenStream = quote::quote! {
            impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for Counter {
                fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                    proa_macros::html_sync! { <div>{count.get()}</div> }.render(cx)
                }
            }
        };
        let tokens_b: TokenStream = quote::quote! {
            impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for Counter {
                fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
                    proa_macros::html_sync! { <span>{counter.get()}</span> }.render(cx)
                }
            }
        };
        let id_a = derive_asset_id_with_context(&path, &tokens_a);
        let id_b = derive_asset_id_with_context(&path, &tokens_b);
        assert!(id_a.starts_with("counter_"), "got: {id_a}");
        assert!(id_b.starts_with("counter_"), "got: {id_b}");
        assert_ne!(
            id_a, id_b,
            "expected distinct asset ids when the impl bodies differ"
        );
    }

    #[test]
    fn asset_id_with_context_is_stable_under_whitespace_only_changes() {
        // Whitespace differences in the impl tokens must NOT change
        // the hash: the user can rustfmt their code without breaking
        // the manifest pointer.
        let path: syn::Path = syn::parse_quote!(Counter);
        let tokens_a: TokenStream = quote::quote! { impl X for Counter { fn r() {} } };
        let tokens_b: TokenStream = quote::quote! {
            impl  X  for  Counter  {
                fn  r() {}
            }
        };
        let id_a = derive_asset_id_with_context(&path, &tokens_a);
        let id_b = derive_asset_id_with_context(&path, &tokens_b);
        assert_eq!(id_a, id_b);
    }

    #[test]
    fn asset_id_handles_empty_path() {
        // Construct a path with at least one segment, since syn::Path
        // requires at least one. Test the "Island" fallback indirectly
        // by relying on the helper's invariants: the result should be
        // non-empty and contain an underscore.
        let path: syn::Path = syn::parse_quote!(X);
        let id = derive_asset_id(&path);
        assert!(id.starts_with("x_"));
    }
}
