// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_dsl::content::{ContentPart, ContentPolicy, OmitContent, OmitSelector};
use proc_macro2::TokenStream;
use quote::quote;

pub fn policy(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let _: ContentPolicy = syn::parse2(args)?;
    if syn::parse2::<syn::ItemImpl>(input.clone()).is_err()
        && syn::parse2::<syn::ImplItemFn>(input.clone()).is_err()
    {
        return Err(syn::Error::new_spanned(
            input,
            "`content` belongs on a render trait impl or its render method",
        ));
    }
    Ok(input)
}

pub fn omit(input: TokenStream) -> syn::Result<TokenStream> {
    let omission: OmitContent = syn::parse2(input)?;
    Ok(match omission.selector {
        OmitSelector::Value(value) => quote! {{ if false { let _ = &(#value); } }},
        OmitSelector::Child(ty) => quote! {{ let _ = ::core::marker::PhantomData::<#ty>; }},
        OmitSelector::Role(_) => quote! { () },
    })
}

pub fn part(input: TokenStream) -> syn::Result<TokenStream> {
    let part: ContentPart = syn::parse2(input)?;
    let value = part.value;
    Ok(quote! { (#value) })
}
