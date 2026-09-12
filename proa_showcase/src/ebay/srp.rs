// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Pixel-perfect mirror of an actual eBay search results page card.
//!
//! Rendering is fully optimized:
//!   - `html_sync!` consolidates static markup at compile time
//!   - Default listings are built once into a `OnceLock` cache (zero alloc
//!     on the no-override hot path)
//!   - Custom `AttrValue` impls render URLs / item-ids straight to buffer
//!     (no per-card `format!` strings)
//!   - `SafeText::from_trusted` skips the escape SIMD scan for
//!     guaranteed-safe content (caption labels, format labels, locations)
//!   - Three render entry points for benchmarking:
//!       * `render_listings_into(listings, &mut Vec<u8>)` — Vec target
//!       * `render_listings_into_chained(listings, &mut RenderBuffer)`
//!       * `render_listings_into_zerocopy(listings) -> ZeroCopyBuf`

use proa_core::shared::write_buf::WriteError as CoreWriteError;
use proa_core::{
    shared::attr_value::AttrValue, text, RenderBuffer, SafeText, WebContext, WebRenderSync,
    WriteBuf, ZeroCopyBuf,
};
use proa_macros::html_sync;
use std::borrow::Cow;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Domain
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CaptionSignal {
    None,
    NewLowPrice,
    Trending,
    EndsSoon,
    EditorsPick,
}

impl CaptionSignal {
    const fn label(self) -> &'static str {
        match self {
            Self::None => "",
            Self::NewLowPrice => "NEW LOW PRICE",
            Self::Trending => "TRENDING",
            Self::EndsSoon => "ENDS SOON",
            Self::EditorsPick => "eBay's CHOICE",
        }
    }
    const fn classes(self) -> &'static str {
        match self {
            Self::None => "",
            Self::NewLowPrice => "signal signal--trustworthy",
            Self::Trending => "signal signal--informational",
            Self::EndsSoon => "signal signal--negative",
            Self::EditorsPick => "signal signal--brand",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ListingFormat {
    BuyItNow,
    Auction,
    BestOffer,
}

impl ListingFormat {
    const fn label(self) -> &'static str {
        match self {
            Self::BuyItNow => "Buy It Now",
            Self::Auction => "or Best Offer",
            Self::BestOffer => "Best Offer",
        }
    }
    /// Stable lowercase slug used for the per-card `data-format` filter
    /// attribute. The runtime coordinator builds CSS attribute selectors
    /// against these slugs (e.g. `li.s-card:not([data-format="buyitnow"])`).
    pub const fn slug(self) -> &'static str {
        match self {
            Self::BuyItNow => "buyitnow",
            Self::Auction => "auction",
            Self::BestOffer => "bestoffer",
        }
    }
}

#[derive(Clone)]
pub struct Listing {
    pub item_id: u64,
    pub image_hash: Cow<'static, str>,
    pub title: Cow<'static, str>,
    pub subtitle_parts: Vec<Cow<'static, str>>, // joined with " · "
    pub caption: CaptionSignal,
    pub stars_full: u8,
    pub has_half_star: bool,
    pub review_count: u32,
    pub price_cents: i64,
    pub original_price_cents: Option<i64>,
    pub format: ListingFormat,
    pub delivery_cents: i64, // 0 = "Free delivery"
    pub location: Cow<'static, str>,
    pub watcher_count: u32,
    pub coupon_pct: Option<u8>,
    pub seller_name: Cow<'static, str>,
    pub seller_feedback_pct_x10: u16,
    pub seller_feedback_count: u32,
}

// ---------------------------------------------------------------------------
// Static dataset
// ---------------------------------------------------------------------------

