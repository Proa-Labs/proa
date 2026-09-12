// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::sync::Arc;

use proa_core::{DataLoader, RenderOutcome, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

use crate::components::{Tooltip, TooltipContent, TooltipDelay, TooltipTrigger};
use crate::loader::dossier_key;
use crate::model::{carrier_name, site_for, Dossier, Site};

use super::recipe::{base, state};

/// One resolved data view shared by every pane: live RIPEstat data when the
/// loader succeeds, the curated dossier when the network is unavailable but
/// the query is one of the guided four, and an offline marker otherwise.
pub enum DossierView {
    Live(Arc<Dossier>),
    Curated(&'static Site),
    /// The query itself does not resolve: a typo'd name, an ASN announcing
    /// nothing. The data source is fine; the input is the problem.
    Unresolvable,
    /// RIPEstat could not be reached; the query may be perfectly good.
    Offline,
}

async fn view<L: DataLoader>(cx: &WebContext<L>, query: &str) -> DossierView {
    match cx.load_typed::<Dossier>(dossier_key(query)).await {
        Ok(dossier) => DossierView::Live(dossier),
        Err(error) => match site_for(query) {
            Some(site) => DossierView::Curated(site),
            // proa-perf-ok: one classification string per failed lookup.
            None if error.to_string().starts_with("unresolvable") => DossierView::Unresolvable,
            None => DossierView::Offline,
        },
    }
}

fn curated_overlay(host_or_query: &str) -> Option<&'static Site> {
    site_for(host_or_query)
}

pub struct PaneTitle {
    pub label: &'static str,
    pub tip: &'static str,
}

impl<L: DataLoader> WebRenderSync<L> for PaneTitle {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            {Tooltip {
                delay: TooltipDelay::Short,
                trigger: Some(TooltipTrigger {
                    children: Some(html_sync! {
                        <span class={base::PANE_TITLE}>{self.label}</span>
                    }),
                    ..Default::default()
                }),
                content: Some(TooltipContent {
                    children: Some(self.tip),
                    ..Default::default()
                }),
                ..Default::default()
            }}
        }
        .render(cx)
    }
}

// ---------------------------------------------------------------------------
// Trace strip
// ---------------------------------------------------------------------------

pub struct TraceStrip {
    pub query: String,
}

impl<L: DataLoader> proa_core::WebRender<L> for TraceStrip {
    // proa-perf-ok(scope): guided demo page; the per-request view model and
    // its owned strings are intentional and sized to one lookup.
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let view = view(cx, &self.query).await;
            let (host, address, prefix, net_label, asn_line) = match &view {
                DossierView::Live(dossier) => {
                    let overlay = dossier.subject.domain.as_deref().and_then(curated_overlay);
                    (
                        dossier
                            .subject
                            .domain
                            .clone()
                            .unwrap_or_else(|| dossier.subject.query.clone()),
                        dossier
                            .subject
                            .address
                            .clone()
                            .unwrap_or_else(|| "—".into()),
                        dossier.subject.prefix.clone(),
                        overlay
                            // proa-perf-ok: guided demo page; small per-request strings.
                            .map(|site| site.net_label.to_string())
                            // proa-perf-ok: guided demo page; small per-request strings.
                            .unwrap_or_else(|| dossier.display_name().to_string()),
                        overlay
                            // proa-perf-ok: guided demo page; small per-request strings.
                            .map(|site| site.asn_line.to_string())
                            // proa-perf-ok: guided demo page; small per-request strings.
                            .unwrap_or_else(|| format!("AS{}", dossier.subject.asn)),
                    )
                }
                DossierView::Curated(site) => (
                    // proa-perf-ok: guided demo page; small per-request strings.
                    site.host.to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    site.ip.to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    site.prefix.to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    site.net_label.to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    site.asn_line.to_string(),
                ),
                DossierView::Unresolvable => (
                    self.query.clone(), // proa-perf-ok: guided demo page; small per-request strings.
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "—".to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "—".to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "Name not found".to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "we could not resolve this; check the spelling or try an example above"
                        .to_string(), // proa-perf-ok: guided demo page; small per-request strings.
                ),
                DossierView::Offline => (
                    self.query.clone(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "—".to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "—".to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "Signal lost".to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "RIPEstat unreachable; retry shortly".to_string(),
                ),
            };
            let net_style = match &view {
                DossierView::Unresolvable | DossierView::Offline => state::NET_NAME_MUTED,
                _ => state::NET_NAME,
            };
            WebRenderSync::render(
                html_sync! {
                    <section
                        id="trace"
                        data-trace-strip
                        data-screen-label="The trace"
                        class="border-b border-line"
                    >
                        <div
                            data-trace-grid
                            class="grid grid-cols-[auto_minmax(max-content,1fr)_auto_minmax(max-content,1fr)_auto] items-start gap-0 box-border pt-[40px] px-[22px] pb-[44px]"
                        >
                            <div
                                data-trace-connector
                                class="col-[1/5] row-[1] self-start mt-[36px] h-[1px] z-0 bg-[repeating-linear-gradient(90deg,color-mix(in_srgb,var(--cyan)_55%,transparent)_0_3px,transparent_3px_8px)]"
                            ></div>
                            <div class="col-[1] row-[1] relative z-[1]">
                                <div class="whitespace-nowrap text-[9px] tracking-[0.18em] text-ink4 uppercase">
                                    "The website"
                                </div>
                                <div class="font-display inline-block bg-bg pr-[12px] text-[clamp(18px,2.6vw,34px)] font-bold tracking-[-0.035em] leading-[1.05] mt-[12px] whitespace-nowrap">
                                    {host.as_str()}
                                </div>
                                <div class="text-[11px] text-ink4 mt-[8px]">
                                    "what you asked for"
                                </div>
                            </div>
                            <div class="col-[2] row-[1] relative z-[2] self-stretch box-border min-w-max pt-[52px] px-[10px] pb-0">
                                <div
                                    data-trace-arrow
                                    class="absolute right-0 top-[32px] w-0 h-0 [border-left:9px_solid_var(--cyan)] [border-top:4.5px_solid_transparent] [border-bottom:4.5px_solid_transparent]"
                                ></div>
                                <div class="whitespace-nowrap text-center text-[10px] leading-[1.4] tracking-[0.14em] text-cyan uppercase">
                                    "DNS looks up"
                                    <br />
                                    "its address"
                                </div>
                            </div>
                            <div class="col-[3] row-[1] relative z-[1] box-border pl-[16px]">
                                <div class="whitespace-nowrap text-[9px] tracking-[0.18em] text-ink4 uppercase">
                                    "Its address"
                                </div>
                                <div class="font-display inline-block bg-bg pr-[12px] text-[clamp(18px,2.6vw,34px)] font-bold tracking-[-0.03em] leading-[1.05] mt-[12px] whitespace-nowrap">
                                    {address.as_str()}
                                </div>
                                <div class="text-[11px] text-ink4 mt-[8px]">
                                    "inside the range "
                                    {prefix.as_str()}
                                </div>
                            </div>
                            <div class="col-[4] row-[1] relative z-[2] self-stretch box-border min-w-max pt-[52px] px-[10px] pb-0">
                                <div
                                    data-trace-arrow
                                    class="absolute right-0 top-[32px] w-0 h-0 [border-left:9px_solid_var(--cyan)] [border-top:4.5px_solid_transparent] [border-bottom:4.5px_solid_transparent]"
                                ></div>
                                <div class="whitespace-nowrap text-center text-[10px] leading-[1.4] tracking-[0.14em] text-cyan uppercase">
                                    "BGP decides where"
                                    <br />
                                    "traffic goes"
                                </div>
                            </div>
                            <div class="col-[5] row-[1] relative z-[1] box-border pl-[16px]">
                                <div class="whitespace-nowrap text-[9px] tracking-[0.18em] text-ink4 uppercase">
                                    "The network"
                                </div>
                                <div class={net_style}>{net_label.as_str()}</div>
                                <div class="text-[11px] text-ink4 mt-[8px]">
                                    {asn_line.as_str()}
                                </div>
                            </div>
                        </div>
                    </section>
                },
                cx,
            )
        })
    }
}

