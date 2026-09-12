// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Nike Men's Shoes catalog — 120 product cards for benchmarking.
#![allow(unused_must_use)]

use super::shared;
use proa_core::{RenderBuffer, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

// ─── Data types ──────────────────────────────────────────────────────

pub struct Product<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub price: &'a str,
    pub messaging: &'a str,
    /// Full image path, e.g. "/images/nike/AIR+JORDAN+14+RETRO.png"
    pub image: &'a str,
    /// Pre-formatted inline styles, e.g. "background-color:#26292B"
    pub colors: &'a [&'a str],
    /// Pre-formatted count text, e.g. "3 Colors"
    pub color_count: &'a str,
}

pub struct CatalogData<'a> {
    pub title: &'a str,
    pub products: &'a [Product<'a>],
}

// ─── Render ──────────────────────────────────────────────────────────

pub fn render() -> String {
    let data = default_data();
    let mut buf = RenderBuffer::new();
    render_into(&mut WebContext::with_buffer(&mut buf), &data);
    buf.into_string()
}

pub fn render_into<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &CatalogData<'_>,
) {
    shared::render_document_start(cx, "Men's Shoes & Sneakers. Nike.com", "Shop Men's shoes and sneakers at Nike.com. Browse the latest styles, filter by sport, price, and more.");
    shared::render_header(cx);
    render_content(cx, data);
    shared::render_footer(cx);
    shared::render_document_end(cx);
}

pub fn render_content<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &CatalogData<'_>,
) {
    let _ = html_sync! {
        <main>
            <div class="nike-page-container">
                <div class="nike-page-header">
                    <h1 class="nike-page-title">{data.title}</h1>
                </div>
                <div class="nike-results-bar">
                    <span class="nike-results-count">{data.products.len()}" Results"</span>
                    <div class="nike-sort">
                        <span class="nike-sort-label">"Sort By"</span>
                        <span class="nike-sort-value">"Featured"</span>
                    </div>
                </div>
                <div class="nike-layout">
                    {shared::render_filters(cx)}
                    <div class="nike-grid">
                        <div class="nike-grid__items">
                            {for (i, product) in data.products.iter().enumerate() {
                                render_product_card(cx, product, i);
                            }}
                        </div>
                    </div>
                </div>
            </div>
        </main>
    }
    .render(cx);
}

fn render_product_card<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    product: &Product<'_>,
    index: usize,
) {
    let above_fold = index < 6;
    let _ = html_sync! {
        <div class="nike-card">
            <a class="nike-card__link" href="/showcase/nike/proa/product">
                <div class="nike-card__img-wrap">
                    {if index == 0 {
                        let _ = html_sync! {
                            <img class="nike-card__img"
                                 src={product.image}
                                 alt={product.title}
                                 loading="eager"
                                 fetchpriority="high"
                                 decoding="async"
                                 width="592"
                                 height="592"/>
                        }.render(cx);
                    } else if above_fold {
                        let _ = html_sync! {
                            <img class="nike-card__img"
                                 src={product.image}
                                 alt={product.title}
                                 loading="eager"
                                 decoding="async"
                                 width="592"
                                 height="592"/>
                        }.render(cx);
                    } else {
                        let _ = html_sync! {
                            <img class="nike-card__img"
                                 src={product.image}
                                 alt={product.title}
                                 loading="lazy"
                                 decoding="async"
                                 width="592"
                                 height="592"/>
                        }.render(cx);
                    }}
                </div>
                <div class="nike-card__info">
                    {if !product.messaging.is_empty() {
                        let _ = html_sync! { <div class="nike-card__messaging">{product.messaging}</div> }.render(cx);
                    }}
                    <div class="nike-card__title">{product.title}</div>
                    <div class="nike-card__subtitle">{product.subtitle}</div>
                    {if !product.colors.is_empty() {
                        let _ = html_sync! {
                            <div class="nike-card__swatches">
                                {for style in product.colors.iter() {
                                    let _ = html_sync! {
                                        <div class="nike-swatch" style={proa_core::style_ok(*style)}></div>
                                    }.render(cx);
                                }}
                            </div>
                            <div class="nike-card__color-count">{product.color_count}</div>
                        }.render(cx);
                    }}
                    {if !product.price.is_empty() {
                        let _ = html_sync! { <div class="nike-card__price">{product.price}</div> }.render(cx);
                    }}
                </div>
            </a>
        </div>
    }
    .render(cx);
}

// ─── Default data ────────────────────────────────────────────────────

pub fn default_data() -> CatalogData<'static> {
    CatalogData {
        title: "Men's Shoes & Sneakers",
        products: DEFAULT_PRODUCTS,
    }
}

