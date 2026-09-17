// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub use proa_showcase::ebay::srp::*;

use proa_core::shared::write_buf::WriteError as CoreWriteError;
use proa_core::{WebContext, WebRenderSync, WriteBuf};

/// Interactive variant: every card's watch-heart is an rsjs island,
/// so each listing toggles its heart locally without any framework JS.
/// The reusable eBay card markup lives in `proa_showcase`; this wrapper
/// supplies the demo-only rsjs heart slot.
pub struct InteractiveListingsList<'a> {
    pub listings: &'a [Listing],
}

impl<'a, L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for InteractiveListingsList<'a>
{
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        cx.out
            .extend(b"<ul class=\"srp-results srp-list clearfix\">")?;
        for listing in self.listings {
            Card {
                listing,
                heart_outer_class: INTERACTIVE_WATCH_HEART_CLASS,
                heart: crate::ebay_srp_islands::WatchHeart,
            }
            .render(cx)?;
        }
        cx.out.extend(b"</ul>")
    }
}

/// Standalone FilterChips render. Spliced into the page header by the
/// playground's showcase handler so the chips sit above the `<ul>` the
/// eBay shell provides.
pub fn render_filter_chips_to_string() -> String {
    render_filter_chips_with_initial_to_string(crate::ebay_srp_islands::FilterChips::default())
}

/// Initial-state-aware variant of [`render_filter_chips_to_string`].
pub fn render_filter_chips_with_initial_to_string(
    chips: crate::ebay_srp_islands::FilterChips,
) -> String {
    proa_core::render_to_string(chips)
}

/// Interactive variant of `render_listings_to_string`: every card's
/// watch-heart is an rsjs island. The page that loads this output must
/// also load `rsjs::shared_runtime_js()` plus the per-island JS sources
/// from `rsjs::manifest()`.
pub fn render_listings_interactive_to_string(listings: &[Listing]) -> String {
    proa_core::render_to_string(InteractiveListingsList { listings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_watchheart_omits_static_visibility_modifier() {
        let listings = build_listings(1);
        let html = render_listings_interactive_to_string(&listings);
        assert!(
            html.contains(
                r#"<span class="s-card__watchheart on-image s-card__watchheart--rsjs"><rsjs-island"#
            ),
            "expected rsjs heart wrapper class without eBay static visibility modifier, got:\n{html}"
        );
        assert!(
            !html.contains(
                r#"<span class="s-card__watchheart on-image s-card__watchheart--watch"><rsjs-island"#
            ),
            "static --watch modifier would hide the rsjs-controlled outline/fill icons, got:\n{html}"
        );
    }
}