const TITLES: &[&str] = &[
    "Unlocked Apple iPhone 12 Pro Max - 128GB Gold - 78% Batt - Crcks on Front & Back",
    "Apple iPhone 14 Pro Max 256GB - Deep Purple - Unlocked - <Excellent Condition>",
    "Apple iPhone 14 Pro 128GB - Space Black - Verizon - Open Box & Tested",
    "Apple iPhone 14 Pro Max 1TB - Silver - AT&T Locked - Battery 92%",
    "Apple iPhone 14 Pro 512GB - Gold - T-Mobile Unlocked - Pristine",
    "Apple iPhone 14 Pro Max 256GB - Deep Purple - Sprint Locked - Used",
    "Apple iPhone 14 Pro 128GB - Space Black - Factory Unlocked Dual SIM",
    "Apple iPhone 14 Pro Max 512GB - Silver - Cricket Locked - Pre-Owned",
    "Apple iPhone 14 Pro 256GB - Deep Purple - Verizon - <Brand New & Sealed>",
    "Apple iPhone 14 Pro Max 128GB - Gold - Visible Locked - 89% Battery",
    "Apple iPhone 14 Pro 1TB - Space Black - Unlocked - \"Like New\" Condition",
    "Apple iPhone 14 Pro Max 256GB - Silver - Boost Mobile - Excellent",
    "Apple iPhone 14 Pro 128GB - Deep Purple - Metro by T-Mobile - 7/10",
    "Apple iPhone 14 Pro Max 512GB - Gold - Mint Mobile - Pre-Owned",
    "Apple iPhone 14 Pro 256GB - Space Black & Cracked Back - For Parts",
    "Apple iPhone 14 Pro Max 128GB - Silver - US Cellular Locked",
];
const SUBTITLE_VARIANTS: &[&[&str]] = &[
    &["Pre-Owned", "Apple iPhone 14 Pro Max", "128 GB", "Unlocked"],
    &["Brand New", "Apple iPhone 14 Pro", "256 GB", "Verizon"],
    &["Pre-Owned", "Apple iPhone 14 Pro", "512 GB", "AT&T"],
    &["Open Box", "Apple iPhone 14 Pro Max", "1 TB", "T-Mobile"],
    &[
        "For parts or not working",
        "Apple iPhone 14 Pro",
        "Unlocked",
    ],
    &["Refurbished", "Apple iPhone 14 Pro Max", "256 GB", "Sprint"],
    &["Pre-Owned", "Apple iPhone 14 Pro", "128 GB", "Cricket"],
    &[
        "Brand New",
        "Apple iPhone 14 Pro Max",
        "512 GB",
        "Boost Mobile",
    ],
];
const SELLERS: &[&str] = &[
    "cocosprinkles",
    "tech_outlet_usa",
    "phonefactory_uk",
    "smart-deals & co",
    "best>buy_outlet",
    "the.gadget_shop",
    "global-tech\"vendor\"",
    "premium_resale",
    "digital_drop&co",
    "pacific_electronics",
];
const LOCATIONS: &[&str] = &[
    "United States",
    "Hong Kong",
    "China",
    "United Kingdom",
    "Germany",
    "Japan",
    "Australia",
    "Canada",
];
const IMAGE_HASHES: &[&str] = &[
    "C1EAAeSw3bpp1CD9",
    "DkAAAOSw9oZl5kZ4",
    "K2EAAOSwUVZmJ8x9",
    "M5EAAOSwx9plQ7Wq",
    "B3EAAOSw7P5kJ2x1",
    "G4EAAOSwY8plPwBk",
    "H6EAAOSw0XJlNVwR",
    "P7EAAOSwm5RkV2nT",
];

const DEFAULT_CACHE_SIZE: usize = 1000;
static DEFAULT_LISTINGS: OnceLock<Vec<Listing>> = OnceLock::new();

fn default_listings_cache() -> &'static [Listing] {
    DEFAULT_LISTINGS.get_or_init(|| (0..DEFAULT_CACHE_SIZE).map(default_listing).collect())
}

/// Zero-allocation handle to the first `n` cached default listings.
/// The hot path (no overrides) uses this directly — never clones.
pub fn default_listings(n: usize) -> &'static [Listing] {
    let cache = default_listings_cache();
    &cache[..n.min(cache.len())]
}

/// Backwards-compatible builder — clones from the cache when caller needs ownership.
pub fn build_listings(n: usize) -> Vec<Listing> {
    default_listings(n).to_vec()
}

/// Build the i-th default listing (used to populate the cache once).
pub fn default_listing(i: usize) -> Listing {
    let base = 14_999 + ((i as i64) * 4_337) % 195_000;
    let on_sale = i.is_multiple_of(3);
    let original = if on_sale { Some(base + base / 5) } else { None };
    let free_delivery = i.is_multiple_of(2);
    Listing {
        item_id: 336_000_000_000 + (i as u64) * 71,
        image_hash: Cow::Borrowed(IMAGE_HASHES[i % IMAGE_HASHES.len()]),
        title: Cow::Borrowed(TITLES[i % TITLES.len()]),
        subtitle_parts: SUBTITLE_VARIANTS[i % SUBTITLE_VARIANTS.len()]
            .iter()
            .map(|s| Cow::Borrowed(*s))
            .collect(),
        caption: match i % 9 {
            0 => CaptionSignal::NewLowPrice,
            3 => CaptionSignal::Trending,
            5 => CaptionSignal::EndsSoon,
            7 => CaptionSignal::EditorsPick,
            _ => CaptionSignal::None,
        },
        stars_full: 3 + (i as u8 % 3),
        has_half_star: i.is_multiple_of(2),
        review_count: 5 + (i as u32 * 17) % 1_240,
        price_cents: base,
        original_price_cents: original,
        format: match i % 5 {
            0 => ListingFormat::Auction,
            1 => ListingFormat::BestOffer,
            _ => ListingFormat::BuyItNow,
        },
        delivery_cents: if free_delivery {
            0
        } else {
            599 + (i as i64 * 13) % 1_400
        },
        location: Cow::Borrowed(LOCATIONS[i % LOCATIONS.len()]),
        watcher_count: 5 + (i as u32 * 7) % 200,
        coupon_pct: if i % 4 == 1 {
            Some(5 + (i as u8 % 20))
        } else {
            None
        },
        seller_name: Cow::Borrowed(SELLERS[i % SELLERS.len()]),
        seller_feedback_pct_x10: 950 + ((i as u16) * 7) % 50,
        seller_feedback_count: 1_000 + (i as u32 * 137) % 99_000,
    }
}

