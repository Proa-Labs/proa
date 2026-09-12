// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Nike Product Detail Page (PDP) — Alphafly 3 for benchmarking.

use super::shared;
use proa_core::{RenderBuffer, StaticRaw, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

// ─── Data types ──────────────────────────────────────────────────────

pub struct ProductData<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub current_price: &'a str,
    pub original_price: &'a str,
    pub discount_pct: &'a str,
    pub color_name: &'a str,
    pub style_code: &'a str,
    pub description: &'a str,
    pub reason_to_buy: &'a str,
    pub weight: &'a str,
    pub drop: &'a str,
    pub benefits: &'a [Benefit<'a>],
    pub whats_new: &'a [&'a str],
    pub sizes: &'a [Size<'a>],
    pub colorways: &'a [Colorway<'a>],
    pub reviews: &'a [Review<'a>],
    pub avg_rating: &'a str,
    pub review_count: &'a str,
    pub recommended: &'a [RecommendedProduct<'a>],
}

pub struct Benefit<'a> {
    pub title: &'a str,
    pub description: &'a str,
}

pub struct Size<'a> {
    pub label: &'a str,
    pub available: bool,
}

pub struct Colorway<'a> {
    pub style_code: &'a str,
    pub selected: bool,
}

pub struct Review<'a> {
    pub author: &'a str,
    pub date: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub rating: u8,
}

pub struct RecommendedProduct<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub price: &'a str,
    pub image: &'a str,
}

// ─── SVG constants ───────────────────────────────────────────────────

const STAR_FILLED_SVG: &str = r##"<svg viewBox="0 0 24 24" width="16" height="16" fill="#111"><path d="M12 1.5l3.09 6.26L22 8.77l-5 4.87 1.18 6.86L12 17.27 5.82 20.5 7 13.64l-5-4.87 6.91-1.01L12 1.5z"/></svg>"##;

const STAR_EMPTY_SVG: &str = r##"<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="#111" stroke-width="1.5"><path d="M12 1.5l3.09 6.26L22 8.77l-5 4.87 1.18 6.86L12 17.27 5.82 20.5 7 13.64l-5-4.87 6.91-1.01L12 1.5z"/></svg>"##;

const HEART_OUTLINE_SVG: &str = r##"<svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" width="24" height="24" fill="none"><path stroke="currentColor" stroke-width="1.5" d="M16.794 3.75c1.324 0 2.568.516 3.504 1.451a4.96 4.96 0 010 7.008L12 20.508l-8.299-8.299a4.96 4.96 0 010-7.008A4.923 4.923 0 017.205 3.75c1.324 0 2.568.516 3.504 1.451l.53.53.53-.53c.937-.935 2.18-1.451 3.504-1.451h.521z"></path></svg>"##;

const DELIVERY_SVG: &str = r##"<svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="#111" stroke-width="1.5"><path d="M1.5 12h5l3-9 5 18 3-9h5"/></svg>"##;

const STORE_SVG: &str = r##"<svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="#111" stroke-width="1.5"><path d="M3 9l1.5-5h15L21 9M3 9h18M3 9v11a1 1 0 001 1h16a1 1 0 001-1V9"/><path d="M9 21V13h6v8"/></svg>"##;

// ─── Render ──────────────────────────────────────────────────────────

pub fn render() -> String {
    let data = default_data();
    let mut buf = RenderBuffer::new();
    render_into(&mut WebContext::with_buffer(&mut buf), &data);
    buf.into_string()
}

pub fn render_into<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &ProductData<'_>,
) {
    shared::render_document_start(cx, "Nike Alphafly 3. Nike.com", "Shop the Nike Alphafly 3 men's road racing shoes. Fine-tuned for marathon speed with ZoomX foam, Air Zoom units, and a full-length carbon fiber plate.");
    let _ = html_sync! {
        <link
            rel="preload"
            as="image"
            href="/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(10).png"
            fetchpriority="high"
        />
    }
    .render(cx);
    shared::render_header(cx);
    render_content(cx, data);
    shared::render_footer(cx);
    shared::render_document_end(cx);
}

