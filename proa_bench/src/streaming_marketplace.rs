// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Synthetic marketplace/product-detail streaming workload.
//!
//! The shape is intentionally "eBay-like":
//! - static product shell and purchase summary
//! - multiple independently delayed sections
//! - some sections matter early (shipping, seller trust)
//! - some sections are lower-value late content (reviews, recommendations)
//!
//! Unlike the initial prototype, section delays now flow through
//! `cx.load(...)` so `ChunkedInOrder` can pre-start them before the render
//! blocks on each boundary.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::future::join_all;
use proa_core::{
    to_async, DynDataLoader, LoadError, LoadKey, LoadRequest, RenderBuffer, RenderOutcome,
    WebContext, WebRender, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::{html, html_sync};
use proa_stream::{AxumLoader, CollectLoadKeys, Suspense};

#[derive(Clone, Copy, Debug)]
pub struct MarketplacePanelSpec {
    pub key: &'static str,
    pub title: &'static str,
    pub fallback_label: &'static str,
    pub delay_ms: u64,
    pub eyebrow: &'static str,
    pub body: &'static str,
}

pub const MARKETPLACE_PANEL_SPECS: &[MarketplacePanelSpec] = &[
    MarketplacePanelSpec {
        key: "shipping",
        title: "Shipping and delivery",
        fallback_label: "Loading shipping and delivery",
        delay_ms: 8,
        eyebrow: "Fast path",
        body: "Get it by Tuesday with 2-day shipping. Local pickup available at checkout.",
    },
    MarketplacePanelSpec {
        key: "seller",
        title: "Seller reputation",
        fallback_label: "Loading seller reputation",
        delay_ms: 16,
        eyebrow: "Trust",
        body: "Top Rated seller with 99.8% positive feedback across 24,118 completed orders.",
    },
    MarketplacePanelSpec {
        key: "returns",
        title: "Returns and protection",
        fallback_label: "Loading returns and protection",
        delay_ms: 24,
        eyebrow: "Policy",
        body: "30-day free returns plus purchase protection when the item arrives not as described.",
    },
    MarketplacePanelSpec {
        key: "similar",
        title: "Similar sponsored items",
        fallback_label: "Loading similar sponsored items",
        delay_ms: 42,
        eyebrow: "Discovery",
        body: "Canon FD 50mm f/1.4, Nikon Series E 50mm f/1.8, and Olympus Zuiko 50mm f/1.8.",
    },
    MarketplacePanelSpec {
        key: "reviews",
        title: "Buyer reviews",
        fallback_label: "Loading buyer reviews",
        delay_ms: 65,
        eyebrow: "Social proof",
        body: "Vintage glass with excellent character. Buyers mention sharp center detail and smooth bokeh.",
    },
    MarketplacePanelSpec {
        key: "qa",
        title: "Questions and answers",
        fallback_label: "Loading questions and answers",
        delay_ms: 90,
        eyebrow: "Long tail",
        body: "Adapters tested on Sony E-mount and Fuji X. Aperture ring remains fully manual.",
    },
];

pub const MARKETPLACE_SHELL_MARKER: &str = "data-benchmark-shell=\"marketplace\"";

const MARKETPLACE_PANEL_NAMESPACE: &str = "marketplace_panel";

#[derive(Clone, Copy, Debug)]
pub struct MarketplaceStreamPage;

#[derive(Clone, Copy, Debug)]
pub struct MarketplaceBufferedPage;

#[derive(Clone, Copy, Debug)]
pub struct MarketplaceLoadedSyncPage;

#[derive(Clone, Copy, Debug)]
struct LoadedPanel {
    spec: &'static MarketplacePanelSpec,
}

#[derive(Clone, Copy, Debug)]
struct ReadyPanel {
    spec: &'static MarketplacePanelSpec,
}

#[derive(Clone, Copy, Debug)]
struct FallbackPanel {
    spec: &'static MarketplacePanelSpec,
}

#[derive(Clone, Copy, Debug)]
struct LoadedPanels;

#[derive(Clone, Copy, Debug)]
struct StreamingPanels;

impl<L: ::proa_core::DataLoader + 'static> WebRender<L, RenderBuffer> for MarketplaceStreamPage {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! {
            <main class="marketplace-pdp" data-benchmark-shell="marketplace">
                {to_async(MarketplaceHero)}
                <div class="content-grid">{StreamingPanels}</div>
            </main>
        }
        .render(cx)
    }
}

impl<L: ::proa_core::DataLoader> WebRender<L, RenderBuffer> for MarketplaceBufferedPage {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let results = join_all(
                MARKETPLACE_PANEL_SPECS
                    .iter()
                    .map(|spec| cx.load_json::<()>(panel_load_key(spec))),
            )
            .await;

            for result in results {
                result?;
            }

            WebRenderSync::render(
                html_sync! {
                    <main class="marketplace-pdp" data-benchmark-shell="marketplace">
                        {MarketplaceHero}
                        <div class="content-grid">{LoadedPanels}</div>
                    </main>
                },
                cx,
            )
        })
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for MarketplaceLoadedSyncPage
where
    MarketplaceHero: WebRenderSync<L, B>,
    LoadedPanels: WebRenderSync<L, B>,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                <main class="marketplace-pdp" data-benchmark-shell="marketplace">
                    {MarketplaceHero}
                    <div class="content-grid">{LoadedPanels}</div>
                </main>
            },
            cx,
        )
    }
}