/// See module-level docs for the override schema.
pub fn apply_overrides(listings: &mut [Listing], overrides: &serde_json::Value) {
    let arr = match overrides.as_array() {
        Some(a) => a,
        None => return,
    };
    for (i, ov) in arr.iter().enumerate() {
        if i >= listings.len() {
            break;
        }
        let l = &mut listings[i];
        if let Some(s) = ov.get("title").and_then(|v| v.as_str()) {
            l.title = Cow::Owned(s.to_owned());
        }
        if let Some(s) = ov.get("subtitle").and_then(|v| v.as_str()) {
            l.subtitle_parts = s
                .split(['·', '|'])
                .map(|p| Cow::Owned(p.trim().to_owned()))
                .filter(|p: &Cow<'static, str>| !p.is_empty())
                .collect();
        }
        if let Some(s) = ov.get("imageHash").and_then(|v| v.as_str()) {
            l.image_hash = Cow::Owned(s.to_owned());
        }
        if let Some(n) = ov.get("priceCents").and_then(|v| v.as_i64()) {
            l.price_cents = n;
        }
        if let Some(v) = ov.get("originalPriceCents") {
            l.original_price_cents = v.as_i64();
        }
        if let Some(b) = ov.get("freeDelivery").and_then(|v| v.as_bool()) {
            if b {
                l.delivery_cents = 0;
            }
        }
        if let Some(n) = ov.get("deliveryCents").and_then(|v| v.as_i64()) {
            l.delivery_cents = n;
        }
        if let Some(s) = ov.get("location").and_then(|v| v.as_str()) {
            l.location = Cow::Owned(s.to_owned());
        }
        if let Some(n) = ov.get("watcherCount").and_then(|v| v.as_u64()) {
            l.watcher_count = n as u32;
        }
        if let Some(v) = ov.get("couponPct") {
            l.coupon_pct = v.as_u64().map(|n| n as u8);
        }
        if let Some(n) = ov.get("starsFull").and_then(|v| v.as_u64()) {
            l.stars_full = (n as u8).min(5);
        }
        if let Some(b) = ov.get("hasHalfStar").and_then(|v| v.as_bool()) {
            l.has_half_star = b;
        }
        if let Some(n) = ov.get("reviewCount").and_then(|v| v.as_u64()) {
            l.review_count = n as u32;
        }
        if let Some(s) = ov.get("format").and_then(|v| v.as_str()) {
            l.format = match s {
                "Auction" => ListingFormat::Auction,
                "BestOffer" => ListingFormat::BestOffer,
                _ => ListingFormat::BuyItNow,
            };
        }
        if let Some(s) = ov.get("caption").and_then(|v| v.as_str()) {
            l.caption = match s {
                "NewLowPrice" => CaptionSignal::NewLowPrice,
                "Trending" => CaptionSignal::Trending,
                "EndsSoon" => CaptionSignal::EndsSoon,
                "EditorsPick" | "EbayChoice" => CaptionSignal::EditorsPick,
                _ => CaptionSignal::None,
            };
        }
        if let Some(s) = ov.get("sellerName").and_then(|v| v.as_str()) {
            l.seller_name = Cow::Owned(s.to_owned());
        }
        if let Some(n) = ov.get("sellerFeedbackPctX10").and_then(|v| v.as_u64()) {
            l.seller_feedback_pct_x10 = n as u16;
        }
        if let Some(n) = ov.get("sellerFeedbackCount").and_then(|v| v.as_u64()) {
            l.seller_feedback_count = n as u32;
        }
    }
}

pub fn build_listings_with_overrides(n: usize, overrides: &serde_json::Value) -> Vec<Listing> {
    let mut listings = build_listings(n);
    apply_overrides(&mut listings, overrides);
    listings
}

// ---------------------------------------------------------------------------
// Zero-allocation formatters (write decimals/dates straight into the buffer)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Currency(i64);
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for Currency {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        let cents = self.0;
        if cents < 0 {
            cx.out.extend(b"-")?;
        }
        let abs = cents.unsigned_abs();
        cx.out.extend(b"$")?;
        write_with_commas(cx, abs / 100)?;
        cx.out.extend(b".")?;
        write_two_digits(cx, (abs % 100) as u32)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Uint(pub(crate) u64);
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for Uint {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        let mut buf = [0u8; 20];
        let s = format_u64(&mut buf, self.0);
        cx.out.extend(s.as_bytes())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Commas(pub(crate) u64);
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for Commas {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        write_with_commas(cx, self.0)
    }
}

/// "99.9%" from 999
#[derive(Clone, Copy)]
struct FeedbackPct(u16);
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for FeedbackPct {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        let v = self.0;
        let whole = (v / 10) as u64;
        let tenth = (v % 10) as u8;
        let mut buf = [0u8; 20];
        let s = format_u64(&mut buf, whole);
        cx.out.extend(s.as_bytes())?;
        cx.out.extend(b".")?;
        cx.out.extend(&[b'0' + tenth])?;
        cx.out.extend(b"%")
    }
}

/// "37.9K" from 37_912
#[derive(Clone, Copy)]
struct AbbrevCount(u32);
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for AbbrevCount {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        let n = self.0;
        let mut buf = [0u8; 20];
        if n < 1_000 {
            let s = format_u64(&mut buf, n as u64);
            return cx.out.extend(s.as_bytes());
        }
        if n < 1_000_000 {
            let whole = n / 1_000;
            let tenth = (n % 1_000) / 100;
            let s = format_u64(&mut buf, whole as u64);
            cx.out.extend(s.as_bytes())?;
            if tenth > 0 {
                cx.out.extend(b".")?;
                cx.out.extend(&[b'0' + tenth as u8])?;
            }
            return cx.out.extend(b"K");
        }
        let whole = n / 1_000_000;
        let tenth = (n % 1_000_000) / 100_000;
        let s = format_u64(&mut buf, whole as u64);
        cx.out.extend(s.as_bytes())?;
        if tenth > 0 {
            cx.out.extend(b".")?;
            cx.out.extend(&[b'0' + tenth as u8])?;
        }
        cx.out.extend(b"M")
    }
}

