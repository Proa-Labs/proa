// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Small standalone documents that docs pages embed with `<Demo />`.
//!
//! Docs articles are inert HTML by design, so a demo that must actually run is
//! served as its own page and framed. Each demo is a full document: it links the
//! site stylesheet, restores the saved theme, and emits its own rsjs runtime
//! tags. The framing page therefore needs no client code of its own.
//!
//! If a deployment ever adds a framing policy, these routes need
//! `frame-ancestors 'self'` (or `X-Frame-Options: SAMEORIGIN`) or the embedded
//! demo renders as a blank box.

use axum::{extract::Form, response::Html};
use proa_core::{
    raw_text_ok, DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;
use serde::Deserialize;

use crate::components::demos::quantity_stepper::QuantityStepper;
use crate::layout::{render_chained_html, RsjsRuntimeTags, STYLESHEET_HREF, THEME_BOOT};

/// Price of the tutorial's example product, in whole dollars.
const ATLAS_JACKET_USD: i64 = 248;

/// Gallery shots, as `(src, alt)`. The tutorial's product is a jacket and every
/// photographic fixture in this repository is footwear, so these are original
/// geometric illustrations in the same house style as
/// `images/synthetic/product-shoe.svg`.
const ATLAS_JACKET_SHOTS: &[(&str, &str)] = &[
    (
        "/static/images/synthetic/atlas-jacket-front.svg",
        "Atlas Jacket, front",
    ),
    (
        "/static/images/synthetic/atlas-jacket-back.svg",
        "Atlas Jacket, back",
    ),
    (
        "/static/images/synthetic/atlas-jacket-hood.svg",
        "Atlas Jacket, hood raised",
    ),
    (
        "/static/images/synthetic/atlas-jacket-cuff.svg",
        "Atlas Jacket, zip and cuff detail",
    ),
];

pub async fn quantity_stepper_handler() -> Html<String> {
    quantity_stepper_page()
}

fn quantity_stepper_page() -> Html<String> {
    render_chained_html(proa_core::to_async(DemoDocument {
        title: "Quantity stepper demo",
        content: QuantityStepperDemo,
    }))
}

#[derive(Deserialize)]
pub struct AddToCart {
    sku: String,
    qty: u32,
}

/// Echo what the form actually posted.
///
/// This is the point of the demo: the number the server receives comes from the
/// hidden input the island keeps up to date, and it is still posted when the
/// island never hydrates.
pub async fn quantity_stepper_cart_handler(Form(form): Form<AddToCart>) -> Html<String> {
    quantity_stepper_cart_page(form)
}

fn quantity_stepper_cart_page(form: AddToCart) -> Html<String> {
    render_chained_html(proa_core::to_async(DemoDocument {
        title: "Quantity stepper demo",
        content: CartReceipt {
            sku: form.sku,
            qty: form.qty,
            subtotal: i64::from(form.qty) * ATLAS_JACKET_USD,
        },
    }))
}

struct DemoDocument<C> {
    title: &'static str,
    content: C,
}

impl<L, B, C> WebRenderSync<L, B> for DemoDocument<C>
where
    B: WriteBuf,
    L: DataLoader,
    C: WebRenderSync<L, B>,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <meta name="robots" content="noindex" />
                    <title>{raw_text_ok(self.title)}</title>
                    <link rel="stylesheet" href={STYLESHEET_HREF} />
                    <script>{StaticRaw(THEME_BOOT)}</script>
                </head>
                <body class="demo-frame-body">
                    <main class="demo-frame-main">{self.content}</main>
                    {RsjsRuntimeTags}
                </body>
            </html>
        }
        .render(cx)
    }
}

/// The product gallery: a slide per shot, switched by the thumbnail rail.
///
/// The radios are the same inert pattern the docs containers use. Nothing here
/// is an island on purpose: the tutorial's claim is that exactly one component
/// on the page needs JavaScript, and a carousel that quietly needed a second
/// one would undercut it.
struct Gallery;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Gallery {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="demo-gallery">
                {for (index, (_, alt)) in ATLAS_JACKET_SHOTS.iter().enumerate() {
                    html_sync! {
                        <input
                            class="demo-gallery-radio"
                            type="radio"
                            name="atlas-jacket-shot"
                            id={gallery_input_id(index)}
                            checked={index == 0}
                            aria-label={*alt}
                        />
                    }
                }}
                <div class="demo-gallery-viewport">
                    <div class="demo-gallery-track">
                        {for (index, (src, alt)) in ATLAS_JACKET_SHOTS.iter().enumerate() {
                            html_sync! {
                                <img
                                    class="demo-gallery-shot"
                                    src={*src}
                                    alt={*alt}
                                    width="600"
                                    height="600"
                                    loading={if index == 0 { "eager" } else { "lazy" }}
                                    decoding="async"
                                />
                            }
                        }}
                    </div>
                </div>
                <div class="demo-gallery-thumbs">
                    {for (index, (src, _)) in ATLAS_JACKET_SHOTS.iter().enumerate() {
                        html_sync! {
                            <label class="demo-gallery-thumb" for={gallery_input_id(index)}>
                                <img src={*src} alt="" width="72" height="72" loading="lazy" />
                            </label>
                        }
                    }}
                </div>
            </div>
        }
        .render(cx)
    }
}

/// Ids are fixed per slide index, so the label/radio pairing is a compile-time
/// constant rather than a formatted string on the render path.
///
/// The length is also the number of slides the stylesheet can switch between:
/// `.demo-gallery-radio:nth-of-type(N):checked` is spelled out once per slide
/// in `styles/input.css`. A fifth shot would render a thumbnail that selects
/// nothing, so the assertion below turns that into a build failure.
const GALLERY_INPUT_IDS: [&str; 4] = [
    "atlas-jacket-shot-0",
    "atlas-jacket-shot-1",
    "atlas-jacket-shot-2",
    "atlas-jacket-shot-3",
];

