// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
use rsjs::{rsjs, signal};

use super::recipe::base;

/// The render lab. The buttons are an rsjs island (`FloodControls`) that
/// broadcasts the requested row count; building tens of thousands of DOM rows
/// and reading layout timings has no typed rsjs surface, so the engine lives
/// in origin.js as progressive enhancement.
pub struct FloodLab;

impl<L: DataLoader> WebRenderSync<L> for FloodLab {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <section id="lab" data-flood-lab data-screen-label="Lab">
                <div class="grid grid-cols-[1fr_420px] border-b border-line">
                    <div class="py-[32px] px-[22px] border-r border-line">
                        <div class="text-[10px] tracking-[0.18em] text-ink4 uppercase">
                            "The lab"
                        </div>
                        <h2 class="font-display text-[70px] font-bold tracking-[-0.05em] mt-[14px] mx-0 mb-0 leading-[0.9]">
                            "FLOOD THE"
                            <br />
                            "TABLE"
                        </h2>
                        <p class="max-w-[58ch] mt-[24px] mx-0 mb-0 text-[12px] leading-[1.75] text-ink3 text-pretty">
                            "The internet's routing table holds over a million entries. Pick a row count and two things happen: this server renders that many rows and reports how long it took, and your browser builds the same rows into the DOM. Both numbers are measured live. Nothing is paginated, windowed, or hidden below the fold."
                        </p>
                        {FloodControls}
                    </div>
                    <div class="grid grid-cols-[1fr_1fr]">
                        <div class="p-[20px] border-r border-b border-line">
                            <div class="text-[9px] tracking-[0.16em] text-ink4 uppercase">
                                "Rows rendered"
                            </div>
                            <div data-lab-rows class={base::READOUT_VALUE}>"—"</div>
                            <div class="text-[10px] text-ink4 mt-[2px]">"every one in the DOM"</div>
                        </div>
                        <div class="p-[20px] border-r border-b border-line">
                            <div class="text-[9px] tracking-[0.16em] text-ink4 uppercase">
                                "Server render (measured)"
                            </div>
                            <div data-lab-server class={base::READOUT_VALUE}>"—"</div>
                            <div class="text-[10px] text-ink4 mt-[2px]">
                                "same rows, timed on this server"
                            </div>
                        </div>
                        <div class="p-[20px] border-r border-b border-line">
                            <div class="text-[9px] tracking-[0.16em] text-ink4 uppercase">
                                "Browser · build HTML"
                            </div>
                            <div data-lab-build class={base::READOUT_VALUE}>"—"</div>
                            <div class="text-[10px] text-ink4 mt-[2px]">
                                "same work the server timed"
                            </div>
                        </div>
                        <div class="p-[20px] border-r border-b border-line">
                            <div class="text-[9px] tracking-[0.16em] text-ink4 uppercase">
                                "Browser · build + layout"
                            </div>
                            <div data-lab-paint class={base::READOUT_VALUE}>"—"</div>
                            <div
                                data-lab-bytes
                                class="text-[10px] text-ink4 mt-[2px]"
                            >
                                "nothing rendered yet"
                            </div>
                        </div>
                    </div>
                </div>
                <p
                    data-lab-verdict
                    hidden
                    class="m-0 py-[14px] px-[22px] border-b border-line text-[12px] leading-[1.7] text-ink text-pretty"
                ></p>
                <div data-island="FloodLab" class="relative">
                    <div
                        data-flood-stage
                        class="relative h-[180px] [transition:height_0.25s_ease]"
                    >
                        <div
                            data-flood-table
                            class="absolute inset-0 overflow-y-auto [contain:strict]"
                        ></div>
                        <div
                            data-flood-empty
                            class="absolute inset-0 flex items-center justify-center pointer-events-none text-[11px] tracking-[0.14em] text-ink3 uppercase"
                        >
                            "Pick a row count above to render the table"
                        </div>
                    </div>
                </div>
            </section>
        }
        .render(cx)
    }
}

/// AS numbers from the RFC 6996 private-use range; the lab renders rows, not
/// claims about real networks.
const HOLDERS: [&str; 8] = [
    "64496", "64497", "64498", "64499", "64500", "64501", "64502", "64503",
];

