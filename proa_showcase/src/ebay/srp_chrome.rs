// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Fully dynamic eBay SRP chrome: global header, search, mega-nav, refine
//! rail, result controls, pagination, related searches, footer. Every
//! element is rendered per request (no pre-rendered shell strings), with
//! refinement counts derived from the listing dataset so the chrome does
//! real work on each render.
//!
//! Mirrors `proa_demo/react-ssr/chrome.tsx` element-for-element. Keep
//! markup, class names, constant tables, and derivation arithmetic in sync
//! between the two files, or the cross-engine byte/element parity check
//! drifts.

use super::srp::{Commas, Listing, ListingFormat, Uint};
use proa_core::shared::write_buf::WriteError as CoreWriteError;
use proa_core::{raw_text_ok, text, SafeText, StaticRaw, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

pub const DEFAULT_QUERY: &str = "iphone 14 pro max";

// ---------------------------------------------------------------------------
// Constant chrome data (mirrored in chrome.tsx)
// ---------------------------------------------------------------------------

const TOPBAR_LEFT: [(&str, &str); 5] = [
    ("Hi! Sign in", "#signin"),
    ("Daily Deals", "#deals"),
    ("Brand Outlet", "#outlet"),
    ("Gift Cards", "#gift"),
    ("Help & Contact", "#help"),
];
const TOPBAR_RIGHT: [(&str, &str); 4] = [
    ("Ship to", "#ship"),
    ("Sell", "#sell"),
    ("Watchlist", "#watchlist"),
    ("My eBay", "#myebay"),
];
const SEARCH_CATS: [&str; 9] = [
    "All Categories",
    "Cell Phones & Accessories",
    "Consumer Electronics",
    "Computers, Tablets & Network",
    "Cameras & Photo",
    "Video Games & Consoles",
    "Clothing, Shoes & Accessories",
    "Home & Garden",
    "Sporting Goods",
];
const NAV_CATS: [(&str, &str); 12] = [
    ("eBay Live", "#live"),
    ("Saved", "#saved"),
    ("Electronics", "#electronics"),
    ("Motors", "#motors"),
    ("Fashion", "#fashion"),
    ("Collectibles and Art", "#collectibles"),
    ("Sports", "#sports"),
    ("Health & Beauty", "#health"),
    ("Industrial equipment", "#industrial"),
    ("Home & Garden", "#home"),
    ("Deals", "#deals-nav"),
    ("Sell", "#sell-nav"),
];
const MEGANAV: [(&str, [&str; 8]); 6] = [
    (
        "Top Categories",
        [
            "Cell Phones & Smartphones",
            "Laptops & Netbooks",
            "Watches",
            "Sneakers",
            "Trading Cards",
            "Video Games",
            "Handbags",
            "Power Tools",
        ],
    ),
    (
        "Electronics",
        [
            "Smartphones",
            "Smart Watches",
            "Tablets & eReaders",
            "Headphones",
            "Drones",
            "Cameras",
            "TV & Home Audio",
            "Computer Components",
        ],
    ),
    (
        "Fashion",
        [
            "Men's Sneakers",
            "Women's Handbags",
            "Luxury Watches",
            "Sunglasses",
            "Fine Jewelry",
            "Vintage Clothing",
            "Designer Shoes",
            "Streetwear",
        ],
    ),
    (
        "Collectibles",
        [
            "Pokémon Cards",
            "Sports Trading Cards",
            "Coins & Money",
            "Comic Books",
            "Action Figures",
            "Stamps",
            "Militaria",
            "Antiques",
        ],
    ),
    (
        "Home & Garden",
        [
            "Major Appliances",
            "Power Tools",
            "Outdoor Living",
            "Kitchen & Dining",
            "Furniture",
            "Smart Home",
            "Lighting",
            "Yard & Garden",
        ],
    ),
    (
        "Motors",
        [
            "Cars & Trucks",
            "Motorcycles",
            "Parts & Accessories",
            "Wheels & Tires",
            "Engines",
            "Exterior Parts",
            "Performance Parts",
            "Automotive Tools",
        ],
    ),
];
const RAIL_CATS: [(&str, u64); 8] = [
    ("Cell Phones & Smartphones", 97),
    ("Cell Phone Cases, Covers & Skins", 41),
    ("Cell Phone Accessories", 29),
    ("Smart Watches", 17),
    ("Cell Phone & Smartphone Parts", 13),
    ("Tablets & eBook Readers", 7),
    ("Cell Phone Screen Protectors", 5),
    ("Other Cell Phones & Accessories", 3),
];
const RELATED_SEARCHES: [&str; 8] = [
    "iphone 13 pro max",
    "iphone 15 pro max",
    "iphone 14 pro max unlocked",
    "iphone 14 pro max 256gb",
    "samsung galaxy s24 ultra",
    "iphone 14 plus",
    "iphone 14 pro",
    "iphone 14 pro max for parts",
];
const FOOTER_COLS: [(&str, [&str; 7]); 5] = [
    (
        "Buy",
        [
            "Registration",
            "Bidding & buying help",
            "Stores",
            "eBay for Charity",
            "Charity Shop",
            "Seasonal sales and events",
            "eBay Gift Cards",
        ],
    ),
    (
        "Sell",
        [
            "Start selling",
            "How to sell",
            "Business sellers",
            "Affiliates",
            "Site map",
            "eBay Live",
            "Security center",
        ],
    ),
    (
        "Tools & apps",
        [
            "Developers",
            "eBay official app",
            "Free shipping zone",
            "Money back guarantee",
            "Shipping labels",
            "Site preferences",
            "Accessibility",
        ],
    ),
    (
        "eBay companies",
        [
            "eBay Classifieds",
            "eBay Kleinanzeigen",
            "StubHub",
            "TCGplayer",
            "Careers",
            "Company news",
            "Investors",
        ],
    ),
    (
        "Stay connected",
        [
            "eBay's Blogs",
            "Facebook",
            "X (Twitter)",
            "Instagram",
            "YouTube",
            "Press room",
            "Policies",
        ],
    ),
];
const FOOTER_LEGAL: [&str; 9] = [
    "Accessibility",
    "User Agreement",
    "Privacy",
    "Consumer Health Data",
    "Payments Terms of Use",
    "Cookies",
    "CA Privacy Notice",
    "Your Privacy Choices",
    "AdChoice",
];
const SORT_OPTIONS: [&str; 6] = [
    "Best Match",
    "Time: ending soonest",
    "Time: newly listed",
    "Price + Shipping: lowest first",
    "Price + Shipping: highest first",
    "Distance: nearest first",
];
const CONDITION_BUCKETS: [(&str, &str); 5] = [
    ("Brand New", "New"),
    ("Open Box", "Open box"),
    ("Pre-Owned", "Pre-owned"),
    ("Refurbished", "Certified - Refurbished"),
    ("For parts or not working", "For parts or not working"),
];
const LOCATION_ORDER: [&str; 8] = [
    "United States",
    "Hong Kong",
    "China",
    "United Kingdom",
    "Germany",
    "Japan",
    "Australia",
    "Canada",
];
const PER_PAGE_OPTIONS: [u64; 3] = [60, 120, 240];

// ---------------------------------------------------------------------------
// Derived per-request refinement metadata (mirrored in chrome.tsx)
// ---------------------------------------------------------------------------

pub struct SrpMeta {
    pub total: u64,
    pub cart_count: u32,
    pub condition_counts: [(&'static str, u32); 5],
    pub format_counts: [(&'static str, u32); 4],
    pub price_counts: [(&'static str, u32); 4],
    pub delivery_counts: [(&'static str, u32); 2],
    pub location_counts: [(&'static str, u32); 8],
    pub rating_counts: [(&'static str, u32); 3],
}

pub fn derive_srp_meta(listings: &[Listing]) -> SrpMeta {
    let n = listings.len() as u32;
    let mut buy_it_now = 0u32;
    let mut auction = 0u32;
    let mut best_offer = 0u32;
    let mut free_delivery = 0u32;
    let mut coupons = 0u32;
    let mut price_buckets = [0u32; 4];
    let mut cond = [0u32; 5];
    let mut locs = [0u32; 8];
    let mut ratings = [0u32; 3];

    for l in listings {
        match l.format {
            ListingFormat::Auction => auction += 1,
            ListingFormat::BestOffer => best_offer += 1,
            ListingFormat::BuyItNow => buy_it_now += 1,
        }
        if l.delivery_cents == 0 {
            free_delivery += 1;
        }
        if l.coupon_pct.is_some() {
            coupons += 1;
        }
        if l.price_cents < 50_000 {
            price_buckets[0] += 1;
        } else if l.price_cents < 100_000 {
            price_buckets[1] += 1;
        } else if l.price_cents < 150_000 {
            price_buckets[2] += 1;
        } else {
            price_buckets[3] += 1;
        }
        if let Some(first) = l.subtitle_parts.first() {
            if let Some(idx) = CONDITION_BUCKETS
                .iter()
                .position(|(key, _)| *key == first.as_ref())
            {
                cond[idx] += 1;
            }
        }
        if let Some(idx) = LOCATION_ORDER
            .iter()
            .position(|loc| *loc == l.location.as_ref())
        {
            locs[idx] += 1;
        }
        let pct = l.seller_feedback_pct_x10;
        if pct >= 990 {
            ratings[0] += 1;
        }
        if pct >= 980 {
            ratings[1] += 1;
        }
        if pct >= 950 {
            ratings[2] += 1;
        }
    }

    SrpMeta {
        total: 117_406 + (listings.len() as u64) * 7,
        cart_count: coupons,
        condition_counts: [
            (CONDITION_BUCKETS[0].1, cond[0]),
            (CONDITION_BUCKETS[1].1, cond[1]),
            (CONDITION_BUCKETS[2].1, cond[2]),
            (CONDITION_BUCKETS[3].1, cond[3]),
            (CONDITION_BUCKETS[4].1, cond[4]),
        ],
        format_counts: [
            ("All Listings", n),
            ("Buy It Now", buy_it_now),
            ("Auction", auction),
            ("Best Offer", best_offer),
        ],
        price_counts: [
            ("Under $500.00", price_buckets[0]),
            ("$500.00 to $1,000.00", price_buckets[1]),
            ("$1,000.00 to $1,500.00", price_buckets[2]),
            ("Over $1,500.00", price_buckets[3]),
        ],
        delivery_counts: [
            ("Free delivery", free_delivery),
            ("Paid delivery", n - free_delivery),
        ],
        location_counts: [
            (LOCATION_ORDER[0], locs[0]),
            (LOCATION_ORDER[1], locs[1]),
            (LOCATION_ORDER[2], locs[2]),
            (LOCATION_ORDER[3], locs[3]),
            (LOCATION_ORDER[4], locs[4]),
            (LOCATION_ORDER[5], locs[5]),
            (LOCATION_ORDER[6], locs[6]),
            (LOCATION_ORDER[7], locs[7]),
        ],
        rating_counts: [
            ("99% positive or more", ratings[0]),
            ("98% positive or more", ratings[1]),
            ("95% positive or more", ratings[2]),
        ],
    }
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

/// Handwritten chrome stylesheet, shared verbatim with the JS engines.
const CHROME_STYLE: &str = include_str!("data/srp_chrome_style.css");

/// Card styles: the `<style>` block extracted from the captured eBay page
/// (covers the s-card / su-* classes). Sliced once from the compiled-in
/// shell snapshot; zero copies at request time.
fn card_style() -> &'static str {
    static CELL: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    CELL.get_or_init(|| {
        const PRE: &str = include_str!("data/shell_pre.html");
        let Some(start) = PRE.find("<style>") else {
            return "";
        };
        let body_start = start + "<style>".len();
        let Some(end_rel) = PRE[body_start..].find("</style>") else {
            return "";
        };
        &PRE[body_start..body_start + end_rel]
    })
}

// ---------------------------------------------------------------------------
// Row helpers (html_sync! has no inline `for`, so loops live in components)
// ---------------------------------------------------------------------------

struct TopbarItem {
    label: &'static str,
    href: &'static str,
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for TopbarItem {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        html_sync! {
            <li class="gh-topbar__item">
                <a href={self.href}>{SafeText::from_trusted(self.label)}</a>
            </li>
        }
        .render(cx)
    }
}

struct RefineGroup<'a> {
    title: &'static str,
    rows: &'a [(&'static str, u32)],
}

impl<'a, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for RefineGroup<'a> {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        html_sync! {
            <div class="x-refine__group">
                <h3 class="x-refine__title">{SafeText::from_trusted(self.title)}</h3>
                <ul>{RefineRows { rows: self.rows }}</ul>
            </div>
        }
        .render(cx)
    }
}

struct RefineRows<'a> {
    rows: &'a [(&'static str, u32)],
}

impl<'a, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for RefineRows<'a> {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for (label, count) in self.rows {
            html_sync! {
                <li class="x-refine__item">
                    <label class="x-refine__label">
                        <input type="checkbox" class="x-refine__input" />
                        <span class="x-refine__text">{SafeText::from_trusted(label)}</span>
                        <span class="x-refine__count">"("{Commas(*count as u64)}")"</span>
                    </label>
                </li>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct SearchCatOptions;

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SearchCatOptions {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for (idx, cat) in SEARCH_CATS.iter().enumerate() {
            // React's defaultValue on the <select> marks the matching
            // option as selected in the SSR output; index 1 mirrors
            // chrome.tsx's SEARCH_CATS[1].
            if idx == 1 {
                html_sync! {
                    <option value={*cat} selected="">{SafeText::from_trusted(cat)}</option>
                }
                .render(cx)?;
            } else {
                html_sync! {
                    <option value={*cat}>{SafeText::from_trusted(cat)}</option>
                }
                .render(cx)?;
            }
        }
        Ok(())
    }
}

struct SortOptions;

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SortOptions {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for (idx, opt) in SORT_OPTIONS.iter().enumerate() {
            if idx == 0 {
                html_sync! {
                    <option value={*opt} selected="">{SafeText::from_trusted(opt)}</option>
                }
                .render(cx)?;
            } else {
                html_sync! {
                    <option value={*opt}>{SafeText::from_trusted(opt)}</option>
                }
                .render(cx)?;
            }
        }
        Ok(())
    }
}

struct PerPageOptions {
    per_page: u64,
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for PerPageOptions {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for opt in PER_PAGE_OPTIONS {
            if opt == self.per_page {
                html_sync! {
                    <option value={opt} selected="">{Uint(opt)}</option>
                }
                .render(cx)?;
            } else {
                html_sync! {
                    <option value={opt}>{Uint(opt)}</option>
                }
                .render(cx)?;
            }
        }
        Ok(())
    }
}

struct NavCats;

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for NavCats {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for (label, href) in NAV_CATS {
            html_sync! {
                <li>
                    <a href={href}>{SafeText::from_trusted(label)}</a>
                </li>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct MeganavSections;

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for MeganavSections {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for (title, links) in MEGANAV {
            html_sync! {
                <section class="gh-meganav__section">
                    <h3>{SafeText::from_trusted(title)}</h3>
                    <ul>{MeganavLinks { links }}</ul>
                </section>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct MeganavLinks {
    links: [&'static str; 8],
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for MeganavLinks {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for link in self.links {
            html_sync! {
                <li>
                    <a href="#category">{SafeText::from_trusted(link)}</a>
                </li>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct RailCats {
    n: u64,
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for RailCats {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for (label, weight) in RAIL_CATS {
            html_sync! {
                <li>
                    <a href="#refine-category">{SafeText::from_trusted(label)}</a>
                    <span class="x-refine__count">"("{Commas(weight * self.n)}")"</span>
                </li>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct RelatedChips;

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for RelatedChips {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for term in RELATED_SEARCHES {
            html_sync! {
                <li>
                    <a href="#related">{SafeText::from_trusted(term)}</a>
                </li>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct FooterCols;

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for FooterCols {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for (title, links) in FOOTER_COLS {
            html_sync! {
                <div class="gh-footer__col">
                    <h3>{SafeText::from_trusted(title)}</h3>
                    <ul>{FooterLinks { links }}</ul>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct FooterLinks {
    links: [&'static str; 7],
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for FooterLinks {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for link in self.links {
            html_sync! {
                <li>
                    <a href="#footer-link">{SafeText::from_trusted(link)}</a>
                </li>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct FooterLegalLinks;

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for FooterLegalLinks {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for label in FOOTER_LEGAL {
            html_sync! {
                <a href="#legal">{SafeText::from_trusted(label)}</a>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct PaginationPages {
    pages: u64,
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for PaginationPages {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        let shown = self.pages.min(7);
        for p in 1..=shown {
            if p == 1 {
                html_sync! {
                    <a class="srp-pagination__page srp-pagination__page--active" href="#page">
                        {Uint(p)}
                    </a>
                }
                .render(cx)?;
            } else {
                html_sync! {
                    <a class="srp-pagination__page" href="#page">{Uint(p)}</a>
                }
                .render(cx)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Chrome sections
// ---------------------------------------------------------------------------

pub struct SrpHeader<'a> {
    pub query: &'a str,
    pub meta: &'a SrpMeta,
}

impl<'a, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SrpHeader<'a> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        html_sync! {
            <header class="gh">
                <nav class="gh-topbar" aria-label="Account and shopping shortcuts">
                    <ul class="gh-topbar__nav">{TopbarRow { items: &TOPBAR_LEFT }}</ul>
                    <ul class="gh-topbar__nav gh-topbar__nav--right">
                        {TopbarRow { items: &TOPBAR_RIGHT }}
                        <li class="gh-topbar__item gh-topbar__item--icon">
                            <a href="#notifications" aria-label="Notifications">
                                <svg class="gh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                                    <path d="M12 22a2.4 2.4 0 0 0 2.4-2.4H9.6A2.4 2.4 0 0 0 12 22zm7-5.2v-1l-1.6-1.6v-4.6a5.4 5.4 0 0 0-4-5.5V3.4a1.4 1.4 0 1 0-2.8 0v.7a5.4 5.4 0 0 0-4 5.5v4.6L5 15.8v1z" />
                                </svg>
                            </a>
                        </li>
                        <li class="gh-topbar__item gh-topbar__item--icon">
                            <a href="#cart" aria-label="Your shopping cart">
                                <svg class="gh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                                    <path d="M7 20a1.6 1.6 0 1 0 0-3.2A1.6 1.6 0 0 0 7 20zm10 0a1.6 1.6 0 1 0 0-3.2 1.6 1.6 0 0 0 0 3.2zM3 3h2.2l2.5 10.4a2 2 0 0 0 2 1.6h7.6a2 2 0 0 0 2-1.5L21 6H6.2" />
                                </svg>
                                <span class="gh-cart-badge">
                                    {Uint(self.meta.cart_count as u64)}
                                </span>
                            </a>
                        </li>
                    </ul>
                </nav>
                <div class="gh-main">
                    <a class="gh-logo" href="#home" aria-label="eBay Home">
                        <svg viewBox="0 0 122 48" aria-hidden="true" focusable="false">
                            <path class="gh-logo__e" d="M9 18c-5 0-9 3-9 8 0 6 4 9 9 9 4 0 7-2 8-5h-5c-1 1-2 2-3 2-3 0-4-2-4-4h12v-2c0-5-3-8-8-8z" />
                            <path class="gh-logo__b" d="M21 8h5v12c1-2 3-3 6-3 5 0 8 4 8 9s-3 9-8 9c-3 0-5-1-6-3v2h-5z" />
                            <path class="gh-logo__a" d="M50 17c-5 0-8 2-8 6h5c0-2 1-2 3-2s3 1 3 3v1h-3c-5 0-9 2-9 6 0 3 3 5 7 5 3 0 4-1 6-3v2h5V23c0-4-3-6-9-6z" />
                            <path class="gh-logo__y" d="M61 18h6l4 11 4-11h6l-9 22h-6l3-7z" />
                        </svg>
                    </a>
                    <div class="gh-search">
                        <form class="gh-search__form" action="#search" method="get">
                            <span class="gh-search__icon">
                                <svg class="gh-search__icon-svg" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                                    <path d="M10.5 3a7.5 7.5 0 1 0 4.7 13.4l4.7 4.7 1.4-1.4-4.7-4.7A7.5 7.5 0 0 0 10.5 3z" />
                                </svg>
                            </span>
                            <input class="gh-search__input" type="text" name="q" value={self.query} placeholder="Search for anything" aria-label="Search for anything" />
                            <select class="gh-search__category" aria-label="Select a category for search">
                                {SearchCatOptions}
                            </select>
                            <button class="gh-search__btn" type="submit">"Search"</button>
                        </form>
                        <a class="gh-search__advanced" href="#advanced">"Advanced"</a>
                    </div>
                </div>
                <nav class="gh-cats" aria-label="Categories">
                    <ul>{NavCats}</ul>
                </nav>
                <div class="gh-meganav" aria-hidden="false">{MeganavSections}</div>
            </header>
        }
        .render(cx)
    }
}

struct TopbarRow {
    items: &'static [(&'static str, &'static str)],
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for TopbarRow {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for (label, href) in self.items {
            TopbarItem { label, href }.render(cx)?;
        }
        Ok(())
    }
}

pub struct SrpControls<'a> {
    pub query: &'a str,
    pub meta: &'a SrpMeta,
    pub per_page: u64,
}

impl<'a, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SrpControls<'a> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        html_sync! {
            <div class="srp-controls">
                <nav class="srp-controls__breadcrumb" aria-label="Breadcrumb">
                    <ol>
                        <li>
                            <a href="#home">"eBay"</a>
                        </li>
                        <li>
                            <a href="#cellphones">"Cell Phones & Accessories"</a>
                        </li>
                        <li>
                            <a href="#smartphones">"Cell Phones & Smartphones"</a>
                        </li>
                    </ol>
                </nav>
                <h1 class="srp-controls__count">
                    <span class="srp-controls__count-bold">{Commas(self.meta.total)}</span>
                    " results for "
                    <span class="srp-controls__count-bold">{text(self.query)}</span>
                </h1>
                <div class="srp-controls__row">
                    <button type="button" class="srp-controls__save">
                        <svg class="gh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                            <path d="M12 21s-7.5-4.8-10-9.3C.5 8 2.6 4.5 6.2 4.5c2.2 0 3.9 1.2 4.8 3 .9-1.8 2.6-3 4.8-3 3.6 0 5.7 3.5 4.2 7.2C19.5 16.2 12 21 12 21z" />
                        </svg>
                        <span>"Save this search"</span>
                    </button>
                    <label class="srp-controls__sort">
                        <span>"Sort:"</span>
                        <select aria-label="Sort selector">{SortOptions}</select>
                    </label>
                    <label class="srp-controls__perpage">
                        <span>"Items per page:"</span>
                        <select aria-label="Items per page">
                            {PerPageOptions { per_page: self.per_page }}
                        </select>
                    </label>
                </div>
            </div>
        }
        .render(cx)
    }
}

pub struct SrpRail<'a> {
    pub meta: &'a SrpMeta,
}

impl<'a, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SrpRail<'a> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        let n = self.meta.format_counts[0].1 as u64;
        html_sync! {
            <aside class="srp-rail" aria-label="Refine results">
                <h2 class="srp-rail__heading">"Shop by category"</h2>
                <ul class="srp-rail__cats">{RailCats { n }}</ul>
                {RefineGroup { title: "Buying Format", rows: &self.meta.format_counts }}
                {RefineGroup { title: "Condition", rows: &self.meta.condition_counts }}
                {RefineGroup { title: "Price", rows: &self.meta.price_counts }}
                {RefineGroup { title: "Delivery Options", rows: &self.meta.delivery_counts }}
                {RefineGroup { title: "Item Location", rows: &self.meta.location_counts }}
                {RefineGroup { title: "Seller Rating", rows: &self.meta.rating_counts }}
                <div class="srp-rail__cta">
                    <a href="#completed">"Completed items"</a>
                    <a href="#sold">"Sold items"</a>
                    <a href="#deals">"Deals & savings"</a>
                </div>
            </aside>
        }
        .render(cx)
    }
}

pub struct SrpPagination {
    pub total: u64,
    pub per_page: u64,
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SrpPagination {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        let pages = (self.total.div_ceil(self.per_page)).max(1);
        html_sync! {
            <nav class="srp-pagination" aria-label="Result pagination">
                <a class="srp-pagination__nav srp-pagination__nav--prev" href="#prev" aria-label="Previous page">
                    <svg class="gh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                        <path d="M15 4l-8 8 8 8" />
                    </svg>
                </a>
                {PaginationPages { pages }}
                <span class="srp-pagination__ellipsis">"…"</span>
                <a class="srp-pagination__page" href="#page-last">{Commas(pages)}</a>
                <a class="srp-pagination__nav srp-pagination__nav--next" href="#next" aria-label="Next page">
                    <svg class="gh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                        <path d="M9 4l8 8-8 8" />
                    </svg>
                </a>
            </nav>
        }
        .render(cx)
    }
}

pub struct SrpRelated;

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SrpRelated {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        html_sync! {
            <section class="srp-related" aria-label="Related searches">
                <h2>"Related searches"</h2>
                <ul>{RelatedChips}</ul>
            </section>
        }
        .render(cx)
    }
}

pub struct SrpFooter;

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SrpFooter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        html_sync! {
            <footer class="gh-footer">
                <div class="gh-footer__cols">{FooterCols}</div>
                <div class="gh-footer__legal">
                    {FooterLegalLinks}
                    <span>"Copyright © 1995-2026 eBay Inc. All Rights Reserved."</span>
                </div>
            </footer>
        }
        .render(cx)
    }
}

// ---------------------------------------------------------------------------
// Whole-document entry
// ---------------------------------------------------------------------------

/// Body-level skeleton: chrome around the river region. `river` is the
/// chips + listing component, rendered inline in the same pass as the
/// chrome (no intermediate buffer).
pub struct SrpBody<'a, R> {
    pub query: &'a str,
    pub meta: &'a SrpMeta,
    pub per_page: u64,
    pub river: R,
}

impl<'a, R, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SrpBody<'a, R>
where
    R: WebRenderSync<L, B>,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        html_sync! {
            <div class="srp-page">
                {SrpHeader { query: self.query, meta: self.meta }}
                <main class="srp">
                    {SrpControls { query: self.query, meta: self.meta, per_page: self.per_page }}
                    <div class="srp-layout">
                        {SrpRail { meta: self.meta }}
                        <section class="srp-river">
                            <div id="__app_root__">{self.river}</div>
                        </section>
                    </div>
                    {SrpPagination { total: self.meta.total, per_page: self.per_page }}
                    {SrpRelated}
                </main>
                {SrpFooter}
            </div>
        }
        .render(cx)
    }
}

/// Full-document component. `river` renders inside the app root and `tail`
/// renders just before `</body>` in the same pass. The response owner then
/// finalizes exactly the runtime artifacts recorded while the page rendered.
pub struct SrpDocument<'a, R, T> {
    pub query: &'a str,
    pub meta: &'a SrpMeta,
    pub per_page: u64,
    pub river: R,
    pub tail: T,
}

impl<'a, R, T, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for SrpDocument<'a, R, T>
where
    R: WebRenderSync<L, B>,
    T: WebRenderSync<L, B>,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        StaticRaw("<!DOCTYPE html>").render(cx)?;
        html_sync! {
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <title>{raw_text_ok(text(self.query))}" for sale | eBay"</title>
                    <style>{StaticRaw(card_style())}</style>
                    <style>{StaticRaw(CHROME_STYLE)}</style>
                </head>
                <body>
                    {SrpBody {
                        query: self.query,
                        meta: self.meta,
                        per_page: self.per_page,
                        river: self.river,
                    }}
                    {self.tail}
                </body>
            </html>
        }
        .render(cx)
    }
}

/// Render the complete SRP document in a single pass into one buffer:
/// doctype, head, chrome, river, island tail, no intermediate copies.
pub fn render_document<R, T>(query: &str, listings: &[Listing], river: R, tail: T) -> String
where
    R: WebRenderSync,
    T: WebRenderSync,
{
    let meta = derive_srp_meta(listings);
    let mut cx = proa_core::WebContext::new();
    proa_core::WebRenderSync::render(
        SrpDocument {
            query,
            meta: &meta,
            per_page: listings.len() as u64,
            river,
            tail,
        },
        &mut cx,
    )
    .unwrap();
    cx.into_output().into_string()
}
