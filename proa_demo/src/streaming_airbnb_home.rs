// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Airbnb home page variants for async/streaming overhead experiments.
//!
//! This module intentionally renders the same page shape three ways:
//! - pure sync (`SyncHomePage`)
//! - buffered async (`BufferedAsyncHomePage`)
//! - chunked in-order streaming (`StreamingHomePage`)
//!
//! The child sections are made async without artificial sleeps so the
//! benchmark isolates Proa's async/streaming overhead instead of network time.

use std::future::Future;
use std::rc::Rc;

use proa_core::{
    render_in_context_async, to_async, RenderBuffer, RenderMode, RenderOutcome, StaticRaw,
    StreamBoundaryRecorder, StreamChunkFlusher, WebContext, WebRender, WebRenderSync, WriteBuf,
    WriteError,
};
use proa_macros::{html, html_sync};
use proa_showcase::airbnb::home::{HomeData, HomeListing, HomeSection};
use proa_showcase::airbnb::shared;
use proa_stream::Suspense;

const STAR_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="#FF385C"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/></svg>"##;
const SEARCH_SVG: &str = r##"<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg" style="display:block;fill:none;height:16px;width:16px;stroke:currentColor;stroke-width:4;overflow:visible" aria-hidden="true" role="presentation" focusable="false"><path d="m13 24c6.0751 0 11-4.9249 11-11 0-6.0751-4.9249-11-11-11-6.0751 0-11 4.9249-11 11 0 6.0751 4.9249 11 11 11zm8-3 10 10"/></svg>"##;
const HOUSE_POSTER: &str = "/images/airbnb/external/4aae4ed7-5939-4e76-b100-e69440ebeae4.png";
const BALLOON_POSTER: &str = "/images/airbnb/external/search-icon-1.png";
const SERVICE_POSTER: &str = "/images/airbnb/external/search-icon-2.png";

const HOMES_RAIL_IDS: [&str; 8] = [
    "demo-homes-rail-0",
    "demo-homes-rail-1",
    "demo-homes-rail-2",
    "demo-homes-rail-3",
    "demo-homes-rail-4",
    "demo-homes-rail-5",
    "demo-homes-rail-6",
    "demo-homes-rail-7",
];
const EXPERIENCES_RAIL_IDS: [&str; 8] = [
    "demo-experiences-rail-0",
    "demo-experiences-rail-1",
    "demo-experiences-rail-2",
    "demo-experiences-rail-3",
    "demo-experiences-rail-4",
    "demo-experiences-rail-5",
    "demo-experiences-rail-6",
    "demo-experiences-rail-7",
];
const SERVICES_RAIL_IDS: [&str; 8] = [
    "demo-services-rail-0",
    "demo-services-rail-1",
    "demo-services-rail-2",
    "demo-services-rail-3",
    "demo-services-rail-4",
    "demo-services-rail-5",
    "demo-services-rail-6",
    "demo-services-rail-7",
];

const TITLE_PARTS: &[&str] = &["Airbnb - Home (Streaming Demo)"];

#[derive(Clone, Copy)]
pub struct DemoHomeData {
    listings: &'static [HomeListing<'static>],
    homes_sections: &'static [HomeSection<'static>],
    experiences_sections: &'static [HomeSection<'static>],
    services_sections: &'static [HomeSection<'static>],
}

impl Default for DemoHomeData {
    fn default() -> Self {
        let data = HomeData::default_la();
        Self {
            listings: data.listings,
            homes_sections: data.homes_sections,
            experiences_sections: data.experiences_sections,
            services_sections: data.services_sections,
        }
    }
}

#[derive(Clone, Copy)]
pub struct SyncHomePage {
    pub data: DemoHomeData,
}

#[derive(Clone, Copy)]
pub struct BufferedAsyncHomePage {
    pub data: DemoHomeData,
}

#[derive(Clone, Copy)]
pub struct StreamingHomePage {
    pub data: DemoHomeData,
}

#[derive(Clone, Copy)]
struct ImmediateAsync<T>(T);

