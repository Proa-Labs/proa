// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND (ISC AND MIT)

//! Spinner component implementation
use super::recipe::{SpinnerRecipeProps, SpinnerSize, SPINNER_RECIPE};
use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;
/// Type-safe Spinner component using SpinnerRecipe.
///
/// This struct is fully stack-allocated with no heap allocations.
/// All fields use static string slices or references.
///
/// The Spinner displays an animated loading icon (lucide loader-circle).
///
/// # Example
///
/// ```ignore
/// use crate::components::{Spinner, SpinnerSize};
/// use proa_macros::html_sync;
///
/// html_sync! {
///     <div>
///         {Spinner {
///             size: Some(SpinnerSize::Lg),
///             ..Spinner::new()
///         }}
///     </div>
/// }.render(cx);
/// ```
pub struct Spinner {
    pub size: Option<SpinnerSize>,
    pub class: Option<&'static str>,
    pub id: Option<&'static str>,
}
impl Spinner {
    /// Create a new spinner with default settings
    pub const fn new() -> Self {
        Self {
            size: None,
            class: None,
            id: None,
        }
    }
}
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for Spinner {
    #[inline(always)]
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = SPINNER_RECIPE.resolve(SpinnerRecipeProps { size: self.size });
        html_sync! {
            <svg
                data-spinner
                data-slot="spinner"
                data-size={style.size().as_str()}
                xmlns="http://www.w3.org/2000/svg"
                width="24"
                height="24"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                class={proa_macros::text!("{} {}", style, self.class)}
                role="status"
                aria-label="Loading"
                id={self.id}
            >
                <path d="M21 12a9 9 0 1 1-6.219-8.56" />
            </svg>
        }
        .render(cx)
    }
}
impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}
