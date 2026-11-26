// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Interactive islands for the eBay SRP demo, compiled by `rsjs` to
//! per-island vanilla JS plus a shared runtime. The static SSR path
//! (`ebay_srp::render_listings_into`) stays untouched so the rendering
//! benchmarks remain comparable; the islands here are wired into the
//! interactive variant served by the playground.
//!
//! Two island shapes ship today:
//! - `WatchHeart`: ~60 instances per page, one per listing card. The
//!   "many tiny islands" workload for the bundle-size comparison.
//! - `FilterChips`: a single island holding three filter toggles
//!   (Buy It Now, Auction, Free shipping). The "one island, multi-
//!   signal, multi-handler" workload, paired with `WatchHeart` to give
//!   the bundle-size comparison two distinct shapes to measure.
//!
//! Filter chips today only toggle their own visual state; wiring them
//! to actually filter the listings is a separate piece of work that
//! either (a) consolidates listings into the same island as the chips
//! so signals can drive `class=` reactivity per card, or (b) writes
//! filter state to the URL with a small DOM coordinator.

#[allow(unused_imports)]
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
#[allow(unused_imports)]
use proa_macros::html_sync;
#[allow(unused_imports)]
use rsjs::signal;
use rsjs_macros::rsjs;

/// One watch-heart toggle per listing card. Local-only client state:
/// flips a boolean signal on click, swapping the filled vs hollow
/// heart icon. ~50 instances render per page, so this is the headline
/// "many tiny islands" workload for the bundle-size comparison.
pub struct WatchHeart;

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for WatchHeart {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let watching = signal(false);
        html_sync! {
            <button
                type="button"
                class="s-card__watchheart-click"
                aria-label="watch listing"
                on_click={|_| watching.set(!watching.get())}
            >
                <span class="s-card__watchheart-icon" aria-hidden="true">
                    {if watching.get() {
                        html_sync! {
                            <svg class="icon icon--16" focusable="false" aria-hidden="true">
                                <use href="#icon-heart-filled-16"></use>
                            </svg>
                        }
                    }}
                </span>
                <span class="s-card__watchheart-icon" aria-hidden="true">
                    {if !watching.get() {
                        html_sync! {
                            <svg class="icon icon--16" focusable="false" aria-hidden="true">
                                <use href="#icon-heart-16"></use>
                            </svg>
                        }
                    }}
                </span>
            </button>
        }
        .render(cx)
    }
}

/// Filter chip toolbar: three independent toggleable filters (Buy It
/// Now, Auction, Free shipping). Each chip carries its own bool
/// signal and toggles on click. One island, three signals, three
/// handlers — the multi-signal-and-multi-handler workload, paired
/// with `WatchHeart`'s "many tiny one-signal islands" workload, gives
/// the bundle-size comparison two distinct shapes to measure.
///
/// Earlier versions of this file split each chip into its own island
/// to work around an rsjs handler-index mismatch (sequential indices
/// in the SSR HTML vs NodeId indices in the per-island JS). That
/// mismatch was fixed in `382d4d0 fix(rsjs_macros): emit NodeId in
/// data-rsjs-handler / data-rsjs-bind attributes`, so the chips now
/// share one registered island and one shared `<rsjs-island>` script
/// seed — fewer per-island JS files served and a tighter total
/// gzipped wire size.
/// One rsjs island wrapping the entire SRP `<ul>` with 60 hand-
/// unrolled watch-heart buttons. Headline bundle-size demo: collapses
/// 60 `<rsjs-island>` wrappers, 60 seed-state JSON blobs, and 60
/// hydration cycles per page into 1 of each.
///
/// How the splice works: each `Listing` is rendered through `Card`
/// once with a `HEART_SENTINEL` marker in the heart slot, then split
/// into a (pre, post) pair of opaque byte chunks. Inside the rsjs
/// body, a `seq!`-unrolled triple per card emits
/// `pre, <button on_click=...>, post`. The pre/post chunks are
/// individually unbalanced HTML; their concatenation around the
/// button reconstitutes the original card markup, so the rendered
/// DOM is identical byte-for-byte to the per-card-island variant —
/// only the rsjs wrapper count changes.
///
/// Lifetime + generics on this impl ride the M11 macro changes (see
/// commit 34fbf53) — pre-M11 the `<'a>` here would have been
/// rejected.
pub struct WatchListings60<'a> {
    pub listings: &'a [crate::ebay_srp::Listing; 60],
}