#[inline]
fn format_u64(buf: &mut [u8; 20], mut v: u64) -> &str {
    if v == 0 {
        buf[0] = b'0';
        return std::str::from_utf8(&buf[..1]).unwrap();
    }
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    std::str::from_utf8(&buf[i..]).unwrap()
}

#[inline]
fn write_two_digits<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    v: u32,
) -> Result<(), CoreWriteError> {
    let bytes = [b'0' + ((v / 10) % 10) as u8, b'0' + (v % 10) as u8];
    cx.out.extend(&bytes)
}

#[inline]
fn write_with_commas<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    v: u64,
) -> Result<(), CoreWriteError> {
    if v == 0 {
        return cx.out.extend(b"0");
    }
    let mut digits = [0u8; 20];
    let s = format_u64(&mut digits, v);
    let bytes = s.as_bytes();
    let len = bytes.len();
    let first = ((len - 1) % 3) + 1;
    cx.out.extend(&bytes[..first])?;
    let mut i = first;
    while i < len {
        cx.out.extend(b",")?;
        cx.out.extend(&bytes[i..i + 3])?;
        i += 3;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Custom AttrValue impls — emit URLs / item-ids straight into the attribute
// writer without ever allocating an intermediate `String`.
// ---------------------------------------------------------------------------

/// `id="item123456"` — eBay's per-card DOM id format.
#[derive(Clone, Copy)]
struct ItemId(u64);
impl AttrValue for ItemId {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }
    #[inline]
    fn render_attr_value<B: WriteBuf, L: ::proa_core::DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), CoreWriteError> {
        cx.out.extend(b"item")?;
        let mut buf = [0u8; 20];
        let s = format_u64(&mut buf, self.0);
        cx.out.extend(s.as_bytes())
    }
}

/// `href="https://www.ebay.com/itm/123456"`
#[derive(Clone, Copy)]
struct ItemUrl(u64);
impl AttrValue for ItemUrl {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }
    #[inline]
    fn render_attr_value<B: WriteBuf, L: ::proa_core::DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), CoreWriteError> {
        cx.out.extend(b"https://www.ebay.com/itm/")?;
        let mut buf = [0u8; 20];
        let s = format_u64(&mut buf, self.0);
        cx.out.extend(s.as_bytes())
    }
}

/// `src="https://i.ebayimg.com/images/g/HASH/s-l500.webp"`
struct ImgUrl<'a>(&'a str);
impl<'a> AttrValue for ImgUrl<'a> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }
    #[inline]
    fn render_attr_value<B: WriteBuf, L: ::proa_core::DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), CoreWriteError> {
        cx.out.extend(b"https://i.ebayimg.com/images/g/")?;
        cx.out.extend(self.0.as_bytes())?;
        cx.out.extend(b"/s-l500.webp")
    }
}

/// `aria-label="watch <escaped title>"`
struct WatchLabel<'a>(&'a str);
impl<'a> AttrValue for WatchLabel<'a> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }
    #[inline]
    fn render_attr_value<B: WriteBuf, L: ::proa_core::DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), CoreWriteError> {
        cx.out.extend(b"watch ")?;
        proa_core::shared::escape::escape_attr_value(&mut cx.out, self.0)
    }
}

// ---------------------------------------------------------------------------
// Card sub-components (variable-length / conditional sections that need
// their own helpers because html_sync! doesn't support inline `for`).
// ---------------------------------------------------------------------------

struct SubtitleRow<'a> {
    parts: &'a [Cow<'static, str>],
}