pub fn render_content<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &ProductData<'_>,
) {
    let _ = html_sync! {
        <main>
            <div class="nike-pdp">
                // Breadcrumbs
                <nav class="nike-pdp__breadcrumbs">
                    <a href="/showcase/nike/proa">"Home"</a>
                    <span class="nike-pdp__breadcrumb-sep">"/"</span>
                    <a href="/showcase/nike/proa">"Men"</a>
                    <span class="nike-pdp__breadcrumb-sep">"/"</span>
                    <a href="/showcase/nike/proa">"Running"</a>
                    <span class="nike-pdp__breadcrumb-sep">"/"</span>
                    <span>"Running Shoes"</span>
                </nav>

                // Hero: image + info
                <div class="nike-pdp__hero">
                    <div class="nike-pdp__gallery">
                        <img class="nike-pdp__main-img"
                             src="/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(10).png"
                             alt={data.title}
                             width="800"
                             height="800"
                             fetchpriority="high"/>
                    </div>
                    <div class="nike-pdp__info">
                        {render_product_info(cx, data)}
                    </div>
                </div>

                // Description
                <section class="nike-pdp__section">
                    <h2 class="nike-pdp__section-title">{data.title}</h2>
                    <p class="nike-pdp__description">{data.description}</p>
                    <p class="nike-pdp__reason">{data.reason_to_buy}</p>
                </section>

                // Benefits
                <section class="nike-pdp__section">
                    <h2 class="nike-pdp__section-title">"Benefits"</h2>
                    <div class="nike-pdp__benefits">
                        {for benefit in data.benefits.iter() {
                            let _ = html_sync! {
                                <div class="nike-pdp__benefit-card">
                                    <h3 class="nike-pdp__benefit-title">{benefit.title}</h3>
                                    <p class="nike-pdp__benefit-desc">{benefit.description}</p>
                                </div>
                            }.render(cx);
                        }}
                    </div>
                </section>

                // What's New
                <section class="nike-pdp__section">
                    <h2 class="nike-pdp__section-title">"What's New"</h2>
                    <ul class="nike-pdp__whats-new">
                        {for item in data.whats_new.iter() {
                            let _ = html_sync! {
                                <li>{*item}</li>
                            }.render(cx);
                        }}
                    </ul>
                </section>

                // Product Details
                <section class="nike-pdp__section">
                    <h2 class="nike-pdp__section-title">"Product Details"</h2>
                    <dl class="nike-pdp__details">
                        <div class="nike-pdp__detail-row">
                            <dt>"Weight"</dt>
                            <dd>{data.weight}</dd>
                        </div>
                        <div class="nike-pdp__detail-row">
                            <dt>"Drop"</dt>
                            <dd>{data.drop}</dd>
                        </div>
                        <div class="nike-pdp__detail-row">
                            <dt>"Shown"</dt>
                            <dd>{data.color_name}</dd>
                        </div>
                        <div class="nike-pdp__detail-row">
                            <dt>"Style"</dt>
                            <dd>{data.style_code}</dd>
                        </div>
                    </dl>
                </section>

                // Reviews
                <section class="nike-pdp__section">
                    <h2 class="nike-pdp__section-title">"Reviews ("{data.review_count}")"</h2>
                    <div class="nike-pdp__rating-summary">
                        <div class="nike-pdp__stars">
                            {StaticRaw(STAR_FILLED_SVG)}
                            {StaticRaw(STAR_FILLED_SVG)}
                            {StaticRaw(STAR_FILLED_SVG)}
                            {StaticRaw(STAR_FILLED_SVG)}
                            {StaticRaw(STAR_EMPTY_SVG)}
                        </div>
                        <span class="nike-pdp__avg-rating">{data.avg_rating}" out of 5"</span>
                    </div>
                    <div class="nike-pdp__reviews">
                        {for review in data.reviews.iter() {
                            render_review_card(cx, review);
                        }}
                    </div>
                </section>

                // You Might Also Like
                <section class="nike-pdp__section">
                    <h2 class="nike-pdp__section-title">"You Might Also Like"</h2>
                    <div class="nike-pdp__recommended">
                        {for rec in data.recommended.iter() {
                            render_rec_card(cx, rec);
                        }}
                    </div>
                </section>
            </div>
        </main>
    }
    .render(cx);
}