seq_macro::seq!(N in 0..60 {
    #[rsjs(component, client)]
    impl<'a, L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for WatchListings60<'a> {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            #(let s~N = signal(false);)*
            #(let chunks~N = crate::ebay_srp::split_card_for_island(&self.listings[N]);)*
            html_sync! {
                <ul class="srp-results srp-list clearfix">
                    #(
                        <li class="s-card s-card--horizontal">
                            { proa_core::UnsafeRaw(chunks~N.0.as_str()) }
                            <button
                                type="button"
                                class="s-card__watchheart-click"
                                aria-label="watch listing"
                                on_click={|_| s~N.set(!s~N.get())}
                            >
                                <span class="s-card__watchheart-icon" aria-hidden="true">
                                    {if s~N.get() {
                                        html_sync! {
                                            <svg class="icon icon--16" focusable="false" aria-hidden="true">
                                                <use href="#icon-heart-filled-16"></use>
                                            </svg>
                                        }
                                    }}
                                </span>
                                <span class="s-card__watchheart-icon" aria-hidden="true">
                                    {if !s~N.get() {
                                        html_sync! {
                                            <svg class="icon icon--16" focusable="false" aria-hidden="true">
                                                <use href="#icon-heart-16"></use>
                                            </svg>
                                        }
                                    }}
                                </span>
                            </button>
                            { proa_core::UnsafeRaw(chunks~N.1.as_str()) }
                        </li>
                    )*
                </ul>
            }
            .render(cx)
        }
    }
});

