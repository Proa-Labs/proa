// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{
    ClassList, DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;

use crate::data::benchmarks::BenchmarkChartEntry;

const ROOT_CLASS: &str = "rounded-[32px] border border-white/10 bg-white/[0.035] p-4 shadow-[0_35px_100px_-65px_rgba(0,0,0,0.9)] sm:p-6";
const ROOT_STYLE: &str =
    "border:1px solid rgba(255,255,255,0.10);border-radius:32px;background:rgba(255,255,255,0.035);padding:1rem;color:white;";
const CHART_FRAME_STYLE: &str =
    "border:1px solid rgba(255,255,255,0.10);border-radius:26px;background:rgba(255,255,255,0.025);padding:1rem;";
const TRACK_STYLE: &str =
    "height:10px;border-radius:9999px;background:rgba(255,255,255,0.10);overflow:hidden;";
const SELECTOR_CSS: &str = r#"
[data-benchmark-selector] [data-benchmark-panel] {
    display: none;
}
[data-benchmark-tab] {
    border-color: rgba(255, 255, 255, 0.10);
    color: rgba(255, 255, 255, 0.58);
}
#benchmark-tab-airbnb-pages:checked ~ [data-benchmark-tabs] [for="benchmark-tab-airbnb-pages"],
#benchmark-tab-airbnb-tail:checked ~ [data-benchmark-tabs] [for="benchmark-tab-airbnb-tail"] {
    border-color: rgba(255, 255, 255, 0.28);
    background: rgba(255, 255, 255, 0.11);
    color: white;
}
#benchmark-tab-airbnb-pages:focus-visible ~ [data-benchmark-tabs] [for="benchmark-tab-airbnb-pages"],
#benchmark-tab-airbnb-tail:focus-visible ~ [data-benchmark-tabs] [for="benchmark-tab-airbnb-tail"] {
    outline: 2px solid var(--color-brand);
    outline-offset: 2px;
}
#benchmark-tab-airbnb-pages:checked ~ [data-benchmark-panels] [data-benchmark-panel="airbnb-pages"],
#benchmark-tab-airbnb-tail:checked ~ [data-benchmark-panels] [data-benchmark-panel="airbnb-tail"] {
    display: block;
}
"#;

pub struct BenchmarkTypeSelector {
    pub class: Option<&'static str>,
    pub airbnb_page_entries: &'static [BenchmarkChartEntry],
    pub airbnb_tail_entries: &'static [BenchmarkChartEntry],
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for BenchmarkTypeSelector {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let root_classes = ClassList::<2>::new().add(ROOT_CLASS).add_opt(self.class);

        html_sync! {
            <section
                data-benchmark-selector
                class={root_classes}
                style={proa_core::style_ok(ROOT_STYLE)}
            >
                <style>{StaticRaw(SELECTOR_CSS)}</style>
                <fieldset class="contents">
                    <legend class="sr-only">"Benchmark type"</legend>
                    <input
                        class="sr-only"
                        type="radio"
                        id="benchmark-tab-airbnb-pages"
                        name="benchmark-type"
                        value="airbnb-pages"
                        checked="checked"
                    />
                    <input
                        class="sr-only"
                        type="radio"
                        id="benchmark-tab-airbnb-tail"
                        name="benchmark-type"
                        value="airbnb-tail"
                    />
                    <div
                        data-benchmark-tabs
                        class="mb-5 flex w-fit max-w-full flex-nowrap gap-2 overflow-x-auto"
                    >
                        <label
                            data-benchmark-tab
                            for="benchmark-tab-airbnb-pages"
                            class="inline-flex w-fit shrink-0 cursor-pointer items-center justify-center whitespace-nowrap rounded-2xl border px-4 py-2.5 text-center text-xs font-semibold transition-colors"
                        >
                            "Airbnb-scale pages"
                        </label>
                        <label
                            data-benchmark-tab
                            for="benchmark-tab-airbnb-tail"
                            class="inline-flex w-fit shrink-0 cursor-pointer items-center justify-center whitespace-nowrap rounded-2xl border px-4 py-2.5 text-center text-xs font-semibold transition-colors"
                        >
                            "Airbnb home tail latency"
                        </label>
                    </div>
                    <div data-benchmark-panels>
                        <div data-benchmark-panel="airbnb-pages">
                            {BenchmarkChartPanel {
                                description: "Home, listing, and search outputs at realistic page sizes.",
                                entries: self.airbnb_page_entries,
                                caption: "Proa stays below 5 microseconds across 50 KB, 94 KB, and 158 KB outputs.",
                            }}
                        </div>
                        <div data-benchmark-panel="airbnb-tail">
                            {BenchmarkChartPanel {
                                description: "P50 and P99 on the 158 KB home-page output.",
                                entries: self.airbnb_tail_entries,
                                caption: "Proa P99 is 6.50 microseconds; React on Node reaches 399.08 microseconds.",
                            }}
                        </div>
                    </div>
                </fieldset>
            </section>
        }
        .render(cx)
    }
}

struct BenchmarkChartPanel {
    description: &'static str,
    entries: &'static [BenchmarkChartEntry],
    caption: &'static str,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for BenchmarkChartPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <p class="mb-3 text-xs leading-5 text-white/55">{self.description}</p>
            <div style={proa_core::style_ok(CHART_FRAME_STYLE)}>
                <div class="grid gap-3">{ChartRows { entries: self.entries }}</div>
            </div>
            <p class="mt-3 text-xs leading-5 text-white/55">{self.caption}</p>
        }
        .render(cx)
    }
}

struct ChartRows {
    entries: &'static [BenchmarkChartEntry],
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for ChartRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for entry in self.entries {
            ChartRow { entry }.render(cx)?;
        }

        Ok(())
    }
}

struct ChartRow {
    entry: &'static BenchmarkChartEntry,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for ChartRow {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let entry = self.entry;

        html_sync! {
            <div
                class="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] gap-x-3 gap-y-2 md:grid-cols-[10rem_minmax(0,1fr)_8rem_7rem] md:items-center lg:grid-cols-[12rem_minmax(0,1fr)_8rem_8rem]"
                data-benchmark-row
                data-tone={entry.tone.as_str()}
            >
                <div class="min-w-0 break-words text-sm font-medium text-white/80">
                    {entry.name}
                </div>
                <div
                    class="col-span-2 row-start-3 min-w-0 md:col-auto md:row-auto"
                    style={proa_core::style_ok(TRACK_STYLE)}
                    aria-hidden="true"
                >
                    <div style={proa_core::style_ok(entry.width_style)}>
                        <div style={proa_core::style_ok(entry.tone.fill_style())}></div>
                    </div>
                </div>
                <div class="col-start-2 row-start-1 whitespace-nowrap text-right font-mono text-sm text-white tabular-nums md:col-auto md:row-auto md:text-left">
                    {entry.value_label}
                </div>
                <div class="min-w-0 break-words text-xs text-white/45 md:col-auto md:row-auto">
                    {entry.detail}
                </div>
            </div>
        }
        .render(cx)
    }
}