fn render_product_info<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &ProductData<'_>,
) {
    let _ = html_sync! {
        <h1 class="nike-pdp__title">{data.title}</h1>
        <p class="nike-pdp__subtitle">{data.subtitle}</p>

        // Price
        <div class="nike-pdp__price">
            <span class="nike-pdp__current-price">{data.current_price}</span>
            {if !data.original_price.is_empty() {
                let _ = html_sync! {
                    <span class="nike-pdp__original-price">{data.original_price}</span>
                }.render(cx);
            }}
            {if !data.discount_pct.is_empty() {
                let _ = html_sync! {
                    <span class="nike-pdp__discount">{data.discount_pct}</span>
                }.render(cx);
            }}
        </div>

        // Colorways
        <div class="nike-pdp__color-section">
            <p class="nike-pdp__color-name">{data.color_name}</p>
            <div class="nike-pdp__colors">
                {for colorway in data.colorways.iter() {
                    let _ = html_sync! {
                        <div class={if colorway.selected {"nike-pdp__color-chip selected"} else {"nike-pdp__color-chip"}}>
                            <img src={colorway.style_code}
                                 alt="Colorway"
                                 width="80"
                                 height="80"
                                 loading="lazy"/>
                        </div>
                    }.render(cx);
                }}
            </div>
        </div>

        // Size selector
        <div class="nike-pdp__size-section">
            <div class="nike-pdp__size-header">
                <span class="nike-pdp__size-label">"Select Size"</span>
                <a href="#" class="nike-pdp__size-guide">"Size Guide"</a>
            </div>
            <div class="nike-pdp__sizes">
                {for size in data.sizes.iter() {
                    let _ = html_sync! {
                        <button class={if size.available {"nike-pdp__size"} else {"nike-pdp__size disabled"}}
                                type="button">
                            {size.label}
                        </button>
                    }.render(cx);
                }}
            </div>
        </div>

        // Add to Bag
        <button class="nike-pdp__btn-bag" type="button">"Add to Bag"</button>

        // Favorite
        <button class="nike-pdp__btn-fav" type="button">
            "Favorite"
            {StaticRaw(HEART_OUTLINE_SVG)}
        </button>

        // Shipping
        <div class="nike-pdp__shipping">
            <div class="nike-pdp__shipping-row">
                {StaticRaw(STORE_SVG)}
                <div>
                    <strong>"Free Pickup"</strong>
                    <span>" — "</span>
                    <a href="#">"Find a Store"</a>
                </div>
            </div>
            <div class="nike-pdp__shipping-row">
                {StaticRaw(DELIVERY_SVG)}
                <p>"You'll see our shipping options at checkout."</p>
            </div>
        </div>
    }
    .render(cx);
}