impl<'a, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SubtitleRow<'a> {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        let last = self.parts.len().saturating_sub(1);
        for (idx, part) in self.parts.iter().enumerate() {
            html_sync! {
                <span class="su-styled-text secondary default">{ text(part) }</span>
            }
            .render(cx)?;
            if idx != last {
                cx.out.extend(" \u{00b7} ".as_bytes())?;
            }
        }
        Ok(())
    }
}

struct StarRow {
    full: u8,
    half: bool,
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for StarRow {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        for _ in 0..self.full {
            cx.out.extend(
                b"<svg class=\"icon icon--16\" focusable=\"false\" aria-hidden=\"true\"><use href=\"#icon-star-filled-16\"></use></svg>",
            )?;
        }
        if self.half {
            cx.out.extend(
                b"<svg class=\"icon icon--16-colored\" focusable=\"false\" aria-hidden=\"true\"><use href=\"#icon-star-half-16-colored\"></use></svg>",
            )?;
        }
        let empty = 5u8.saturating_sub(self.full + self.half as u8);
        for _ in 0..empty {
            cx.out.extend(
                b"<svg class=\"icon icon--16\" focusable=\"false\" aria-hidden=\"true\"><use href=\"#icon-star-empty-16\"></use></svg>",
            )?;
        }
        Ok(())
    }
}

struct OriginalPriceCell(Option<i64>);
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for OriginalPriceCell {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        if let Some(orig) = self.0 {
            html_sync! {
                <span class="su-spacer --spacing-100"></span>
                <span class="su-styled-text secondary strikethrough large">
                    { Currency(orig) }
                </span>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct CaptionBadge(CaptionSignal);
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for CaptionBadge {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        if self.0 == CaptionSignal::None {
            return Ok(());
        }
        let classes = self.0.classes();
        let label = self.0.label();
        // Caption labels are a small fixed set ("NEW LOW PRICE", "TRENDING",
        // …) — they're guaranteed safe so we skip the escape SIMD scan.
        html_sync! {
            <span class={classes} aria-label={label}>
                { SafeText::from_trusted(label) }
            </span>
        }
        .render(cx)
    }
}

struct DeliveryCell(i64);
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for DeliveryCell {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        if self.0 == 0 {
            cx.out.extend(b"Free delivery")
        } else {
            cx.out.extend(b"+")?;
            Currency(self.0).render(cx)?;
            cx.out.extend(b" delivery")
        }
    }
}

/// eBay's watch-heart click target for the static SSR path: an
/// `<a class="s-card__watchheart-click">` wrapping a two-icon SVG
/// sprite pair. Owns its full clickable surface so the interactive
/// `WatchHeart` island can swap in cleanly without leaving a stray
/// `<a>` parent (the source of the nested-button bug).
struct HeartIcons<'a> {
    aria_label: &'a str,
}

impl<'a, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for HeartIcons<'a> {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        let label = self.aria_label;
        html_sync! {
            <a aria-label={WatchLabel(label)}
               class="s-card__watchheart-click"
               href="#">
                <span class="s-card__watchheart-icon">
                    <svg class="icon icon--16" focusable="false" aria-hidden="true">
                        <use href="#icon-heart-16"></use>
                    </svg>
                    <svg class="icon icon--16" focusable="false" aria-hidden="true">
                        <use href="#icon-heart-filled-16"></use>
                    </svg>
                </span>
            </a>
        }
        .render(cx)
    }
}