#[derive(Clone, Copy)]
struct HomeTopBar;

#[derive(Clone, Copy)]
struct HomeFooter;

#[derive(Clone, Copy)]
struct HomesPanel {
    data: DemoHomeData,
}

#[derive(Clone, Copy)]
struct ExperiencesPanel {
    data: DemoHomeData,
}

#[derive(Clone, Copy)]
struct ServicesPanel {
    data: DemoHomeData,
}

#[derive(Clone, Copy)]
struct PanelFallback {
    title: &'static str,
}

#[derive(Clone, Copy)]
enum PanelKind {
    Homes,
    Experiences,
    Services,
}

#[derive(Clone, Copy)]
struct RailCard {
    kind: PanelKind,
    listing: &'static HomeListing<'static>,
}

pub fn render_sync() -> String {
    let mut cx = WebContext::new();
    SyncHomePage {
        data: DemoHomeData::default(),
    }
    .render(&mut cx)
    .expect("sync home page should render");
    cx.into_output().into_string()
}

pub fn render_sync_into<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: DemoHomeData,
) -> Result<(), WriteError> {
    SyncHomePage { data }.render(cx)
}

pub async fn render_buffered_async_into(
    out: &mut Vec<u8>,
    data: DemoHomeData,
) -> Result<(), WriteError> {
    let mut cx = WebContext::new();
    render_in_context_async(&mut cx, BufferedAsyncHomePage { data }).await?;
    *out = cx.into_output().into_vec();
    Ok(())
}

pub async fn render_chunked_in_order_into(
    tail: &mut Vec<u8>,
    data: DemoHomeData,
    flusher: Rc<dyn StreamChunkFlusher>,
) -> Result<(), WriteError> {
    let mut cx = WebContext::new()
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(flusher);
    render_in_context_async(&mut cx, StreamingHomePage { data }).await?;
    *tail = cx.into_output().into_vec();
    Ok(())
}

pub async fn render_chunked_out_of_order_into(
    shell: &mut Vec<u8>,
    data: DemoHomeData,
    recorder: Rc<dyn StreamBoundaryRecorder>,
) -> Result<(), WriteError> {
    let mut cx = WebContext::new()
        .with_render_mode(RenderMode::ChunkedOutOfOrder)
        .with_stream_recorder(recorder);
    render_in_context_async(&mut cx, StreamingHomePage { data }).await?;
    *shell = cx.into_output().into_vec();
    Ok(())
}