// ---------------------------------------------------------------------------
// Summary bar + metric tiles
// ---------------------------------------------------------------------------

pub struct SummaryBar {
    pub query: String,
}

impl<L: DataLoader> proa_core::WebRender<L> for SummaryBar {
    // proa-perf-ok(scope): guided demo page; the per-request view model and
    // its owned strings are intentional and sized to one lookup.
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let view = view(cx, &self.query).await;
            let (host, hint) = match &view {
                DossierView::Live(dossier) => (
                    dossier
                        .subject
                        .domain
                        .clone()
                        .unwrap_or_else(|| dossier.subject.query.clone()),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    format!(
                        "every range AS{} announces, who carries its traffic, and the routes in between",
                        dossier.subject.asn
                    ),
                ),
                DossierView::Curated(site) => (
                    // proa-perf-ok: guided demo page; small per-request strings.
                    site.host.to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    format!(
                        "every range {} announces, who carries its traffic, and the routes in between",
                        site.asn
                    ),
                ),
                DossierView::Unresolvable => (
                    self.query.clone(), // proa-perf-ok: guided demo page; small per-request strings.
                    "we could not resolve this name; check the spelling or try one of the examples above"
                        .to_string(), // proa-perf-ok: guided demo page; small per-request strings.
                ),
                DossierView::Offline => (
                    self.query.clone(), // proa-perf-ok: guided demo page; small per-request strings.
                    "RIPEstat is unreachable right now; the dossier will load once it answers"
                        .to_string(), // proa-perf-ok: guided demo page; small per-request strings.
                ),
            };
            WebRenderSync::render(
                html_sync! {
                    <div
                        data-summary-bar
                        class="flex items-baseline gap-[14px] py-[15px] px-[22px] border-b border-line"
                    >
                        <span class="font-display text-[18px] font-semibold tracking-[-0.02em]">
                            "Just "
                            {host.as_str()}
                        </span>
                        <span class="text-[11px] text-ink4">{hint.as_str()}</span>
                    </div>
                },
                cx,
            )
        })
    }
}

struct MetricTile {
    label: String,
    value: String,
    sub: String,
    cyan: bool,
}

pub struct DossierMetrics {
    pub query: String,
}

