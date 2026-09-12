// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Airbnb Showcase — Search Results
//!
//! Full-page standalone search results with filter pills, split layout, and map placeholder.

use super::image::{AirImage, AirImageFetchPriority, AirImageLoading};
use super::shared;
use proa_core::md::MdContext;
use proa_core::{
    MdRenderSync, RenderBuffer, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::{html_sync, md_sync};

const STAR_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="#FF385C"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/></svg>"##;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

pub struct SearchListing<'a> {
    pub title: &'a str,
    pub location: &'a str,
    pub price_per_night: u32,
    pub rating: &'a str,
    pub reviews_count: u32,
    pub property_type: &'a str,
    pub beds: u8,
    pub baths: u8,
    pub image: &'a str,
}

pub struct SearchData<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub stay_info: &'a str,
    pub filters: &'a [&'a str],
    pub listings: &'a [SearchListing<'a>],
}

impl SearchData<'static> {
    pub fn default_malibu() -> Self {
        SearchData {
            title: "Malibu \u{2014} Vacation Rentals",
            subtitle: "Entire homes and unique stays",
            stay_info: "Feb 15\u{2013}22 \u{00b7} 2 guests",
            filters: &[
                "Price",
                "Type of place",
                "Rooms and beds",
                "Amenities",
                "Book instantly",
            ],
            listings: &[
                SearchListing {
                    title: "Malibu Beach House",
                    location: "Malibu, California",
                    price_per_night: 350,
                    rating: "4.92",
                    reviews_count: 128,
                    property_type: "Entire home",
                    beds: 3,
                    baths: 2,
                    image: "/images/airbnb/bedroom.jpg",
                },
                SearchListing {
                    title: "Santa Monica Studio",
                    location: "Santa Monica, California",
                    price_per_night: 185,
                    rating: "4.87",
                    reviews_count: 94,
                    property_type: "Entire guest suite",
                    beds: 1,
                    baths: 1,
                    image: "/images/airbnb/deck-wine.jpg",
                },
                SearchListing {
                    title: "Pacific Palisades Villa",
                    location: "Pacific Palisades, California",
                    price_per_night: 520,
                    rating: "4.95",
                    reviews_count: 67,
                    property_type: "Entire villa",
                    beds: 5,
                    baths: 4,
                    image: "/images/airbnb/exterior.jpg",
                },
                SearchListing {
                    title: "Topanga Canyon Cabin",
                    location: "Topanga, California",
                    price_per_night: 210,
                    rating: "4.89",
                    reviews_count: 156,
                    property_type: "Entire cabin",
                    beds: 2,
                    baths: 1,
                    image: "/images/airbnb/ocean-view.jpg",
                },
                SearchListing {
                    title: "Venice Beach Loft",
                    location: "Venice, California",
                    price_per_night: 275,
                    rating: "4.78",
                    reviews_count: 203,
                    property_type: "Entire loft",
                    beds: 2,
                    baths: 2,
                    image: "/images/airbnb/deck.jpg",
                },
                SearchListing {
                    title: "Malibu Airstream Suite",
                    location: "Malibu, California",
                    price_per_night: 295,
                    rating: "4.80",
                    reviews_count: 247,
                    property_type: "Tiny home",
                    beds: 1,
                    baths: 1,
                    image: "/images/airbnb/bedroom.jpg",
                },
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub fn render() -> String {
    let data = SearchData::default_malibu();
    let mut buf = RenderBuffer::new();
    render_into(&mut WebContext::with_buffer(&mut buf), &data);
    buf.into_string()
}

pub fn render_into<B: proa_core::WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &SearchData<'_>,
) {
    shared::render_document_start(cx, &[data.title, " | Airbnb"]);
    shared::render_header(cx);

    // Title + filters
    html_sync! {
        <div class="air-shell pt-8 pb-5">
            <p class="mb-2 text-sm font-medium text-[var(--air-muted)]">{data.subtitle}</p>
            <h1 class="mb-1 text-[32px] font-semibold tracking-[-0.04em] text-[#222]">
                {data.title}
            </h1>
            <p class="mb-5 text-sm text-[var(--air-muted)]">
                {data.listings.len()}
                " stays \u{00b7} "
                {data.stay_info}
            </p>
            <div class="flex flex-wrap gap-2">
                {for filter in data.filters.iter() {
                    html_sync! {
                        <button class="filter-pill rounded-full border border-[var(--air-line-strong)] bg-white px-4 py-2.5 text-sm text-[#222] transition-colors hover:border-[#222]">
                            {*filter}
                        </button>
                    }.render(cx);
                }}
            </div>
        </div>
    }
    .render(cx);

    // Split layout: listings + map
    html_sync! {
        <div class="air-shell pb-8">
            <div class="grid grid-cols-1 gap-8 xl:grid-cols-[minmax(0,58%)_minmax(360px,42%)]">
                // Listings column
                <div class="divide-y divide-[var(--air-line)]">
                    {for (i, listing) in data.listings.iter().enumerate() {
                        let badge_text = if i % 2 == 0 { "Guest favorite" } else { "" };
                        let badge_class = if i % 2 == 0 {
                            "absolute left-3 top-3 rounded-full bg-white/95 px-3 py-1 text-xs font-semibold text-[#222] shadow-sm"
                        } else {
                            "hidden"
                        };
                        html_sync! {
                            <a href="/showcase/airbnb/proa/listing" class="search-result-card group grid grid-cols-1 gap-5 rounded-[20px] py-6 transition-colors md:grid-cols-[315px_minmax(0,1fr)]" data-map-index={i}>
                                <div class="air-card-image relative md:h-[212px]">
                                    {AirImage {
                                        id: None,
                                        src: listing.image,
                                        alt: listing.title,
                                        class: Some("h-full w-full object-cover"),
                                        loading: AirImageLoading::Lazy,
                                        fetch_priority: AirImageFetchPriority::Low,
                                        fallback_src: None,
                                    }}
                                    <span class={badge_class}>{badge_text}</span>
                                    <button class="heart-btn absolute right-3 top-3" aria-label="Save to favorites">
                                        {StaticRaw(shared::HEART_SVG)}
                                    </button>
                                </div>
                                <div class="flex min-w-0 flex-col justify-between">
                                    <div>
                                        <p class="text-sm text-[var(--air-muted)]">
                                            {listing.property_type}
                                            " \u{00b7} "
                                            {listing.beds}" bed"{if listing.beds != 1 { "s" } else { "" }}
                                            " \u{00b7} "
                                            {listing.baths}" bath"{if listing.baths != 1 { "s" } else { "" }}
                                        </p>
                                        <h3 class="mt-2 text-[22px] font-medium leading-7 tracking-[-0.02em] text-[#222]">
                                            {listing.title}
                                        </h3>
                                        <div class="my-4 h-px w-10 bg-[var(--air-line)]"></div>
                                        <p class="text-[15px] leading-5 text-[var(--air-muted)]">{listing.location}</p>
                                        <p class="mt-1 text-[15px] leading-5 text-[var(--air-muted)]">
                                            "Free cancellation for 48 hours \u{00b7} Ocean-facing deck"
                                        </p>
                                    </div>
                                    <div class="mt-6 flex items-end justify-between gap-4">
                                        <div class="flex items-center gap-1 text-sm text-[#222]">
                                            {StaticRaw(STAR_SVG)}
                                            <span>{listing.rating}</span>
                                            <span class="text-[var(--air-muted)]">"("{listing.reviews_count}" reviews)"</span>
                                        </div>
                                        <p class="text-right text-[15px] leading-5 text-[#222]">
                                            <span class="font-semibold">"$"{listing.price_per_night}</span>
                                            " night"
                                            <span class="block text-[var(--air-muted)]">"7 nights total available"</span>
                                        </p>
                                    </div>
                                </div>
                            </a>
                        }.render(cx);
                    }}
                </div>

                // Map placeholder
                <div class="hidden xl:block">
                    <div class="air-map sticky top-24 h-[calc(100vh-8rem)] overflow-hidden rounded-[24px] border border-[var(--air-line)]">
                        <div class="absolute left-[18%] top-[18%] h-36 w-52 rounded-full bg-white/45 blur-3xl"></div>
                        <div class="absolute inset-0 opacity-40">
                            <svg viewBox="0 0 600 900" class="h-full w-full" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
                                <path d="M32 160C170 70 300 112 372 184c84 84 172 108 196 186 30 98-34 184-110 232-122 76-230 34-316 92-80 54-118 138-118 138" fill="none" stroke="#d7d2c7" stroke-width="20" stroke-linecap="round"/>
                                <path d="M110 94c56 46 116 66 166 62 74-6 98 18 158 74" fill="none" stroke="#cfd7cb" stroke-width="16" stroke-linecap="round"/>
                                <path d="M420 530c18 48 18 106-4 162" fill="none" stroke="#d7d2c7" stroke-width="14" stroke-linecap="round"/>
                            </svg>
                        </div>
                        {if !data.listings.is_empty() { html_sync! { <button class="air-price-pin left-[20%] top-[24%]" data-map-pin="0" type="button">"$"{data.listings[0].price_per_night}</button> }.render(cx); }}
                        {if data.listings.len() > 5 { html_sync! { <button class="air-price-pin left-[58%] top-[19%]" data-map-pin="5" type="button">"$"{data.listings[5].price_per_night}</button> }.render(cx); }}
                        {if data.listings.len() > 1 { html_sync! { <button class="air-price-pin left-[50%] top-[41%]" data-map-pin="1" type="button">"$"{data.listings[1].price_per_night}</button> }.render(cx); }}
                        {if data.listings.len() > 2 { html_sync! { <button class="air-price-pin left-[26%] top-[56%]" data-map-pin="2" type="button">"$"{data.listings[2].price_per_night}</button> }.render(cx); }}
                        {if data.listings.len() > 4 { html_sync! { <button class="air-price-pin left-[64%] top-[63%]" data-map-pin="4" type="button">"$"{data.listings[4].price_per_night}</button> }.render(cx); }}
                        <div class="absolute bottom-5 left-5 rounded-full bg-[#222] px-5 py-3 text-sm font-semibold text-white shadow-lg">
                            "Show map"
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
    .render(cx);

    shared::render_footer(cx);
    shared::render_document_end(cx);
}

// ---------------------------------------------------------------------------
// Markdown rendering — `MdRenderSync` trait impls + `md_sync!` templates.
//
// Same composability story as the HTML side: each data type owns its
// own Markdown shape via an `MdRenderSync<B>` impl, and `md_sync!` raw
// templates compose them through `{expr}` children. The page
// render is a single template that iterates `{ for listing in ... }`
// and the inner `{listing}` slot dispatches into the per-listing
// impl below — exactly mirroring how `html_sync!` composes
// `WebRenderSync` children.
// ---------------------------------------------------------------------------

/// One search listing renders as a heading + meta + price + link block.
///
/// Implemented on `&SearchListing<'a>` rather than `SearchListing<'a>` so
/// the page-level template can iterate `&'a [SearchListing<'a>]` without
/// cloning or moving — the listing data is read-only for render.
impl<'a, B: WriteBuf> MdRenderSync<B> for &SearchListing<'a> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        let beds_suffix = if self.beds == 1 { "" } else { "s" };
        let baths_suffix = if self.baths == 1 { "" } else { "s" };
        md_sync! { r###"
            ### {self.title}

            {self.property_type} · {self.beds} bed{beds_suffix} · {self.baths} bath{baths_suffix}

            {self.location} · **${self.price_per_night}/night** · ★{self.rating} ({self.reviews_count} reviews)

            [View listing](/showcase/airbnb/proa/listing)
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()
    }
}

/// The search page = title block + summary + listings list. The
/// `{ for listing in self.listings { listing } }` block dispatches
/// into the per-listing impl above — pure composition, no procedural
/// orchestration in this template.
impl<'a, B: WriteBuf> MdRenderSync<B> for &SearchData<'a> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            # {self.title}

            {self.listings.len()} stays · {self.stay_info}

            ---

            {for listing in self.listings {
            {listing}}}
            "###
        }
        .render_md(cx)
    }
}

/// Render the Airbnb Search showcase as Markdown.
///
/// Thin entry point: build default fixture, dispatch through the
/// `MdRenderSync` trait. All formatting decisions live in the `md_sync!`
/// templates on the impls above.
pub fn render_md() -> String {
    let data = SearchData::default_malibu();
    let mut buf = RenderBuffer::new();
    let mut cx = MdContext::with_buffer(&mut buf);
    (&data).render_md(&mut cx).unwrap();
    String::from_utf8(buf.to_vec()).expect("MdRenderSync writes UTF-8")
}