impl<T, B, L: ::proa_core::DataLoader> WebRender<L, B> for ImmediateAsync<T>
where
    T: WebRenderSync<L, B>,
    B: WriteBuf,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move { self.0.render(cx) })
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for HomeTopBar {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
            <header class="air-topbar">
                <div class="air-shell pt-5 pb-6 md:pt-6 md:pb-7">
                    <div class="grid grid-cols-[auto_1fr_auto] items-center gap-4 md:gap-8">
                        <a href="/showcase/airbnb/proa" class="flex items-center gap-2.5 text-[#FF385C]">
                            {StaticRaw(shared::AIRBNB_LOGO_SVG)}
                            <span class="text-[30px] font-semibold leading-none tracking-[-0.02em]">"airbnb"</span>
                        </a>

                        <nav class="hidden md:flex items-center justify-center gap-8 lg:gap-10" role="tablist" aria-label="Search modes">
                            <button class="home-mode-tab active" type="button">
                                <span class="home-mode-icon">
                                    <img src={HOUSE_POSTER} alt="" class="home-mode-media" loading="eager" fetchpriority="low"/>
                                </span>
                                <span>"Homes"</span>
                            </button>
                            <button class="home-mode-tab" type="button">
                                <span class="home-mode-icon relative">
                                    <img src={BALLOON_POSTER} alt="" class="home-mode-media" loading="eager" fetchpriority="low"/>
                                    <span class="home-mode-badge">"NEW"</span>
                                </span>
                                <span>"Experiences"</span>
                            </button>
                            <button class="home-mode-tab" type="button">
                                <span class="home-mode-icon relative">
                                    <img src={SERVICE_POSTER} alt="" class="home-mode-media" loading="eager" fetchpriority="low"/>
                                    <span class="home-mode-badge">"NEW"</span>
                                </span>
                                <span>"Services"</span>
                            </button>
                        </nav>

                        <div class="flex items-center justify-end gap-2 md:gap-3">
                            <a href="/showcase/airbnb/proa" class="hidden rounded-full px-4 py-3 text-sm font-semibold hover:bg-[var(--air-soft)] lg:block">"Become a host"</a>
                            <button class="hidden md:flex h-10 w-10 items-center justify-center rounded-full hover:bg-[var(--air-soft)]" aria-label="Language">
                                {StaticRaw(shared::GLOBE_SVG)}
                            </button>
                            <button class="air-search-pill flex items-center gap-3 rounded-full border border-[var(--air-line)] bg-white pl-3 pr-2.5 py-1.5">
                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 32 32" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" aria-hidden="true"><line x1="6" y1="9" x2="26" y2="9"/><line x1="6" y1="16" x2="26" y2="16"/><line x1="6" y1="23" x2="26" y2="23"/></svg>
                                <div class="flex h-8 w-8 items-center justify-center rounded-full bg-[#6A6A6A] text-white">
                                    <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="currentColor"><path d="M12 12c2.209 0 4-1.791 4-4S14.209 4 12 4s-4 1.791-4 4 1.791 4 4 4zm0 2c-2.673 0-8 1.327-8 3.963V20h16v-2.037C20 15.327 14.673 14 12 14z"/></svg>
                                </div>
                            </button>
                        </div>
                    </div>

                    <div class="mt-6 flex justify-center">
                        <div class="home-search-shell w-full max-w-[850px]">
                            <div class="home-search-bar">
                                <button class="home-search-segment active" type="button">
                                    <span class="home-search-label">"Where"</span>
                                    <span class="home-search-value">"Search destinations"</span>
                                </button>
                                <span class="home-search-divider"></span>
                                <button class="home-search-segment" type="button">
                                    <span class="home-search-label">"When"</span>
                                    <span class="home-search-value">"Add dates"</span>
                                </button>
                                <span class="home-search-divider"></span>
                                <button class="home-search-segment" type="button">
                                    <span class="home-search-label">"Who"</span>
                                    <span class="home-search-value">"Add guests"</span>
                                </button>
                                <button class="home-search-submit" type="button" aria-label="Search">
                                    {StaticRaw(SEARCH_SVG)}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </header>
            },
            cx,
        )
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for HomeFooter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        shared::render_footer(cx);
        Ok(())
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for HomesPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        render_panel_group(
            cx,
            PanelKind::Homes,
            "homes",
            self.data.listings,
            self.data.homes_sections,
            &HOMES_RAIL_IDS,
        )
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for ExperiencesPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        render_panel_group(
            cx,
            PanelKind::Experiences,
            "experiences",
            self.data.listings,
            self.data.experiences_sections,
            &EXPERIENCES_RAIL_IDS,
        )
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for ServicesPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        render_panel_group(
            cx,
            PanelKind::Services,
            "services",
            self.data.listings,
            self.data.services_sections,
            &SERVICES_RAIL_IDS,
        )
    }
}