impl CollectLoadKeys for MarketplaceStreamPage {
    fn collect_load_keys(&self, target: &mut Vec<LoadKey>) {
        target.extend(MARKETPLACE_PANEL_SPECS.iter().map(panel_load_key));
    }
}

impl CollectLoadKeys for MarketplaceBufferedPage {
    fn collect_load_keys(&self, target: &mut Vec<LoadKey>) {
        target.extend(MARKETPLACE_PANEL_SPECS.iter().map(panel_load_key));
    }
}

impl<L: ::proa_core::DataLoader + 'static> WebRender<L, RenderBuffer> for StreamingPanels {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            for spec in MARKETPLACE_PANEL_SPECS {
                proa_core::render_child(
                    cx,
                    Suspense::new(LoadedPanel { spec }, FallbackPanel { spec }),
                )
                .await?;
            }
            Ok(())
        })
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRender<L, B> for LoadedPanel
where
    ReadyPanel: WebRenderSync<L, B>,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let spec = self.spec;
        RenderOutcome::pending(async move {
            let (): () = cx.load_json(panel_load_key(spec)).await?;
            proa_core::render_child(cx, to_async(ReadyPanel { spec })).await
        })
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for MarketplaceHero {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                <section class="hero-shell">
                    <div class="media-column">
                        <div class="gallery">
                            <div class="photo">"Lens photo"</div>
                            <div class="thumbs">
                                <span>"Front element"</span>
                                <span>"Side profile"</span>
                                <span>"Mount"</span>
                            </div>
                        </div>
                    </div>
                    <div class="summary-column">
                        <p class="eyebrow">"eBay-like marketplace PDP"</p>
                        <h1>"Canon FD 50mm f/1.4 S.S.C. Vintage Prime Lens"</h1>
                        <p class="price">"Bid US $129.50"</p>
                        <p class="meta">"Condition: Used · Watchers: 214 · 12 bids"</p>
                        <div class="actions">
                            <span>"Place bid"</span>
                            <span>"Buy it now"</span>
                            <span>"Add to watchlist"</span>
                        </div>
                    </div>
                </section>
            },
            cx,
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct MarketplaceHero;

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for ReadyPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                <section
                    class="marketplace-panel ready"
                    data-panel={self.spec.key}
                    data-panel-ready={self.spec.key}
                >
                    <p class="panel-eyebrow">{self.spec.eyebrow}</p>
                    <h2>{self.spec.title}</h2>
                    <p>{self.spec.body}</p>
                </section>
            },
            cx,
        )
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for FallbackPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                <section
                    class="marketplace-panel fallback"
                    data-panel={self.spec.key}
                    data-panel-fallback={self.spec.key}
                    aria-busy="true"
                    aria-label={self.spec.fallback_label}
                >
                    <div class="panel-skeleton panel-skeleton-badge"></div>
                    <div class="panel-skeleton-group">
                        <div class="panel-skeleton panel-skeleton-title"></div>
                        <div class="panel-skeleton panel-skeleton-line"></div>
                        <div class="panel-skeleton panel-skeleton-line panel-skeleton-line-short"></div>
                    </div>
                </section>
            },
            cx,
        )
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for LoadedPanels {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for spec in MARKETPLACE_PANEL_SPECS {
            ReadyPanel { spec }.render(cx)?;
        }
        Ok(())
    }
}

pub fn marketplace_loader(delay_scale: u64) -> Arc<dyn DynDataLoader> {
    let delay_scale = normalize_delay_scale(delay_scale);
    Arc::new(AxumLoader::new(move |request: LoadRequest| async move {
        let Some(spec) = panel_spec_for_key(request.key()) else {
            return Err(LoadError::custom(format!(
                "unexpected marketplace panel key: {}",
                request.key()
            )));
        };

        tokio::time::sleep(Duration::from_millis(scaled_delay_ms(
            spec.delay_ms,
            delay_scale,
        )))
        .await;

        Ok(Bytes::from_static(b"null"))
    }))
}

pub fn marketplace_preload_keys() -> Vec<LoadKey> {
    MarketplaceStreamPage.load_keys()
}

fn panel_load_key(spec: &MarketplacePanelSpec) -> LoadKey {
    LoadKey::named(MARKETPLACE_PANEL_NAMESPACE, spec.key)
}

fn panel_spec_for_key(key: &LoadKey) -> Option<&'static MarketplacePanelSpec> {
    if key.namespace() != MARKETPLACE_PANEL_NAMESPACE {
        return None;
    }

    let key_name = match key.kind() {
        proa_core::LoadKeyKind::Named(name) => name.as_str(),
        proa_core::LoadKeyKind::Hashed(_) => return None,
    };

    MARKETPLACE_PANEL_SPECS
        .iter()
        .find(|spec| spec.key == key_name)
}

fn normalize_delay_scale(delay_scale: u64) -> u64 {
    delay_scale.max(1)
}

fn scaled_delay_ms(base_delay_ms: u64, delay_scale: u64) -> u64 {
    base_delay_ms.saturating_mul(normalize_delay_scale(delay_scale))
}