fn render_review_card<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    review: &Review<'_>,
) {
    let _ = html_sync! {
        <div class="nike-pdp__review-card">
            <div class="nike-pdp__review-header">
                <div class="nike-pdp__stars">
                    {if review.rating >= 1 { let _ = html_sync! {
                        {StaticRaw(STAR_FILLED_SVG)}
                    }.render(cx); } else { let _ = html_sync! {
                        {StaticRaw(STAR_EMPTY_SVG)}
                    }.render(cx); }}
                    {if review.rating >= 2 { let _ = html_sync! {
                        {StaticRaw(STAR_FILLED_SVG)}
                    }.render(cx); } else { let _ = html_sync! {
                        {StaticRaw(STAR_EMPTY_SVG)}
                    }.render(cx); }}
                    {if review.rating >= 3 { let _ = html_sync! {
                        {StaticRaw(STAR_FILLED_SVG)}
                    }.render(cx); } else { let _ = html_sync! {
                        {StaticRaw(STAR_EMPTY_SVG)}
                    }.render(cx); }}
                    {if review.rating >= 4 { let _ = html_sync! {
                        {StaticRaw(STAR_FILLED_SVG)}
                    }.render(cx); } else { let _ = html_sync! {
                        {StaticRaw(STAR_EMPTY_SVG)}
                    }.render(cx); }}
                    {if review.rating >= 5 { let _ = html_sync! {
                        {StaticRaw(STAR_FILLED_SVG)}
                    }.render(cx); } else { let _ = html_sync! {
                        {StaticRaw(STAR_EMPTY_SVG)}
                    }.render(cx); }}
                </div>
                <span class="nike-pdp__review-date">{review.date}</span>
            </div>
            <h3 class="nike-pdp__review-title">{review.title}</h3>
            <p class="nike-pdp__review-body">{review.body}</p>
            <span class="nike-pdp__review-author">{review.author}</span>
        </div>
    }
    .render(cx);
}

fn render_rec_card<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    rec: &RecommendedProduct<'_>,
) {
    let _ = html_sync! {
        <div class="nike-pdp__rec-card">
            <a class="nike-pdp__rec-link" href="/showcase/nike/proa/product">
                <div class="nike-pdp__rec-img-wrap">
                    <img
                        class="nike-pdp__rec-img"
                        src={rec.image}
                        alt={rec.title}
                        loading="lazy"
                        decoding="async"
                        width="400"
                        height="400"
                    />
                </div>
                <div class="nike-pdp__rec-info">
                    <div class="nike-pdp__rec-title">{rec.title}</div>
                    <div class="nike-pdp__rec-subtitle">{rec.subtitle}</div>
                    <div class="nike-pdp__rec-price">{rec.price}</div>
                </div>
            </a>
        </div>
    }
    .render(cx);
}

// ─── Default data ────────────────────────────────────────────────────

pub fn default_data() -> ProductData<'static> {
    ProductData {
        title: "Nike Alphafly 3",
        subtitle: "Men's Road Racing Shoes",
        current_price: "$250.97",
        original_price: "$295",
        discount_pct: "15% off",
        color_name: "White/Soft Pearl/Hyper Crimson/Black",
        style_code: "IM6673-100",
        description: "Fine-tuned for marathon speed, the Alphafly 3 helps you push beyond what you thought possible with three innovative technologies. A double dose of Air Zoom units works with a full-length carbon fiber plate to help launch you into your next step. The ZoomX foam midsole helps keep you fresh from mile 1 to 26.2. Time to leave your old personal records in the dust.",
        reason_to_buy: "Pinnacle tech creates a propulsive ride fine-tuned for marathon speed.",
        weight: "Approx. 218g/7.7oz (Men's US 10)",
        drop: "8mm",
        benefits: DEFAULT_BENEFITS,
        whats_new: DEFAULT_WHATS_NEW,
        sizes: DEFAULT_SIZES,
        colorways: DEFAULT_COLORWAYS,
        reviews: DEFAULT_REVIEWS,
        avg_rating: "4.5",
        review_count: "50",
        recommended: DEFAULT_RECOMMENDED,
    }
}

static DEFAULT_BENEFITS: &[Benefit<'static>] = &[
    Benefit {
        title: "Propulsive Air Zoom Units",
        description: "Two forefoot Air Zoom units deliver a burst of energy with every stride, converting pressure into a powerful forward spring that propels you toward the finish line.",
    },
    Benefit {
        title: "ZoomX Foam Cushioning",
        description: "Nike's lightest and most energy-returning foam delivers a soft, responsive ride that helps keep your legs feeling fresh through all 26.2 miles of the marathon.",
    },
    Benefit {
        title: "Full-Length Carbon Fiber Plate",
        description: "A full-length carbon fiber plate adds stiffness and snap to each toe-off, working with the Air Zoom units to create a smooth, propulsive transition from landing to takeoff.",
    },
];