struct CouponRow(Option<u8>);
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for CouponRow {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        if let Some(pct) = self.0 {
            html_sync! {
                <div class="s-card__attribute-row">
                    <span class="su-styled-text negative bold large">
                        "Extra "{ Uint(pct as u64) }"% off with coupon"
                    </span>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Card — html_sync! consolidates ~80 static byte sequences into one fused
// write per static run. Compare to the previous hand-rolled `cx.out.extend(b"...")`
// chain (80+ separate `extend` calls per card).
// ---------------------------------------------------------------------------

/// `H` is the watch-heart slot. The static path passes
/// `HeartIcons` (an `<a>`-wrapped sprite pair); the interactive
/// variant substitutes the rsjs `WatchHeart` island. Both heart
/// types own their entire click target so Card doesn't end up with
/// nested `<a>`/`<button>` markup when the island variant is used.
const STATIC_WATCH_HEART_CLASS: &str = "s-card__watchheart on-image s-card__watchheart--watch";
pub const INTERACTIVE_WATCH_HEART_CLASS: &str =
    "s-card__watchheart on-image s-card__watchheart--rsjs";

pub const RSJS_ISLAND_STYLE: &str = r#"<style>.s-card__watchheart--rsjs{background:transparent}.s-card__watchheart--rsjs .s-card__watchheart-click{position:static}</style>"#;

pub struct Card<'a, H> {
    pub listing: &'a Listing,
    pub heart_outer_class: &'static str,
    pub heart: H,
}

impl<'a, H, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for Card<'a, H>
where
    H: WebRenderSync<L, B>,
{
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        let l = self.listing;
        let title = l.title.as_ref();
        let img_hash = l.image_hash.as_ref();
        let heart_outer_class = self.heart_outer_class;
        // Caption / format / location are tightly controlled values we
        // populate from enums or our own static tables — guaranteed HTML-safe.
        let format_label = l.format.label();
        let location_str = l.location.as_ref();

        // Filter chip attrs: surfaced as `data-*` so the URL-driven CSS
        // coordinator (see `ebay_srp_filter_runtime`) can hide cards via
        // pure attribute-selector rules — zero DOM traversal at filter
        // time. Keep the surface MINIMAL: only the attrs the chips today
        // filter on (`format`, `free-shipping`). Adding more here costs
        // SSR bytes per card * cards-per-page, so each one needs to pay
        // its way.
        let format_slug = l.format.slug();
        let free_shipping_flag = if l.delivery_cents == 0 { "1" } else { "0" };
        // Data attrs (`id`, `data-format`, `data-free-shipping`,
        // `data-listingid`) live on the inner `<div>` rather than
        // `<li>` so the consolidated `WatchListings60` rsjs island can
        // wrap each card in a literal `<li class="s-card">` without
        // needing dynamic attribute support on `<li>` (which rsjs v0.1
        // does not provide for non-reactive attrs). The filter
        // coordinator CSS targets the parent `li.s-card` via
        // `:has(> [data-format="..."])` to compensate.
        html_sync! {
            <li class="s-card s-card--horizontal">
                <div id={ItemId(l.item_id)} data-listingid={l.item_id}
                     data-format={format_slug}
                     data-free-shipping={free_shipping_flag}
                     class="su-card-container su-card-container--horizontal">
                    <div class="su-card-container__media">
                        <div class="su-media-container s-card__media-wrapper">
                            <div class="su-media-multi-image">
                                <div class="su-media-multi-image__primary">
                                    <div class="su-media-container">
                                        <div class="su-image">
                                            <a class="s-card__link image-treatment"
                                               href={ItemUrl(l.item_id)}
                                               target="_blank"
                                               style="--aspect-ratio: 1;">
                                                <img class="s-card__image"
                                                     fetchpriority="high"
                                                     loading="eager"
                                                     src={ImgUrl(img_hash)}
                                                     alt={title}
                                                     style="width:auto" />
                                            </a>
                                        </div>
                                    </div>
                                </div>
                            </div>
                            <div class="su-media__action su-media__action--top-right">
                                <span class={heart_outer_class}>
                                    { self.heart }
                                </span>
                            </div>
                        </div>
                    </div>
                    <div class="su-card-container__content">
                        <div class="su-card-container__header">
                            <div class="s-card__caption">{ CaptionBadge(l.caption) }</div>
                            <a class="s-card__link"
                               target="_blank"
                               tabindex="-1"
                               href={ItemUrl(l.item_id)}>
                                // `_a11y_ok` mirrors eBay's production
                                // markup byte-for-byte. eBay ships
                                // `<div role="heading" aria-level="3">`
                                // (not <h3>) for the card title; we
                                // preserve that to keep the rendered
                                // DOM identical to the source page the
                                // bench mirrors. The opt-out is stripped
                                // from emitted HTML by html_sync.
                                <div _a11y_ok role="heading" aria-level="3" class="s-card__title">
                                    <span class="su-styled-text primary default">
                                        { text(title) }
                                    </span>
                                </div>
                            </a>
                            <div class="s-card__subtitle-row">
                                <div class="s-card__subtitle">
                                    { SubtitleRow { parts: &l.subtitle_parts } }
                                </div>
                            </div>
                            <div class="s-card__product-reviews">
                                <div class="s-card__reviews">
                                    <a href="#UserReviews">
                                        <div class="x-star-rating">
                                            { StarRow { full: l.stars_full, half: l.has_half_star } }
                                        </div>
                                        <span class="s-card__reviews-count">
                                            <span aria-hidden="false">
                                                { Commas(l.review_count as u64) }" product ratings"
                                            </span>
                                        </span>
                                    </a>
                                </div>
                            </div>
                        </div>
                        <div class="su-card-container__attributes su-card-container__attributes--has-secondary">
                            <div class="su-card-container__attributes__primary">
                                <div class="s-card__attribute-row">
                                    <span class="su-styled-text primary bold large-1 s-card__price">
                                        { Currency(l.price_cents) }
                                    </span>
                                    { OriginalPriceCell(l.original_price_cents) }
                                </div>
                                <div class="s-card__attribute-row">
                                    <span class="su-styled-text secondary large">
                                        { SafeText::from_trusted(format_label) }
                                    </span>
                                </div>
                                <div class="s-card__attribute-row">
                                    <span class="su-styled-text secondary large">
                                        { DeliveryCell(l.delivery_cents) }
                                    </span>
                                </div>
                                <div class="s-card__attribute-row">
                                    <span class="su-styled-text secondary large">
                                        "Located in "{ text(location_str) }
                                    </span>
                                </div>
                                <div class="s-card__attribute-row">
                                    <span class="su-styled-text primary bold large">
                                        { Commas(l.watcher_count as u64) }" watchers"
                                    </span>
                                </div>
                                { CouponRow(l.coupon_pct) }
                            </div>
                            <div class="su-card-container__attributes__secondary">
                                <div class="s-card__attribute-row">
                                    <span class="su-styled-text primary large">
                                        { text(l.seller_name.as_ref()) }" "
                                    </span>
                                    <span class="su-styled-text primary large">
                                        { FeedbackPct(l.seller_feedback_pct_x10) }
                                        " positive ("
                                        { AbbrevCount(l.seller_feedback_count) }
                                        ")"
                                    </span>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </li>
        }
        .render(cx)
    }
}

// ---------------------------------------------------------------------------
// Whole-listings entry: <ul> wrapper + N cards.
// ---------------------------------------------------------------------------

pub struct ListingsList<'a> {
    pub listings: &'a [Listing],
}