static DEFAULT_PRODUCTS: &[Product<'static>] = &[
    Product {
        title: "Air Jordan 14 Retro",
        subtitle: "Men's Shoes",
        price: "$205",
        messaging: "Coming Soon",
        image: "/images/nike/AIR+JORDAN+14+RETRO.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Mind 002",
        subtitle: "Men's Shoes",
        price: "$145",
        messaging: "Just In",
        image: "/images/nike/NIKE+MIND+002.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#808080",
            "background-color:#6C5627",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike Pegasus Premium",
        subtitle: "Men's Road Running Shoes",
        price: "$220",
        messaging: "Best Seller",
        image: "/images/nike/NIKE+PEGASUS+PREMIUM+CM.png",
        colors: &[
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Vomero Plus",
        subtitle: "Men's Road Running Shoes",
        price: "$190",
        messaging: "",
        image: "/images/nike/NIKE+VOMERO+PLUS.png",
        colors: &[
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#1F4090",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Ava X",
        subtitle: "Men's Shoes",
        price: "$125",
        messaging: "",
        image: "/images/nike/NIKE+AVA+X.png",
        colors: &["background-color:#EFBF4D", "background-color:#808080"],
        color_count: "2 Colors",
    },
    Product {
        title: "Nike Air Max Plus VII",
        subtitle: "Men's Shoes",
        price: "$190",
        messaging: "",
        image: "/images/nike/NIKE+AIR+MAX+PLUS+VII.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#E1533D",
            "background-color:#EFBF4D",
            "background-color:#B40033",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Air Max Dn8",
        subtitle: "Men's Shoes",
        price: "$220",
        messaging: "",
        image: "/images/nike/AIR+MAX+DN8+NRG.png",
        colors: &[
            "background-color:#FF00FF",
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#1F4090",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Air Max Dn8 Leather",
        subtitle: "Men's Shoes",
        price: "$210",
        messaging: "",
        image: "/images/nike/AIR+MAX+DN8+LTHR.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#26292B",
            "background-color:#808080",
            "background-color:#6C5627",
        ],
        color_count: "4 Colors",
    },
    Product {
        title: "Nike Mercurial Vapor 16 Elite",
        subtitle: "Firm-Ground Low-Top Soccer Cleats",
        price: "$280",
        messaging: "",
        image: "/images/nike/ZM+VAPOR+16+ELITE+KM+FG.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#FF00FF",
            "background-color:#007A44",
            "background-color:#26292B",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Mercurial Superfly 10 Elite",
        subtitle: "FG High-Top Soccer Cleats",
        price: "$305",
        messaging: "",
        image: "/images/nike/ZM+SUPERFLY+10+ELITE+FG+KM.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#FF00FF",
            "background-color:#26292B",
            "background-color:#007A44",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Book 2 \"Spiridon\"",
        subtitle: "Basketball Shoes",
        price: "$145",
        messaging: "Best Seller",
        image: "/images/nike/BOOK+2+SP.png",
        colors: &[
            "background-color:#808080",
            "background-color:#FFFFFF",
            "background-color:#26292B",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike Air Max Uptempo '95",
        subtitle: "Men's Shoes",
        price: "$170",
        messaging: "Just In",
        image: "/images/nike/AIR+MAX+UPTEMPO+'95.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Giannis Freak 7",
        subtitle: "Basketball Shoes",
        price: "$115",
        messaging: "",
        image: "/images/nike/GIANNIS+FREAK+7.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#B40033",
            "background-color:#EEF3FF",
            "background-color:#472B7E",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Sabrina 3 \"Oregon\"",
        subtitle: "Basketball Shoes",
        price: "$135",
        messaging: "Just In",
        image: "/images/nike/SABRINA+3+OR.png",
        colors: &[
            "background-color:#EFBF4D",
            "background-color:#EEF3FF",
            "background-color:#007A44",
            "background-color:#E1533D",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Air Jordan Skyline Low",
        subtitle: "Men's Shoes",
        price: "$100",
        messaging: "Best Seller",
        image: "/images/nike/AIR+JORDAN+SKYLINE+LOW.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
        ],
        color_count: "4 Colors",
    },
    Product {
        title: "Jordan Trunner O/S",
        subtitle: "Men's Shoes",
        price: "$115",
        messaging: "Best Seller",
        image: "/images/nike/JORDAN+TRUNNER+O_S.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#6C5627",
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#B40033",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Air Max Plus",
        subtitle: "Men's Shoes",
        price: "$190",
        messaging: "",
        image: "/images/nike/NIKE+AIR+MAX+PLUS.png",
        colors: &[
            "background-color:#EEF3FF",
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#26292B",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Air Force 1 Low Protro",
        subtitle: "Men's Shoes",
        price: "$150",
        messaging: "",
        image: "/images/nike/AIR+FORCE+1+LOW+PROTRO.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike G.T. Cut 4 CNY",
        subtitle: "Basketball Shoes",
        price: "$220",
        messaging: "",
        image: "/images/nike/G.T.+CUT+4+CNY.png",
        colors: &[
            "background-color:#EEF3FF",
            "background-color:#26292B",
            "background-color:#B40033",
            "background-color:#EFBF4D",
            "background-color:#1F4090",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "LeBron XXIII",
        subtitle: "Basketball Shoes",
        price: "$210",
        messaging: "Best Seller",
        image: "/images/nike/LEBRON+XXIII.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#EEF3FF",
            "background-color:#26292B",
            "background-color:#B40033",
            "background-color:#1F4090",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Air Max Moto 2K",
        subtitle: "Men's Shoes",
        price: "$145",
        messaging: "",
        image: "/images/nike/AIR+MAX+MOTO+2K.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#808080",
            "background-color:#808080",
            "background-color:#808080",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Air Max 95 Big Bubble Leather",
        subtitle: "Men's Shoes",
        price: "$210",
        messaging: "",
        image: "/images/nike/AIR+MAX+95+BIG+BBL+LTR+QS.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Kiger 10",
        subtitle: "Trail Running Shoes",
        price: "$170",
        messaging: "",
        image: "/images/nike/NIKE+TERRA+KIGER+10.png",
        colors: &[
            "background-color:#FF00FF",
            "background-color:#1F4090",
            "background-color:#808080",
            "background-color:#007A44",
            "background-color:#6C5627",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike ACG Ultrafly Trail",
        subtitle: "Trail Racing Shoes",
        price: "$260",
        messaging: "",
        image: "/images/nike/NIKE+ACG+ULTRAFLY+TRAIL.png",
        colors: &[
            "background-color:#EFBF4D",
            "background-color:#E1533D",
            "background-color:#007A44",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Air Jordan 4 RM",
        subtitle: "Men's Shoes",
        price: "$155",
        messaging: "",
        image: "/images/nike/AIR+JORDAN+4+RM.png",
        colors: &[
            "background-color:#808080",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#B40033",
            "background-color:#6C5627",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Air Jordan 1 Low SE",
        subtitle: "Men's Shoes",
        price: "$130",
        messaging: "Best Seller",
        image: "/images/nike/AIR+JORDAN+1+LOW+SE.png",
        colors: &[
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Zoom Fly 6 Glam",
        subtitle: "Men's Road Running Shoes",
        price: "$190",
        messaging: "",
        image: "/images/nike/ZOOM+FLY+6+GLAM.png",
        colors: &[
            "background-color:#1F4090",
            "background-color:#1F4090",
            "background-color:#26292B",
            "background-color:#FF00FF",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Structure Plus",
        subtitle: "Men's Road Running Shoes",
        price: "$170",
        messaging: "",
        image: "/images/nike/NIKE+STRUCTURE+PLUS.png",
        colors: &[
            "background-color:#472B7E",
            "background-color:#EEF3FF",
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Air Jordan 40",
        subtitle: "Basketball Shoes",
        price: "$205",
        messaging: "",
        image: "/images/nike/AIR+JORDAN+40+SL.png",
        colors: &[
            "background-color:#E1533D",
            "background-color:#808080",
            "background-color:#007A44",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Luka 5",
        subtitle: "Basketball Shoes",
        price: "$135",
        messaging: "Best Seller",
        image: "/images/nike/JORDAN+LUKA+5.png",
        colors: &["background-color:#26292B", "background-color:#007A44"],
        color_count: "2 Colors",
    },
    Product {
        title: "Nike Vapor Edge 360",
        subtitle: "Football Cleats",
        price: "$190",
        messaging: "",
        image: "/images/nike/VAPOR+EDGE+360+UT.png",
        colors: &[
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#26292B",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike Vapor Pro 1",
        subtitle: "Football Cleats",
        price: "$145",
        messaging: "",
        image: "/images/nike/NIKE+ZOOM+AIR+VAPOR+PRO+1.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Vaporfly 4",
        subtitle: "Men's Road Racing Shoes",
        price: "$270",
        messaging: "",
        image: "/images/nike/ZOOMX+VAPORFLY+NEXT%+4.png",
        colors: &[
            "background-color:#EFBF4D",
            "background-color:#EEF3FF",
            "background-color:#E1533D",
            "background-color:#FF00FF",
            "background-color:#1F4090",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Alphafly 3",
        subtitle: "Men's Road Racing Shoes",
        price: "$295",
        messaging: "",
        image: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3.png",
        colors: &[
            "background-color:#E1533D",
            "background-color:#EEF3FF",
            "background-color:#1F4090",
            "background-color:#E1533D",
            "background-color:#FFFFFF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike T90 SP",
        subtitle: "Men's Shoes",
        price: "$115",
        messaging: "",
        image: "/images/nike/T90+SP.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Air Force 1 '07 LV8",
        subtitle: "Men's Shoes",
        price: "$125",
        messaging: "",
        image: "/images/nike/AIR+FORCE+1+'07+LV8.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#808080",
            "background-color:#B40033",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike P-6000",
        subtitle: "Men's Shoes",
        price: "$115",
        messaging: "",
        image: "/images/nike/NIKE+P-6000.png",
        colors: &[
            "background-color:#1F4090",
            "background-color:#007A44",
            "background-color:#808080",
            "background-color:#808080",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Zoom Vomero 5",
        subtitle: "Men's Shoes",
        price: "$170",
        messaging: "",
        image: "/images/nike/NIKE+ZOOM+VOMERO+5.png",
        colors: &[
            "background-color:#1F4090",
            "background-color:#808080",
            "background-color:#808080",
            "background-color:#808080",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Tatum 4",
        subtitle: "Basketball Shoes",
        price: "$130",
        messaging: "",
        image: "/images/nike/JORDAN+TATUM+4+LEA.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#EFBF4D",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Jordan Heir Series 2",
        subtitle: "Basketball Shoes",
        price: "$115",
        messaging: "",
        image: "/images/nike/WMNS+JORDAN+HEIR+SERIES+2+DUR.png",
        colors: &["background-color:#26292B", "background-color:#EEF3FF"],
        color_count: "2 Colors",
    },
    Product {
        title: "Nike Vomero 18",
        subtitle: "Men's Road Running Shoes",
        price: "$165",
        messaging: "",
        image: "/images/nike/NIKE+VOMERO+18+CM.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#808080",
            "background-color:#EEF3FF",
            "background-color:#6C5627",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Pegasus 41",
        subtitle: "Men's Road Running Shoes",
        price: "$155",
        messaging: "",
        image: "/images/nike/AIR+ZOOM+PEGASUS+41+CM.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#6C5627",
            "background-color:#472B7E",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike ReactX Rejuven8",
        subtitle: "Men's Shoes",
        price: "$75",
        messaging: "Best Seller",
        image: "/images/nike/NIKE+REACTX+REJUVEN8.png",
        colors: &[
            "background-color:#EFBF4D",
            "background-color:#1F4090",
            "background-color:#6C5627",
            "background-color:#007A44",
            "background-color:#1F4090",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Metcon 10",
        subtitle: "Men's Workout Shoes",
        price: "$155",
        messaging: "Best Seller",
        image: "/images/nike/M+NIKE+METCON+10.png",
        colors: &[
            "background-color:#007A44",
            "background-color:#EEF3FF",
            "background-color:#26292B",
            "background-color:#B40033",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Free Metcon 6",
        subtitle: "Men's Workout Shoes",
        price: "$125",
        messaging: "Best Seller",
        image: "/images/nike/NIKE+FREE+METCON+6.png",
        colors: &[
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#007A44",
            "background-color:#007A44",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Gato",
        subtitle: "Men's Shoes",
        price: "$115",
        messaging: "",
        image: "/images/nike/NIKE+GATO.png",
        colors: &[
            "background-color:#808080",
            "background-color:#007A44",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#6C5627",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Dunk Low SE",
        subtitle: "Men's Shoes",
        price: "$130",
        messaging: "Best Seller",
        image: "/images/nike/NIKE+DUNK+LOW.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#808080",
            "background-color:#808080",
            "background-color:#007A44",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike LD-1000 SE",
        subtitle: "Men's Shoes",
        price: "$115",
        messaging: "",
        image: "/images/nike/LD-1000.png",
        colors: &[
            "background-color:#B40033",
            "background-color:#26292B",
            "background-color:#007A44",
            "background-color:#6C5627",
            "background-color:#007A44",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Shox R4",
        subtitle: "Men's Shoes",
        price: "$155",
        messaging: "",
        image: "/images/nike/NIKE+SHOX+R4.png",
        colors: &[
            "background-color:#808080",
            "background-color:#808080",
            "background-color:#EEF3FF",
            "background-color:#808080",
            "background-color:#6C5627",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Shox NZ",
        subtitle: "Men's Shoes",
        price: "$135",
        messaging: "",
        image: "/images/nike/NIKE+SHOX+NZ.png",
        colors: &[
            "background-color:#EEF3FF",
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#1F4090",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Pegasus Trail 5",
        subtitle: "Men's Trail Running Shoes",
        price: "$155",
        messaging: "",
        image: "/images/nike/NIKE+REACTX+PEGASUS+TRAIL+5.png",
        colors: &[
            "background-color:#1F4090",
            "background-color:#26292B",
            "background-color:#FF00FF",
            "background-color:#26292B",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Pegasus Trail 5 GORE-TEX",
        subtitle: "Men's Waterproof Trail Running Shoes",
        price: "$180",
        messaging: "",
        image: "/images/nike/NIKE+PEGASUS+TRAIL+5+GTX.png",
        colors: &[
            "background-color:#1F4090",
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#6C5627",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Air Jordan 1 Low G",
        subtitle: "Golf Shoes",
        price: "$155",
        messaging: "Best Seller",
        image: "/images/nike/AIR+JORDAN+1+LOW+G.png",
        colors: &[
            "background-color:#808080",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Air Jordan 1 Low G Spiked",
        subtitle: "Golf Shoes",
        price: "$175",
        messaging: "Best Seller",
        image: "/images/nike/AIR+JORDAN+1+LOW+G+SPK.png",
        colors: &["background-color:#26292B", "background-color:#808080"],
        color_count: "2 Colors",
    },
    Product {
        title: "Nike Phantom 6 Low Elite",
        subtitle: "Firm-Ground Soccer Cleats",
        price: "$270",
        messaging: "",
        image: "/images/nike/PHANTOM+6+LOW+ELITE+FG.png",
        colors: &[
            "background-color:#1F4090",
            "background-color:#1F4090",
            "background-color:#E1533D",
            "background-color:#E1533D",
            "background-color:#007A44",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Phantom 6 High Elite",
        subtitle: "Firm-Ground Soccer Cleats",
        price: "$295",
        messaging: "",
        image: "/images/nike/PHANTOM+6+HIGH+ELITE+FG.png",
        colors: &[
            "background-color:#1F4090",
            "background-color:#007A44",
            "background-color:#1F4090",
            "background-color:#6C5627",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Air VaporMax Plus",
        subtitle: "Men's Shoes",
        price: "$220",
        messaging: "",
        image: "/images/nike/AIR+VAPORMAX+PLUS.png",
        colors: &[
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#1F4090",
            "background-color:#26292B",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Shox TL",
        subtitle: "Men's Shoes",
        price: "$180",
        messaging: "",
        image: "/images/nike/NIKE+SHOX+TL.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#808080",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike C1TY Premium",
        subtitle: "Men's Shoes",
        price: "$115",
        messaging: "",
        image: "/images/nike/NIKE+C1TY+PRM.png",
        colors: &[
            "background-color:#472B7E",
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#6C5627",
            "background-color:#6C5627",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Air Force 1 '07 Premium",
        subtitle: "Men's Shoes",
        price: "$130",
        messaging: "",
        image: "/images/nike/AIR+FORCE+1+'07+PRM.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#6C5627",
            "background-color:#6C5627",
            "background-color:#007A44",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Waffle Racer",
        subtitle: "Men's Shoes",
        price: "$95",
        messaging: "",
        image: "/images/nike/WAFFLE+RACER.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#6C5627",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Jordan Flight Court",
        subtitle: "Men's Shoes",
        price: "$105",
        messaging: "",
        image: "/images/nike/JORDAN+FLIGHT+COURT.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#007A44",
            "background-color:#EEF3FF",
            "background-color:#1F4090",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Air Jordan MVP 92",
        subtitle: "Men's Shoes",
        price: "$145",
        messaging: "Best Seller",
        image: "/images/nike/JORDAN+MVP+92.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#B40033",
            "background-color:#808080",
            "background-color:#007A44",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Air Jordan 4 Retro",
        subtitle: "Men's Shoes",
        price: "$220",
        messaging: "",
        image: "/images/nike/AIR+JORDAN+4+RETRO+OG+FC.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Air Jordan 1 Mid",
        subtitle: "Men's Shoes",
        price: "$130",
        messaging: "",
        image: "/images/nike/AIR+JORDAN+1+MID.png",
        colors: &[
            "background-color:#EEF3FF",
            "background-color:#808080",
            "background-color:#1F4090",
            "background-color:#26292B",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Diamond Standout MTL",
        subtitle: "Baseball Cleats",
        price: "$140",
        messaging: "",
        image: "/images/nike/NIKE+DIAMOND+STANDOUT+MTL.png",
        colors: &[
            "background-color:#808080",
            "background-color:#EEF3FF",
            "background-color:#26292B",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike Diamond Showcase MCS",
        subtitle: "Baseball Cleats",
        price: "$110",
        messaging: "",
        image: "/images/nike/NIKE+DIAMOND+SHOWCASE+MCS.png",
        colors: &[
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#FF00FF",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike ACG Phassad",
        subtitle: "Men's Shoes",
        price: "$145",
        messaging: "",
        image: "/images/nike/ACG+PHASSAD.png",
        colors: &[
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#472B7E",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike Air Max Dn Roam",
        subtitle: "Men's Shoes",
        price: "$190",
        messaging: "",
        image: "/images/nike/AIR+MAX+DN+ROAM.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#808080",
            "background-color:#1F4090",
            "background-color:#EEF3FF",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Zoom Vomero Roam",
        subtitle: "Men's Winterized Shoes",
        price: "$190",
        messaging: "",
        image: "/images/nike/NIKE+ZOOM+VOMERO+ROAM.png",
        colors: &[
            "background-color:#007A44",
            "background-color:#808080",
            "background-color:#6C5627",
            "background-color:#26292B",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Mercurial Vapor 16 Pro",
        subtitle: "Artificial-Grass Soccer Cleats",
        price: "$160",
        messaging: "",
        image: "/images/nike/ZM+VAPOR+16+PRO+AG-PRO.png",
        colors: &["background-color:#1F4090", "background-color:#26292B"],
        color_count: "2 Colors",
    },
    Product {
        title: "Nike Mercurial Superfly 10 Elite",
        subtitle: "AG High-Top Soccer Cleats",
        price: "$295",
        messaging: "",
        image: "/images/nike/ZM+SUPERFLY+10+ELITE+AG-PRO.png",
        colors: &[
            "background-color:#1F4090",
            "background-color:#007A44",
            "background-color:#26292B",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike Dunk Low GORE-TEX",
        subtitle: "Men's Shoes",
        price: "$155",
        messaging: "",
        image: "/images/nike/NIKE+DUNK+LOW+GTX.png",
        colors: &[
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#6C5627",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike Calm Mule",
        subtitle: "Men's Mules",
        price: "$75",
        messaging: "",
        image: "/images/nike/NIKE+CALM+MULE+-+REALTREE+W+NA.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#007A44",
            "background-color:#EEF3FF",
            "background-color:#1F4090",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Calm Slide",
        subtitle: "Men's Slides",
        price: "$65",
        messaging: "",
        image: "/images/nike/NIKE+CALM+SLIDE+-+REALTREE.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Air Force 1 '07",
        subtitle: "Men's Shoes",
        price: "$125",
        messaging: "",
        image: "/images/nike/AIR+FORCE+1+'07.png",
        colors: &[
            "background-color:#FFFFFF",
            "background-color:#6C5627",
            "background-color:#26292B",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike Lunar Force 1 Duckboot",
        subtitle: "Men's Winterized Shoes",
        price: "$170",
        messaging: "",
        image: "/images/nike/LUNAR+FORCE+1+DUCKBOOT.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Air Force 1 '07",
        subtitle: "Men's Shoes",
        price: "$115",
        messaging: "Best Seller",
        image: "/images/nike/AIR+FORCE+1+'07(1).png",
        colors: &[
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#26292B",
            "background-color:#26292B",
        ],
        color_count: "4 Colors",
    },
    Product {
        title: "Nike Dunk High Retro",
        subtitle: "Men's Shoes",
        price: "$155",
        messaging: "",
        image: "/images/nike/NIKE+DUNK+HI+RETRO.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Gato LV8",
        subtitle: "Men's Shoes",
        price: "$115",
        messaging: "",
        image: "/images/nike/NIKE+GATO+LV8.png",
        colors: &[
            "background-color:#808080",
            "background-color:#B40033",
            "background-color:#6C5627",
            "background-color:#FF00FF",
            "background-color:#808080",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Killshot 2 PRM",
        subtitle: "Men's Shoes",
        price: "$105",
        messaging: "",
        image: "/images/nike/KILLSHOT+2+PRM.png",
        colors: &["background-color:#808080", "background-color:#26292B"],
        color_count: "2 Colors",
    },
    Product {
        title: "KD18",
        subtitle: "Basketball Shoes",
        price: "$155",
        messaging: "",
        image: "/images/nike/KD18+GI.png",
        colors: &[
            "background-color:#007A44",
            "background-color:#EEF3FF",
            "background-color:#472B7E",
            "background-color:#6C5627",
            "background-color:#1F4090",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Air Jordan 1 Retro High OG",
        subtitle: "Men's Shoes",
        price: "$185",
        messaging: "",
        image: "/images/nike/AIR+JORDAN+1+RETRO+HIGH+OG.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Calm 2.0",
        subtitle: "Men's Slides",
        price: "$55",
        messaging: "Best Seller",
        image: "/images/nike/NIKE+CALM+SLIDE+2.0.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#1F4090",
            "background-color:#6C5627",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike V5 RNR",
        subtitle: "Men's Shoes",
        price: "$95",
        messaging: "Best Seller",
        image: "/images/nike/NIKE+V5+RNR.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#808080",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Air Max 270",
        subtitle: "Men's Shoes",
        price: "$170",
        messaging: "Best Seller",
        image: "/images/nike/AIR+MAX+270.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#26292B",
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Ava Rover",
        subtitle: "Shoes with Reflective Accents",
        price: "$145",
        messaging: "",
        image: "/images/nike/NIKE+AVA+ROVER.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#6C5627",
            "background-color:#26292B",
            "background-color:#007A44",
            "background-color:#1F4090",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Mind 001",
        subtitle: "Men's Pregame Mules",
        price: "$95",
        messaging: "Just In",
        image: "/images/nike/NIKE+MIND+001.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike x Hyperice Hyperboot",
        subtitle: "Shoes",
        price: "$799",
        messaging: "",
        image: "/images/nike/Hyperboot+by+Nike+x+Hyperice.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Air Diamond Turf",
        subtitle: "Men's Shoes",
        price: "$155",
        messaging: "",
        image: "/images/nike/NIKE+AIR+DIAMOND+TURF.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Air Force 1 '07 EasyOn",
        subtitle: "Shoes",
        price: "$115",
        messaging: "",
        image: "/images/nike/AIR+FORCE+1+'07+FLYEASE.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Air Max Uptempo '95",
        subtitle: "Men's Shoes",
        price: "$170",
        messaging: "",
        image: "/images/nike/AIR+MAX+UPTEMPO+'95(1).png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike SB Air Max 95",
        subtitle: "Skate Shoes",
        price: "$185",
        messaging: "",
        image: "/images/nike/SB+AIR+MAX+95.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Killshot 2 Leather",
        subtitle: "Men's Shoes",
        price: "$100",
        messaging: "Best Seller",
        image: "/images/nike/KILLSHOT+2+LEATHER.png",
        colors: &[
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#808080",
            "background-color:#26292B",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike Structure 26",
        subtitle: "Men's Road Running Shoes",
        price: "$145",
        messaging: "",
        image: "/images/nike/NIKE+STRUCTURE+26+WIDE.png",
        colors: &["background-color:#26292B", "background-color:#EEF3FF"],
        color_count: "2 Colors",
    },
    Product {
        title: "Air Jordan 1 Mid SE Edge",
        subtitle: "Women's Shoes",
        price: "$140",
        messaging: "Best Seller",
        image: "/images/nike/WMNS+AIR+JORDAN+1+MID+SE+EDG.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Jordan Spizike Low SE",
        subtitle: "Men's Shoes",
        price: "$165",
        messaging: "",
        image: "/images/nike/JORDAN+SPIZIKE+LOW+SE.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Air Zoom Spiridon SP",
        subtitle: "Men's Shoes",
        price: "$160",
        messaging: "",
        image: "/images/nike/AIR+ZOOM+SPIRIDON+SP.png",
        colors: &[
            "background-color:#808080",
            "background-color:#1F4090",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
        ],
        color_count: "4 Colors",
    },
    Product {
        title: "LeBron Witness 9",
        subtitle: "Basketball Shoes",
        price: "$105",
        messaging: "",
        image: "/images/nike/LEBRON+WITNESS+IX.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#1F4090",
            "background-color:#EEF3FF",
            "background-color:#FF00FF",
            "background-color:#EEF3FF",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Jordan Trunner LX",
        subtitle: "Women's Shoes",
        price: "$135",
        messaging: "Best Seller",
        image: "/images/nike/WMNS+JORDAN+TRUNNER+LX.png",
        colors: &[
            "background-color:#007A44",
            "background-color:#FF00FF",
            "background-color:#FFFFFF",
            "background-color:#808080",
            "background-color:#26292B",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Air Jordan 1 Mid SE",
        subtitle: "Women's Shoes",
        price: "$140",
        messaging: "",
        image: "/images/nike/WMNS+AIR+JORDAN+1+MID+SE.png",
        colors: &[
            "background-color:#6C5627",
            "background-color:#26292B",
            "background-color:#EEF3FF",
            "background-color:#EEF3FF",
            "background-color:#007A44",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Jordan 11 Mid TD",
        subtitle: "Men's Football Cleats",
        price: "$225",
        messaging: "Best Seller",
        image: "/images/nike/JORDAN+11+MID+TD.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Tawa SE",
        subtitle: "Men's Wrestling Shoes",
        price: "$155",
        messaging: "",
        image: "/images/nike/NIKE+TAWA+SE.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Jordan 1 Retro Low Metal",
        subtitle: "Baseball Cleats",
        price: "$145",
        messaging: "Best Seller",
        image: "/images/nike/JORDAN+1+RETRO+LOW+METAL.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Air Max Torch 4",
        subtitle: "Men's Shoes",
        price: "$100",
        messaging: "",
        image: "/images/nike/AIR+MAX+TORCH+4.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#808080",
            "background-color:#EEF3FF",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike Pegasus 41 Wide",
        subtitle: "Men's Road Running Shoes",
        price: "$145",
        messaging: "",
        image: "/images/nike/AIR+ZOOM+PEGASUS+41+WIDE.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Zoom Mamba 5 BTC",
        subtitle: "Running Shoes",
        price: "$125",
        messaging: "",
        image: "/images/nike/NIKE+ZOOM+MAMBA+V+BTC.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Flex Experience Run 12",
        subtitle: "Men's Road Running Shoes",
        price: "$80",
        messaging: "",
        image: "/images/nike/FLEX+EXPERIENCE+RN+12+WIDE.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Tennessee Pegasus 41",
        subtitle: "Men's College Road Running Shoes",
        price: "$155",
        messaging: "",
        image: "/images/nike/NIKE+ZOOM+PEGASUS+41+TENNESSEE.png",
        colors: &[
            "background-color:#E1533D",
            "background-color:#B40033",
            "background-color:#007A44",
            "background-color:#B40033",
            "background-color:#007A44",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike SB Malor TE",
        subtitle: "Skate Shoes",
        price: "$80",
        messaging: "",
        image: "/images/nike/NIKE+SB+MALOR+TE.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Phantom 6 High Academy",
        subtitle: "Turf Soccer Shoes",
        price: "$100",
        messaging: "",
        image: "/images/nike/PHANTOM+6+HIGH+ACAD+TF.png",
        colors: &[
            "background-color:#E1533D",
            "background-color:#26292B",
            "background-color:#1F4090",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike Field General",
        subtitle: "Men's Shoes",
        price: "$105",
        messaging: "",
        image: "/images/nike/NIKE+FIELD+GENERAL.png",
        colors: &[
            "background-color:#26292B",
            "background-color:#808080",
            "background-color:#808080",
            "background-color:#6C5627",
            "background-color:#6C5627",
        ],
        color_count: "5 Colors",
    },
    Product {
        title: "Nike SFB Air Field Ukiah 8\"",
        subtitle: "Tactical Boots",
        price: "$185",
        messaging: "",
        image: "/images/nike/NIKE+SFB+AIR+FIELD+UKIAH+8_.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Dragonfly 2",
        subtitle: "Track & Field Distance Spikes",
        price: "$170",
        messaging: "",
        image: "/images/nike/ZOOMX+DRAGONFLY+2.png",
        colors: &[
            "background-color:#EEF3FF",
            "background-color:#EFBF4D",
            "background-color:#472B7E",
        ],
        color_count: "3 Colors",
    },
    Product {
        title: "Nike College Air Max Verse (UNC)",
        subtitle: "Men's Shoes",
        price: "$125",
        messaging: "",
        image: "/images/nike/AIR+MAX+VERSE+NORTH+CAROLINA.png",
        colors: &[],
        color_count: "",
    },
    Product {
        title: "Nike Phantom 6 High Academy",
        subtitle: "Multi-Ground Soccer Cleats",
        price: "$100",
        messaging: "",
        image: "/images/nike/PHANTOM+6+HIGH+ACAD+FG_MG.png",
        colors: &[
            "background-color:#E1533D",
            "background-color:#1F4090",
            "background-color:#26292B",
            "background-color:#1F4090",
        ],
        color_count: "4 Colors",
    },
    Product {
        title: "Nike College Air Max Verse (Tennessee)",
        subtitle: "Men's Shoes",
        price: "$125",
        messaging: "",
        image: "/images/nike/AIR+MAX+VERSE+TENNESSEE.png",
        colors: &[],
        color_count: "",
    },
];