static DEFAULT_WHATS_NEW: &[&str] = &[
    "Updated Atomknit 3.0 upper is lighter and more breathable",
    "Redesigned heel counter improves lockdown and reduces slippage",
    "Wider forefoot platform increases stability at speed",
    "Refined ZoomX foam geometry for smoother transitions",
    "New outsole rubber compound improves wet-surface traction",
    "Streamlined tongue construction reduces weight and eliminates bunching",
];

static DEFAULT_SIZES: &[Size<'static>] = &[
    Size {
        label: "M 4",
        available: false,
    },
    Size {
        label: "M 4.5",
        available: false,
    },
    Size {
        label: "M 5",
        available: false,
    },
    Size {
        label: "M 5.5",
        available: false,
    },
    Size {
        label: "M 6",
        available: false,
    },
    Size {
        label: "M 6.5",
        available: false,
    },
    Size {
        label: "M 7",
        available: true,
    },
    Size {
        label: "M 7.5",
        available: true,
    },
    Size {
        label: "M 8",
        available: true,
    },
    Size {
        label: "M 8.5",
        available: true,
    },
    Size {
        label: "M 9",
        available: true,
    },
    Size {
        label: "M 9.5",
        available: true,
    },
    Size {
        label: "M 10",
        available: true,
    },
    Size {
        label: "M 10.5",
        available: true,
    },
    Size {
        label: "M 11",
        available: true,
    },
    Size {
        label: "M 11.5",
        available: true,
    },
    Size {
        label: "M 12",
        available: true,
    },
    Size {
        label: "M 12.5",
        available: true,
    },
    Size {
        label: "M 13",
        available: true,
    },
    Size {
        label: "M 13.5",
        available: true,
    },
    Size {
        label: "M 14",
        available: true,
    },
    Size {
        label: "M 15",
        available: true,
    },
];

static DEFAULT_COLORWAYS: &[Colorway<'static>] = &[
    Colorway {
        style_code: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(1).png",
        selected: false,
    },
    Colorway {
        style_code: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(2).png",
        selected: false,
    },
    Colorway {
        style_code: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(3).png",
        selected: false,
    },
    Colorway {
        style_code: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3.png",
        selected: true,
    },
    Colorway {
        style_code: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(4).png",
        selected: false,
    },
    Colorway {
        style_code: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(5).png",
        selected: false,
    },
    Colorway {
        style_code: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3+EK.png",
        selected: false,
    },
];

