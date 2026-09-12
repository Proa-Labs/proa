// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
use rsjs::{rsjs, signal};

use crate::model::RESOLVED;

/// The "Across the internet" narrated feed, the raw route tape behind it, and
/// the rate rail. The row engines are progressive client behavior in
/// origin.js; the controls are the `TapeControls` rsjs island, which talks to
/// the engine through document events.
pub struct LiveWire;

impl<L: DataLoader> WebRenderSync<L> for LiveWire {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        // proa-perf-ok(scope): chart paths and spark bars are built once per
        // request for a guided demo page.
        let (vol_line_a, vol_line_w, vol_area_a) = volume_paths();
        let spark = spark_styles();
        html_sync! {
            <section
                data-live-wire
                class="grid grid-cols-[1fr_300px] border-b border-line"
            >
                <div class="relative border-r border-line">
                    <div class="flex items-center justify-between py-[13px] px-[22px] border-b border-line">
                        <div class="flex items-baseline gap-[14px]">
                            <span class="shrink-0 whitespace-nowrap font-display text-[18px] font-semibold tracking-[-0.02em]">
                                "Across the internet"
                            </span>
                        </div>
                        <div class="flex items-center gap-[14px] text-[10px] tracking-[0.12em] uppercase">
                            <span class="flex items-center gap-[7px] shrink-0 whitespace-nowrap text-ink3">
                                <span
                                    data-feed-dot
                                    class="shrink-0 w-[6px] h-[6px] bg-ink3 animate-[org-pulse_1.6s_ease-in-out_infinite]"
                                ></span>
                                <span data-feed-status>"Connecting to RIS Live…"</span>
                            </span>
                            <span
                                data-clock
                                class="shrink-0 whitespace-nowrap text-ink4"
                            >
                                "00:00:00 UTC"
                            </span>
                            {PauseControl}
                        </div>
                    </div>
                    <div data-events></div>
                    <div
                        id="feed"
                        data-island="LiveRouteTape"
                        data-route-tape
                        class="relative border-t border-line"
                    >
                        <div class="flex items-center justify-between gap-[24px] py-[13px] px-[22px]">
                            <div class="flex items-baseline gap-[14px]">
                                <span class="font-display text-[18px] font-semibold tracking-[-0.02em]">
                                    "Every road sign, as it is posted"
                                </span>
                                <span class="text-[11px] text-ink4">
                                    "the unfiltered stream behind the explanations"
                                </span>
                            </div>
                            {FilterControls}
                        </div>
                        <div
                            data-tape-header
                            class="grid grid-cols-[84px_30px_112px_50px_minmax(0,1fr)_52px] gap-x-[12px] box-border py-[8px] px-[22px] text-[9px] tracking-[0.16em] text-ink4 uppercase border-t border-b border-line bg-bg2"
                        >
                            <div>"Time"</div>
                            <div>"Type"</div>
                            <div>"Address range"</div>
                            <div>"Origin"</div>
                            <div>"Route"</div>
                            <div class="text-right">"Seen at"</div>
                        </div>
                        <div data-tape class="h-[340px] overflow-hidden"></div>
                    </div>
                </div>
                <aside>
                    <div class="py-[17px] px-[20px] border-b border-line">
                        <div class="whitespace-nowrap text-[9px] tracking-[0.18em] text-ink4 uppercase">
                            "Changes per minute · every RIS peer"
                        </div>
                        <div class="flex items-baseline gap-[10px] mt-[10px]">
                            <span
                                data-rate-announce
                                class="font-display text-[40px] font-semibold tracking-[-0.04em] text-cyan"
                            >
                                "—"
                            </span>
                            <span class="text-[10px] tracking-[0.12em] text-ink4 uppercase">
                                "announcements seen"
                            </span>
                        </div>
                        <div class="flex items-baseline gap-[10px] mt-[2px]">
                            <span
                                data-rate-withdraw
                                class="font-display text-[40px] font-semibold tracking-[-0.04em] text-red"
                            >
                                "—"
                            </span>
                            <span class="text-[10px] tracking-[0.12em] text-ink4 uppercase">
                                "withdrawals seen"
                            </span>
                        </div>
                        <div
                            data-spark
                            class="flex gap-[2px] items-end h-[42px] mt-[16px]"
                        >
                            {SparkBars { styles: &spark }}
                        </div>
                    </div>
                    <div class="py-[17px] px-[20px] border-b border-line">
                        <div class="text-[9px] tracking-[0.18em] text-ink4 uppercase mb-[12px]">
                            "Volume · typical day, illustrative"
                        </div>
                        <svg
                            viewBox="0 0 260 88"
                            width="100%"
                            height="88"
                            class="block"
                            role="img"
                            aria-label="Route change volume over 24 hours"
                        >
                            <line
                                x1="0"
                                y1="87"
                                x2="260"
                                y2="87"
                                stroke="var(--ink, #E5E1D6)"
                                stroke-opacity="0.12"
                            />
                            <path
                                d={vol_area_a.as_str()}
                                fill-opacity="0.14"
                                style="fill: var(--cyan, #3DE1E8);"
                            />
                            <path
                                d={vol_line_a.as_str()}
                                fill="none"
                                stroke-width="1.4"
                                style="stroke: var(--cyan, #3DE1E8);"
                            />
                            <path
                                d={vol_line_w.as_str()}
                                fill="none"
                                stroke-width="1.2"
                                stroke-opacity="0.85"
                                style="stroke: var(--red, #FF3B30);"
                            />
                        </svg>
                        <div class="flex justify-between text-[9px] tracking-[0.12em] text-ink5 uppercase mt-[5px]">
                            <span>"00:00"</span>
                            <span>"12:00"</span>
                            <span>"now"</span>
                        </div>
                    </div>
                    <div class="py-[17px] px-[20px]">
                        <div class="text-[9px] tracking-[0.18em] text-ink4 uppercase mb-[12px]">
                            "Recently resolved · replay"
                        </div>
                        {ResolvedItems}
                    </div>
                </aside>
            </section>
        }
        .render(cx)
    }
}