/// The server half of the lab: the same synthetic rows the browser builds,
/// rendered here so `/lab/rows` can report a measured (not modelled) time.
/// Documentation ranges only (RFC 5737 / RFC 3849).
pub struct LabRows {
    pub count: usize,
}

impl<L: DataLoader> WebRenderSync<L> for LabRows {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        // proa-perf-ok(scope): per-row formatting is the workload being
        // measured; the lab times realistic dynamic rendering, not a
        // best-case static path.
        for i in 0..self.count {
            let owner = HOLDERS[i % HOLDERS.len()];
            let range = if i % 6 == 5 {
                format!("2001:db8:{:x}::/48", 1000 + (i * 13) % 8000) // proa-perf-ok: the measured workload itself.
            } else {
                let block = ["192.0.2", "198.51.100", "203.0.113"][i % 3];
                format!("{block}.{}/32", (i * 7) % 256) // proa-perf-ok: the measured workload itself.
            };
            let index = (i + 1).to_string(); // proa-perf-ok: the measured workload itself.
            let path = format!("64496 {owner}"); // proa-perf-ok: the measured workload itself.
            let age = format!("{}d", (i * 7) % 1400); // proa-perf-ok: the measured workload itself.
            html_sync! {
                <div style="display: grid; grid-template-columns: 76px 176px 104px minmax(0, 1fr) 84px; column-gap: 10px; box-sizing: border-box; padding: 4px 22px; border-bottom: 1px solid var(--line-hair, rgba(229,225,214,0.05)); font-size: 11px; white-space: nowrap;">
                    <span style="color: var(--ink5, #4A4844);">{index.as_str()}</span>
                    <span style="color: var(--ink, #E5E1D6);">{range.as_str()}</span>
                    <span style="color: var(--cyan, #3DE1E8);">"AS"{owner}</span>
                    <span style="color: var(--ink3, #8C887E);">{path.as_str()}</span>
                    <span style="color: var(--ink4, #6F6C64); text-align: right;">
                        {age.as_str()}
                    </span>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

/// The lab's control row: an rsjs island that tracks the requested count for
/// assistive tech and hands the actual work to the progressive engine.
struct FloodControls;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for FloodControls {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let requested = signal(0_i64);
        html_sync! {
            <div data-flood-controls class="flex gap-[10px] mt-[28px]">
                <button
                    type="button"
                    class="hv-bg-cyan border border-ink bg-transparent text-ink py-[12px] px-[20px] [font-family:inherit] text-[11px] font-semibold tracking-[0.14em] uppercase cursor-pointer"
                    data-flood-count="1000"
                    on_click={|_| { requested.set(1_000); rsjs::document::dispatch_event_with_detail("origin:flood", "1000"); }}
                >
                    "1,000 routes"
                </button>
                <button
                    type="button"
                    class="hv-bg-cyan border border-ink bg-transparent text-ink py-[12px] px-[20px] [font-family:inherit] text-[11px] font-semibold tracking-[0.14em] uppercase cursor-pointer"
                    data-flood-count="10000"
                    on_click={|_| { requested.set(10_000); rsjs::document::dispatch_event_with_detail("origin:flood", "10000"); }}
                >
                    "10,000 routes"
                </button>
                <button
                    type="button"
                    class="hv-bg-cyan border border-ink bg-transparent text-ink py-[12px] px-[20px] [font-family:inherit] text-[11px] font-semibold tracking-[0.14em] uppercase cursor-pointer"
                    data-flood-count="100000"
                    on_click={|_| { requested.set(100_000); rsjs::document::dispatch_event_with_detail("origin:flood", "100000"); }}
                >
                    "100,000 routes"
                </button>
                <button
                    type="button"
                    class="hv-bg-cyan border border-line-mid bg-transparent text-ink3 py-[12px] px-[18px] [font-family:inherit] text-[11px] tracking-[0.14em] uppercase cursor-pointer"
                    data-flood-clear
                    on_click={|_| { requested.set(0); rsjs::document::dispatch_event_with_detail("origin:flood", "0"); }}
                >
                    "Clear"
                </button>
                <span data-flood-requested hidden aria-live="polite">
                    {requested.get()}
                    " rows requested"
                </span>
            </div>
        }
        .render(cx)
    }
}