impl<'a, B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for ListingsList<'a> {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        cx.out
            .extend(b"<ul class=\"srp-results srp-list clearfix\">")?;
        for listing in self.listings {
            Card {
                listing,
                heart_outer_class: STATIC_WATCH_HEART_CLASS,
                heart: HeartIcons {
                    aria_label: listing.title.as_ref(),
                },
            }
            .render(cx)?;
        }
        cx.out.extend(b"</ul>")
    }
}

// ---------------------------------------------------------------------------
// Card splitting for the consolidated WatchListings island.
// ---------------------------------------------------------------------------

/// Sentinel emitted in place of the heart slot when we render a Card
/// for the consolidated `WatchListings` island. The island splits the
/// rendered bytes on this string to produce two `UnsafeRaw` chunks
/// (the markup before the heart, and the markup after) which are
/// spliced around an rsjs `<button>` inside one large island. Pick a
/// string that cannot appear in normal SSR output; an HTML comment
/// guarantees the parser ignores any stray sentinel that survives a
/// split (defence in depth — `splitn(2)` should never leave one).
pub const HEART_SENTINEL: &str = "<!--__rsjs_heart_slot__-->";

pub struct HeartSentinelMarker;
impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for HeartSentinelMarker {
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), CoreWriteError> {
        cx.out.extend(HEART_SENTINEL.as_bytes())
    }
}

/// Render a single `Card` to bytes with the heart slot replaced by
/// `HEART_SENTINEL`, strip the outer `<li>` wrapper, then split on
/// the sentinel. The two resulting `String`s — the pre-heart prefix
/// and the post-heart suffix — are fed back into the consolidated
/// `WatchListings60` island as `UnsafeRaw` chunks bracketing the
/// rsjs `<button>`. Each chunk is individually unbalanced HTML;
/// their concatenation around the button (inside the island's own
/// literal `<li class="s-card">`) reconstitutes the original card
/// markup byte-for-byte.
///
/// The outer `<li>` is stripped because the island re-wraps each
/// card in its own literal `<li>` (rsjs v0.1 only allows literal
/// `<li>` as a direct child of `<ul>`, not slot expressions).
pub fn split_card_for_island(listing: &Listing) -> (String, String) {
    let card = Card {
        listing,
        heart_outer_class: INTERACTIVE_WATCH_HEART_CLASS,
        heart: HeartSentinelMarker,
    };
    let mut buf = Vec::with_capacity(4 * 1024);
    let mut cx = proa_core::ctx::Ctx::with_buffer(&mut buf);
    card.render(&mut cx)
        .expect("card render into Vec cannot fail");
    let rendered = String::from_utf8(buf).expect("card markup is UTF-8");

    // Strip leading `<li ...>` and trailing `</li>` so the inner
    // markup can be re-wrapped by the island.
    let after_li_open = {
        let trimmed = rendered.trim_start();
        let li_open_end = trimmed
            .find('>')
            .expect("rendered card starts with `<li ...>`");
        &trimmed[li_open_end + 1..]
    };
    let inner = {
        let li_close = after_li_open
            .rfind("</li>")
            .expect("rendered card ends with `</li>`");
        &after_li_open[..li_close]
    };

    let mut parts = inner.splitn(2, HEART_SENTINEL);
    let pre = parts
        .next()
        .expect("splitn always yields at least one part")
        .to_string();
    let post = parts
        .next()
        .expect("HEART_SENTINEL must be present in the rendered card")
        .to_string();
    (pre, post)
}

// ---------------------------------------------------------------------------
// Public render entry points (Vec, RenderBuffer, ZeroCopyBuf).
// ---------------------------------------------------------------------------

pub fn render_listings_into(listings: &[Listing], buffer: &mut Vec<u8>) {
    let mut out = std::mem::take(buffer);
    out.clear();
    let mut cx = WebContext::with_buffer(out);
    WebRenderSync::render(ListingsList { listings }, &mut cx).unwrap();
    *buffer = cx.into_output();
}

pub fn render_listings_to_string(listings: &[Listing]) -> String {
    proa_core::render_to_string(ListingsList { listings })
}