/// Tape filters: an rsjs island in the road-sign header that broadcasts the
/// selection to the plain-JS row engine through document events.
struct FilterControls;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for FilterControls {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <span data-tape-controls class="flex items-center gap-[8px]">
                <button
                    type="button"
                    data-tape-filter="all"
                    on_click={|_| rsjs::document::dispatch_event_with_detail("origin:filter", "all")}
                    class="border border-ink bg-transparent [font-family:inherit] text-ink py-[5px] px-[12px] text-[10px] tracking-[0.14em] uppercase cursor-pointer"
                >
                    "All"
                </button>
                <button
                    type="button"
                    data-tape-filter="new"
                    on_click={|_| rsjs::document::dispatch_event_with_detail("origin:filter", "new")}
                    class="border border-line-soft bg-transparent [font-family:inherit] text-ink3 py-[5px] px-[12px] text-[10px] tracking-[0.14em] uppercase cursor-pointer"
                >
                    "New"
                </button>
                <button
                    type="button"
                    data-tape-filter="removed"
                    on_click={|_| rsjs::document::dispatch_event_with_detail("origin:filter", "removed")}
                    class="border border-line-soft bg-transparent [font-family:inherit] text-ink3 py-[5px] px-[12px] text-[10px] tracking-[0.14em] uppercase cursor-pointer"
                >
                    "Removed"
                </button>
            </span>
        }
        .render(cx)
    }
}

/// Pause toggle for both feeds, next to the clock.
struct PauseControl;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for PauseControl {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let paused = signal(false);
        html_sync! {
            <button
                type="button"
                class="hv-cyan border border-line-strong bg-transparent [font-family:inherit] py-[4px] px-[11px] tracking-[0.14em] cursor-pointer text-ink text-[10px] uppercase"
                data-feed-pause
                on_click={|_| { paused.update(|p| !p); rsjs::document::dispatch_event_with_detail("origin:pause", "toggle"); }}
            >
                {if paused.get() { html_sync! {
                    <span>"Resume"</span>
                } } else { html_sync! {
                    <span>"Pause"</span>
                } }}
            </button>
        }
        .render(cx)
    }
}

struct SparkBars<'a> {
    styles: &'a [String],
}
impl<L: DataLoader> WebRenderSync<L> for SparkBars<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for style in self.styles {
            html_sync! {
                <div style={proa_core::style_ok(style.as_str())}></div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct ResolvedItems;
impl<L: DataLoader> WebRenderSync<L> for ResolvedItems {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for (what, when) in RESOLVED {
            html_sync! {
                <div class="border-l-2 border-line-mid pt-[3px] pr-0 pb-[5px] pl-[10px] mb-[13px]">
                    <div class="text-[11px] text-ink leading-[1.5]">{*what}</div>
                    <div class="text-[10px] text-ink4 mt-[3px]">{*when}</div>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

// proa-perf-ok(scope): three short strings built once per request.
fn volume_paths() -> (String, String, String) {
    let mut line_a = String::new();
    let mut line_w = String::new();
    for i in 0..45u32 {
        let t = f64::from(i) / 44.0;
        let a = 0.42
            + 0.3 * (t * 6.1).sin()
            + 0.12 * (t * 19.0).sin()
            + if i == 29 { 0.32 } else { 0.0 };
        let w = 0.14 + 0.09 * (t * 7.4 + 1.0).sin() + if i == 29 { 0.4 } else { 0.0 };
        let a = a.clamp(0.05, 1.0);
        let w = w.clamp(0.03, 1.0);
        let x = (f64::from(i) / 44.0) * 260.0;
        let command = if i == 0 { "M" } else { " L" };
        let _ = write!(line_a, "{command}{x:.1},{:.1}", 87.0 - a * 78.0);
        let _ = write!(line_w, "{command}{x:.1},{:.1}", 87.0 - w * 78.0);
    }
    let area_a = format!("{line_a} L260,87 L0,87 Z");
    (line_a, line_w, area_a)
}

// proa-perf-ok(scope): 42 short strings once per request. The bars start as a
// flat, dim gauge; the client engine replaces them with real per-second
// message counts once the RIS Live socket delivers.
fn spark_styles() -> Vec<String> {
    (0..42u32)
        .map(|_| {
            "flex: 1; height: 6%; background: var(--cyan, #3DE1E8); opacity: 0.25;".to_string()
        })
        .collect()
}
