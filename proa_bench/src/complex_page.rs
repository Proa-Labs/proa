// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Complex control flow benchmark page
//!
//! Exercises features that simple template engines (Handlebars, Mustache) cannot do:
//! - Deeply nested conditionals
//! - Pattern matching on enums
//! - Computed/dynamic class composition
//! - Inline filter/map/sort chains
//! - Format/transform function calls
//! - Conditional rendering with fallbacks
use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;
#[derive(Clone, Copy)]
pub enum Category {
    Electronics,
    Clothing,
    Food,
    Books,
    Sports,
}
impl Category {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Electronics => "electronics",
            Self::Clothing => "clothing",
            Self::Food => "food",
            Self::Books => "books",
            Self::Sports => "sports",
        }
    }
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Electronics => "⚡",
            Self::Clothing => "👕",
            Self::Food => "🍎",
            Self::Books => "📚",
            Self::Sports => "⚽",
        }
    }
    pub const fn badge_class(self) -> &'static str {
        match self {
            Self::Electronics => "bg-blue-100 text-blue-800",
            Self::Clothing => "bg-purple-100 text-purple-800",
            Self::Food => "bg-green-100 text-green-800",
            Self::Books => "bg-amber-100 text-amber-800",
            Self::Sports => "bg-red-100 text-red-800",
        }
    }
}
#[derive(Clone, Copy)]
pub enum StockStatus {
    InStock(u32),
    LowStock(u32),
    OutOfStock,
    PreOrder { days_until: u32 },
}
#[derive(Clone, Copy)]
pub enum Badge {
    Sale { percent: u8 },
    New,
    Limited,
    BestSeller,
    Clearance,
}
impl Badge {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sale { .. } => "SALE",
            Self::New => "NEW",
            Self::Limited => "LIMITED",
            Self::BestSeller => "BEST SELLER",
            Self::Clearance => "CLEARANCE",
        }
    }
    pub const fn class(self) -> &'static str {
        match self {
            Self::Sale { .. } => "bg-red-500 text-white",
            Self::New => "bg-emerald-500 text-white",
            Self::Limited => "bg-amber-500 text-white",
            Self::BestSeller => "bg-indigo-500 text-white",
            Self::Clearance => "bg-gray-500 text-white",
        }
    }
}
pub struct Variant {
    pub name: &'static str,
    pub available: bool,
    pub price_modifier: f64,
}
pub struct Product {
    pub name: &'static str,
    pub price: f64,
    pub category: Category,
    pub stock: StockStatus,
    pub rating: f64,
    pub review_count: u32,
    pub badges: &'static [Badge],
    pub tags: &'static [&'static str],
    pub variants: &'static [Variant],
    pub description: &'static str,
    pub featured: bool,
}
pub const PRODUCTS: &[Product] = &[
    Product {
        name: "Wireless Noise-Canceling Headphones",
        price: 299.99,
        category: Category::Electronics,
        stock: StockStatus::InStock(142),
        rating: 4.7,
        review_count: 2847,
        badges: &[Badge::BestSeller],
        tags: &["wireless", "bluetooth", "noise-canceling", "premium"],
        variants: &[
            Variant {
                name: "Black",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "White",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "Navy",
                available: false,
                price_modifier: 10.0,
            },
        ],
        description: "Premium wireless headphones with industry-leading noise cancellation.",
        featured: true,
    },
    Product {
        name: "Organic Cotton T-Shirt",
        price: 34.99,
        category: Category::Clothing,
        stock: StockStatus::LowStock(3),
        rating: 4.2,
        review_count: 523,
        badges: &[Badge::Sale { percent: 20 }, Badge::Limited],
        tags: &["organic", "cotton", "sustainable"],
        variants: &[
            Variant {
                name: "S",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "M",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "L",
                available: false,
                price_modifier: 0.0,
            },
            Variant {
                name: "XL",
                available: true,
                price_modifier: 5.0,
            },
        ],
        description: "Ethically sourced, 100% organic cotton t-shirt.",
        featured: false,
    },
    Product {
        name: "Artisan Sourdough Bread Mix",
        price: 12.99,
        category: Category::Food,
        stock: StockStatus::InStock(500),
        rating: 4.9,
        review_count: 1205,
        badges: &[Badge::New, Badge::BestSeller],
        tags: &["artisan", "sourdough", "organic", "baking", "vegan"],
        variants: &[
            Variant {
                name: "500g",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "1kg",
                available: true,
                price_modifier: 8.0,
            },
        ],
        description: "Professional-grade sourdough starter and flour blend.",
        featured: true,
    },
    Product {
        name: "The Rust Programming Language",
        price: 49.99,
        category: Category::Books,
        stock: StockStatus::OutOfStock,
        rating: 4.8,
        review_count: 3421,
        badges: &[Badge::BestSeller],
        tags: &["programming", "rust", "systems"],
        variants: &[
            Variant {
                name: "Paperback",
                available: false,
                price_modifier: 0.0,
            },
            Variant {
                name: "Hardcover",
                available: false,
                price_modifier: 15.0,
            },
            Variant {
                name: "eBook",
                available: true,
                price_modifier: -20.0,
            },
        ],
        description: "The official book on the Rust programming language.",
        featured: false,
    },
    Product {
        name: "Carbon Fiber Tennis Racket",
        price: 189.99,
        category: Category::Sports,
        stock: StockStatus::PreOrder { days_until: 14 },
        rating: 3.9,
        review_count: 87,
        badges: &[Badge::New],
        tags: &["tennis", "carbon-fiber", "professional"],
        variants: &[
            Variant {
                name: "Grip 4",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "Grip 4 1/4",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "Grip 4 3/8",
                available: true,
                price_modifier: 0.0,
            },
        ],
        description: "Tournament-grade carbon fiber racket with vibration dampening.",
        featured: false,
    },
    Product {
        name: "Smart Home Hub Pro",
        price: 149.99,
        category: Category::Electronics,
        stock: StockStatus::LowStock(7),
        rating: 4.1,
        review_count: 1893,
        badges: &[Badge::Sale { percent: 30 }, Badge::Clearance],
        tags: &[
            "smart-home",
            "iot",
            "voice-control",
            "wifi",
            "zigbee",
            "matter",
        ],
        variants: &[
            Variant {
                name: "Standard",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "Pro",
                available: true,
                price_modifier: 50.0,
            },
        ],
        description: "Central hub for all your smart home devices.",
        featured: true,
    },
    Product {
        name: "Merino Wool Running Socks",
        price: 24.99,
        category: Category::Clothing,
        stock: StockStatus::InStock(892),
        rating: 4.6,
        review_count: 445,
        badges: &[],
        tags: &["merino", "wool", "running", "moisture-wicking"],
        variants: &[
            Variant {
                name: "S (6-8)",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "M (8-10)",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "L (10-12)",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "XL (12-14)",
                available: true,
                price_modifier: 2.0,
            },
        ],
        description: "Temperature-regulating merino wool socks for long runs.",
        featured: false,
    },
    Product {
        name: "Japanese Matcha Powder",
        price: 32.99,
        category: Category::Food,
        stock: StockStatus::InStock(234),
        rating: 4.4,
        review_count: 678,
        badges: &[Badge::Limited],
        tags: &["matcha", "japanese", "ceremonial-grade", "organic"],
        variants: &[
            Variant {
                name: "30g Tin",
                available: true,
                price_modifier: 0.0,
            },
            Variant {
                name: "100g Bag",
                available: true,
                price_modifier: 28.0,
            },
        ],
        description: "Ceremonial-grade matcha from Uji, Kyoto.",
        featured: false,
    },
];
pub struct ComplexPage;
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComplexPage {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <html lang="en">
                <head>
                    <title>"Product Listing"</title>
                    <meta
                        name="viewport"
                        content="width=device-width, initial-scale=1"
                    />
                </head>
                <body>
                    <header class="border-b px-6 py-4">
                        <h1 class="text-2xl font-bold">"Products"</h1>
                        <p class="text-gray-500">{ProductCount}</p>
                    </header>
                    <main class="grid gap-6 p-6 md:grid-cols-2 lg:grid-cols-3">{ProductList}</main>
                </body>
            </html>
        }
        .render(cx)
    }
}
struct ProductCount;
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ProductCount {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let in_stock = PRODUCTS
            .iter()
            .filter(|p| matches!(p.stock, StockStatus::InStock(_) | StockStatus::LowStock(_)))
            .count();
        let total = PRODUCTS.len();
        if in_stock == total {
            html_sync! {
                <span class="text-green-600">
                    "All "
                    {proa_core::text(itoa(total))}
                    " products in stock"
                </span>
            }
            .render(cx)
        } else if in_stock == 0 {
            html_sync! {
                <span class="text-red-600">"All products out of stock"</span>
            }
            .render(cx)
        } else {
            html_sync! {
                <span>
                    {proa_core::text(itoa(in_stock))}
                    " of "
                    {proa_core::text(itoa(total))}
                    " in stock"
                </span>
            }
            .render(cx)
        }
    }
}
struct ProductList;
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ProductList {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        // proa-perf-ok: benchmark page intentionally exercises allocation-heavy sort setup
        let mut indices: Vec<usize> = (0..PRODUCTS.len()).collect();
        indices.sort_by(|&a, &b| {
            let pa = &PRODUCTS[a];
            let pb = &PRODUCTS[b];
            pb.featured
                .cmp(&pa.featured)
                .then(pb.rating.partial_cmp(&pa.rating).unwrap())
        });
        for &idx in &indices {
            ProductCard {
                product: &PRODUCTS[idx],
            }
            .render(cx)?;
        }
        Ok(())
    }
}
struct ProductCard<'a> {
    product: &'a Product,
}
impl<'a, L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ProductCard<'a> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let p = self.product;
        let card_class = match p.stock {
            StockStatus::OutOfStock => {
                "rounded-xl border p-5 shadow-sm transition-shadow hover:shadow-md border-red-200 opacity-75"
            }
            StockStatus::LowStock(_) => {
                "rounded-xl border p-5 shadow-sm transition-shadow hover:shadow-md border-amber-200"
            }
            StockStatus::PreOrder { .. } => {
                "rounded-xl border p-5 shadow-sm transition-shadow hover:shadow-md border-blue-200"
            }
            StockStatus::InStock(_) if p.featured => {
                "rounded-xl border p-5 shadow-sm transition-shadow hover:shadow-md border-emerald-300 ring-1 ring-emerald-100"
            }
            StockStatus::InStock(_) => {
                "rounded-xl border p-5 shadow-sm transition-shadow hover:shadow-md border-gray-200"
            }
        };
        html_sync! {
            <article class={card_class}>
                {BadgeRow { badges : p.badges }}
                {CategoryHeader { product : p }}
                {PriceDisplay { product : p }}
                {StockIndicator { stock : & p.stock }}
                {RatingDisplay {
                    rating: p.rating,
                    count: p.review_count,
                }}
                {TagList { tags : p.tags }}
                {VariantSelector {
                    variants: p.variants,
                    stock: & p.stock,
                }}
                {Description {
                    text: p.description,
                    featured: p.featured,
                }}
            </article>
        }
        .render(cx)
    }
}
struct BadgeRow {
    badges: &'static [Badge],
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for BadgeRow {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if self.badges.is_empty() {
            return Ok(());
        }
        html_sync! {
            <div class="mb-3 flex gap-2">{BadgeItems { badges : self.badges }}</div>
        }
        .render(cx)
    }
}
struct BadgeItems {
    badges: &'static [Badge],
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for BadgeItems {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        for badge in self.badges {
            let label = badge.label();
            // proa-perf-ok: benchmark page intentionally exercises dynamic class formatting
            let badge_class: String = format!(
                "inline-block rounded-full px-2 py-0.5 text-xs font-semibold {}",
                badge.class()
            );
            match badge {
                Badge::Sale { percent } => {
                    let pct = itoa(*percent as usize);
                    html_sync! {
                        <span class={badge_class.as_str()}>
                            {proa_core::text(label)}
                            " -"
                            {proa_core::text(pct)}
                            "%"
                        </span>
                    }
                    .render(cx)?;
                }
                _ => {
                    html_sync! {
                        <span class={badge_class.as_str()}>{proa_core::text(label)}</span>
                    }
                    .render(cx)?;
                }
            }
        }
        Ok(())
    }
}
struct CategoryHeader<'a> {
    product: &'a Product,
}
impl<'a, L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for CategoryHeader<'a> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let p = self.product;
        let cat = p.category;
        // proa-perf-ok: benchmark page intentionally exercises dynamic class formatting
        let cat_badge: String = format!(
            "inline-block rounded px-2 py-0.5 text-xs {}",
            cat.badge_class()
        );
        match cat {
            Category::Electronics => html_sync! {
                <div class="mb-2">
                    <div class="flex items-center gap-2">
                        <span class="text-lg">{proa_core::text(cat.icon())}</span>
                        <h2 class="text-lg font-semibold">{proa_core::text(p.name)}</h2>
                    </div>
                    <span class={cat_badge.as_str()}>{proa_core::text(cat.as_str())}</span>
                </div>
            }
            .render(cx),
            Category::Books => html_sync! {
                <div class="mb-2 border-l-4 border-amber-300 pl-3">
                    <h2 class="text-lg font-semibold italic">{proa_core::text(p.name)}</h2>
                    <span class="text-xs text-gray-500">
                        {proa_core::text(cat.icon())}
                        " "
                        {proa_core::text(cat.as_str())}
                    </span>
                </div>
            }
            .render(cx),
            _ => html_sync! {
                <div class="mb-2">
                    <h2 class="text-lg font-semibold">{proa_core::text(p.name)}</h2>
                    <span class={cat_badge.as_str()}>
                        {proa_core::text(cat.icon())}
                        " "
                        {proa_core::text(cat.as_str())}
                    </span>
                </div>
            }
            .render(cx),
        }
    }
}
struct PriceDisplay<'a> {
    product: &'a Product,
}
impl<'a, L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for PriceDisplay<'a> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let p = self.product;
        let discount_pct = p.badges.iter().find_map(|b| match b {
            Badge::Sale { percent } => Some(*percent),
            _ => None,
        });
        match discount_pct {
            Some(pct) => {
                let discounted = p.price * (1.0 - pct as f64 / 100.0);
                html_sync! {
                    <div class="mb-2 flex items-baseline gap-2">
                        <span class="text-xl font-bold text-red-600">
                            {proa_core::text(format_price(discounted))}
                        </span>
                        <span class="text-sm text-gray-400 line-through">
                            {proa_core::text(format_price(p.price))}
                        </span>
                        <span class="text-xs font-medium text-red-500">
                            {proa_core::text(itoa(pct as
                            usize))}
                            "% off"
                        </span>
                    </div>
                }
                .render(cx)
            }
            None => html_sync! {
                <div class="mb-2">
                    <span class="text-xl font-bold">{proa_core::text(format_price(p.price))}</span>
                </div>
            }
            .render(cx),
        }
    }
}
struct StockIndicator<'a> {
    stock: &'a StockStatus,
}
impl<'a, L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for StockIndicator<'a> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        match self.stock {
            StockStatus::InStock(qty) => {
                let (dot_class, text_class, label) = if *qty > 100 {
                    (
                        "inline-block h-2 w-2 rounded-full bg-green-500",
                        "text-xs font-medium text-green-700",
                        "In Stock",
                    )
                } else if *qty > 20 {
                    (
                        "inline-block h-2 w-2 rounded-full bg-green-400",
                        "text-xs font-medium text-green-600",
                        "In Stock",
                    )
                } else {
                    (
                        "inline-block h-2 w-2 rounded-full bg-yellow-400",
                        "text-xs font-medium text-yellow-600",
                        "Limited Stock",
                    )
                };
                if *qty <= 50 {
                    html_sync! {
                        <div class="mb-2 flex items-center gap-1.5">
                            <span class={dot_class}></span>
                            <span class={text_class}>
                                {proa_core::text(label)}
                                " ("
                                {proa_core::text(itoa(* qty as
                                usize))}
                                " left)"
                            </span>
                        </div>
                    }
                    .render(cx)
                } else {
                    html_sync! {
                        <div class="mb-2 flex items-center gap-1.5">
                            <span class={dot_class}></span>
                            <span class={text_class}>{proa_core::text(label)}</span>
                        </div>
                    }
                    .render(cx)
                }
            }
            StockStatus::LowStock(qty) => html_sync! {
                <div class="mb-2 flex items-center gap-1.5">
                    <span class="inline-block h-2 w-2 animate-pulse rounded-full bg-amber-500"></span>
                    <span class="text-xs font-medium text-amber-600">
                        "Only "
                        {proa_core::text(itoa(* qty as usize))}
                        " left — order soon"
                    </span>
                </div>
            }
            .render(cx),
            StockStatus::OutOfStock => html_sync! {
                <div class="mb-2 flex items-center gap-1.5">
                    <span class="inline-block h-2 w-2 rounded-full bg-red-500"></span>
                    <span class="text-xs font-medium text-red-600">"Out of Stock"</span>
                </div>
            }
            .render(cx),
            StockStatus::PreOrder { days_until } => {
                let urgency_class = if *days_until <= 7 {
                    "text-xs text-blue-700 font-semibold"
                } else {
                    "text-xs text-blue-500"
                };
                html_sync! {
                    <div class="mb-2 flex items-center gap-1.5">
                        <span class="inline-block h-2 w-2 rounded-full bg-blue-500"></span>
                        <span class={urgency_class}>
                            "Pre-order — ships in "
                            {proa_core::text(itoa(* days_until as usize))}
                            " days"
                        </span>
                    </div>
                }
                .render(cx)
            }
        }
    }
}
struct RatingDisplay {
    rating: f64,
    count: u32,
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RatingDisplay {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let full_stars = self.rating as u32;
        let partial_pct = ((self.rating - full_stars as f64) * 100.0) as u32;
        html_sync! {
            <div class="mb-2 flex items-center gap-1">
                {Stars {
                    full: full_stars,
                    partial_pct,
                }}
                <span class="ml-1 text-xs text-gray-500">
                    {proa_core::text(format_rating(self.rating))}
                    " ("
                    {proa_core::text(format_count(self.count))}
                    ")"
                </span>
            </div>
        }
        .render(cx)
    }
}
struct Stars {
    full: u32,
    partial_pct: u32,
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Stars {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        for i in 0..5u32 {
            let fill = if i < self.full {
                "100"
            } else if i == self.full && self.partial_pct > 0 {
                match self.partial_pct {
                    0..=25 => "25",
                    26..=50 => "50",
                    51..=75 => "75",
                    _ => "100",
                }
            } else {
                "0"
            };
            let class = if fill == "0" {
                "text-gray-300"
            } else {
                "text-yellow-400"
            };
            html_sync! {
                <span class={class} data-fill={fill}>"★"</span>
            }
            .render(cx)?;
        }
        Ok(())
    }
}
struct TagList {
    tags: &'static [&'static str],
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TagList {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if self.tags.is_empty() {
            return Ok(());
        }
        // proa-perf-ok: benchmark page intentionally exercises filtered tag sorting
        let mut sorted: Vec<&str> = self.tags.iter().filter(|t| t.len() > 3).copied().collect();
        sorted.sort();
        let display_count = sorted.len().min(4);
        let remaining = sorted.len().saturating_sub(4);
        html_sync! {
            <div class="mb-2 flex flex-wrap gap-1">
                {TagItems {
                    tags: & sorted [..display_count],
                }}
            </div>
        }
        .render(cx)?;
        if remaining > 0 {
            html_sync! {
                <span class="rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-500">
                    "+"
                    {proa_core::text(itoa(remaining))}
                    " more"
                </span>
            }
            .render(cx)?;
        }
        Ok(())
    }
}
struct TagItems<'a> {
    tags: &'a [&'static str],
}
impl<'a, L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TagItems<'a> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        for tag in self.tags {
            html_sync! {
                <span class="rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-600">
                    {proa_core::text(tag)}
                </span>
            }
            .render(cx)?;
        }
        Ok(())
    }
}
struct VariantSelector<'a> {
    variants: &'static [Variant],
    stock: &'a StockStatus,
}
impl<'a, L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for VariantSelector<'a> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let is_orderable = !matches!(self.stock, StockStatus::OutOfStock);
        html_sync! {
            <div class="mb-2">
                <div class="flex flex-wrap gap-1">
                    {VariantButtons { variants : self.variants, orderable : is_orderable }}
                </div>
            </div>
        }
        .render(cx)
    }
}
struct VariantButtons {
    variants: &'static [Variant],
    orderable: bool,
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for VariantButtons {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        for v in self.variants {
            let enabled = self.orderable && v.available && v.price_modifier < 100.0;
            let btn_class = if enabled {
                "rounded border px-2 py-1 text-xs hover:bg-gray-50"
            } else {
                "rounded border px-2 py-1 text-xs opacity-40 line-through cursor-not-allowed"
            };
            if v.price_modifier > 0.0 {
                let suffix = format_price_modifier(v.price_modifier);
                html_sync! {
                    <button
                        class={btn_class}
                        type="button"
                        disabled={(!enabled).then_some("true")}
                    >
                        {proa_core::text(v.name)}
                        " "
                        <span class="text-gray-400">{proa_core::text(suffix)}</span>
                    </button>
                }
                .render(cx)?;
            } else {
                html_sync! {
                    <button
                        class={btn_class}
                        type="button"
                        disabled={(!enabled).then_some("true")}
                    >
                        {proa_core::text(v.name)}
                    </button>
                }
                .render(cx)?;
            }
        }
        Ok(())
    }
}
struct Description {
    text: &'static str,
    featured: bool,
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Description {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let display = if self.featured || self.text.len() <= 80 {
            self.text
        } else {
            match self.text[..80].rfind(' ') {
                Some(pos) => &self.text[..pos],
                None => &self.text[..80],
            }
        };
        let truncated = display.len() < self.text.len();
        if truncated {
            html_sync! {
                <p class="text-sm text-gray-600">
                    {proa_core::text(display)}
                    <span class="text-gray-400">"..."</span>
                </p>
            }
            .render(cx)
        } else {
            html_sync! {
                <p class="text-sm text-gray-600">{proa_core::text(display)}</p>
            }
            .render(cx)
        }
    }
}
// proa-perf-ok(scope): benchmark fixture intentionally exercises owned numeric formatting
fn itoa(n: usize) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        5 => "5",
        6 => "6",
        7 => "7",
        8 => "8",
        9 => "9",
        10 => "10",
        14 => "14",
        20 => "20",
        30 => "30",
        _ => Box::leak(n.to_string().into_boxed_str()),
    }
}
// proa-perf-ok(scope): benchmark fixture intentionally exercises owned price formatting
fn format_price(price: f64) -> &'static str {
    Box::leak(format!("${:.2}", price).into_boxed_str())
}
// proa-perf-ok(scope): benchmark fixture intentionally exercises owned modifier formatting
fn format_price_modifier(modifier: f64) -> &'static str {
    Box::leak(format!("+${:.0}", modifier).into_boxed_str())
}
// proa-perf-ok(scope): benchmark fixture intentionally exercises owned rating formatting
fn format_rating(rating: f64) -> &'static str {
    Box::leak(format!("{:.1}", rating).into_boxed_str())
}
// proa-perf-ok(scope): benchmark fixture intentionally exercises owned count formatting
fn format_count(count: u32) -> &'static str {
    if count >= 1000 {
        Box::leak(format!("{:.1}K", count as f64 / 1000.0).into_boxed_str())
    } else {
        Box::leak(count.to_string().into_boxed_str())
    }
}
/// Render the complex benchmark page to a String
pub fn render() -> String {
    // proa-perf-ok: benchmark convenience API intentionally returns an owned String
    proa_core::render_to_string(ComplexPage)
}
/// Render the complex benchmark page into an existing buffer
pub fn render_into(buffer: &mut Vec<u8>) {
    crate::render_into_vec(buffer, ComplexPage);
}
/// Render using the standard [`proa_core::RenderBuffer`].
pub fn render_into_chained(buffer: &mut proa_core::RenderBuffer) {
    buffer.render_into(ComplexPage);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complex_page_renders() {
        let html = render();
        assert_eq!(html.matches("<article class=").count(), PRODUCTS.len());
        assert!(html.contains("border-red-200 opacity-75"));
        assert!(html.contains("border-blue-200"));
        assert!(html.contains("border-emerald-300 ring-1 ring-emerald-100"));

        let expected_first = PRODUCTS
            .iter()
            .filter(|product| product.featured)
            .max_by(|left, right| left.rating.partial_cmp(&right.rating).unwrap())
            .expect("the benchmark fixture should contain a featured product");
        let first_card = html
            .split_once("<article class=")
            .and_then(|(_, cards)| cards.split_once("</article>"))
            .map(|(card, _)| card)
            .expect("the benchmark should render at least one product card");
        assert!(first_card.contains(expected_first.name));
    }
}