/// Render directly into a caller-provided RenderBuffer. The buffer is
/// cleared first; on return its `slices()` can be flushed to the response.
pub fn render_listings_into_chained(listings: &[Listing], buf: &mut RenderBuffer) {
    buf.clear();
    let mut cx = WebContext::with_buffer(buf);
    ListingsList { listings }.render(&mut cx).unwrap();
}

/// Render into a `ZeroCopyBuf` — every static byte sequence emitted by
/// `html_sync!` becomes a `Part::Static` (pointer, no copy). Only dynamic
/// (escaped or formatted) bytes land in the owned buffer.
pub fn render_listings_into_zerocopy(listings: &[Listing]) -> ZeroCopyBuf {
    // Estimate: ~30 parts per card × N cards + headroom for dynamic bytes.
    let est_parts = listings.len().saturating_mul(40) + 8;
    let est_owned = listings.len().saturating_mul(800) + 4096;
    let mut buf = ZeroCopyBuf::with_capacity(est_parts, est_owned);
    {
        let mut cx = WebContext::with_buffer(&mut buf);
        ListingsList { listings }.render(&mut cx).unwrap();
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_real_card_markup() {
        let listings = build_listings(3);
        let html = render_listings_to_string(&listings);
        assert!(html.contains("s-card s-card--horizontal"));
        assert!(html.contains("su-card-container__media"));
        assert!(html.contains("s-card__price"));
        assert!(html.contains("#icon-star-filled-16"));
        assert!(html.contains("#icon-heart-16"));
        assert!(html.contains("$"));
        // Escaped title chars
        assert!(html.contains("&amp;"));
    }

    #[test]
    fn cached_defaults_are_stable() {
        let a = default_listings(60);
        let b = default_listings(60);
        assert_eq!(a.as_ptr(), b.as_ptr(), "cache should return the same slice");
        assert_eq!(a.len(), 60);
    }

    #[test]
    fn accepts_legacy_and_branded_caption_overrides() {
        for caption in ["EditorsPick", "EbayChoice"] {
            let overrides = serde_json::json!([{"caption": caption}]);
            let listings = build_listings_with_overrides(1, &overrides);
            assert!(listings[0].caption == CaptionSignal::EditorsPick);
            assert!(render_listings_to_string(&listings).contains("eBay's CHOICE"));
        }
    }

    #[test]
    fn card_emits_data_filter_attributes() {
        // The URL-driven filter runtime relies on these attrs being on
        // every card's outermost `<li>`. If you're tempted to remove or
        // rename them, update `ebay_srp_filter_runtime.rs` in lockstep.
        let listings = build_listings(8);
        let html = render_listings_to_string(&listings);
        assert!(
            html.contains(r#"data-format="buyitnow""#)
                || html.contains(r#"data-format="auction""#)
                || html.contains(r#"data-format="bestoffer""#),
            "expected at least one `data-format` attr in rendered cards, got:\n{html}"
        );
        assert!(
            html.contains(r#"data-free-shipping="1""#),
            "expected at least one `data-free-shipping=\"1\"` attr (free-delivery cards), got:\n{html}"
        );
        assert!(
            html.contains(r#"data-free-shipping="0""#),
            "expected at least one `data-free-shipping=\"0\"` attr (paid-delivery cards), got:\n{html}"
        );
    }

    #[test]
    fn format_slug_round_trips() {
        assert_eq!(ListingFormat::BuyItNow.slug(), "buyitnow");
        assert_eq!(ListingFormat::Auction.slug(), "auction");
        assert_eq!(ListingFormat::BestOffer.slug(), "bestoffer");
    }

    #[test]
    fn zerocopy_matches_vec_render() {
        let listings = build_listings(5);
        let mut vec_buf = Vec::new();
        render_listings_into(&listings, &mut vec_buf);

        let zc = render_listings_into_zerocopy(&listings);
        let mut zc_flat = Vec::new();
        zc.flatten_into(&mut zc_flat);
        assert_eq!(vec_buf, zc_flat);
    }

    #[test]
    fn a11y_ok_attr_is_stripped_from_emitted_html() {
        // Pin the contract: the `_a11y_ok` opt-out attribute on the
        // card heading must (a) silence html_sync's "prefer h1-h6"
        // warning at compile time AND (b) never reach the rendered
        // HTML. If the attribute leaked into output, anyone scraping
        // the demo would see framework internals.
        let listings = build_listings(1);
        let mut buf = Vec::new();
        let mut cx = ::proa_core::ctx::Ctx::with_buffer(&mut buf);
        ListingsList {
            listings: &listings,
        }
        .render(&mut cx)
        .expect("render");
        let html = String::from_utf8(buf).expect("utf8");
        assert!(
            !html.contains("a11y-ok") && !html.contains("a11y_ok"),
            "_a11y_ok must be stripped from emitted HTML; got snippet: {}",
            html.split("s-card__title").next().unwrap_or("")
        );
        assert!(
            html.contains(r#"role="heading""#),
            "the underlying mirrored heading markup must still ship"
        );
    }
}
