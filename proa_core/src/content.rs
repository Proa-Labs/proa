// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Typed selectors used by `proa_macros::omit_content!`.

use std::marker::PhantomData;

/// A zero-sized selector for a child type; it contains no child instance.
pub struct Child<T: ?Sized>(PhantomData<fn() -> *const T>);

/// Select a child type without constructing it.
///
/// Import `proa_core::content::child`, then put
/// `proa_macros::omit_content!(child::<Notice<'_>>())` inside the render
/// method that intentionally omits that child. Ordinary Rust type arguments
/// apply, including lifetimes and generic parameters.
pub const fn child<T: ?Sized>() -> Child<T> {
    Child(PhantomData)
}
