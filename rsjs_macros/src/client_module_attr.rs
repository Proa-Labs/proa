// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stage M7e: the `#[rsjs::client_module]` attribute macro.
//!
//! `#[rsjs::client_module]` annotates an inline `pub mod <name> { ... }`
//! containing one or more `#[rsjs::client]` helpers. The attribute:
//!
//! 1. Validates that the item is a literal inline module (`pub mod
//!    <name> { ... }`, not `pub mod <name>;`).
//! 2. Walks the module's items and collects every `pub fn` (in source
//!    order) as a helper. v0.1 simplifies to "every public fn must be
//!    `#[rsjs::client]`-marked"; private helpers are allowed (the
//!    macro skips them) but not collected. Type-level validation that
//!    the public fns actually carry `#[rsjs::client]` is left to
//!    rustc: the attribute references each fn's
//!    `__rsjs_client_<name>_meta::META`, so name resolution catches
//!    a missing or mistyped attribute.
//! 3. Emits a sibling const inside the module:
//!    ```ignore
//!    pub const __rsjs_client_module_<name>_meta:
//!        &'static rsjs::ClientModuleMeta = &rsjs::ClientModuleMeta {
//!            module_path: "<name>",
//!            helpers: &[ <helper_meta>, ... ],
//!        };
//!    ```
//! 4. Emits a linkme manifest entry that points at the same helper
//!    metadata. The runtime/collector later stitches the bundle JS
//!    from that post-expansion metadata, so this outer macro does not
//!    re-lower helper function bodies from raw `syn`.
//!
//! This grouping attribute is optional. Each helper's own
//! `#[rsjs::client]` annotation grants eligibility, and consuming components
//! call helpers through ordinary Rust paths without an allowlist.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned, ToTokens};
use sha2::{Digest, Sha256};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Ident, LitStr, Token};

struct ClientModuleAttr {
    module_path: Option<LitStr>,
}

impl Parse for ClientModuleAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(Self { module_path: None });
        }

        let key: Ident = input.parse()?;
        if key != "path" {
            return Err(syn::Error::new(key.span(), "expected `path = \"...\"`"));
        }
        let _: Token![=] = input.parse()?;
        let module_path: LitStr = input.parse()?;
        if input.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
        }
        if !input.is_empty() {
            return Err(input.error("expected end of `#[rsjs::client_module(...)]`"));
        }

        Ok(Self {
            module_path: Some(module_path),
        })
    }
}

/// Entry point invoked from the `#[proc_macro_attribute]` shim.
///
/// `attr` may be empty or `path = "..."` to override the emitted
/// runtime bundle path while keeping Rust name resolution pointed at
/// the inline module's identifier.
pub fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = match syn::parse2::<ClientModuleAttr>(attr) {
        Ok(attr) => attr,
        Err(err) => return err.to_compile_error(),
    };

    let module: syn::ItemMod = match syn::parse2(item.clone()) {
        Ok(m) => m,
        Err(_) => {
            return error_at(
                item.span(),
                "`#[rsjs::client_module]` must be applied to a `pub mod <name> { ... }` item",
                "wrap the helpers in `pub mod <name> { ... }` directly under the attribute",
                "the attribute reads each public function inside the module to build the bundle; \
                 it can only do that when the module body is inline",
            );
        }
    };

    match try_expand(module, attr.module_path) {
        Ok(ts) => ts,
        Err(diag) => diag.to_tokens(),
    }
}