/// Filter chip toolbar with server-derivable initial state.
///
/// `initial_*` lets the playground parse `?format=auction&shipping=free`
/// from the request URL and render the chips with their checked-visual
/// already correct on first paint. Refresh keeps both the cards
/// filtered (via the runtime coordinator) AND the chip buttons in
/// sync (via these initial values flowing into the rsjs signals).
///
/// Default is all-off — matches a no-query-param page load and keeps
/// the existing call sites that construct `FilterChips::default()`
/// behaving as they did before.
#[derive(Default)]
pub struct FilterChips {
    pub initial_buy_it_now: bool,
    pub initial_auction: bool,
    pub initial_free_shipping: bool,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for FilterChips {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let buy_it_now = signal::<bool>(self.initial_buy_it_now);
        let auction = signal::<bool>(self.initial_auction);
        let free_shipping = signal::<bool>(self.initial_free_shipping);
        html_sync! {
            <div class="srp-filter-chips" role="toolbar" aria-label="Listing filters">
                <button type="button" class="filter-chip" on_click={|_| buy_it_now.update(|v| !v)}>
                    <span>
                        {if buy_it_now.get() {
                            html_sync! { <span class="filter-chip-label filter-chip-label--active">"\u{2611} Buy It Now"</span> }
                        }}
                    </span>
                    <span>
                        {if !buy_it_now.get() {
                            html_sync! { <span class="filter-chip-label">"\u{2610} Buy It Now"</span> }
                        }}
                    </span>
                </button>
                <button
                    type="button"
                    class="filter-chip"
                    on_click={|_| auction.update(|v| !v)}
                >
                    <span>
                        {if auction.get() {
                            html_sync! { <span class="filter-chip-label filter-chip-label--active">"\u{2611} Auction"</span> }
                        }}
                    </span>
                    <span>
                        {if !auction.get() {
                            html_sync! { <span class="filter-chip-label">"\u{2610} Auction"</span> }
                        }}
                    </span>
                </button>
                <button
                    type="button"
                    class="filter-chip"
                    on_click={|_| free_shipping.update(|v| !v)}
                >
                    <span>
                        {if free_shipping.get() {
                            html_sync! { <span class="filter-chip-label filter-chip-label--active">"\u{2611} Free shipping"</span> }
                        }}
                    </span>
                    <span>
                        {if !free_shipping.get() {
                            html_sync! { <span class="filter-chip-label">"\u{2610} Free shipping"</span> }
                        }}
                    </span>
                </button>
            </div>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_asset_graph_records_only_rendered_islands() {
        // Route asset graph (rsjs/OPTIMIZATIONS.md § 1):
        // the generated render path records one artifact per hydrated
        // `#[rsjs(component, client)]` owner, so a page that renders
        // WatchListings60 + FilterChips must record both asset_ids and must
        // NOT record WatchHeart's id (different owner, never instantiated on
        // the consolidated route).
        use crate::ebay_srp::build_listings;
        use proa_core::{WebContext, WebRenderSync};

        let listings = build_listings(60);
        let listings_array: &[crate::ebay_srp::Listing; 60] =
            (&listings[..]).try_into().expect("60 listings");

        // First, locate the asset_ids the macro derived for the
        // three island types via the global manifest. The asset_id
        // is `<lower_struct>_<8hex>` so we match by struct_name.
        let manifest = rsjs::manifest();
        let watch_listings_id = manifest
            .islands
            .iter()
            .find(|i| i.struct_name == "WatchListings60")
            .expect("WatchListings60 registered in manifest")
            .asset_id;
        let filter_chips_id = manifest
            .islands
            .iter()
            .find(|i| i.struct_name == "FilterChips")
            .expect("FilterChips registered in manifest")
            .asset_id;
        let watch_heart_id = manifest
            .islands
            .iter()
            .find(|i| i.struct_name == "WatchHeart")
            .expect("WatchHeart registered in manifest")
            .asset_id;

        let mut buf = Vec::with_capacity(256 * 1024);
        let used = {
            let mut cx = WebContext::with_buffer(&mut buf);
            WatchListings60 {
                listings: listings_array,
            }
            .render(&mut cx)
            .expect("WatchListings60 renders");
            FilterChips::default()
                .render(&mut cx)
                .expect("FilterChips renders");
            cx.rsjs_artifacts_snapshot()
                .rendered_islands()
                .collect::<Vec<_>>()
        };

        assert!(
            used.contains(&watch_listings_id),
            "expected WatchListings60 asset_id ({watch_listings_id}) to be recorded, got {used:?}"
        );
        assert!(
            used.contains(&filter_chips_id),
            "expected FilterChips asset_id ({filter_chips_id}) to be recorded, got {used:?}"
        );
        assert!(
            !used.contains(&watch_heart_id),
            "WatchHeart ({watch_heart_id}) is NOT rendered on this route; \
             route-asset-graph leakage would re-introduce per-card islands. got {used:?}"
        );
    }

    #[test]
    fn route_asset_graph_dedupes_repeated_renders() {
        // WatchListings60 unrolls 60 cards via seq_macro inside a
        // single `#[rsjs(component, client)]` owner, so that path records one
        // rendered artifact. Validate the dedupe contract on a hand-rolled
        // loop: FilterChips rendered six times in a row should land in the set
        // exactly once.
        use proa_core::{WebContext, WebRenderSync};

        let mut buf = Vec::with_capacity(64 * 1024);
        let used = {
            let mut cx = WebContext::with_buffer(&mut buf);
            for _ in 0..6 {
                FilterChips::default().render(&mut cx).unwrap();
            }
            cx.rsjs_artifacts_snapshot()
                .rendered_islands()
                .collect::<Vec<_>>()
        };

        assert_eq!(
            used.len(),
            1,
            "expected one asset_id entry after six identical renders, got {used:?}"
        );
    }
}
