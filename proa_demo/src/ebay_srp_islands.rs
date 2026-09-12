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
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for WatchHeart {
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
    impl<'a, B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for WatchListings60<'a> {
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
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for FilterChips {
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
    use proa_core::render_to_string;

    #[test]
    fn watch_heart_emits_rsjs_island_wrapper() {
        let html = render_to_string(WatchHeart);
        assert!(
            html.starts_with(r#"<rsjs-island data-rsjs-island-root="watchheart_"#),
            "expected rsjs-island wrapper at the root of the WatchHeart render, got:\n{html}"
        );
    }

    #[test]
    fn watch_heart_emits_handler_marker() {
        let html = render_to_string(WatchHeart);
        assert!(
            html.contains(r#"data-rsjs-handler="0""#),
            "expected the on_click closure to be lifted into a handler marker, got:\n{html}"
        );
    }

    #[test]
    fn watch_heart_uses_ebay_icon_wrapper() {
        let html = render_to_string(WatchHeart);
        assert!(
            html.contains(r#"class="s-card__watchheart-icon""#),
            "expected the original eBay icon wrapper class, got:\n{html}"
        );
        assert!(
            !html.contains("s-card__watchheart-icon-on")
                && !html.contains("s-card__watchheart-icon-off"),
            "expected no custom icon wrapper classes that eBay CSS does not style, got:\n{html}"
        );
    }

    #[test]
    fn watch_heart_emits_two_conditional_icons() {
        let html = render_to_string(WatchHeart);
        assert!(
            html.contains(r#"data-rsjs-cond-then="#),
            "expected at least one conditional icon section, got:\n{html}"
        );
        // Both sprite refs are present (one branch hidden); hydration
        // is a pure display:none toggle.
        assert!(
            html.contains(r##"href="#icon-heart-16""##),
            "expected the empty-heart sprite ref in SSR, got:\n{html}"
        );
        assert!(
            html.contains(r##"href="#icon-heart-filled-16""##),
            "expected the filled-heart sprite ref in SSR (hidden branch), got:\n{html}"
        );
        let filled_idx = html.find(r##"href="#icon-heart-filled-16""##).unwrap();
        let surrounding = &html[filled_idx.saturating_sub(160)..filled_idx];
        assert!(
            surrounding.contains(r#"style="display:none""#),
            "expected the inactive (filled-heart) branch to carry display:none, surrounding:\n{surrounding}"
        );
    }

    #[test]
    fn filter_chips_write_url_on_click() {
        // The URL hash IS the source of truth for filter state, but
        // the chip rsjs handler bodies themselves only mutate their
        // local signals. The chip → URL projection happens in the
        // companion `ebay_srp_filter_runtime` coordinator, which
        // observes the chips' rendered DOM state on click and writes
        // `location.hash` from outside the island. Reason: rsjs v0.1
        // handler bodies cannot lower `window.location.hash = ...`
        // (see the module-level doc on `ebay_srp_filter_runtime`).
        //
        // This test pins the contract: (a) the chip island's
        // rendered HTML still carries the three handler markers it
        // always has, AND (b) the coordinator script defines the
        // URL-write surface.
        let html = render_to_string(FilterChips::default());
        assert_eq!(
            html.matches(r#"data-rsjs-handler="#).count(),
            3,
            "expected three click-handler markers (one per chip), got: {html}"
        );
        let runtime = crate::ebay_srp_filter_runtime::SRP_FILTER_RUNTIME_SCRIPT;
        assert!(
            runtime.contains("history.replaceState"),
            "filter runtime must write the URL via `history.replaceState` for the chip click projection"
        );
        assert!(
            runtime.contains("location.search"),
            "filter runtime must read `location.search` so the server can also see the URL state"
        );
        assert!(
            runtime.contains(".srp-filter-chips"),
            "filter runtime must hook the chip toolbar to project clicks to the URL"
        );
    }

    #[test]
    fn filter_chips_render_one_island_with_three_handlers() {
        let html = render_to_string(FilterChips::default());
        // One island wraps the entire toolbar.
        let island_count = html
            .matches(r#"<rsjs-island data-rsjs-island-root="filterchips_"#)
            .count();
        assert_eq!(
            island_count, 1,
            "expected exactly one filterchips island wrapper, got: {html}"
        );
        // Three independent click handlers (one per chip button).
        let handler_count = html.matches(r#"data-rsjs-handler="#).count();
        assert_eq!(
            handler_count, 3,
            "expected three handler markers (one per chip), got: {html}"
        );
        // Three bool signals seeded at false in one shared seed script.
        assert!(
            html.contains(r#"{"signals":[false,false,false]}"#),
            "expected three bool signals seeded false in one seed, got:\n{html}"
        );
    }

    #[test]
    fn filter_chips_initial_state_seeds_signals_for_url_round_trip() {
        // The playground parses `?format=auction&shipping=free` and
        // constructs a `FilterChips { initial_buy_it_now: false,
        // initial_auction: true, initial_free_shipping: true }`. The
        // rsjs runtime then seeds the chips' signals with those
        // initial values, so the chip buttons render their
        // checked-visual on first paint matching the URL.
        let chips = FilterChips {
            initial_buy_it_now: false,
            initial_auction: true,
            initial_free_shipping: true,
        };
        let html = render_to_string(chips);
        assert!(
            html.contains(r#"{"signals":[false,true,true]}"#),
            "expected initial-state booleans to flow into the seed, got:\n{html}"
        );
    }

    #[test]
    fn interactive_listings_emit_one_island_per_card() {
        use crate::ebay_srp::{build_listings, InteractiveListingsList};
        let listings = build_listings(60);
        let html = proa_core::render_to_string(InteractiveListingsList {
            listings: &listings,
        });
        let island_count = html
            .matches(r#"<rsjs-island data-rsjs-island-root="watchheart_"#)
            .count();
        assert_eq!(
            island_count, 60,
            "expected 60 watch-heart islands for 60 listings, got {island_count}"
        );
        let seed_count = html
            .matches(r#"<script type="application/json" data-rsjs-island="watchheart_"#)
            .count();
        assert_eq!(
            seed_count, 60,
            "expected 60 seed scripts (one per island), got {seed_count}"
        );
    }

    #[test]
    fn watch_listings60_emits_one_island_with_60_handlers() {
        use crate::ebay_srp::build_listings;
        let listings = build_listings(60);
        let listings_array: &[crate::ebay_srp::Listing; 60] =
            (&listings[..]).try_into().expect("60 listings");
        let html = render_to_string(WatchListings60 {
            listings: listings_array,
        });

        // The headline consolidation: one island wrapper, one seed
        // script, in place of 60 of each.
        let island_count = html
            .matches(r#"<rsjs-island data-rsjs-island-root="watchlistings60_"#)
            .count();
        assert_eq!(
            island_count, 1,
            "expected exactly one consolidated island wrapper, got {island_count}"
        );
        let seed_count = html
            .matches(r#"<script type="application/json" data-rsjs-island="watchlistings60_"#)
            .count();
        assert_eq!(
            seed_count, 1,
            "expected exactly one consolidated seed script, got {seed_count}"
        );

        // 60 buttons → 60 handler markers.
        let handler_count = html.matches(r#"data-rsjs-handler="#).count();
        assert_eq!(
            handler_count, 60,
            "expected 60 handler markers (one per card), got {handler_count}"
        );

        // Seed JSON carries 60 booleans (one per card signal).
        let expected_seed = format!("{{\"signals\":[{}]}}", vec!["false"; 60].join(","));
        assert!(
            html.contains(&expected_seed),
            "expected seed with 60 false booleans, got:\n{}",
            &html[..html.len().min(2000)]
        );

        // The 60 cards render between the open `<ul>` and close
        // `</ul>` tags inside the rsjs island wrapper.
        let li_open_count = html
            .matches(r#"<li class="s-card s-card--horizontal">"#)
            .count();
        assert_eq!(
            li_open_count, 60,
            "expected 60 <li> wrappers, got {li_open_count}"
        );
    }

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

    #[test]
    fn watch_heart_emits_signal_seed() {
        let html = render_to_string(WatchHeart);
        assert!(
            html.contains(r#"<script type="application/json" data-rsjs-island="#),
            "expected the seed-state script tag, got:\n{html}"
        );
        assert!(
            html.contains(r#"{"signals":[false]}"#),
            "expected one bool signal initialised to false in the seed, got:\n{html}"
        );
    }
}
