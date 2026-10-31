// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The ORIGIN atlas document: a composition of the component families. Every
//! dossier pane loads through the request-scoped RIPEstat loader; the page
//! itself awaits the same key once to build the markdown overlay, so the
//! whole render still costs exactly one RIPEstat fan-out.

use std::future::Future;
use std::sync::Arc;

use proa_core::{
    ComponentRole, DataLoader, MdRenderSync, RenderOutcome, WebContext, WebRender, WriteBuf,
    WriteError,
};
use proa_macros::html;

use crate::components::agent_surface::{AgentSurface, AgentSurfaceMd};
use crate::components::dossier::{
    CarriersPane, DossierMd, DossierMetrics, NeighbourhoodGraph, PathsPane, RangesPane, SummaryBar,
    TraceStrip,
};
use crate::components::flood_lab::{FloodLab, FloodLabMd};
use crate::components::hero::{Hero, HeroMd};
use crate::components::live_wire::{LiveWire, LiveWireMd};
use crate::components::receipt::{Receipt, ReceiptFallback, ReceiptMd};
use crate::components::site_header::SiteHeader;
use crate::model::{normalize_query, Dossier};
use proa_stream::Suspense;

pub struct AtlasPage {
    pub query: String,
    /// The request's Host header when known (goldens render without one).
    pub host: Option<String>,
}

impl AtlasPage {
    pub fn resolve(raw_query: Option<&str>) -> Self {
        let query = match raw_query {
            None | Some("") => crate::model::default_site().host.to_string(),
            Some(raw) => normalize_query(raw),
        };
        Self { query, host: None }
    }

    pub fn with_host(mut self, host: Option<String>) -> Self {
        self.host = host;
        self
    }
}

/// Render the dossier markdown document to a string (for the overlay).
// proa-perf-ok(scope): one owned document per request.
pub fn markdown_string(query: &str, dossier: Option<Arc<Dossier>>) -> String {
    markdown_string_with_base_path(query, dossier, &proa_core::BasePath::default())
}

// proa-perf-ok(scope): one owned overlay document per request.
pub fn markdown_string_with_base_path(
    query: &str,
    dossier: Option<Arc<Dossier>>,
    base_path: &proa_core::BasePath,
) -> String {
    let mut out = Vec::new();
    {
        let mut cx = WebContext::with_buffer(&mut out);
        cx.request_mut().set_base_path(base_path.clone());
        let _ = AtlasMd {
            query: query.to_string(),
            host: None,
            dossier,
        }
        .render_md(&mut cx);
    }
    String::from_utf8(out).unwrap_or_default()
}

impl<L: DataLoader + 'static> WebRender<L> for AtlasPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    // proa-perf-ok(scope): guided demo page; the per-request view model and
    // its owned strings are intentional and sized to one lookup.
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let query = self.query.clone();
            let active_host = crate::model::site_for(&query).map(|site| site.host);
            html! {
                <div
                    data-origin-atlas
                    data-xray="0"
                    class="min-h-screen bg-bg text-ink font-mono text-[12px] font-light tracking-[-0.01em]"
                >
                    {SiteHeader { query: query.clone() }}
                    {proa_core::to_async(Hero { query: query.clone(), active_host })}
                    {Suspense::new(TraceStrip { query: query.clone() }, PaneFallback { label: "Resolving the trace" })}
                    {Suspense::new(SummaryBar { query: query.clone() }, PaneFallback { label: "Loading" })}
                    <div>
                        {Suspense::new(DossierMetrics { query: query.clone() }, PaneFallback { label: "Loading dossier metrics" })}
                        {Suspense::new(NeighbourhoodGraph { query: query.clone() }, PaneFallback { label: "Mapping the neighbourhood" })}
                        <div
                            data-pane-grid
                            class="grid grid-cols-[1fr_1fr_1fr] border-b border-line"
                        >
                            {Suspense::new(RangesPane { query: query.clone() }, PaneFallback { label: "Loading announced ranges" })}
                            {Suspense::new(CarriersPane { query: query.clone() }, PaneFallback { label: "Loading neighbours" })}
                            {Suspense::new(PathsPane { query: query.clone() }, PaneFallback { label: "Loading observed routes" })}
                        </div>
                    </div>
                    {proa_core::to_async(LiveWire)}
                    {proa_core::to_async(AgentSurface { query: query.clone(), host: self.host.clone() })}
                    {proa_core::to_async(FloodLab)}
                    {Suspense::new(Receipt { query: query.clone() }, ReceiptFallback)}
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

/// The streamed-section fallback shown while a pane's data resolves.
struct PaneFallback {
    label: &'static str,
}

impl<L: DataLoader> proa_core::WebRenderSync<L> for PaneFallback {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        proa_core::WebRenderSync::render(
            proa_macros::html_sync! {
                <div
                    data-pane-loading
                    class="py-[26px] px-[22px] border-b border-line text-[10px] tracking-[0.16em] text-ink4 uppercase animate-[org-pulse_1.6s_ease-in-out_infinite]"
                >
                    {self.label}
                    "…"
                </div>
            },
            cx,
        )
    }
}

/// The markdown representation: composed from the component families' own
/// markdown renders.
pub struct AtlasMd {
    pub query: String,
    pub host: Option<String>,
    pub dossier: Option<Arc<Dossier>>,
}

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for AtlasMd {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        crate::components::site_header::SiteHeaderMd.render_md(cx)?;
        HeroMd.render_md(cx)?;
        let query = self.query.clone();
        DossierMd {
            query: self.query,
            dossier: self.dossier,
        }
        .render_md(cx)?;
        LiveWireMd.render_md(cx)?;
        AgentSurfaceMd {
            query,
            host: self.host.clone(), // proa-perf-ok: one short clone per markdown response.
        }
        .render_md(cx)?;
        FloodLabMd.render_md(cx)?;
        ReceiptMd.render_md(cx)
    }
}
