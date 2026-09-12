// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Airbnb Showcase — Homepage
//!
//! Standalone Airbnb-style homepage with mode tabs, hero search, and content rails.

use super::image::{AirImage, AirImageFetchPriority, AirImageLoading};
use super::shared;
use proa_core::md::MdContext;
use proa_core::{RenderBuffer, StaticRaw, WebContext, WebRenderSync};
use proa_macros::html_sync;

const STAR_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="#FF385C"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/></svg>"##;
const SEARCH_SVG: &str = r##"<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg" style="display:block;fill:none;height:16px;width:16px;stroke:currentColor;stroke-width:4;overflow:visible" aria-hidden="true" role="presentation" focusable="false"><path d="m13 24c6.0751 0 11-4.9249 11-11 0-6.0751-4.9249-11-11-11-6.0751 0-11 4.9249-11 11 0 6.0751 4.9249 11 11 11zm8-3 10 10"/></svg>"##;
const HOUSE_POSTER: &str = "/images/airbnb/external/4aae4ed7-5939-4e76-b100-e69440ebeae4.png";
const BALLOON_POSTER: &str = "/images/airbnb/external/search-icon-1.png";
const SERVICE_POSTER: &str = "/images/airbnb/external/search-icon-2.png";

#[allow(dead_code)]
const HOME_IMAGES: &[&str] = &[
    "/images/airbnb/bedroom.jpg",
    "/images/airbnb/deck-wine.jpg",
    "/images/airbnb/exterior.jpg",
    "/images/airbnb/ocean-view.jpg",
    "/images/airbnb/deck.jpg",
    "/images/airbnb/bedroom.jpg",
    "/images/airbnb/exterior.jpg",
    "/images/airbnb/deck.jpg",
];

#[allow(dead_code)]
const HOMES_SECTIONS: &[(&str, &str, &[usize])] = &[
    ("Popular homes in Los Angeles", "", &[0, 1, 2, 3, 4, 5, 0]),
    (
        "Featured hotels in Los Angeles",
        "A collection of independent and handpicked hotels",
        &[2, 4, 1, 5, 3, 0, 2],
    ),
    (
        "Stay by the beach in Malibu",
        "Ocean-facing stays with relaxed design details",
        &[5, 0, 1, 4, 2, 3, 5],
    ),
];

#[allow(dead_code)]
const EXPERIENCES_SECTIONS: &[(&str, &str, &[usize])] = &[
    (
        "Book experiences led by locals",
        "Design-minded activities around Los Angeles",
        &[1, 3, 5, 0, 2, 4, 1],
    ),
    (
        "Food, movement, and culture",
        "Small-group experiences with polished presentation",
        &[4, 2, 0, 5, 1, 3, 4],
    ),
];

#[allow(dead_code)]
const SERVICES_SECTIONS: &[(&str, &str, &[usize])] = &[
    (
        "Services for your stay",
        "Private chefs, photography, and wellness add-ons",
        &[2, 5, 3, 1, 4, 0, 2],
    ),
    (
        "Top-rated service pros nearby",
        "Curated services that feel native to the Airbnb flow",
        &[3, 0, 4, 1, 5, 2, 3],
    ),
];

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

pub struct HomeListing<'a> {
    pub title: &'a str,
    pub location: &'a str,
    pub price_per_night: u32,
    pub rating: &'a str,
    pub property_type: &'a str,
    pub image: &'a str,
}

pub struct HomeSection<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub listing_indexes: &'a [usize],
}

pub struct HomeData<'a> {
    pub listings: &'a [HomeListing<'a>],
    pub homes_sections: &'a [HomeSection<'a>],
    pub experiences_sections: &'a [HomeSection<'a>],
    pub services_sections: &'a [HomeSection<'a>],
}

