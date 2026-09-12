// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Separator component implementation
//!
//! Renders a visual divider between content sections.
use super::recipe::{SeparatorOrientation, SeparatorRecipeProps, SEPARATOR_RECIPE};
use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;
/// Separator component - a visual divider between content sections
///
/// A pure CSS separator that supports horizontal and vertical orientations.
/// When `decorative` is true (default), sets `role="none"` to indicate
/// the separator is purely visual. When false, sets `role="separator"`
/// for semantic meaning.
///
/// # Example
///
/// ```ignore
/// use crate::components::{Separator, SeparatorOrientation};
/// use proa_macros::html_sync;
///
/// html_sync! {
///     <div>
///         <p>"Above"</p>
///         {Separator::new()}
///         <p>"Below"</p>
///     </div>
/// }.render(cx);
/// ```
pub struct Separator {
    /// Orientation of the separator
    pub orientation: SeparatorOrientation,
    /// Whether the separator is purely decorative (default: true)
    ///
    /// If true, sets `role="none"`. If false, sets `role="separator"`.
    pub decorative: bool,
    /// Additional CSS classes
    pub class: Option<&'static str>,
}
impl Separator {
    /// Create a new separator with default settings
    pub const fn new() -> Self {
        Self {
            orientation: SeparatorOrientation::Horizontal,
            decorative: true,
            class: None,
        }
    }
}
impl Default for Separator {
    fn default() -> Self {
        Self::new()
    }
}
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for Separator {
    #[inline(always)]
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = SEPARATOR_RECIPE.resolve(SeparatorRecipeProps {
            orientation: Some(self.orientation),
        });
        let role = if self.decorative { "none" } else { "separator" };
        html_sync! {
            <div
                data-separator
                data-slot="separator"
                data-orientation={style.orientation().as_str()}
                role={role}
                class={proa_macros::text!("{} {}", style, self.class)}
            ></div>
        }
        .render(cx)
    }
}