static DEFAULT_REVIEWS: &[Review<'static>] = &[
    Review {
        author: "MarathonMike",
        date: "DATE",
        title: "Best marathon shoe I've ever worn",
        body: "PR'd by 4 minutes in these at the LA Marathon. The energy return is insane and they felt great from start to finish. Worth every penny.",
        rating: 5,
    },
    Review {
        author: "SpeedyRunner",
        date: "DATE",
        title: "Incredible speed but narrow fit",
        body: "The propulsion is unmatched. You genuinely feel faster in these shoes. However, they run narrow so I'd recommend going up half a size if you have wider feet.",
        rating: 4,
    },
    Review {
        author: "RunnerJane",
        date: "DATE",
        title: "Game changer for race day",
        body: "I've raced in Vaporflys for years but the Alphafly 3 is on another level. The double Air Zoom units make a real difference on long runs. Finished my first sub-3 marathon!",
        rating: 5,
    },
    Review {
        author: "CasualJogger42",
        date: "DATE",
        title: "Too much shoe for my needs",
        body: "These are clearly built for serious racers. For my 10-minute mile pace, I found them unstable. Great shoe but not for everyone.",
        rating: 3,
    },
    Review {
        author: "TrackCoachTom",
        date: "DATE",
        title: "Perfect for my athletes",
        body: "I've had three of my marathon runners switch to the Alphafly 3 and all three set PRs. The combination of ZoomX, Air Zoom, and the carbon plate is elite.",
        rating: 5,
    },
    Review {
        author: "UltraDistanceAce",
        date: "DATE",
        title: "Holds up great over distance",
        body: "Wore these for a 50K training run. They held up better than I expected, though they're really optimized for marathon distance. The cushioning never bottomed out.",
        rating: 4,
    },
    Review {
        author: "BostonQualifier",
        date: "DATE",
        title: "Qualified for Boston in these",
        body: "Finally broke the BQ barrier after two years of trying. These shoes gave me the edge I needed. The ride is smooth and the energy return is real.",
        rating: 5,
    },
    Review {
        author: "DataDrivenRunner",
        date: "DATE",
        title: "Measurably faster",
        body: "I tracked my splits across identical workouts in Vaporfly 4 vs Alphafly 3. Consistently 3-5 seconds per mile faster in the Alphafly. The data doesn't lie.",
        rating: 5,
    },
    Review {
        author: "WeekendWarrior",
        date: "DATE",
        title: "Expensive but delivers",
        body: "At nearly $300 these aren't cheap, but if you're serious about racing they deliver. I knocked 8 minutes off my half marathon time. The price reflects the technology.",
        rating: 4,
    },
    Review {
        author: "RecoveryFocused",
        date: "DATE",
        title: "Less leg fatigue post-race",
        body: "The biggest benefit for me was how my legs felt after the race. Way less soreness compared to my old racing flats. The ZoomX foam really cushions the impact.",
        rating: 4,
    },
];

static DEFAULT_RECOMMENDED: &[RecommendedProduct<'static>] = &[
    RecommendedProduct {
        title: "Nike Zoom Fly 6",
        subtitle: "Men's Road Running Shoes",
        price: "$190",
        image: "/images/nike/ZOOM+FLY+6+GLAM.png",
    },
    RecommendedProduct {
        title: "Nike Zoom Vomero 5",
        subtitle: "Men's Shoes",
        price: "$170",
        image: "/images/nike/NIKE+ZOOM+VOMERO+5.png",
    },
    RecommendedProduct {
        title: "Nike Pegasus Plus",
        subtitle: "Men's Road Running Shoes",
        price: "$180",
        image: "/images/nike/NIKE+PEGASUS+PREMIUM+CM.png",
    },
    RecommendedProduct {
        title: "Nike Alphafly 3 Glam",
        subtitle: "Men's Road Racing Shoes",
        price: "$295",
        image: "/images/nike/alphafly-3-glam-mens-road-racing-shoes-5gZBgL.png",
    },
    RecommendedProduct {
        title: "Nike Pegasus Premium",
        subtitle: "Men's Road Running Shoes",
        price: "$220",
        image: "/images/nike/NIKE+PEGASUS+PREMIUM+CM.png",
    },
    RecommendedProduct {
        title: "Nike Vaporfly 4",
        subtitle: "Men's Road Racing Shoes",
        price: "$270",
        image: "/images/nike/ZOOMX+VAPORFLY+NEXT%+4.png",
    },
    RecommendedProduct {
        title: "Nike Vomero 18",
        subtitle: "Men's Road Running Shoes",
        price: "$165",
        image: "/images/nike/NIKE+VOMERO+18+CM.png",
    },
    RecommendedProduct {
        title: "Nike Pegasus 41",
        subtitle: "Men's Road Running Shoes",
        price: "$155",
        image: "/images/nike/AIR+ZOOM+PEGASUS+41+CM.png",
    },
    RecommendedProduct {
        title: "Nike Structure Plus",
        subtitle: "Men's Road Running Shoes",
        price: "$170",
        image: "/images/nike/NIKE+STRUCTURE+PLUS.png",
    },
    RecommendedProduct {
        title: "Nike Kiger 10",
        subtitle: "Men's Trail Running Shoes",
        price: "$170",
        image: "/images/nike/NIKE+TERRA+KIGER+10.png",
    },
];