impl<L: ::proa_core::DataLoader> WebRenderSync<L, RenderBuffer> for PanelFallback {
    fn render(self, cx: &mut WebContext<L, RenderBuffer>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
            <section class="home-rail-section">
                <div class="mb-4">
                    <h2 class="text-[28px] font-semibold tracking-[-0.03em] text-[#222]">{self.title}</h2>
                    <p class="mt-1 text-[15px] text-[var(--air-muted)]">"Loading section…"</p>
                </div>
                <div class="rounded-[24px] border border-[var(--air-line)] bg-[var(--air-soft)] px-6 py-12 text-[15px] text-[var(--air-muted)]">
                    "Preparing content"
                </div>
            </section>
            },
            cx,
        )
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for RailCard {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let listing = self.listing;
        match self.kind {
            PanelKind::Homes => {
                let price = listing.price_per_night * 2;
                WebRenderSync::render(
                    html_sync! {
                    <a href="/showcase/airbnb/proa/listing" class="home-rail-card group">
                        <div class="air-card-image relative aspect-[1.03/1] bg-[var(--air-soft)]">
                            <img src={listing.image} alt={listing.title} class="h-full w-full object-cover" loading="lazy" fetchpriority="low"/>
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
                    },
                    cx,
                )
            }
            PanelKind::Experiences => {
                let price = listing.price_per_night / 2 + 42;
                WebRenderSync::render(
                    html_sync! {
                    <a href="/showcase/airbnb/proa/search" class="home-rail-card group">
                        <div class="air-card-image relative aspect-[1.03/1] bg-[var(--air-soft)]">
                            <img src={listing.image} alt={listing.title} class="h-full w-full object-cover" loading="lazy" fetchpriority="low"/>
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
                    },
                    cx,
                )
            }
            PanelKind::Services => {
                let price = listing.price_per_night / 2 + 65;
                WebRenderSync::render(
                    html_sync! {
                    <a href="/showcase/airbnb/proa/search" class="home-rail-card group">
                        <div class="air-card-image relative aspect-[1.03/1] bg-[var(--air-soft)]">
                            <img src={listing.image} alt={listing.title} class="h-full w-full object-cover" loading="lazy" fetchpriority="low"/>
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
                    },
                    cx,
                )
            }
        }
    }
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SyncHomePage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        shared::render_document_start(cx, TITLE_PARTS);
        WebRenderSync::render(
            html_sync! {
            {HomeTopBar}
            <main class="bg-white">
                <div class="home-mode-panel active" data-home-panel="homes">
                    <div class="air-shell py-10 md:py-12">
                        {HomesPanel { data: self.data }}
                    </div>
                </div>
                <div class="home-mode-panel" data-home-panel="experiences">
                    <div class="air-shell py-10 md:py-12">
                        {ExperiencesPanel { data: self.data }}
                    </div>
                </div>
                <div class="home-mode-panel" data-home-panel="services">
                    <div class="air-shell py-10 md:py-12">
                        {ServicesPanel { data: self.data }}
                    </div>
                </div>
            </main>
            {HomeFooter}
            },
            cx,
        )?;
        shared::render_document_end(cx);
        Ok(())
    }
}

impl<L: ::proa_core::DataLoader> WebRender<L, RenderBuffer> for BufferedAsyncHomePage {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            shared::render_document_start(cx, TITLE_PARTS);
            html! {
                {to_async(HomeTopBar)}
                <main class="bg-white">
                    <div class="home-mode-panel active" data-home-panel="homes">
                        <div class="air-shell py-10 md:py-12">
                            {ImmediateAsync(HomesPanel { data: self.data })}
                        </div>
                    </div>
                    <div class="home-mode-panel" data-home-panel="experiences">
                        <div class="air-shell py-10 md:py-12">
                            {ImmediateAsync(ExperiencesPanel { data: self.data })}
                        </div>
                    </div>
                    <div class="home-mode-panel" data-home-panel="services">
                        <div class="air-shell py-10 md:py-12">
                            {ImmediateAsync(ServicesPanel { data: self.data })}
                        </div>
                    </div>
                </main>
                {to_async(HomeFooter)}
            }
            .render(cx)
            .resolve()
            .await?;
            shared::render_document_end(cx);
            Ok(())
        })
    }
}