impl HomeData<'static> {
    pub fn default_la() -> Self {
        HomeData {
            listings: &[
                HomeListing {
                    title: "Malibu Beach House",
                    location: "Malibu, California",
                    price_per_night: 350,
                    rating: "4.92",
                    property_type: "Entire home",
                    image: "/images/airbnb/bedroom.jpg",
                },
                HomeListing {
                    title: "Santa Monica Studio",
                    location: "Santa Monica, California",
                    price_per_night: 185,
                    rating: "4.87",
                    property_type: "Entire guest suite",
                    image: "/images/airbnb/deck-wine.jpg",
                },
                HomeListing {
                    title: "Pacific Palisades Villa",
                    location: "Pacific Palisades, California",
                    price_per_night: 520,
                    rating: "4.95",
                    property_type: "Entire villa",
                    image: "/images/airbnb/exterior.jpg",
                },
                HomeListing {
                    title: "Topanga Canyon Cabin",
                    location: "Topanga, California",
                    price_per_night: 210,
                    rating: "4.89",
                    property_type: "Entire cabin",
                    image: "/images/airbnb/ocean-view.jpg",
                },
                HomeListing {
                    title: "Venice Beach Loft",
                    location: "Venice, California",
                    price_per_night: 275,
                    rating: "4.78",
                    property_type: "Entire loft",
                    image: "/images/airbnb/deck.jpg",
                },
                HomeListing {
                    title: "Malibu Airstream Suite",
                    location: "Malibu, California",
                    price_per_night: 295,
                    rating: "4.80",
                    property_type: "Tiny home",
                    image: "/images/airbnb/bedroom.jpg",
                },
            ],
            homes_sections: &[
                HomeSection {
                    title: "Popular homes in Los Angeles",
                    subtitle: "",
                    listing_indexes: &[0, 1, 2, 3, 4, 5, 0],
                },
                HomeSection {
                    title: "Featured hotels in Los Angeles",
                    subtitle: "A collection of independent and handpicked hotels",
                    listing_indexes: &[2, 4, 1, 5, 3, 0, 2],
                },
                HomeSection {
                    title: "Stay by the beach in Malibu",
                    subtitle: "Ocean-facing stays with relaxed design details",
                    listing_indexes: &[5, 0, 1, 4, 2, 3, 5],
                },
            ],
            experiences_sections: &[
                HomeSection {
                    title: "Book experiences led by locals",
                    subtitle: "Design-minded activities around Los Angeles",
                    listing_indexes: &[1, 3, 5, 0, 2, 4, 1],
                },
                HomeSection {
                    title: "Food, movement, and culture",
                    subtitle: "Small-group experiences with polished presentation",
                    listing_indexes: &[4, 2, 0, 5, 1, 3, 4],
                },
            ],
            services_sections: &[
                HomeSection {
                    title: "Services for your stay",
                    subtitle: "Private chefs, photography, and wellness add-ons",
                    listing_indexes: &[2, 5, 3, 1, 4, 0, 2],
                },
                HomeSection {
                    title: "Top-rated service pros nearby",
                    subtitle: "Curated services that feel native to the Airbnb flow",
                    listing_indexes: &[3, 0, 4, 1, 5, 2, 3],
                },
            ],
        }
    }
}

/// Pre-computed rail IDs — avoids format!() in the hot render path.
const HOMES_RAIL_IDS: [&str; 8] = [
    "homes-rail-0",
    "homes-rail-1",
    "homes-rail-2",
    "homes-rail-3",
    "homes-rail-4",
    "homes-rail-5",
    "homes-rail-6",
    "homes-rail-7",
];
const EXPERIENCES_RAIL_IDS: [&str; 8] = [
    "experiences-rail-0",
    "experiences-rail-1",
    "experiences-rail-2",
    "experiences-rail-3",
    "experiences-rail-4",
    "experiences-rail-5",
    "experiences-rail-6",
    "experiences-rail-7",
];
const SERVICES_RAIL_IDS: [&str; 8] = [
    "services-rail-0",
    "services-rail-1",
    "services-rail-2",
    "services-rail-3",
    "services-rail-4",
    "services-rail-5",
    "services-rail-6",
    "services-rail-7",
];

pub fn render() -> String {
    let data = HomeData::default_la();
    let mut buf = RenderBuffer::new();
    render_into(&mut WebContext::with_buffer(&mut buf), &data);
    buf.into_string()
}

pub fn render_into<B: proa_core::WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &HomeData<'_>,
) {
    shared::render_document_start(cx, &["Airbnb - Vacation Rentals, Cabins, Beach Houses"]);
    render_content(cx, data);
    shared::render_document_end(cx);
}

