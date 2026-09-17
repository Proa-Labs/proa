// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::response::Html;
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

use crate::layout::{render_chained_html, PageMeta};

use super::data::{
    ecosystem_data, EcosystemData, EcosystemFilter, EcosystemTool, ToolVersionTone, ToolVisual,
};
use super::layout::{EcosystemMockFooter, EcosystemStandaloneDocument};

pub(crate) fn render() -> Html<String> {
    // proa-perf-ok: top-level Axum response finalization owns the HTML String
    render_chained_html(EcosystemStandaloneDocument {
            meta: PageMeta {
                title: "Ecosystem Developer Tools - Proa",
                description:
                    "Libraries and developer tools for building Proa apps with UI components, typed islands, docs, code generation, and integrations.",
                path: "/ecosystem/developer-tools",
            },
            content: EcosystemDeveloperToolsPage {
                data: ecosystem_data(),
            },
        },
    )
}

struct EcosystemDeveloperToolsPage<'a> {
    data: &'a EcosystemData<'a>,
}

struct ToolFilterButton<'a> {
    filter: &'a EcosystemFilter<'a>,
    active: bool,
}

struct EcosystemToolCard<'a> {
    tool: &'a EcosystemTool<'a>,
}

struct ToolVisualPreview<'a> {
    visual: ToolVisual<'a>,
}