impl<L: DataLoader> proa_core::WebRender<L> for DossierMetrics {
    // proa-perf-ok(scope): guided demo page; the per-request view model and
    // its owned strings are intentional and sized to one lookup.
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let view = view(cx, &self.query).await;
            let tiles: Vec<MetricTile> = match &view {
                DossierView::Live(dossier) => {
                    let v4 = dossier
                        .prefixes
                        .iter()
                        .filter(|p| !p.prefix.contains(':'))
                        .count();
                    let v6 = dossier.prefixes.len() - v4;
                    let (v4_value, v6_value, range_sub) = if dossier.prefixes.is_empty() {
                        // proa-perf-ok: guided demo page; small per-request strings.
                        ("—".to_string(), "—".to_string(), "list unavailable")
                    } else {
                        // proa-perf-ok: guided demo page; small per-request strings.
                        (v4.to_string(), v6.to_string(), "announced now")
                    };
                    // proa-perf-ok: guided demo page; small per-request strings.
                    vec![
                        MetricTile {
                            label: "IPv4 ranges".into(),
                            value: v4_value,
                            sub: range_sub.into(),
                            cyan: false,
                        },
                        MetricTile {
                            label: "IPv6 ranges".into(),
                            value: v6_value,
                            sub: range_sub.into(),
                            cyan: false,
                        },
                        MetricTile {
                            label: "Neighbours".into(),
                            // proa-perf-ok: guided demo page; small per-request strings.
                            value: dossier.neighbours.len().to_string(),
                            sub: "adjacent networks".into(),
                            cyan: false,
                        },
                        MetricTile {
                            label: "RPKI".into(),
                            value: dossier.rpki_status.to_uppercase(),
                            sub: "queried prefix".into(),
                            cyan: true,
                        },
                        MetricTile {
                            label: "Visibility".into(),
                            // proa-perf-ok: guided demo page; small per-request strings.
                            value: format!(
                                "{}/{}",
                                dossier.routing.seen_by, dossier.routing.collectors
                            ),
                            sub: "RIS peers seeing it".into(),
                            cyan: true,
                        },
                        MetricTile {
                            label: "Last seen".into(),
                            // proa-perf-ok: guided demo page; small per-request strings.
                            value: dossier.routing.last_seen.chars().take(10).collect(),
                            sub: "in the RIS window".into(),
                            cyan: false,
                        },
                    ]
                }
                DossierView::Curated(site) => site
                    .metrics
                    .iter()
                    .enumerate()
                    .map(|(index, metric)| MetricTile {
                        label: metric.label.into(),
                        value: metric.value.into(),
                        sub: metric.sub.into(),
                        cyan: index == 3 || index == 4,
                    })
                    // proa-perf-ok: guided demo page; small per-request strings.
                    .collect(),
                // proa-perf-ok: guided demo page; small per-request strings.
                DossierView::Unresolvable => vec![MetricTile {
                    label: "Not found".into(),
                    value: "—".into(),
                    sub: "this name does not resolve".into(),
                    cyan: false,
                }],
                // proa-perf-ok: guided demo page; small per-request strings.
                DossierView::Offline => vec![MetricTile {
                    label: "Signal lost".into(),
                    value: "—".into(),
                    sub: "RIPEstat unreachable".into(),
                    cyan: false,
                }],
            };
            WebRenderSync::render(
                html_sync! {
                    <div
                        data-dossier-metrics
                        class="grid grid-cols-[repeat(6,1fr)] border-b border-line"
                    >
                        {Tiles { tiles: &tiles }}
                    </div>
                },
                cx,
            )
        })
    }
}

/// Plain-language explanations keyed by tile label, covering both the live
/// and curated metric sets; unknown labels render without a tooltip.
fn metric_tip(label: &str) -> Option<&'static str> {
    Some(match label {
        "IPv4 ranges" | "IPv6 ranges" => {
            "How many separate blocks of addresses this network announces to the internet right now, per address family."
        }
        "Neighbours" => "Networks observed directly adjacent to this one in real routes.",
        "RPKI" | "Verified" => {
            "Whether the owner of the address space has cryptographically authorized this network to announce it."
        }
        "Visibility" | "Reachability" => {
            "How many of RIPE's route collectors currently see a way to reach the queried range."
        }
        "Last seen" | "Last change" => {
            "The most recent routing activity for the queried range inside the RIS data window."
        }
        _ => return None,
    })
}