fn try_expand(
    mut module: syn::ItemMod,
    module_path_override: Option<LitStr>,
) -> Result<TokenStream, Diagnostic> {
    // 1. The module must be inline (`pub mod <name> { ... }`), not an
    //    extern declaration (`pub mod <name>;`).
    let (brace, items) = match &module.content {
        Some(c) => (c.0, c.1.clone()),
        None => {
            return Err(Diagnostic::new(
                module.ident.span(),
                "`#[rsjs::client_module]` requires an inline module body",
                "replace `pub mod <name>;` with `pub mod <name> { ... }` and put the helpers \
                 inside",
                "the attribute reads the module's items at expansion time; an extern \
                 declaration leaves them in another file the proc macro can't see",
            ));
        }
    };

    // 2. Visibility check. Accept `pub` and `pub(<scope>)`
    //    visibilities (`pub(crate)`, `pub(super)`, `pub(in <path>)`):
    //    `Restricted` visibilities still let consuming islands within
    //    the allowed scope name helpers through normal Rust path resolution.
    //    Cross-crate consumers need true `pub`; that is documented on
    //    the `#[rsjs::client_module]` attribute.
    //
    //    We continue to reject inherited (private) visibility because
    //    a private bundle module is unreachable even from siblings.
    match &module.vis {
        syn::Visibility::Public(_) | syn::Visibility::Restricted(_) => {}
        syn::Visibility::Inherited => {
            return Err(Diagnostic::new(
                module.ident.span(),
                "`#[rsjs::client_module]` modules must be visible to consuming islands",
                "add `pub` (or `pub(crate)` / `pub(super)`) in front of `mod <name>`",
                "helper calls resolve through Rust's normal name resolution; a private bundle \
                 module is unreachable from outside its parent. `pub(crate)` works within the \
                 same crate; cross-crate consumers need true `pub`",
            ));
        }
    }

    let mod_name = module.ident.clone();
    let mod_name_str = mod_name.to_string();
    let module_path_lit =
        module_path_override.unwrap_or_else(|| syn::LitStr::new(&mod_name_str, mod_name.span()));
    // Round-3 review #7: reject raw module names so `format_ident!`
    // does not panic when synthesising the bundle-const path below.
    if mod_name_str.starts_with("r#") {
        return Err(Diagnostic::new(
            mod_name.span(),
            "`#[rsjs::client_module]` module names cannot be raw identifiers",
            "rename the module to a non-keyword identifier",
            "the macro derives `__rsjs_client_module_<name>_meta` from the module's name; \
             raw identifiers cannot be used as part of a generated ident",
        ));
    }

    // 3. Walk the module's items, collect every `pub fn` as a helper.
    //    Private fns are allowed but not collected (they may be
    //    helpers private to the bundle that the public fns call).
    //    Round-3 review #7: reject raw identifiers up front so
    //    `format_ident!` does not panic when synthesising the
    //    `__rsjs_client_<name>_meta` path below.
    let mut helper_idents: Vec<syn::Ident> = Vec::new();
    for it in &items {
        if let syn::Item::Fn(f) = it {
            if matches!(f.vis, syn::Visibility::Public(_)) {
                let ident = &f.sig.ident;
                if ident.to_string().starts_with("r#") {
                    return Err(Diagnostic::new(
                        ident.span(),
                        "`#[rsjs::client_module]` helper names cannot be raw identifiers",
                        "rename the function to a non-keyword identifier",
                        "the macro derives `__rsjs_client_<name>_meta` from each helper's \
                         name; raw identifiers cannot be used as part of a generated ident",
                    ));
                }
                helper_idents.push(ident.clone());
            }
        }
    }

    if helper_idents.is_empty() {
        return Err(Diagnostic::new(
            mod_name.span(),
            "`#[rsjs::client_module]` modules must declare at least one `pub fn`",
            "add at least one `#[rsjs::client] pub fn ...` to the module, or remove the \
             `#[rsjs::client_module]` attribute",
            "an empty bundle has nothing to ship to the client; helper eligibility belongs to \
             each function's `#[rsjs::client]` annotation",
        ));
    }

    // 4. Build the bundle const tokens. We reference each helper's
    //    `__rsjs_client_<name>_meta::META` which the M7d attribute
    //    emits. If a public fn was *not* `#[rsjs::client]`-marked,
    //    that path won't resolve, and rustc surfaces a clean error
    //    pointing at the const's site.
    // Round-4 review #3: append a content-derived 8-hex-char suffix to
    // the linkme submission idents so two `pub mod pricing { ... }`
    // blocks under different parents in the same crate disambiguate
    // instead of producing a `duplicate static
    // __RSJS_CLIENT_MODULE_SUBMIT_pricing` linker error. The suffix is
    // derived from the module's full token stream (whitespace-stripped),
    // so byte-different bodies at the same local name resolve to
    // distinct idents while byte-identical inputs are stable across
    // expansions. Mirrors the island-side fix in
    // `derive_asset_id_with_context`.
    let suffix = module_ident_suffix(&module, module_path_lit.value().as_str());
    let bundle_const_ident = format_ident!("__rsjs_client_module_{}_meta", mod_name);
    let entry_ident = format_ident!("__RSJS_CLIENT_MODULE_ENTRY_{}_{}", mod_name, suffix);
    let submit_ident = format_ident!("__RSJS_CLIENT_MODULE_SUBMIT_{}_{}", mod_name, suffix);

    let helper_meta_paths: Vec<TokenStream> = helper_idents
        .iter()
        .map(|h| {
            let m = format_ident!("__rsjs_client_{}_meta", h);
            quote_spanned!(h.span() => #m::META)
        })
        .collect();

    let bundle_const = quote! {
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const #bundle_const_ident: &'static ::rsjs::ClientModuleMeta =
            &::rsjs::ClientModuleMeta {
                module_path: #module_path_lit,
                helpers: &[ #( #helper_meta_paths ),* ],
            };

        #[doc(hidden)]
        #[allow(non_snake_case, non_upper_case_globals)]
        const _: () = {
            static #entry_ident: ::rsjs::ClientModuleManifestEntry =
                ::rsjs::ClientModuleManifestEntry {
                    module_path: #module_path_lit,
                    helpers: &[ #( #helper_meta_paths ),* ],
                };

            #[::rsjs::linkme::distributed_slice(::rsjs::CLIENT_MODULE_REGISTRY)]
            #[linkme(crate = ::rsjs::linkme)]
            static #submit_ident: &'static ::rsjs::ClientModuleManifestEntry = &#entry_ident;
        };
    };

    // 5. Inject the bundle const into the module body, immediately
    //    after the existing items. The `brace` span is reused so any
    //    diagnostic anchored on the module body still points correctly.
    let mut new_items = items;
    let bundle_items: syn::File = syn::parse2(bundle_const).map_err(|e| {
        Diagnostic::new(
            mod_name.span(),
            "rsjs internal error: failed to parse generated bundle items",
            "report this as an rsjs bug",
            &format!("syn error: {e}"),
        )
    })?;
    new_items.extend(bundle_items.items);
    module.content = Some((brace, new_items));

    Ok(module.into_token_stream())
}

/// Build the 8-hex-character disambiguation suffix used in the
/// `__RSJS_CLIENT_MODULE_{ENTRY,SUBMIT}_<name>_<hash>` idents.
///
/// Two `#[rsjs::client_module] pub mod pricing { ... }` blocks under
/// different parents in the same crate would otherwise emit duplicate
/// statics in the linkme distributed slice and fail link with a
/// `duplicate static` error rather than a clean macro diagnostic. The
/// suffix is derived from the module's full token stream (whitespace
/// stripped so cosmetic formatting doesn't perturb it). Byte-identical
/// bodies are stable across expansions; byte-different bodies always
/// hash differently.
fn module_ident_suffix(module: &syn::ItemMod, module_path: &str) -> String {
    let tokens: String = module
        .to_token_stream()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut hasher = Sha256::new();
    hasher.update(module_path.as_bytes());
    hasher.update(tokens.as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(4)
        .map(|b| format!("{:02x}", b))
        .collect()
}

// ---------------------------------------------------------------------------
// Diagnostic helper (mirrors the shape used in client_attr.rs).
// ---------------------------------------------------------------------------

struct Diagnostic {
    span: Span,
    what: String,
    fix: String,
    why: String,
}

impl Diagnostic {
    fn new(span: Span, what: &str, fix: &str, why: &str) -> Self {
        Self {
            span,
            what: what.to_string(),
            fix: fix.to_string(),
            why: why.to_string(),
        }
    }

    fn to_tokens(&self) -> TokenStream {
        let msg = format!(
            "rsjs: {}\n  help: {}\n  note: {}",
            self.what, self.fix, self.why
        );
        let lit = syn::LitStr::new(&msg, self.span);
        quote_spanned!(self.span => ::core::compile_error!(#lit);)
    }
}

fn error_at(span: Span, what: &str, fix: &str, why: &str) -> TokenStream {
    Diagnostic::new(span, what, fix, why).to_tokens()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn expand_ok(input: TokenStream) -> String {
        let module: syn::ItemMod = syn::parse2(input).expect("parses as inline mod");
        let ts = try_expand(module, None).unwrap_or_else(|d| panic!("expected ok: {}", d.what));
        ts.to_string()
    }

    fn expand_err(input: TokenStream) -> String {
        let module: syn::ItemMod = syn::parse2(input).expect("parses as inline mod");
        match try_expand(module, None) {
            Ok(_) => panic!("expected an error, got success"),
            Err(d) => d.what.clone(),
        }
    }

    #[test]
    fn collects_pub_fn_helpers_into_bundle_const() {
        let out = expand_ok(quote! {
            pub mod pricing {
                pub fn subtotal(price: i64, qty: i64) -> i64 { price * qty }
                pub fn with_tax(amount: f64, rate: f64) -> f64 { amount * rate }
            }
        });
        assert!(
            out.contains("__rsjs_client_module_pricing_meta"),
            "missing bundle const: {out}"
        );
        assert!(
            out.contains("__rsjs_client_subtotal_meta :: META"),
            "missing subtotal meta ref: {out}"
        );
        assert!(
            out.contains("__rsjs_client_with_tax_meta :: META"),
            "missing with_tax meta ref: {out}"
        );
        assert!(
            out.contains("module_path : \"pricing\""),
            "missing module_path literal: {out}"
        );
        assert!(
            out.contains("CLIENT_MODULE_REGISTRY"),
            "missing manifest registry submission: {out}"
        );
    }

    #[test]
    fn rejects_extern_module() {
        let module: syn::ItemMod = syn::parse2(quote! { pub mod pricing; }).unwrap();
        let msg = match try_expand(module, None) {
            Err(d) => d.what,
            Ok(_) => panic!("extern mod should be rejected"),
        };
        assert!(msg.contains("inline module body"), "got: {msg}");
    }

    #[test]
    fn accepts_module_path_override() {
        let module: syn::ItemMod = syn::parse2(quote! {
            pub mod client {
                pub fn validate_email(value: String) -> &'static str { "" }
            }
        })
        .expect("parses as inline mod");
        let ts = try_expand(
            module,
            Some(syn::LitStr::new("signup_client", Span::call_site())),
        )
        .unwrap_or_else(|d| panic!("expected ok: {}", d.what));
        let out = ts.to_string();
        assert!(
            out.contains("__rsjs_client_module_client_meta"),
            "missing bundle const: {out}"
        );
        assert!(
            out.contains("module_path : \"signup_client\""),
            "missing overridden module_path literal: {out}"
        );
    }

    #[test]
    fn rejects_empty_module() {
        let msg = expand_err(quote! {
            pub mod pricing { }
        });
        assert!(msg.contains("at least one `pub fn`"), "got: {msg}");
    }

    #[test]
    fn rejects_private_module() {
        let msg = expand_err(quote! {
            mod pricing {
                pub fn subtotal(price: i64, qty: i64) -> i64 { price * qty }
            }
        });
        assert!(
            msg.contains("must be visible to consuming islands"),
            "got: {msg}"
        );
    }

    /// Round-3 review #2: `pub(crate)` is a `Restricted` visibility,
    /// not `Public`. It should be accepted because consuming islands
    /// within the same crate can still resolve the bundle through
    /// normal name resolution.
    #[test]
    fn accepts_pub_crate_module() {
        let out = expand_ok(quote! {
            pub(crate) mod pricing {
                pub fn subtotal(price: i64, qty: i64) -> i64 { price * qty }
            }
        });
        assert!(
            out.contains("__rsjs_client_module_pricing_meta"),
            "missing bundle const for pub(crate) module: {out}"
        );
    }

    /// `pub(super)` is also a `Restricted` visibility and should be
    /// accepted for the same reason as `pub(crate)`.
    #[test]
    fn accepts_pub_super_module() {
        let out = expand_ok(quote! {
            pub(super) mod pricing {
                pub fn subtotal(price: i64, qty: i64) -> i64 { price * qty }
            }
        });
        assert!(
            out.contains("__rsjs_client_module_pricing_meta"),
            "missing bundle const for pub(super) module: {out}"
        );
    }

    /// Round-4 review #3: two `#[rsjs::client_module] pub mod pricing
    /// { ... }` blocks with different bodies in the same crate must
    /// generate distinct submit / entry idents so the linkme
    /// distributed slice does not emit two `__RSJS_CLIENT_MODULE_SUBMIT_pricing`
    /// statics and fail link with `duplicate static`.
    #[test]
    fn submit_ident_disambiguates_byte_different_bodies() {
        let a = expand_ok(quote! {
            pub mod pricing {
                pub fn subtotal(price: i64, qty: i64) -> i64 { price * qty }
            }
        });
        let b = expand_ok(quote! {
            pub mod pricing {
                pub fn discount(price: i64, off: i64) -> i64 { price - off }
            }
        });
        let suffix_a = extract_submit_suffix(&a);
        let suffix_b = extract_submit_suffix(&b);
        assert_ne!(
            suffix_a, suffix_b,
            "expected distinct __RSJS_CLIENT_MODULE_SUBMIT suffixes for distinct bodies; \
             a={suffix_a} b={suffix_b}"
        );
    }

    /// Stability counterpart: identical module bodies must produce the
    /// same submit ident across expansions, so cargo's incremental
    /// rebuilds don't churn the manifest registry.
    #[test]
    fn submit_ident_stable_for_same_body() {
        let body = quote! {
            pub mod pricing {
                pub fn subtotal(price: i64, qty: i64) -> i64 { price * qty }
            }
        };
        let a = expand_ok(body.clone());
        let b = expand_ok(body);
        assert_eq!(
            extract_submit_suffix(&a),
            extract_submit_suffix(&b),
            "expected stable __RSJS_CLIENT_MODULE_SUBMIT suffix for byte-identical input"
        );
    }

    /// Pull the `<hash>` suffix out of a generated
    /// `__RSJS_CLIENT_MODULE_SUBMIT_<name>_<hash>` ident in the
    /// stringified expansion. Returns the empty string if the marker
    /// isn't present (which the caller treats as a test failure).
    fn extract_submit_suffix(out: &str) -> String {
        let marker = "__RSJS_CLIENT_MODULE_SUBMIT_pricing_";
        let start = out.find(marker).expect("missing submit ident");
        let rest = &out[start + marker.len()..];
        rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect()
    }

    #[test]
    fn skips_private_helpers() {
        let out = expand_ok(quote! {
            pub mod pricing {
                pub fn subtotal(price: i64, qty: i64) -> i64 { price * qty }
                fn internal(price: i64) -> i64 { price + 1 }
            }
        });
        assert!(
            out.contains("__rsjs_client_subtotal_meta :: META"),
            "missing subtotal: {out}"
        );
        assert!(
            !out.contains("__rsjs_client_internal_meta"),
            "internal helper should not be collected: {out}"
        );
    }
}