const _: () = assert!(
    ATLAS_JACKET_SHOTS.len() == GALLERY_INPUT_IDS.len(),
    "every gallery shot needs an id, and every id needs a slide rule in styles/input.css"
);

fn gallery_input_id(index: usize) -> &'static str {
    GALLERY_INPUT_IDS[index]
}

struct QuantityStepperDemo;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for QuantityStepperDemo {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="demo-pdp">
                <div class="demo-pdp-media">{Gallery}</div>
                <div class="demo-pdp-info">
                    <nav class="text-[11px] uppercase tracking-[0.16em] text-zinc-400 dark:text-zinc-500">
                        "Men / Outerwear"
                    </nav>
                    <h1 class="mt-2 text-xl font-medium leading-tight text-zinc-950 dark:text-zinc-50">
                        "Atlas Jacket"
                    </h1>
                    <p class="text-sm text-zinc-500 dark:text-zinc-400">
                        "Men's Weatherproof Shell"
                    </p>
                    <p class="mt-3 text-base font-medium text-zinc-950 dark:text-zinc-50">"$248"</p>
                    <p class="mt-3 text-sm leading-6 text-zinc-600 dark:text-zinc-400">
                        "Weatherproof shell with a quiet technical finish."
                    </p>
                    <form method="post" action="/demos/quantity-stepper/cart" class="mt-5">
                        <input type="hidden" name="sku" value="atlas-jacket" />
                        {QuantityStepper {
                            unit_price: ATLAS_JACKET_USD,
                        }}
                        <button
                            class="mt-5 flex h-12 w-full items-center justify-center rounded-full bg-zinc-950 text-base font-medium text-white transition hover:opacity-80 dark:bg-zinc-50 dark:text-zinc-950"
                            type="submit"
                        >
                            "Add to Bag"
                        </button>
                    </form>
                    <div class="mt-5 border-t border-zinc-200 pt-3 text-xs text-zinc-500 dark:border-zinc-800 dark:text-zinc-400">
                        <p class="py-1">"Free standard shipping on orders over $150."</p>
                        <p class="py-1">"Free 60-day returns."</p>
                    </div>
                </div>
            </div>
        }
        .render(cx)
    }
}

struct CartReceipt {
    sku: String,
    qty: u32,
    subtotal: i64,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for CartReceipt {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="demo-receipt">
                <p class="text-[15px] font-medium text-zinc-950 dark:text-zinc-50">
                    "The server received"
                </p>
                <dl class="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 font-mono text-sm text-zinc-600 dark:text-zinc-300">
                    <dt>"sku"</dt>
                    <dd>{self.sku}</dd>
                    <dt>"qty"</dt>
                    <dd>{self.qty}</dd>
                    <dt>"total"</dt>
                    <dd>"$"{self.subtotal}</dd>
                </dl>
                <a
                    class="mt-4 inline-block text-sm text-zinc-500 underline underline-offset-4 dark:text-zinc-400"
                    href="/demos/quantity-stepper"
                >
                    "Run it again"
                </a>
            </div>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A framed demo is worthless if it only renders. It has to arrive with the
    /// island boundary, the seed, and the runtime tags that hydrate it, and it
    /// has to post a usable quantity before any of that runs.
    #[test]
    fn the_demo_page_hydrates_and_still_posts_without_javascript() {
        let html = quantity_stepper_page().0;

        assert!(html.contains("<rsjs-island"), "island boundary is missing");
        assert!(
            html.contains("data-rsjs-island="),
            "signal seed script is missing"
        );
        assert!(
            html.contains("/rsjs-runtime.js"),
            "runtime tags are missing, so nothing hydrates inside the frame"
        );
        assert!(
            html.contains(r#"<input type="hidden" name="qty" value="1""#),
            "the no-script submission would carry no quantity"
        );
        assert!(html.contains(r#"action="/demos/quantity-stepper/cart""#));
    }

    /// The gallery is deliberately not an island. If it ever grows one, the
    /// page stops demonstrating "exactly one component needs JavaScript", so
    /// pin the inert radio/label pairing that switches the slides.
    #[test]
    fn the_gallery_switches_slides_without_javascript() {
        let html = quantity_stepper_page().0;

        for (index, (src, alt)) in ATLAS_JACKET_SHOTS.iter().enumerate() {
            let id = gallery_input_id(index);
            assert!(
                html.contains(&format!(r#"id="{id}""#)),
                "slide {index} has no radio"
            );
            assert!(
                html.contains(&format!(r#"for="{id}""#)),
                "slide {index} has no thumbnail label"
            );
            assert!(html.contains(src), "{src} is not rendered");
            assert!(
                html.contains(alt),
                "{alt} is missing, so the slide is unlabelled"
            );
        }

        assert!(
            html.contains(r#"id="atlas-jacket-shot-0" checked"#),
            "the first slide has to be selected server-side"
        );
        // Only the stepper's markers belong to an island.
        let gallery = html
            .split("demo-gallery\"")
            .nth(1)
            .and_then(|rest| rest.split("demo-pdp-info").next())
            .expect("gallery markup");
        assert!(
            !gallery.contains("data-rsjs"),
            "the gallery must stay inert: {gallery}"
        );
    }

    #[test]
    fn the_cart_route_echoes_the_posted_quantity() {
        let html = quantity_stepper_cart_page(AddToCart {
            sku: "atlas-jacket".to_string(),
            qty: 3,
        })
        .0;

        assert!(html.contains("atlas-jacket"));
        assert!(html.contains(">3<"));
        assert!(html.contains("$744"), "subtotal should be 3 x $248");
    }
}