struct ToolVersionBadge<'a> {
    label: &'a str,
    tone: ToolVersionTone,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ToolFilterButton<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let active = if self.active { "true" } else { "false" };
        html_sync! {
            <button
                data-filter={self.filter.key}
                data-active={active}
                class="rounded-full border border-black/10 bg-white/70 px-[14px] py-2 font-mono text-[12px] text-[#57544e] transition-colors hover:border-[#18181b] hover:bg-white hover:text-[#18181b] data-[active=true]:border-[#18181b] data-[active=true]:bg-[#18181b] data-[active=true]:text-white dark:border-white/15 dark:bg-white/[0.06] dark:text-zinc-300 dark:hover:border-[#dd7627] dark:hover:bg-[#dd7627]/10 dark:hover:text-[#ffb070] dark:data-[active=true]:border-[#dd7627] dark:data-[active=true]:bg-[#dd7627] dark:data-[active=true]:text-[#1a0f06]"
                type="button"
            >
                {self.filter.label}
            </button>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ToolVersionBadge<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        match self.tone {
            ToolVersionTone::Default => html_sync! {
                <span class="shrink-0 font-mono text-[10px] text-[#a59f97]">{self.label}</span>
            }
            .render(cx),
            ToolVersionTone::Beta => html_sync! {
                <span class="shrink-0 font-mono text-[10px] text-[#c2841a]">{self.label}</span>
            }
            .render(cx),
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ToolVisualPreview<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        match self.visual {
            ToolVisual::Island { label } => html_sync! {
                <div class="flex h-[150px] flex-col gap-2 bg-[#f3eee6] p-5 text-[#18181b] dark:bg-[#111216] dark:text-zinc-100">
                    <div class="font-mono text-[10px] uppercase tracking-[0.16em] text-[#9b5a24] dark:text-[#ffb070]">
                        {label}
                    </div>
                    <div class="mt-1 rounded-[18px] border border-black/10 bg-white/85 p-3 shadow-[0_16px_42px_-34px_rgba(24,24,27,0.55)] dark:border-white/10 dark:bg-white/[0.06]">
                        <div class="flex items-center justify-between gap-2">
                            <span class="font-mono text-[10px] text-[#7b746b] dark:text-zinc-400">
                                "signal"
                            </span>
                            <span class="rounded-full bg-[#dd7627]/15 px-2 py-0.5 font-mono text-[9px] text-[#9b5a24] dark:text-[#ffb070]">
                                "typed"
                            </span>
                        </div>
                        <div class="mt-3 grid grid-cols-3 gap-1.5">
                            <div class="h-8 rounded-lg bg-[#18181b] dark:bg-[#dd7627]"></div>
                            <div class="h-8 rounded-lg bg-black/[0.08] dark:bg-white/10"></div>
                            <div class="h-8 rounded-lg bg-black/[0.08] dark:bg-white/10"></div>
                        </div>
                    </div>
                </div>
            }
            .render(cx),
            ToolVisual::Components => html_sync! {
                <div class="flex h-[150px] flex-col gap-2 bg-[#f5f3f1] p-5 text-[#18181b] dark:bg-[#111215] dark:text-zinc-100">
                    <div class="font-mono text-[10px] uppercase tracking-[0.16em] text-[#7b746b] dark:text-zinc-400">
                        "proa_ui"
                    </div>
                    <div class="mt-1 flex flex-wrap gap-2">
                        <span class="rounded-md bg-[#18181b] px-2.5 py-1 text-[11px] text-white dark:bg-[#dd7627] dark:text-[#1a0f06]">
                            "Button"
                        </span>
                        <span class="rounded-md border border-[#00000014] bg-white px-2.5 py-1 text-[11px] dark:border-white/10 dark:bg-white/[0.08]">
                            "Dialog"
                        </span>
                        <span class="rounded-md border border-[#00000014] bg-white px-2.5 py-1 text-[11px] dark:border-white/10 dark:bg-white/[0.08]">
                            "Tabs"
                        </span>
                        <span class="rounded-md border border-[#00000014] bg-white px-2.5 py-1 text-[11px] dark:border-white/10 dark:bg-white/[0.08]">
                            "CodeBlock"
                        </span>
                        <span class="rounded-md border border-[#00000014] bg-white px-2.5 py-1 text-[11px] dark:border-white/10 dark:bg-white/[0.08]">
                            "Chart"
                        </span>
                    </div>
                    <div class="mt-auto h-1.5 w-2/3 rounded bg-[#00000010] dark:bg-white/10"></div>
                </div>
            }
            .render(cx),
            ToolVisual::Cli { command, status } => html_sync! {
                <div class="flex h-[150px] flex-col gap-2 bg-[#0d0d0f] p-5 text-[#e6e7ea]">
                    <div class="flex gap-1.5">
                        <span class="h-[7px] w-[7px] rounded-full bg-[#ff5f57]"></span>
                        <span class="h-[7px] w-[7px] rounded-full bg-[#febc2e]"></span>
                        <span class="h-[7px] w-[7px] rounded-full bg-[#28c840]"></span>
                    </div>
                    <div class="mt-1 flex-1 font-mono text-[11px] leading-[1.7]">
                        <span class="text-[#dd7627]">"$"</span>
                        " "
                        {command}
                        <br />
                        <span class="text-[#4ec77f]">"✓"</span>
                        <span class="text-[#ffffff66]">" generated src/pages"</span>
                        <br />
                        <span class="text-[#4ec77f]">"✓"</span>
                        <span class="text-[#ffffff66]">" "{status}</span>
                    </div>
                </div>
            }
            .render(cx),
            ToolVisual::Crate { initial, command } => html_sync! {
                <div class="flex h-[150px] flex-col items-start justify-center gap-2 bg-white p-5 text-[#18181b] dark:bg-[#111215] dark:text-zinc-100">
                    <div class="flex h-12 w-12 items-center justify-center rounded-xl bg-[#f3ece2] font-mono text-[20px] font-semibold">
                        {initial}
                    </div>
                    <div class="mt-2 font-mono text-[11px] text-[#7b746b] dark:text-zinc-400">
                        {command}
                    </div>
                </div>
            }
            .render(cx),
            ToolVisual::DataBinding { label } => html_sync! {
                <div class="flex h-[150px] flex-col gap-2 bg-[#1a1320] p-5 text-[#e8d9f5]">
                    <div class="font-mono text-[10px] uppercase tracking-[0.16em] text-[#c79bf0]">
                        {label}
                    </div>
                    <div class="mt-1 flex-1 font-mono text-[11px] leading-[1.7] text-[#c9b6ff]">
                        "typed input"
                        <br />
                        <span class="text-[#8a6fb8]">"->"</span>
                        " render context"
                        <br />
                        <span class="text-[#8a6fb8]">"->"</span>
                        " HTML response"
                    </div>
                </div>
            }
            .render(cx),
            ToolVisual::Markdown => html_sync! {
                <div class="flex h-[150px] flex-col gap-2 bg-[#eef1f4] bg-[linear-gradient(#16202b08_1px,transparent_1px),linear-gradient(90deg,#16202b08_1px,transparent_1px)] bg-[length:18px_18px] p-5 text-[#16202b]">
                    <div class="font-mono text-[10px] text-[#3b6fb0]">"src/docs/*.mdx"</div>
                    <div class="mt-1 flex flex-1 flex-col gap-2">
                        <div class="h-2.5 w-[80%] rounded bg-[#16202b18]"></div>
                        <div class="h-2.5 w-[60%] rounded bg-[#16202b18]"></div>
                        <div class="h-2.5 w-[70%] rounded bg-[#16202b18]"></div>
                    </div>
                </div>
            }
            .render(cx),
            ToolVisual::Og => html_sync! {
                <div class="flex h-[150px] flex-col gap-2 bg-[#101114] p-5 text-white">
                    <div class="font-mono text-[10px] uppercase tracking-[0.16em] text-[#ffb070]">
                        "Open Graph"
                    </div>
                    <div class="mt-1 flex-1 rounded-[18px] border border-white/10 bg-white/[0.06] p-3">
                        <div class="h-5 w-24 rounded-full bg-[#dd7627]"></div>
                        <div class="mt-4 h-3 w-36 rounded-full bg-white/20"></div>
                        <div class="mt-2 h-3 w-24 rounded-full bg-white/12"></div>
                    </div>
                    <div class="font-mono text-[10px] text-white/45">"HTML -> PNG"</div>
                </div>
            }
            .render(cx),
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for EcosystemToolCard<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let tool = self.tool;
        html_sync! {
            <a
                href={tool.href}
                data-industry={tool.category}
                class="ecosystem-tool-card group overflow-hidden rounded-2xl border border-[#00000014] bg-white transition-transform hover:-translate-y-1 dark:border-white/10 dark:bg-[#111215]"
            >
                {ToolVisualPreview { visual: tool.visual }}
                <div class="p-4">
                    <div class="flex items-center justify-between gap-2">
                        <div class="text-[15px] font-semibold text-[#18181b] dark:text-zinc-50">
                            {tool.name}
                        </div>
                        {ToolVersionBadge {
                            label: tool.version,
                            tone: tool.version_tone,
                        }}
                    </div>
                    <div class="mt-0.5 text-[13px] leading-5 text-[#7b746b] dark:text-zinc-400">
                        {tool.description}
                    </div>
                    <div class="mt-3 flex items-center gap-3 border-t border-[#00000014] pt-3 dark:border-white/10">
                        <span class="rounded-full border border-[#00000014] bg-[#f7f4ee] px-2.5 py-1 font-mono text-[10px] text-[#57544e] dark:border-white/10 dark:bg-white/5 dark:text-zinc-300">
                            {tool.kind}
                        </span>
                        <span class="min-w-0 truncate font-mono text-[11px] text-[#a59f97] dark:text-zinc-500">
                            {tool.detail}
                        </span>
                        <span class="ml-auto font-mono text-[11px] text-[#dd7627] group-hover:underline">
                            {tool.cta_label}
                        </span>
                    </div>
                </div>
            </a>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for EcosystemDeveloperToolsPage<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let tool_count = self.data.tools.len();

        html_sync! {
            <main>
                <section class="pb-10 pt-[68px]">
                    <div class="mx-auto max-w-[1400px] px-4 sm:px-6 lg:px-8">
                        <span class="mb-[22px] inline-flex items-center gap-[9px] font-mono text-[12px] uppercase tracking-[0.16em] text-[#7b746b]">
                            <span class="h-1.5 w-1.5 rounded-full bg-[#dd7627]"></span>
                            "Ecosystem · Developer tools"
                        </span>
                        <h1 class="max-w-[16ch] text-[52px] font-semibold leading-[0.98] tracking-[-0.04em] sm:text-[60px] lg:text-[68px]">
                            "Tools to build on "
                            <span class="text-[#dd7627]">"Proa."</span>
                        </h1>
                        <p class="mt-6 max-w-[52ch] text-[17px] leading-[1.55] text-[#7b746b]">
                            "Components, libraries, and tools for building Proa apps—from UI and typed islands to docs, code generation, and integrations."
                        </p>
                    </div>
                </section>
                <section class="pb-16 pt-2" id="showcase">
                    <div class="mx-auto max-w-[1400px] px-4 sm:px-6 lg:px-8">
                        <div class="mb-7 flex flex-wrap items-center gap-x-2 gap-y-3">
                            <div id="filter-bar" class="flex flex-wrap gap-2 max-[560px]:w-full">
                                {for filter in self.data.tool_filters {
                                    ToolFilterButton {
                                        filter,
                                        active: filter.key == "all",
                                    }
                                }}
                            </div>
                            <div class="ml-auto flex items-center gap-3 max-[560px]:ml-0 max-[560px]:w-full max-[560px]:flex-wrap">
                                <div class="relative max-[560px]:w-full">
                                    <svg
                                        class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2"
                                        width="14"
                                        height="14"
                                        viewBox="0 0 24 24"
                                        fill="none"
                                        stroke="#a59f97"
                                        stroke-width="2"
                                        aria-hidden="true"
                                    >
                                        <circle cx="11" cy="11" r="7" />
                                        <line x1="21" y1="21" x2="16.65" y2="16.65" />
                                    </svg>
                                    <input
                                        id="ecosystem-tool-search"
                                        type="text"
                                        placeholder="Search tools…"
                                        autocomplete="off"
                                        class="w-[180px] rounded-full border border-[#00000014] bg-white py-2 pl-9 pr-3 font-mono text-[12px] text-[#18181b] placeholder:text-[#a59f97] focus:border-[#18181b] focus:outline-none max-[560px]:w-full dark:border-white/15 dark:bg-white/[0.06] dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-[#dd7627]"
                                    />
                                </div>
                                <span
                                    id="count"
                                    class="whitespace-nowrap font-mono text-[12px] text-[#a59f97] max-[560px]:w-full"
                                >
                                    "Showing "
                                    {tool_count}
                                    " of "
                                    {tool_count}
                                </span>
                            </div>
                        </div>
                        <div
                            id="showcase-grid"
                            class="grid grid-cols-3 gap-4 max-[920px]:grid-cols-2 max-[560px]:grid-cols-1"
                        >
                            {for tool in self.data.tools {
                                EcosystemToolCard { tool }
                            }}
                        </div>
                        <div id="empty-state" class="hidden py-20 text-center">
                            <div class="font-mono text-[13px] text-[#7b746b]">
                                "No tools match that search."
                            </div>
                            <button
                                id="clear-search"
                                class="mt-3 font-mono text-[12px] text-[#dd7627] hover:underline"
                                type="button"
                            >
                                "Clear filters"
                            </button>
                        </div>
                    </div>
                </section>
                <section class="pb-24">
                    <div class="mx-auto max-w-[1400px] px-4 sm:px-6 lg:px-8">
                        <div class="relative overflow-hidden rounded-3xl bg-[#18181b] px-7 py-12 text-center text-white sm:px-14 sm:py-16">
                            <div class="pointer-events-none absolute -top-[120px] left-1/2 h-[360px] w-[620px] -translate-x-1/2 bg-[radial-gradient(closest-side,#dd762733,transparent_72%)]"></div>
                            <h2 class="relative mb-[14px] text-[30px] font-semibold tracking-[-0.04em] sm:text-[42px]">
                                "Built a tool for Proa?"
                            </h2>
                            <p class="relative mb-7 text-[17px] text-[#ffffff99]">
                                "Publish to the registry and reach every team building on Proa."
                            </p>
                            <div class="relative flex flex-wrap justify-center gap-3">
                                <a
                                    class="inline-flex items-center gap-[7px] whitespace-nowrap rounded-full border border-[#dd7627] bg-[#dd7627] px-[17px] py-[9px] text-sm font-medium text-[#1a0f06]"
                                    href="/ecosystem/registry"
                                >
                                    "Publish a package"
                                </a>
                                <a
                                    class="inline-flex items-center gap-[7px] whitespace-nowrap rounded-full border border-[#ffffff26] bg-[#ffffff12] px-[17px] py-[9px] text-sm font-medium text-white"
                                    href="/docs"
                                >
                                    "Read the docs"
                                </a>
                            </div>
                        </div>
                    </div>
                </section>
            </main>
            {EcosystemMockFooter}
        }
        .render(cx)
    }
}