/// Render just the page content (no document shell). Used by benchmarks.
pub fn render_content<B: proa_core::WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &HomeData<'_>,
) {
    html_sync! {
        <header class="air-topbar">
            <div class="air-shell pt-5 pb-6 md:pt-6 md:pb-7">
                <div class="grid grid-cols-[auto_1fr_auto] items-center gap-4 md:gap-8">
                    <a
                        href="/showcase/airbnb/proa"
                        class="flex items-center gap-2.5 text-[#FF385C]"
                    >
                        {StaticRaw(shared::AIRBNB_LOGO_SVG)}
                        <span class="text-[30px] font-semibold leading-none tracking-[-0.02em]">
                            "airbnb"
                        </span>
                    </a>
                    <nav
                        class="hidden md:flex items-center justify-center gap-8 lg:gap-10"
                        role="tablist"
                        aria-label="Search modes"
                    >
                        <button
                            class="home-mode-tab active"
                            data-home-mode="homes"
                            role="tab"
                            aria-selected="true"
                            type="button"
                        >
                            <span class="home-mode-icon">
                                {AirImage {
                                    id: None,
                                    src: HOUSE_POSTER,
                                    alt: "",
                                    class: Some("home-mode-media"),
                                    loading: AirImageLoading::Eager,
                                    fetch_priority: AirImageFetchPriority::Low,
                                    fallback_src: None,
                                }}
                            </span>
                            <span>"Homes"</span>
                        </button>
                        <button
                            class="home-mode-tab"
                            data-home-mode="experiences"
                            role="tab"
                            aria-selected="false"
                            type="button"
                        >
                            <span class="home-mode-icon relative">
                                {AirImage {
                                    id: None,
                                    src: BALLOON_POSTER,
                                    alt: "",
                                    class: Some("home-mode-media"),
                                    loading: AirImageLoading::Eager,
                                    fetch_priority: AirImageFetchPriority::Low,
                                    fallback_src: None,
                                }}
                                <span class="home-mode-badge">"NEW"</span>
                            </span>
                            <span>"Experiences"</span>
                        </button>
                        <button
                            class="home-mode-tab"
                            data-home-mode="services"
                            role="tab"
                            aria-selected="false"
                            type="button"
                        >
                            <span class="home-mode-icon relative">
                                {AirImage {
                                    id: None,
                                    src: SERVICE_POSTER,
                                    alt: "",
                                    class: Some("home-mode-media"),
                                    loading: AirImageLoading::Eager,
                                    fetch_priority: AirImageFetchPriority::Low,
                                    fallback_src: None,
                                }}
                                <span class="home-mode-badge">"NEW"</span>
                            </span>
                            <span>"Services"</span>
                        </button>
                    </nav>
                    <div class="flex items-center justify-end gap-2 md:gap-3">
                        <a
                            href="/showcase/airbnb/proa"
                            class="hidden rounded-full px-4 py-3 text-sm font-semibold hover:bg-[var(--air-soft)] lg:block"
                        >
                            "Become a host"
                        </a>
                        <button
                            class="hidden md:flex h-10 w-10 items-center justify-center rounded-full hover:bg-[var(--air-soft)]"
                            aria-label="Language"
                        >
                            {StaticRaw(shared::GLOBE_SVG)}
                        </button>
                        <button class="air-search-pill flex items-center gap-3 rounded-full border border-[var(--air-line)] bg-white pl-3 pr-2.5 py-1.5">
                            <svg
                                xmlns="http://www.w3.org/2000/svg"
                                width="16"
                                height="16"
                                viewBox="0 0 32 32"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="3"
                                stroke-linecap="round"
                                aria-hidden="true"
                            >
                                <line x1="6" y1="9" x2="26" y2="9" />
                                <line x1="6" y1="16" x2="26" y2="16" />
                                <line x1="6" y1="23" x2="26" y2="23" />
                            </svg>
                            <div class="flex h-8 w-8 items-center justify-center rounded-full bg-[#6A6A6A] text-white">
                                <svg
                                    xmlns="http://www.w3.org/2000/svg"
                                    width="15"
                                    height="15"
                                    viewBox="0 0 24 24"
                                    fill="currentColor"
                                >
                                    <path d="M12 12c2.209 0 4-1.791 4-4S14.209 4 12 4s-4 1.791-4 4 1.791 4 4 4zm0 2c-2.673 0-8 1.327-8 3.963V20h16v-2.037C20 15.327 14.673 14 12 14z" />
                                </svg>
                            </div>
                        </button>
                    </div>
                </div>
                <div class="mt-5 md:hidden">
                    <div class="flex items-center gap-4 overflow-x-auto pb-1">
                        <button class="home-mode-tab active" data-home-mode="homes" type="button">
                            <span class="home-mode-icon">
                                {AirImage {
                                    id: None,
                                    src: HOUSE_POSTER,
                                    alt: "",
                                    class: Some("home-mode-media"),
                                    loading: AirImageLoading::Eager,
                                    fetch_priority: AirImageFetchPriority::Low,
                                    fallback_src: None,
                                }}
                            </span>
                            <span>"Homes"</span>
                        </button>
                        <button class="home-mode-tab" data-home-mode="experiences" type="button">
                            <span class="home-mode-icon relative">
                                {AirImage {
                                    id: None,
                                    src: BALLOON_POSTER,
                                    alt: "",
                                    class: Some("home-mode-media"),
                                    loading: AirImageLoading::Eager,
                                    fetch_priority: AirImageFetchPriority::Low,
                                    fallback_src: None,
                                }}
                                <span class="home-mode-badge">"NEW"</span>
                            </span>
                            <span>"Experiences"</span>
                        </button>
                        <button class="home-mode-tab" data-home-mode="services" type="button">
                            <span class="home-mode-icon relative">
                                {AirImage {
                                    id: None,
                                    src: SERVICE_POSTER,
                                    alt: "",
                                    class: Some("home-mode-media"),
                                    loading: AirImageLoading::Eager,
                                    fetch_priority: AirImageFetchPriority::Low,
                                    fallback_src: None,
                                }}
                                <span class="home-mode-badge">"NEW"</span>
                            </span>
                            <span>"Services"</span>
                        </button>
                    </div>
                </div>
                <div class="mt-6 flex justify-center">
                    <div class="home-search-shell w-full max-w-[850px]">
                        <div class="home-search-bar">
                            <button
                                class="home-search-segment active"
                                data-search-field="where"
                                data-default="Search destinations"
                                type="button"
                            >
                                <span class="home-search-label">"Where"</span>
                                <span class="home-search-value">"Search destinations"</span>
                            </button>
                            <span class="home-search-divider"></span>
                            <button
                                class="home-search-segment"
                                data-search-field="when"
                                data-default="Add dates"
                                type="button"
                            >
                                <span class="home-search-label">"When"</span>
                                <span class="home-search-value">"Add dates"</span>
                            </button>
                            <span class="home-search-divider"></span>
                            <button
                                class="home-search-segment"
                                data-search-field="who"
                                data-default="Add guests"
                                type="button"
                            >
                                <span class="home-search-label">"Who"</span>
                                <span class="home-search-value">"Add guests"</span>
                            </button>
                            <button
                                class="home-search-submit"
                                data-home-search-submit
                                type="button"
                                aria-label="Search"
                            >
                                {StaticRaw(SEARCH_SVG)}
                            </button>
                        </div>
                        <div class="home-search-popover" id="home-search-popover">
                            <div class="mb-3 flex items-center justify-between gap-3">
                                <div>
                                    <p class="text-sm font-semibold text-[#222]" data-popover-title>
                                        "Where do you want to go?"
                                    </p>
                                    <p
                                        class="text-sm text-[var(--air-muted)]"
                                        data-popover-subtitle
                                    >
                                        "Choose a preset to update the search bar."
                                    </p>
                                </div>
                                <button
                                    class="rounded-full px-3 py-1.5 text-sm font-medium hover:bg-[var(--air-soft)]"
                                    data-home-search-close
                                    type="button"
                                >
                                    "Done"
                                </button>
                            </div>
                            <div class="flex flex-wrap gap-2" data-popover-options></div>
                        </div>
                    </div>
                </div>
            </div>
        </header>
        <main class="bg-white">
            <div class="home-mode-panel active" data-home-panel="homes">
                <div class="air-shell py-10 md:py-12">
                    {for (section_index, section) in data.homes_sections.iter().enumerate() {
                        let rail_id = HOMES_RAIL_IDS[section_index];
                        html_sync! {
                            <section class="home-rail-section">
                                <div class="mb-4 flex items-center justify-between gap-4">
                                    <div>
                                        <div class="flex items-center gap-2">
                                            <h2 class="text-[28px] font-semibold tracking-[-0.03em] text-[#222]">{section.title}</h2>
                                            <span class="hidden md:inline-flex h-7 w-7 items-center justify-center rounded-full bg-[var(--air-soft)] text-[#222]">
                                                <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><line x1="5" y1="12" x2="19" y2="12"/><line x1="12" y1="5" x2="19" y2="12"/><line x1="12" y1="19" x2="19" y2="12"/></svg>
                                            </span>
                                        </div>
                                        {if !section.subtitle.is_empty() {
                                            html_sync! { <p class="mt-1 text-[15px] text-[var(--air-muted)]">{section.subtitle}</p> }.render(cx);
                                        }}
                                    </div>
                                    <div class="hidden items-center gap-2 md:flex">
                                        <button class="home-rail-arrow" data-rail-prev={rail_id} type="button" aria-label="Scroll left">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
                                        </button>
                                        <button class="home-rail-arrow" data-rail-next={rail_id} type="button" aria-label="Scroll right">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
                                        </button>
                                    </div>
                                </div>
                                <div class="home-rail" id={rail_id}>
                                    {for listing_index in section.listing_indexes.iter() {
                                        let listing = &data.listings[*listing_index % data.listings.len()];
                                        let image = listing.image;
                                        let price = listing.price_per_night * 2;
                                        html_sync! {
                                            <a href="/showcase/airbnb/proa/listing" class="home-rail-card group">
                                                <div class="air-card-image relative aspect-[1.03/1] bg-[var(--air-soft)]">
                                                    {AirImage {
                                                        id: None,
                                                        src: image,
                                                        alt: listing.title,
                                                        class: Some("h-full w-full object-cover"),
                                                        loading: AirImageLoading::Lazy,
                                                        fetch_priority: AirImageFetchPriority::Low,
                                                        fallback_src: None,
                                                    }}
                                                    <span class="absolute left-3 top-3 rounded-full bg-white/95 px-3 py-1 text-xs font-semibold text-[#222] shadow-sm">"Guest favorite"</span>
                                                    <button class="heart-btn absolute right-3 top-3" aria-label="Save to favorites">{StaticRaw(shared::HEART_SVG)}</button>
                                                </div>
                                                <div class="mt-3">
                                                    <p class="truncate text-[17px] font-medium leading-5 text-[#222]">{listing.title}</p>
                                                    <p class="mt-1 text-[15px] leading-5 text-[var(--air-muted)]">{listing.property_type}</p>
                                                    <p class="flex items-center gap-1 text-[15px] leading-5 text-[var(--air-muted)]">
                                                        <span>"$"{price}" for 2 nights"</span>
                                                        {StaticRaw(STAR_SVG)}
                                                        <span>{listing.rating}</span>
                                                    </p>
                                                </div>
                                            </a>
                                        }.render(cx);
                                    }}
                                </div>
                            </section>
                        }.render(cx);
                    }}
                </div>
            </div>
            <div class="home-mode-panel" data-home-panel="experiences">
                <div class="air-shell py-10 md:py-12">
                    {for (section_index, section) in data.experiences_sections.iter().enumerate() {
                        let rail_id = EXPERIENCES_RAIL_IDS[section_index];
                        html_sync! {
                            <section class="home-rail-section">
                                <div class="mb-4 flex items-center justify-between gap-4">
                                    <div>
                                        <h2 class="text-[28px] font-semibold tracking-[-0.03em] text-[#222]">{section.title}</h2>
                                        <p class="mt-1 text-[15px] text-[var(--air-muted)]">{section.subtitle}</p>
                                    </div>
                                    <div class="hidden items-center gap-2 md:flex">
                                        <button class="home-rail-arrow" data-rail-prev={rail_id} type="button" aria-label="Scroll left"><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg></button>
                                        <button class="home-rail-arrow" data-rail-next={rail_id} type="button" aria-label="Scroll right"><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg></button>
                                    </div>
                                </div>
                                <div class="home-rail" id={rail_id}>
                                    {for listing_index in section.listing_indexes.iter() {
                                        let listing = &data.listings[*listing_index % data.listings.len()];
                                        let image = listing.image;
                                        let price = listing.price_per_night / 2 + 42;
                                        html_sync! {
                                            <a href="/showcase/airbnb/proa/search" class="home-rail-card group">
                                                <div class="air-card-image relative aspect-[1.03/1] bg-[var(--air-soft)]">
                                                    {AirImage {
                                                        id: None,
                                                        src: image,
                                                        alt: listing.title,
                                                        class: Some("h-full w-full object-cover"),
                                                        loading: AirImageLoading::Lazy,
                                                        fetch_priority: AirImageFetchPriority::Low,
                                                        fallback_src: None,
                                                    }}
                                                    <span class="absolute left-3 top-3 rounded-full bg-white/95 px-3 py-1 text-xs font-semibold text-[#222] shadow-sm">"Most loved"</span>
                                                    <button class="heart-btn absolute right-3 top-3" aria-label="Save to favorites">{StaticRaw(shared::HEART_SVG)}</button>
                                                </div>
                                                <div class="mt-3">
                                                    <p class="truncate text-[17px] font-medium leading-5 text-[#222]">"Design walk inspired by "{listing.location}</p>
                                                    <p class="mt-1 text-[15px] leading-5 text-[var(--air-muted)]">"Hosted experience"</p>
                                                    <p class="flex items-center gap-1 text-[15px] leading-5 text-[var(--air-muted)]">
                                                        <span>"From $"{price}" per guest"</span>
                                                        {StaticRaw(STAR_SVG)}
                                                        <span>"4.95"</span>
                                                    </p>
                                                </div>
                                            </a>
                                        }.render(cx);
                                    }}
                                </div>
                            </section>
                        }.render(cx);
                    }}
                </div>
            </div>
            <div class="home-mode-panel" data-home-panel="services">
                <div class="air-shell py-10 md:py-12">
                    {for (section_index, section) in data.services_sections.iter().enumerate() {
                        let rail_id = SERVICES_RAIL_IDS[section_index];
                        html_sync! {
                            <section class="home-rail-section">
                                <div class="mb-4 flex items-center justify-between gap-4">
                                    <div>
                                        <h2 class="text-[28px] font-semibold tracking-[-0.03em] text-[#222]">{section.title}</h2>
                                        <p class="mt-1 text-[15px] text-[var(--air-muted)]">{section.subtitle}</p>
                                    </div>
                                    <div class="hidden items-center gap-2 md:flex">
                                        <button class="home-rail-arrow" data-rail-prev={rail_id} type="button" aria-label="Scroll left"><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg></button>
                                        <button class="home-rail-arrow" data-rail-next={rail_id} type="button" aria-label="Scroll right"><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg></button>
                                    </div>
                                </div>
                                <div class="home-rail" id={rail_id}>
                                    {for listing_index in section.listing_indexes.iter() {
                                        let listing = &data.listings[*listing_index % data.listings.len()];
                                        let image = listing.image;
                                        let price = listing.price_per_night / 2 + 65;
                                        html_sync! {
                                            <a href="/showcase/airbnb/proa/search" class="home-rail-card group">
                                                <div class="air-card-image relative aspect-[1.03/1] bg-[var(--air-soft)]">
                                                    {AirImage {
                                                        id: None,
                                                        src: image,
                                                        alt: listing.title,
                                                        class: Some("h-full w-full object-cover"),
                                                        loading: AirImageLoading::Lazy,
                                                        fetch_priority: AirImageFetchPriority::Low,
                                                        fallback_src: None,
                                                    }}
                                                    <span class="absolute left-3 top-3 rounded-full bg-white/95 px-3 py-1 text-xs font-semibold text-[#222] shadow-sm">"Top rated"</span>
                                                    <button class="heart-btn absolute right-3 top-3" aria-label="Save to favorites">{StaticRaw(shared::HEART_SVG)}</button>
                                                </div>
                                                <div class="mt-3">
                                                    <p class="truncate text-[17px] font-medium leading-5 text-[#222]">"Private service for "{listing.title}</p>
                                                    <p class="mt-1 text-[15px] leading-5 text-[var(--air-muted)]">"Professional service"</p>
                                                    <p class="flex items-center gap-1 text-[15px] leading-5 text-[var(--air-muted)]">
                                                        <span>"From $"{price}" per visit"</span>
                                                        {StaticRaw(STAR_SVG)}
                                                        <span>"5.0"</span>
                                                    </p>
                                                </div>
                                            </a>
                                        }.render(cx);
                                    }}
                                </div>
                            </section>
                        }.render(cx);
                    }}
                </div>
            </div>
        </main>
    }
    .render(cx);

    shared::render_footer(cx);
}

/// Render the Airbnb Home showcase as Markdown.
pub fn render_md() -> String {
    let data = HomeData::default_la();
    let mut buf = Vec::new();
    let mut cx = MdContext::new(&mut buf);

    cx.heading(1, "Airbnb");
    cx.write_prepared_static(
        b"Search for vacation rentals, cabins, beach houses, experiences, and services.\n\n",
    );

    cx.heading(2, "Modes");
    cx.write_prepared_static(b"Homes | Experiences | Services\n\n");

    cx.heading(2, "Featured Rails");
    for listing in data.listings.iter().take(6) {
        cx.heading(3, listing.title);
        let line = format!(
            "{} · ${} per night · ★{}\n",
            listing.location, listing.price_per_night, listing.rating,
        );
        cx.write_prepared_str(&line);
        cx.link("View listing", "/showcase/airbnb/proa/listing");
        cx.blank_line();
    }

    String::from_utf8(buf).unwrap()
}