impl<L: ::proa_core::DataLoader + 'static> WebRender<L, RenderBuffer> for StreamingHomePage {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            shared::render_document_start(cx, TITLE_PARTS);
            html! {
                {to_async(HomeTopBar)}
                <main class="bg-white">
                    <div class="home-mode-panel active" data-home-panel="homes">
                        <div class="air-shell py-10 md:py-12">
                            {Suspense::new(
                                ImmediateAsync(HomesPanel { data: self.data }),
                                PanelFallback { title: "Popular homes in Los Angeles" },
                            )}
                        </div>
                    </div>
                    <div class="home-mode-panel" data-home-panel="experiences">
                        <div class="air-shell py-10 md:py-12">
                            {Suspense::new(
                                ImmediateAsync(ExperiencesPanel { data: self.data }),
                                PanelFallback { title: "Book experiences led by locals" },
                            )}
                        </div>
                    </div>
                    <div class="home-mode-panel" data-home-panel="services">
                        <div class="air-shell py-10 md:py-12">
                            {Suspense::new(
                                ImmediateAsync(ServicesPanel { data: self.data }),
                                PanelFallback { title: "Services for your stay" },
                            )}
                        </div>
                    </div>
                </main>
                {to_async(HomeFooter)}
            }
            .render(cx)
            .resolve()
            .await?;
            shared::render_document_end(cx);
            Ok(())
        })
    }
}

fn render_panel_group<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    kind: PanelKind,
    panel_name: &'static str,
    listings: &'static [HomeListing<'static>],
    sections: &'static [HomeSection<'static>],
    rail_ids: &[&'static str; 8],
) -> Result<(), WriteError> {
    for (section_index, section) in sections.iter().enumerate() {
        let rail_id = rail_ids[section_index];
        WebRenderSync::render(
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
                            WebRenderSync::render(
                                html_sync! { <p class="mt-1 text-[15px] text-[var(--air-muted)]">{section.subtitle}</p> },
                                cx,
                            )?;
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
                <div class="home-rail" id={rail_id} data-home-kind={panel_name}>
                    {for listing_index in section.listing_indexes.iter() {
                        let listing = &listings[*listing_index % listings.len()];
                        WebRenderSync::render(
                            html_sync! {
                            {RailCard { kind, listing }}
                            },
                            cx,
                        )?;
                    }}
                </div>
            </section>
            },
            cx,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use proa_core::RenderMode;
    use proa_core::StreamChunkView;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct CollectingFlusher {
        bytes: RefCell<Vec<u8>>,
        flushes: Cell<usize>,
    }

    impl CollectingFlusher {
        fn new() -> Self {
            Self {
                bytes: RefCell::new(Vec::new()),
                flushes: Cell::new(0),
            }
        }

        fn take(&self) -> Vec<u8> {
            std::mem::take(&mut *self.bytes.borrow_mut())
        }
    }

    impl StreamChunkFlusher for CollectingFlusher {
        fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
            self.flushes.set(self.flushes.get() + 1);
            self.bytes.borrow_mut().extend_from_slice(&chunk);
            Ok(())
        }

        fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
            self.flushes.set(self.flushes.get() + 1);
            chunk.for_each_slice(&mut |slice| {
                self.bytes.borrow_mut().extend_from_slice(slice);
                Ok(())
            })
        }
    }

    #[test]
    fn buffered_async_matches_sync_output() {
        let sync = render_sync();
        let mut cx = WebContext::new();
        block_on(render_in_context_async(
            &mut cx,
            BufferedAsyncHomePage {
                data: DemoHomeData::default(),
            },
        ))
        .unwrap();
        assert_eq!(sync, cx.into_output().into_string());
    }

    #[test]
    fn chunked_in_order_matches_sync_output() {
        let sync = render_sync().into_bytes();
        let collector = Rc::new(CollectingFlusher::new());
        let flusher: Rc<dyn StreamChunkFlusher> = collector.clone();
        let mut cx = WebContext::new()
            .with_render_mode(RenderMode::ChunkedInOrder)
            .with_stream_chunk_flusher(flusher);
        block_on(render_in_context_async(
            &mut cx,
            StreamingHomePage {
                data: DemoHomeData::default(),
            },
        ))
        .unwrap();

        let mut actual = collector.take();
        actual.extend_from_slice(&cx.into_output().into_vec());
        assert_eq!(sync, actual);
        assert!(collector.flushes.get() > 0);
    }
}