struct Tiles<'a> {
    tiles: &'a [MetricTile],
}
impl<L: DataLoader> WebRenderSync<L> for Tiles<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for tile in self.tiles {
            let value_style = if tile.cyan {
                base::METRIC_VALUE_CYAN
            } else {
                base::METRIC_VALUE_BONE
            };
            html_sync! {
                <div class="pt-[18px] px-[20px] pb-[20px] border-r border-line">
                    <div class="text-[9px] tracking-[0.16em] text-ink4 uppercase">
                        {match metric_tip(&tile.label) {
                            Some(tip) => proa_macros::html_sync! {
                                {Tooltip {
                                    delay: TooltipDelay::Short,
                                    trigger: Some(TooltipTrigger {
                                        children: Some(proa_macros::html_sync! {
                                            <span class="cursor-help underline decoration-dotted decoration-line-strong underline-offset-[3px]">
                                                {tile.label.as_str()}
                                            </span>
                                        }),
                                        ..Default::default()
                                    }),
                                    content: Some(TooltipContent {
                                        children: Some(tip),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }}
                            },
                            None => proa_macros::html_sync! {
                                <span>{tile.label.as_str()}</span>
                            },
                        }}
                    </div>
                    <div class={value_style}>{tile.value.as_str()}</div>
                    <div class="text-[10px] text-ink4 mt-[2px]">{tile.sub.as_str()}</div>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Neighbourhood graph
// ---------------------------------------------------------------------------

const CX: f64 = 480.0;
const CY: f64 = 186.0;
const R1: f64 = 106.0;

/// Quantize an SVG coordinate to two decimals. Trig results differ by an
/// ulp across platform libm implementations, and shortest-roundtrip `f64`
/// formatting turns that ulp into different strings; rounding first makes the
/// rendered document byte-identical everywhere.
fn q(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

struct RingNode {
    x: f64,
    y: f64,
    label: String,
    transit: bool,
    lx: f64,
    ly: f64,
    anchor: &'static str,
    edge_style: String,
}

pub struct NeighbourhoodGraph {
    pub query: String,
}

impl<L: DataLoader> proa_core::WebRender<L> for NeighbourhoodGraph {
    // proa-perf-ok(scope): guided demo page; the per-request view model and
    // its owned strings are intentional and sized to one lookup.
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let view = view(cx, &self.query).await;
            let (asn, operator, ring_input, live_total): (
                String,
                String,
                Vec<(String, bool)>,
                Option<usize>,
            ) = match &view {
                DossierView::Live(dossier) => (
                    // proa-perf-ok: guided demo page; small per-request strings.
                    format!("AS{}", dossier.subject.asn),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    dossier.display_name().to_string(),
                    {
                        let mut sorted: Vec<_> = dossier.neighbours.iter().collect(); // proa-perf-ok: guided demo page; small per-request work.
                        sorted.sort_by_key(|n| std::cmp::Reverse(n.v4_peers + n.v6_peers));
                        sorted
                            .iter()
                            .take(8)
                            // proa-perf-ok: guided demo page; small per-request strings.
                            .map(|n| (n.asn.to_string(), n.relation == "left"))
                            // proa-perf-ok: guided demo page; small per-request strings.
                            .collect()
                    },
                    Some(dossier.neighbours.len()),
                ),
                DossierView::Curated(site) => (
                    // proa-perf-ok: guided demo page; small per-request strings.
                    site.asn.to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    site.operator.to_string(),
                    crate::model::TOPO_RING
                        .iter()
                        .filter(|(short, _)| *short != site.asn_short)
                        .take(8)
                        // proa-perf-ok: guided demo page; small per-request strings.
                        .map(|(short, transit)| (short.to_string(), *transit))
                        // proa-perf-ok: guided demo page; small per-request strings.
                        .collect(),
                    None,
                ),
                DossierView::Unresolvable => (
                    "—".to_string(), // proa-perf-ok: guided demo page; small per-request strings.
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "Name not found".to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    Vec::new(),
                    None,
                ),
                DossierView::Offline => (
                    "—".to_string(), // proa-perf-ok: guided demo page; small per-request strings.
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "RIPEstat unreachable".to_string(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    Vec::new(),
                    None,
                ),
            };
            // proa-perf-ok: guided demo page; small per-request strings.
            let headline = match live_total {
                // proa-perf-ok: guided demo page; small per-request work.
                Some(total) => format!(
                    "Neighbourhood · {} · {} of {} adjacent networks, nearest by RIS sessions",
                    asn,
                    ring_input.len(),
                    total
                ),
                // proa-perf-ok: guided demo page; small per-request work.
                None => format!("Neighbourhood · every network within two hops of {asn}"),
            };
            let (legend_upstream, legend_downstream) = if live_total.is_some() {
                ("Upstream in paths", "Downstream in paths")
            } else {
                ("Paid transit", "Direct peer")
            };
            // proa-perf-ok(scope): graph geometry is a few dozen formatted
            // floats built once per request for a guided demo page.
            let ring: Vec<RingNode> = ring_input
                .iter()
                .enumerate()
                .map(|(i, (short, transit))| {
                    let a = (100.0 + i as f64 * 45.0) * std::f64::consts::PI / 180.0;
                    let (sin, cos) = a.sin_cos();
                    let lr = R1 + 18.0;
                    RingNode {
                        x: q(CX + cos * R1),
                        y: q(CY + sin * R1),
                        // proa-perf-ok: guided demo page; small per-request strings.
                        label: format!("AS{short}"),
                        transit: *transit,
                        lx: q(CX + cos * lr),
                        ly: q(CY + sin * lr + 4.0),
                        anchor: if cos > 0.3 { "start" } else if cos < -0.3 { "end" } else { "middle" },
                        // proa-perf-ok: guided demo page; small per-request strings.
                        edge_style: format!(
                            "stroke: var(--cyan, #3DE1E8); animation: org-dash {:.2}s linear infinite;",
                            1.1 + i as f64 * 0.14
                        ),
                    }
                })
                // proa-perf-ok: guided demo page; small per-request strings.
                .collect();
            // proa-perf-ok: guided demo page; small per-request strings.
            let mut outer = Vec::with_capacity(22);
            for i in 0..22usize {
                if ring.is_empty() {
                    break;
                }
                let p = &ring[i % ring.len()];
                let a = (100.0 + (i % ring.len()) as f64 * 45.0) * std::f64::consts::PI / 180.0
                    + ((((i * 37) % 11) as f64) - 5.0) * 0.055;
                let r = 158.0 + ((i * 13) % 5) as f64 * 7.0;
                // proa-perf-ok: guided demo page; small per-request strings.
                outer.push((p.x, p.y, q(CX + a.cos() * r), q(CY + a.sin() * r)));
            }
            let entry = ring
                .iter()
                .min_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            let route_points = entry
                .map(|entry| {
                    // proa-perf-ok: guided demo page; small per-request strings.
                    format!(
                        "46,206 216,224 {:.0},{:.0} {:.0},{:.0}",
                        entry.x, entry.y, CX, CY
                    )
                })
                // proa-perf-ok: guided demo page; small per-request strings.
                .unwrap_or_else(|| "46,206 216,224 480,186".to_string());
            let entry_caption = entry
                .map(|entry| {
                    let short = entry.label.trim_start_matches("AS");
                    let name = carrier_name(short);
                    if name.is_empty() {
                        entry.label.clone()
                    } else {
                        // proa-perf-ok: guided demo page; small per-request strings.
                        format!("{} {}", entry.label, name)
                    }
                })
                // proa-perf-ok: guided demo page; small per-request strings.
                .unwrap_or_else(|| "upstream".to_string());
            // proa-perf-ok: guided demo page; small per-request strings.
            let caption = format!("you → AS7922 Comcast → {entry_caption} → {asn} {operator}");
            // proa-perf-ok: guided demo page; small per-request strings.
            let aria = format!("{asn} neighbour topology");
            WebRenderSync::render(
                html_sync! {
                    <div
                        data-island="TopologyGraph"
                        data-neighbourhood
                        class="relative border-b border-line"
                    >
                        <div class="flex items-baseline justify-between py-[13px] px-[22px] border-b border-line">
                            <span class="shrink-0 whitespace-nowrap text-[10px] tracking-[0.18em] text-ink3 uppercase">
                                {headline.as_str()}
                            </span>
                            <span class="shrink-0 whitespace-nowrap text-[10px] text-ink4">
                                "with one route from an end user threaded through it"
                            </span>
                        </div>
                        <svg
                            viewBox="0 0 960 400"
                            width="100%"
                            height="400"
                            class="block"
                            role="img"
                            aria-label={aria.as_str()}
                        >
                            {Graph {
                                ring: &ring,
                                outer: &outer,
                                route_points: route_points.as_str(),
                                asn: asn.as_str(),
                            }}
                        </svg>
                        <div class="flex items-baseline gap-[14px] py-[11px] px-[22px] border-t border-line">
                            <span class="shrink-0 text-[10px] tracking-[0.14em] text-brand uppercase">
                                "Your route"
                            </span>
                            <span class="shrink-0 whitespace-nowrap text-[12px] text-ink">
                                {caption.as_str()}
                            </span>
                            <span class="shrink min-w-0 text-[10px] text-ink4">
                                "AS7922 stands in for your own network, which the browser can't see"
                            </span>
                        </div>
                        <div class="flex gap-[22px] py-[11px] px-[22px] border-t border-line text-[10px] tracking-[0.12em] text-ink4 uppercase">
                            <span class="flex items-center gap-[7px]">
                                <span class="w-[10px] h-[2px] bg-brand"></span>
                                "Your route"
                            </span>
                            <span class="flex items-center gap-[7px]">
                                <span class="w-[8px] h-[8px] rounded-[50%] bg-brand"></span>
                                {legend_upstream}
                            </span>
                            <span class="flex items-center gap-[7px]">
                                <span class="w-[8px] h-[8px] rounded-[50%] bg-cyan"></span>
                                {legend_downstream}
                            </span>
                            <span class="flex items-center gap-[7px]">
                                <span class="w-[6px] h-[6px] rounded-[50%] bg-ink3"></span>
                                "One hop further out"
                            </span>
                        </div>
                    </div>
                },
                cx,
            )
        })
    }
}

struct Graph<'a> {
    ring: &'a [RingNode],
    outer: &'a [(f64, f64, f64, f64)],
    route_points: &'a str,
    asn: &'a str,
}
impl<L: DataLoader> WebRenderSync<L> for Graph<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for (x1, y1, x2, y2) in self.outer {
            html_sync! {
                <line
                    x1={*x1}
                    y1={*y1}
                    x2={*x2}
                    y2={*y2}
                    stroke="var(--ink, #E5E1D6)"
                    stroke-width="1"
                    stroke-opacity="0.12"
                />
            }
            .render(cx)?;
        }
        for node in self.ring {
            let style = node.edge_style.as_str();
            html_sync! {
                <line
                    x1="480"
                    y1="186"
                    x2={node.x}
                    y2={node.y}
                    stroke-width="1.2"
                    stroke-opacity="0.55"
                    stroke-dasharray="3 6"
                    style={proa_core::style_ok(style)}
                />
            }
            .render(cx)?;
        }
        for (_, _, x, y) in self.outer {
            html_sync! {
                <circle cx={*x} cy={*y} r="2.5" fill="var(--ink3, #8C887E)" fill-opacity="0.75" />
            }
            .render(cx)?;
        }
        for node in self.ring {
            let fill = if node.transit {
                "fill: var(--brand, #DD7627);"
            } else {
                "fill: var(--cyan, #3DE1E8);"
            };
            html_sync! {
                <circle cx={node.x} cy={node.y} r="5" style={proa_core::style_ok(fill)} />
            }
            .render(cx)?;
        }
        for node in self.ring {
            html_sync! {
                <text
                    x={node.lx}
                    y={node.ly}
                    fill="var(--ink2, #A8A49A)"
                    font-family="Azeret Mono, monospace"
                    font-size="11"
                    text-anchor={node.anchor}
                >
                    {node.label.as_str()}
                </text>
            }
            .render(cx)?;
        }
        let points = self.route_points;
        html_sync! {
            <polyline
                points={points}
                fill="none"
                stroke="var(--bg, #0B0B0C)"
                stroke-width="6"
                stroke-opacity="0.85"
            />
            <polyline points={points} fill="none" stroke="var(--ink, #E5E1D6)" stroke-width="1.8" />
            <polyline
                points={points}
                fill="none"
                stroke-width="1.8"
                stroke-dasharray="5 9"
                style="stroke: var(--brand, #DD7627); animation: org-dash 1.1s linear infinite;"
            />
            <circle cx="216" cy="224" r="5" fill="var(--ink, #E5E1D6)" />
            <text
                x="216"
                y="248"
                fill="var(--ink2, #A8A49A)"
                font-family="Azeret Mono, monospace"
                font-size="11"
                text-anchor="middle"
            >
                "AS7922 Comcast"
            </text>
            <circle cx="46" cy="206" r="7" style="fill: var(--brand, #DD7627);" />
            <text
                x="46"
                y="188"
                fill="var(--ink, #E5E1D6)"
                font-family="Instrument Sans, sans-serif"
                font-size="13"
                font-weight="700"
                text-anchor="middle"
                letter-spacing="1"
            >
                "YOU"
            </text>
            <circle
                cx="480"
                cy="186"
                r="44"
                fill="none"
                stroke-opacity="0.25"
                style="stroke: var(--cyan, #3DE1E8);"
            />
            <circle
                cx="480"
                cy="186"
                r="44"
                fill="none"
                stroke-opacity="0.6"
                style="stroke: var(--cyan, #3DE1E8); animation: org-breathe 2.6s ease-in-out infinite;"
            />
            <circle cx="480" cy="186" r="11" style="fill: var(--cyan, #3DE1E8);" />
            <text
                x="480"
                y="256"
                fill="var(--ink, #E5E1D6)"
                font-family="Instrument Sans, sans-serif"
                font-size="19"
                font-weight="700"
                text-anchor="middle"
            >
                {self.asn}
            </text>
        }
        .render(cx)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The three data panes
// ---------------------------------------------------------------------------

struct RangeRow {
    prefix: String,
    state: &'static str,
    state_style: &'static str,
    since: String,
}

pub struct RangesPane {
    pub query: String,
}

impl<L: DataLoader> proa_core::WebRender<L> for RangesPane {
    // proa-perf-ok(scope): guided demo page; the per-request view model and
    // its owned strings are intentional and sized to one lookup.
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let view = view(cx, &self.query).await;
            let overlay = match &view {
                DossierView::Live(dossier) => {
                    dossier.subject.domain.as_deref().and_then(curated_overlay)
                }
                DossierView::Curated(site) => Some(*site),
                DossierView::Unresolvable | DossierView::Offline => None,
            };
            let contested = overlay
                .map(|site| {
                    site.prefixes
                        .iter()
                        .filter(|p| p.contested())
                        .map(|p| p.prefix)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let (rows, count): (Vec<RangeRow>, String) = match &view {
                DossierView::Live(dossier) => (
                    dossier
                        .prefixes
                        .iter()
                        .take(14)
                        .map(|p| {
                            let is_contested = contested.iter().any(|c| *c == p.prefix);
                            RangeRow {
                                prefix: p.prefix.clone(),
                                state: if is_contested { "CONTESTED" } else { "VALID" },
                                state_style: if is_contested {
                                    state::RANGE_CONTESTED
                                } else {
                                    state::RANGE_VALID
                                },
                                // proa-perf-ok: guided demo page; small per-request strings.
                                since: p.since.chars().take(10).collect(),
                            }
                        })
                        // proa-perf-ok: guided demo page; small per-request strings.
                        .collect(),
                    if dossier.prefixes.is_empty() {
                        // proa-perf-ok: guided demo page; small per-request strings.
                        "list unavailable".to_string()
                    } else {
                        // proa-perf-ok: guided demo page; small per-request strings.
                        format!("{} records", dossier.prefixes.len())
                    },
                ),
                DossierView::Curated(site) => (
                    site.prefixes
                        .iter()
                        .map(|p| RangeRow {
                            // proa-perf-ok: guided demo page; small per-request strings.
                            prefix: p.prefix.to_string(),
                            state: if p.contested() { "CONTESTED" } else { "VALID" },
                            state_style: if p.contested() {
                                state::RANGE_CONTESTED
                            } else {
                                state::RANGE_VALID
                            },
                            // proa-perf-ok: guided demo page; small per-request strings.
                            since: p.since.to_string(),
                        })
                        // proa-perf-ok: guided demo page; small per-request strings.
                        .collect(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    site.prefix_count.to_string(),
                ),
                // proa-perf-ok: guided demo page; small per-request strings.
                // proa-perf-ok: guided demo page; small per-request strings.
                DossierView::Unresolvable => (Vec::new(), "name not found".to_string()),
                DossierView::Offline => (Vec::new(), "unavailable".to_string()), // proa-perf-ok: guided demo page; small per-request strings.
            };
            WebRenderSync::render(
                html_sync! {
                    <div
                        data-stream="Suspense: ranges"
                        data-ranges-pane
                        class="relative border-r border-line"
                    >
                        <div class="flex items-baseline justify-between gap-[10px] box-border py-[13px] px-[20px] border-b border-line">
                            {PaneTitle {
                                label: "Announced ranges",
                                tip: "Every block of addresses this network currently tells the internet it can deliver traffic to. Large networks announce thousands; a small one may announce a single block. Observed dates are relative to the RIS query window, not the range's true age.",
                            }}
                            <span class={base::PANE_COUNT}>{count.as_str()}</span>
                        </div>
                        <div class="grid grid-cols-[minmax(112px,1fr)_68px_72px] gap-[10px] box-border py-[7px] px-[14px] text-[9px] tracking-[0.14em] text-ink4 uppercase border-b border-line bg-bg2">
                            <div>"Range"</div>
                            <div>"Ownership"</div>
                            <div class="text-right">"Observed"</div>
                        </div>
                        <div class="max-h-[420px] overflow-y-auto">{RangeRows { rows: &rows }}</div>
                    </div>
                },
                cx,
            )
        })
    }
}

struct RangeRows<'a> {
    rows: &'a [RangeRow],
}
impl<L: DataLoader> WebRenderSync<L> for RangeRows<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for row in self.rows {
            html_sync! {
                <div class="hv-row grid grid-cols-[minmax(112px,1fr)_68px_72px] gap-[10px] box-border py-[7px] px-[14px] border-b border-line-faint text-[11px]">
                    <span class="text-ink">{row.prefix.as_str()}</span>
                    <span class={row.state_style}>{row.state}</span>
                    <span class="text-ink4 text-right whitespace-nowrap">{row.since.as_str()}</span>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct CarrierRow {
    asn: String,
    holder: String,
    rel: &'static str,
    rel_style: &'static str,
}

pub struct CarriersPane {
    pub query: String,
}

impl<L: DataLoader> proa_core::WebRender<L> for CarriersPane {
    // proa-perf-ok(scope): guided demo page; the per-request view model and
    // its owned strings are intentional and sized to one lookup.
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let view = view(cx, &self.query).await;
            let (rows, count): (Vec<CarrierRow>, String) = match &view {
                DossierView::Live(dossier) => {
                    // Direction comes from where a neighbour sits in observed
                    // AS paths, which is not a business relationship, so the
                    // badges say upstream/downstream rather than transit/peer.
                    let mut sorted: Vec<_> = dossier.neighbours.iter().collect(); // proa-perf-ok: guided demo page; small per-request work.
                    sorted.sort_by_key(|n| std::cmp::Reverse(n.v4_peers + n.v6_peers));
                    (
                        sorted
                            .iter()
                            .take(14)
                            .map(|n| {
                                // proa-perf-ok: guided demo page; small per-request strings.
                                let name = carrier_name(&n.asn.to_string());
                                CarrierRow {
                                    // proa-perf-ok: guided demo page; small per-request strings.
                                    asn: format!("AS{}", n.asn),
                                    holder: if name.is_empty() {
                                        // proa-perf-ok: guided demo page; small per-request strings.
                                        format!("seen by {} RIS peers", n.v4_peers + n.v6_peers)
                                    } else {
                                        // proa-perf-ok: guided demo page; small per-request strings.
                                        name.to_string()
                                    },
                                    rel: match n.relation.as_str() {
                                        "left" => "UPSTREAM",
                                        "right" => "DOWNSTREAM",
                                        _ => "ADJACENT",
                                    },
                                    rel_style: if n.relation == "left" {
                                        state::REL_TRANSIT
                                    } else {
                                        state::REL_PEER
                                    },
                                }
                            })
                            // proa-perf-ok: guided demo page; small per-request strings.
                            .collect(),
                        // proa-perf-ok: guided demo page; small per-request strings.
                        format!(
                            "{} of {} neighbours",
                            dossier.neighbours.len().min(14),
                            dossier.neighbours.len()
                        ),
                    )
                }
                DossierView::Curated(site) => (
                    crate::model::PEERS
                        .iter()
                        .filter(|(asn, _, _)| *asn != site.asn)
                        .take(14)
                        .map(|(asn, holder, transit)| CarrierRow {
                            // proa-perf-ok: guided demo page; small per-request strings.
                            asn: asn.to_string(),
                            // proa-perf-ok: guided demo page; small per-request strings.
                            holder: holder.to_string(),
                            rel: if *transit { "TRANSIT" } else { "PEER" },
                            rel_style: if *transit {
                                state::REL_TRANSIT
                            } else {
                                state::REL_PEER
                            },
                        })
                        // proa-perf-ok: guided demo page; small per-request strings.
                        .collect(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    site.peer_count.to_string(),
                ),
                // proa-perf-ok: guided demo page; small per-request strings.
                // proa-perf-ok: guided demo page; small per-request strings.
                DossierView::Unresolvable => (Vec::new(), "name not found".to_string()),
                DossierView::Offline => (Vec::new(), "unavailable".to_string()), // proa-perf-ok: guided demo page; small per-request strings.
            };
            WebRenderSync::render(
                html_sync! {
                    <div
                        data-stream="Suspense: neighbours"
                        data-carriers-pane
                        class="relative border-r border-line"
                    >
                        <div class="flex items-baseline justify-between gap-[10px] box-border py-[13px] px-[20px] border-b border-line">
                            {PaneTitle {
                                label: "Who carries it",
                                tip: "The networks this one connects to directly, sorted by how many RIS peers observe the adjacency. Upstream and downstream describe where the neighbour sits in observed AS paths; the data cannot see contracts, so no transit-versus-peering claim is made.",
                            }}
                            <span class={base::PANE_COUNT}>{count.as_str()}</span>
                        </div>
                        <div class="max-h-[420px] overflow-y-auto">
                            {CarrierRows { rows: &rows }}
                        </div>
                    </div>
                },
                cx,
            )
        })
    }
}

struct CarrierRows<'a> {
    rows: &'a [CarrierRow],
}
impl<L: DataLoader> WebRenderSync<L> for CarrierRows<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for row in self.rows {
            html_sync! {
                <div class="hv-row grid grid-cols-[74px_minmax(0,1fr)_62px] gap-[10px] box-border py-[7px] px-[20px] border-b border-line-faint text-[11px]">
                    <span class="text-cyan">{row.asn.as_str()}</span>
                    <span class="text-ink2 overflow-hidden text-ellipsis whitespace-nowrap">
                        {row.holder.as_str()}
                    </span>
                    <span class={row.rel_style}>{row.rel}</span>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct PathRow {
    collector: String,
    peers: String,
    hops: Vec<String>,
    origin: String,
}

pub struct PathsPane {
    pub query: String,
}

impl<L: DataLoader> proa_core::WebRender<L> for PathsPane {
    // proa-perf-ok(scope): guided demo page; the per-request view model and
    // its owned strings are intentional and sized to one lookup.
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let view = view(cx, &self.query).await;
            let (rows, count): (Vec<PathRow>, String) = match &view {
                DossierView::Live(dossier) => {
                    // One route per collector, prepends collapsed, vantage
                    // points named after their RRC location.
                    let mut seen_collectors: Vec<&str> = Vec::new(); // proa-perf-ok: guided demo page; small per-request work.
                    let mut selected = Vec::new(); // proa-perf-ok: guided demo page; small per-request work.
                    for path in &dossier.paths {
                        let rrc = path.collector.split('-').next().unwrap_or("");
                        if seen_collectors.contains(&rrc) {
                            continue;
                        }
                        seen_collectors.push(rrc); // proa-perf-ok: guided demo page; small per-request work.
                        selected.push(path); // proa-perf-ok: guided demo page; small per-request work.
                        if selected.len() == 8 {
                            break;
                        }
                    }
                    (
                        selected
                            .iter()
                            .map(|path| {
                                let mut hops = crate::model::format_hops(&path.hops);
                                let origin = hops.pop().unwrap_or_default();
                                PathRow {
                                    // proa-perf-ok: guided demo page; small per-request strings.
                                    collector: crate::model::collector_label(&path.collector),
                                    // proa-perf-ok: guided demo page; small per-request strings.
                                    peers: String::new(),
                                    hops,
                                    origin,
                                }
                            })
                            // proa-perf-ok: guided demo page; small per-request strings.
                            .collect(),
                        // proa-perf-ok: guided demo page; small per-request strings.
                        format!("{} observed", dossier.paths.len()),
                    )
                }
                DossierView::Curated(site) => (
                    crate::model::PATH_SHAPES
                        .iter()
                        .enumerate()
                        .map(|(index, shape)| PathRow {
                            collector: crate::model::COLLECTORS
                                [index % crate::model::COLLECTORS.len()]
                            // proa-perf-ok: guided demo page; small per-request strings.
                            .to_string(),
                            peers: String::new(), // proa-perf-ok: guided demo page; small per-request strings.
                            // proa-perf-ok: guided demo page; small per-request strings.
                            hops: shape.iter().map(|s| s.to_string()).collect(),
                            // proa-perf-ok: guided demo page; small per-request strings.
                            origin: site.asn_short.to_string(),
                        })
                        // proa-perf-ok: guided demo page; small per-request strings.
                        .collect(),
                    // proa-perf-ok: guided demo page; small per-request strings.
                    "8 observed".to_string(),
                ),
                // proa-perf-ok: guided demo page; small per-request strings.
                // proa-perf-ok: guided demo page; small per-request strings.
                DossierView::Unresolvable => (Vec::new(), "name not found".to_string()),
                DossierView::Offline => (Vec::new(), "unavailable".to_string()), // proa-perf-ok: guided demo page; small per-request strings.
            };
            WebRenderSync::render(
                html_sync! {
                    <div data-stream="Suspense: routes" data-paths-pane class="relative">
                        <div class="flex items-baseline justify-between gap-[10px] box-border py-[13px] px-[20px] border-b border-line">
                            {PaneTitle {
                                label: "Routes traffic takes",
                                tip: "The actual chains of networks that announcements travelled through to reach a collector. Several routes to the same place normally exist — traffic takes whichever is shortest at the time.",
                            }}
                            <span class={base::PANE_COUNT}>{count.as_str()}</span>
                        </div>
                        <div class="py-[12px] px-[20px]">{PathRowsView { rows: &rows }}</div>
                    </div>
                },
                cx,
            )
        })
    }
}

struct PathRowsView<'a> {
    rows: &'a [PathRow],
}
impl<L: DataLoader> WebRenderSync<L> for PathRowsView<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for row in self.rows {
            html_sync! {
                <div class="py-[8px] px-0 border-b border-line-faint">
                    <div class="flex items-center gap-[6px] flex-wrap text-[11px]">
                        {Hops { hops: &row.hops, origin: row.origin.as_str() }}
                    </div>
                    <div class="flex justify-between text-[9px] tracking-[0.12em] text-ink4 uppercase mt-[5px]">
                        <span>{row.collector.as_str()}</span>
                        <span>{row.peers.as_str()}</span>
                    </div>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct Hops<'a> {
    hops: &'a [String],
    origin: &'a str,
}
impl<L: DataLoader> WebRenderSync<L> for Hops<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for hop in self.hops {
            html_sync! {
                <span class={state::HOP}>"→ "{hop.as_str()}</span>
            }
            .render(cx)?;
        }
        html_sync! {
            <span class={state::HOP_ORIGIN}>"→ "{self.origin}</span>
        }
        .render(cx)?;
        Ok(())
    }
}
