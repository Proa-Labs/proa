// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;

use proa_core::{DataLoader, RenderOutcome, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

use crate::loader::dossier_key;
use crate::model::Dossier;

use super::recipe::base;

/// Wall-clock for one full atlas document render from canned fixture data,
/// measured once at server startup (pure CPU: no network involved). This is
/// the server's own share of a lookup; the upstream RIPEstat fetch is
/// reported separately so seven seconds of somebody else's latency never
/// reads as ours.
static PAGE_RENDER_MICROS: std::sync::OnceLock<u128> = std::sync::OnceLock::new();

pub fn record_page_render(elapsed: std::time::Duration) {
    let _ = PAGE_RENDER_MICROS.set(elapsed.as_micros());
}

// proa-perf-ok(scope): one short string once per request.
fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(character);
    }
    out
}

/// The sticky transmission receipt. Records and fetch time are measured
/// per lookup; the render figure is a startup benchmark over canned data:
/// records and fetch time come from the dossier the loader actually built,
/// and the JavaScript total is summed from the shipped assets.
pub struct Receipt {
    pub query: String,
}

// proa-perf-ok(scope): two short measured strings once per request.
impl<L: DataLoader> proa_core::WebRender<L> for Receipt {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let stats = cx
                .load_typed::<Dossier>(dossier_key(&self.query))
                .await
                .ok()
                .map(|dossier| {
                    // A cache-served lookup did not pay the upstream fetch,
                    // so showing the original latency would be dishonest;
                    // show the cache age instead.
                    let upstream = match dossier.cache_age_seconds {
                        // proa-perf-ok: two short measured per-request strings.
                        Some(age) => format!("cached · {age}s"),
                        // proa-perf-ok: two short measured per-request strings.
                        None => format!("{} ms", dossier.fetch_millis),
                    };
                    (thousands(dossier.records()), upstream)
                });
            let (records, fetch) = match &stats {
                Some((records, fetch)) => (records.as_str(), fetch.as_str()),
                None => ("—", "—"),
            };
            // proa-perf-ok: one short measured string once per request.
            let render = PAGE_RENDER_MICROS
                .get()
                .map(|micros| format!("{:.1} ms", *micros as f64 / 1000.0)); // proa-perf-ok: guided demo page; small per-request work.
            WebRenderSync::render(
                ReceiptShell {
                    records,
                    fetch,
                    render: render.as_deref().unwrap_or("—"),
                },
                cx,
            )
        })
    }
}

/// The zero-data shell, also used as the streaming fallback so the footer
/// never jumps.
pub struct ReceiptFallback;

impl<L: DataLoader> WebRenderSync<L> for ReceiptFallback {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        ReceiptShell {
            records: "—",
            fetch: "—",
            render: "—",
        }
        .render(cx)
    }
}

struct ReceiptShell<'a> {
    records: &'a str,
    fetch: &'a str,
    render: &'a str,
}

impl<L: DataLoader> WebRenderSync<L> for ReceiptShell<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <footer data-receipt class={base::FOOTER}>
                <div class="flex items-center gap-[22px]">
                    <span class="flex items-baseline gap-[7px]">
                        <span data-receipt-records class={base::VALUE}>{self.records}</span>
                        <span class={base::LABEL}>"records"</span>
                    </span>
                    <span class="flex items-baseline gap-[7px]">
                        <span data-receipt-upstream class={base::VALUE}>{self.fetch}</span>
                        <span class={base::LABEL}>"upstream · RIPEstat"</span>
                    </span>
                    <span class="flex items-baseline gap-[7px]">
                        <span data-receipt-render class={base::VALUE}>{self.render}</span>
                        <span class={base::LABEL}>"render · startup bench"</span>
                    </span>
                </div>
                <div class="flex items-center gap-[16px]">
                    <span class="text-ink3 tracking-[0] normal-case font-['Fragment_Mono',monospace] [font-variant-ligatures:none]">
                        "proa new site atlas --template origin"
                    </span>
                    <span class="text-ink6">"·"</span>
                    <a
                        href="https://github.com/Proa-Labs/proa/tree/main/examples/origin-routing-atlas"
                        class="text-ink3 tracking-[0.1em]"
                    >
                        "source"
                    </a>
                    <span class="text-ink6">"·"</span>
                    <a
                        href="https://proa.so/docs"
                        class="hv-cyan flex items-center gap-[8px] text-brand tracking-[0.1em]"
                    >
                        <img
                            src="/static/proa-logo.svg"
                            width="16"
                            height="16"
                            alt=""
                            class="block"
                        />
                        <span>"Proa"</span>
                    </a>
                </div>
            </footer>
        }
        .render(cx)
    }
}
