// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Resolve absolute RSJS paths at the public macro boundary, including nested
//! generated attributes. Literal payloads and semantic source records stay intact.
use proc_macro2::{Group, TokenStream, TokenTree};

pub(crate) fn resolve(tokens: TokenStream) -> TokenStream {
    rewrite(tokens, &proa_dsl::paths::runtime("rsjs"))
}

fn rewrite(tokens: TokenStream, runtime: &TokenStream) -> TokenStream {
    let tokens: Vec<_> = tokens.into_iter().collect();
    let mut output = TokenStream::new();
    let mut index = 0;
    while index < tokens.len() {
        if index + 3 < tokens.len()
            && matches!(&tokens[index], TokenTree::Punct(p) if p.as_char() == ':')
            && matches!(&tokens[index + 1], TokenTree::Punct(p) if p.as_char() == ':')
            && matches!(&tokens[index + 2], TokenTree::Ident(i) if i == "rsjs")
            && matches!(&tokens[index + 3], TokenTree::Punct(p) if p.as_char() == ':')
            // Do not match a segment within another absolute path.
            && !continues_path(&tokens[..index])
        {
            output.extend(runtime.clone());
            index += 3;
            continue;
        }
        let token = match &tokens[index] {
            TokenTree::Group(group) => {
                let mut replacement =
                    Group::new(group.delimiter(), rewrite(group.stream(), runtime));
                replacement.set_span(group.span());
                TokenTree::Group(replacement)
            }
            token => token.clone(),
        };
        output.extend([token]);
        index += 1;
    }
    output
}

// An identifier before `::` makes this a relative path segment. Rust keywords
// (`impl`, `as`, `use`, ...) and lifetime names do not; path keywords do.
fn continues_path(prefix: &[TokenTree]) -> bool {
    let Some(TokenTree::Ident(ident)) = prefix.last() else {
        return false;
    };
    if prefix.len() >= 2
        && matches!(&prefix[prefix.len() - 2], TokenTree::Punct(p) if p.as_char() == '\'')
    {
        return false;
    }
    let word = ident.to_string();
    matches!(word.as_str(), "self" | "Self" | "super" | "crate")
        || syn::parse2::<syn::Ident>(TokenStream::from(TokenTree::Ident(ident.clone()))).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn resolves_nested_paths_without_touching_payloads_or_other_namespaces() {
        let source = quote! {
            #[::rsjs::client]
            fn helper(_: &'a ::rsjs::Event) { other::rsjs::helper("::rsjs::Event"); }
        };
        let expected = quote! {
            #[::app::rsjs::client]
            fn helper(_: &'a ::app::rsjs::Event) { other::rsjs::helper("::rsjs::Event"); }
        };
        assert_eq!(
            rewrite(source, &quote!(::app::rsjs)).to_string(),
            expected.to_string()
        );
    }
}
