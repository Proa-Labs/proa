// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared typed handles used by node references and event current targets.

use core::marker::PhantomData;

use crate::dom::{DomTargetMarker, Element, ElementMarker};

/// Typed reference to an element captured during RSJS hydration.
///
/// This is a zero-sized compiler handle. RSJS must lower its supported
/// operations before browser code executes.
#[derive(Debug, Clone, Copy, Default)]
pub struct ElementRef<E: ElementMarker = Element>(PhantomData<fn() -> E>);

/// Typed handle for the DOM target matched by an RSJS event binding.
///
/// This models the compiler-selected handler element, `Document`, or `Window`,
/// not the raw bubbling `Event.target` value.
#[derive(Debug, Clone, Copy, Default)]
pub struct CurrentTarget<E: DomTargetMarker = Element>(PhantomData<fn() -> E>);

impl<E: ElementMarker> ElementRef<E> {
    /// Construct an authoring witness for compiler-emitted code.
    ///
    /// Application code should obtain handles from RSJS APIs instead.
    #[doc(hidden)]
    #[inline(always)]
    pub const fn __compiler_stub() -> Self {
        Self(PhantomData)
    }
}

impl<E: DomTargetMarker> CurrentTarget<E> {
    /// Construct an authoring witness for compiler-emitted code.
    ///
    /// Application code should obtain handles from RSJS APIs instead.
    #[doc(hidden)]
    #[inline(always)]
    pub const fn __compiler_stub() -> Self {
        Self(PhantomData)
    }
}

crate::__impl_dom_element_ref_methods!(ElementRef);
crate::__impl_dom_current_target_methods!(CurrentTarget);
