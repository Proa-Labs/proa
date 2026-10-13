// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::response::Html;
use proa_core::{DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;
use rsjs::signal;
use rsjs_macros::rsjs;

use crate::layout::{render_page, PageMeta};

pub async fn handler() -> Html<String> {
    render_page(
        PageMeta {
            title: "Proa — The web framework for people and agents",
            description: "The web framework for people and agents. Build with Rust, composable components, and fast server rendering.",
            path: "/",
        },
        HomePage,
    )
}

struct HomePage;

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum BarWidth {
    Pct1,
    Pct3,
    Pct4,
    Pct12,
    Pct72,
    Pct86,
    Full,
}

#[derive(Clone, Copy)]
enum CapabilityVariant {
    Framework,
    Islands,
    Integration,
}

struct TextCard {
    title: &'static str,
    detail: &'static str,
}

/*
const USE_CASES: &[TextCard] = &[
    TextCard {
        title: "Merch drops that stay live under load.",
        detail: "Fresh product, inventory, and cart HTML at the moment traffic spikes.",
    },
    TextCard {
        title: "Live odds, not a cached shell.",
        detail: "Prediction markets and dashboards can render current data in the request path.",
    },
    TextCard {
        title: "Seat maps that survive the onsale.",
        detail:
            "High-fanout transactional pages get native render cost without a separate SSR fleet.",
    },
];
*/

#[allow(dead_code)]
struct FrameworkRow {
    name: &'static str,
    runtime: &'static str,
    latency: &'static str,
    width: BarWidth,
}

#[allow(dead_code)]
const FRAMEWORK_ROWS: &[FrameworkRow] = &[
    FrameworkRow {
        name: "Proa",
        runtime: "Rust",
        latency: "0.46 us",
        width: BarWidth::Pct1,
    },
    FrameworkRow {
        name: "Askama",
        runtime: "Rust",
        latency: "1.90 us",
        width: BarWidth::Pct3,
    },
    FrameworkRow {
        name: "Sailfish",
        runtime: "Rust",
        latency: "2.55 us",
        width: BarWidth::Pct4,
    },
    FrameworkRow {
        name: "Leptos",
        runtime: "Rust",
        latency: "19.50 us",
        width: BarWidth::Pct12,
    },
    FrameworkRow {
        name: "React (Bun)",
        runtime: "JavaScript",
        latency: "583.06 us",
        width: BarWidth::Pct72,
    },
    FrameworkRow {
        name: "React (Deno)",
        runtime: "JavaScript",
        latency: "707.17 us",
        width: BarWidth::Pct86,
    },
    FrameworkRow {
        name: "React (Node)",
        runtime: "JavaScript",
        latency: "816.41 us",
        width: BarWidth::Full,
    },
];

struct CapabilityColumn {
    variant: CapabilityVariant,
    label: &'static str,
    title: &'static str,
    detail: &'static str,
    supporting: &'static [TextCard],
}

const ISLAND_SUPPORT: &[TextCard] = &[
    TextCard {
        title: "RSJS islands",
        detail: "Use signal(...), bind_value, typed events, and named handlers in Rust. The compiler lowers the client behavior.",
    },
    TextCard {
        title: "Tiny per-island JavaScript",
        detail: "Each hydrated RSJS island emits a small vanilla-JS module plus seed state, served with a shared runtime around 1.3 KB gzipped.",
    },
];

const FRAMEWORK_SUPPORT: &[TextCard] = &[
    TextCard {
        title: "Streaming SSR, in order or out of order",
        detail: "Flush HTML in document order when sequence matters, or let Suspense boundaries stream ready chunks out of order when slow data should not block the page.",
    },
    TextCard {
        title: "DataLoader-backed WebContext",
        detail: "Every render has typed access to request-scoped data, response metadata, route state, and loader caching through the same WebContext components already receive.",
    },
    TextCard {
        title: "Typed SSR framework semantics",
        detail: "Layouts, routing, metadata, static assets, forms, mutations, and progressive navigation compose through the same Rust component model instead of template glue.",
    },
];

const ADOPTION_SUPPORT: &[TextCard] = &[
    TextCard {
        title: "Run alongside your existing app",
        detail: "Proxy one route to Proa, leave the rest on Next.js or Rails, then compare latency and cost in production.",
    },
    TextCard {
        title: "Security by default",
        detail: "Auto-escaped output, cross-origin request protection, security headers, and fixed-upstream proxying are part of the framework surface.",
    },
    TextCard {
        title: "Accessibility at compile time",
        detail: "WCAG and WAI-ARIA checks run during cargo build so common markup defects fail before deploy.",
    },
];

const CAPABILITY_COLUMNS: &[CapabilityColumn] = &[
    CapabilityColumn {
        variant: CapabilityVariant::Framework,
        label: "Framework",
        title: "Framework primitives beyond template glue.",
        detail: "Proa keeps streaming, request data, routing, metadata, and response control inside the same typed WebContext as your components.",
        supporting: FRAMEWORK_SUPPORT,
    },
    CapabilityColumn {
        variant: CapabilityVariant::Islands,
        label: "Interactivity",
        title: "Add interactivity without giving up native SSR.",
        detail: "Keep the page as server-rendered HTML, then opt specific controls into client behavior with typed signals and event handlers. Proa compiles only the moving pieces into JavaScript.",
        supporting: ISLAND_SUPPORT,
    },
    CapabilityColumn {
        variant: CapabilityVariant::Integration,
        label: "Adoption",
        title: "Keep the stack you already have. Move one route first.",
        detail: "Go Axum-native for Proa routing and responses, or call Proa over FFI from Node, Go, Python, Ruby, or Java.",
        supporting: ADOPTION_SUPPORT,
    },
];

const CAPABILITY_FRAMEWORK_HTML: &str = r##"<div class="relative h-56 overflow-hidden rounded-[26px] border border-black/10 bg-zinc-950 p-4 text-white dark:border-white/10 dark:bg-black" aria-hidden="true">
    <div class="flex items-center justify-between gap-3 text-[10px] uppercase tracking-[0.22em] text-white/55">
        <div class="flex items-center gap-2">
            <span class="size-2 rounded-full bg-[#dd7627]"></span>
            <span>Framework render</span>
        </div>
        <span>WebContext&lt;L, B&gt;</span>
    </div>
    <div class="mt-4 flex gap-2 text-[11px] font-medium">
        <div class="rounded-full border border-white/10 bg-white/[0.06] px-3 py-1.5 text-white/70">Route</div>
        <div class="rounded-full border border-white/10 bg-white/[0.06] px-3 py-1.5 text-white/70">DataLoader</div>
        <div class="rounded-full bg-[#dd7627] px-3 py-1.5 text-black">Stream</div>
    </div>
    <div class="mt-5 grid grid-cols-[minmax(0,0.88fr)_auto_minmax(0,1.12fr)] items-center gap-3">
        <div class="rounded-[22px] border border-white/10 bg-white/[0.05] p-4">
            <div class="space-y-2">
                <div class="h-3 w-full max-w-[6rem] rounded-full bg-[#dd7627]/45"></div>
                <div class="h-[72px] rounded-[18px] bg-white/[0.08]"></div>
                <div class="grid grid-cols-2 gap-2">
                    <div class="h-12 rounded-2xl bg-white/[0.08]"></div>
                    <div class="h-12 rounded-2xl bg-white/[0.08]"></div>
                </div>
            </div>
        </div>
        <div class="flex flex-col items-center gap-2 text-[#dd7627]">
            <div class="h-0.5 w-10 bg-current"></div>
            <svg viewBox="0 0 24 24" class="size-5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M5 12h14"></path>
                <path d="m12 5 7 7-7 7"></path>
            </svg>
            <div class="h-0.5 w-10 bg-current"></div>
        </div>
        <div class="rounded-[22px] border border-white/10 bg-white p-4 text-zinc-950 shadow-[0_24px_60px_-40px_rgba(0,0,0,0.65)]">
            <div class="space-y-2">
                <div class="h-1.5 w-16 rounded-full bg-[#dd7627]"></div>
                <div class="h-3 w-full max-w-[7rem] rounded-full bg-zinc-200"></div>
                <div class="h-[72px] rounded-[18px] bg-[#ebe5db]"></div>
                <div class="grid grid-cols-2 gap-2">
                    <div class="h-12 rounded-2xl bg-zinc-100"></div>
                    <div class="h-12 rounded-2xl bg-zinc-100"></div>
                </div>
            </div>
        </div>
    </div>
</div>"##;

const CAPABILITY_ISLANDS_HTML: &str = r##"<div class="relative h-56 overflow-hidden rounded-[26px] border border-black/10 bg-[#f4eee5] p-4 dark:border-white/10 dark:bg-[#111216]" aria-hidden="true">
    <div class="landing-grid absolute inset-0 opacity-20 dark:opacity-10"></div>
    <div class="relative flex h-full flex-col">
        <div class="flex items-center justify-between gap-3 text-[10px] uppercase tracking-[0.22em] text-zinc-600 dark:text-zinc-400">
            <span>Interactive SSR page</span>
            <div class="flex items-center gap-2">
                <span class="rounded-full border border-black/10 bg-white/80 px-2.5 py-1 text-[9px] font-medium text-zinc-700 dark:border-white/10 dark:bg-white/[0.06] dark:text-zinc-200">SSR</span>
                <span class="rounded-full border border-[#dd7627]/30 bg-[#dd7627]/[0.12] px-2.5 py-1 text-[9px] font-medium text-zinc-950 dark:text-[#ffb070]">RSJS</span>
            </div>
        </div>
        <div class="mt-4 rounded-[24px] border border-black/10 bg-white/[0.88] p-3 shadow-[0_18px_48px_-36px_rgba(24,24,27,0.35)] dark:border-white/10 dark:bg-[#16171b]">
            <div class="flex items-center gap-2">
                <span class="size-2 rounded-full bg-zinc-300 dark:bg-zinc-600"></span>
                <span class="size-2 rounded-full bg-zinc-300 dark:bg-zinc-600"></span>
                <span class="size-2 rounded-full bg-zinc-300 dark:bg-zinc-600"></span>
                <div class="ml-2 h-2.5 flex-1 rounded-full bg-zinc-100 dark:bg-white/[0.08]"></div>
            </div>
            <div class="mt-3 space-y-3">
                <div class="rounded-[18px] border border-[#dd7627]/35 bg-[#dd7627]/10 p-3">
                    <div class="flex items-center justify-between gap-3">
                        <p class="text-[10px] uppercase tracking-[0.2em] text-zinc-700 dark:text-[#ffb070]">"RSJS island"</p>
                        <span class="rounded-full bg-white/80 px-2 py-1 text-[9px] font-medium text-zinc-600 dark:bg-black/20 dark:text-zinc-200">SearchIsland</span>
                    </div>
                    <div class="mt-2 grid grid-cols-3 gap-2">
                        <div class="h-8 rounded-full bg-white/80 dark:bg-white/[0.12]"></div>
                        <div class="h-8 rounded-full bg-white/65 dark:bg-white/10"></div>
                        <div class="h-8 rounded-full bg-white/65 dark:bg-white/10"></div>
                    </div>
                </div>
                <div class="grid grid-cols-[minmax(0,1.15fr)_minmax(120px,0.85fr)] gap-3">
                    <div class="rounded-[18px] border border-black/10 bg-zinc-50/90 p-3 dark:border-white/10 dark:bg-white/[0.04]">
                        <div class="flex items-center justify-between gap-3">
                            <p class="text-[10px] uppercase tracking-[0.2em] text-zinc-600 dark:text-zinc-400">Rust SSR</p>
                            <div class="h-2 w-16 rounded-full bg-zinc-200 dark:bg-white/10"></div>
                        </div>
                        <div class="mt-3 grid grid-cols-2 gap-2">
                            <div class="rounded-[14px] border border-black/10 bg-white p-2.5 dark:border-white/10 dark:bg-black/20">
                                <div class="h-8 rounded-[10px] bg-[#ebe5db] dark:bg-white/[0.08]"></div>
                                <div class="mt-2 h-2.5 w-3/4 rounded-full bg-zinc-200 dark:bg-white/10"></div>
                                <div class="mt-1 h-2 w-1/2 rounded-full bg-zinc-100 dark:bg-white/[0.08]"></div>
                            </div>
                            <div class="rounded-[14px] border border-black/10 bg-white p-2.5 dark:border-white/10 dark:bg-black/20">
                                <div class="h-8 rounded-[10px] bg-[#ebe5db] dark:bg-white/[0.08]"></div>
                                <div class="mt-2 h-2.5 w-3/4 rounded-full bg-zinc-200 dark:bg-white/10"></div>
                                <div class="mt-1 h-2 w-1/2 rounded-full bg-zinc-100 dark:bg-white/[0.08]"></div>
                            </div>
                            <div class="rounded-[14px] border border-black/10 bg-white p-2.5 dark:border-white/10 dark:bg-black/20">
                                <div class="h-8 rounded-[10px] bg-[#ebe5db] dark:bg-white/[0.08]"></div>
                                <div class="mt-2 h-2.5 w-3/4 rounded-full bg-zinc-200 dark:bg-white/10"></div>
                                <div class="mt-1 h-2 w-1/2 rounded-full bg-zinc-100 dark:bg-white/[0.08]"></div>
                            </div>
                            <div class="rounded-[14px] border border-black/10 bg-white p-2.5 dark:border-white/10 dark:bg-black/20">
                                <div class="h-8 rounded-[10px] bg-[#ebe5db] dark:bg-white/[0.08]"></div>
                                <div class="mt-2 h-2.5 w-3/4 rounded-full bg-zinc-200 dark:bg-white/10"></div>
                                <div class="mt-1 h-2 w-1/2 rounded-full bg-zinc-100 dark:bg-white/[0.08]"></div>
                            </div>
                        </div>
                    </div>
                    <div class="space-y-3">
                        <div class="rounded-[18px] border border-[#dd7627]/35 bg-zinc-950 p-3 text-white dark:bg-black">
                            <p class="text-[10px] uppercase tracking-[0.2em] text-[#ffb070]">rsjs query</p>
                            <div class="mt-2 h-16 rounded-[14px] bg-white/[0.08]"></div>
                            <p class="mt-2 text-[10px] text-white/55">MapViewport</p>
                        </div>
                        <div class="rounded-[18px] border border-[#dd7627]/35 bg-[#dd7627]/10 p-3">
                            <p class="text-[10px] uppercase tracking-[0.2em] text-zinc-700 dark:text-[#ffb070]">rsjs handler</p>
                            <div class="mt-2 flex gap-2">
                                <div class="h-8 flex-1 rounded-full bg-zinc-950 dark:bg-[#dd7627]"></div>
                                <div class="h-8 w-10 rounded-full bg-white/80 dark:bg-white/[0.12]"></div>
                            </div>
                            <p class="mt-2 text-[10px] text-zinc-600 dark:text-zinc-300">on_click</p>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </div>
</div>"##;

const CAPABILITY_INTEGRATION_HTML: &str = r##"<div class="relative h-56 overflow-hidden rounded-[26px] border border-black/10 bg-[#f3eee6] p-4 dark:border-white/10 dark:bg-[#111216]" aria-hidden="true">
    <div class="landing-grid absolute inset-0 opacity-20 dark:opacity-10"></div>
    <div class="relative flex h-full flex-col justify-between">
        <div class="flex items-center justify-between gap-3 text-[10px] uppercase tracking-[0.22em] text-zinc-600 dark:text-zinc-400">
            <span>Adopt incrementally</span>
        </div>
        <div class="grid grid-cols-[minmax(0,0.82fr)_auto_minmax(0,1.08fr)] items-center gap-2 sm:gap-3">
            <div class="space-y-2">
                <div class="rounded-full border border-black/10 bg-white/[0.85] px-2.5 py-1.5 text-[11px] text-zinc-700 dark:border-white/10 dark:bg-white/[0.06] dark:text-zinc-200 sm:px-3 sm:text-xs">Node</div>
                <div class="rounded-full border border-black/10 bg-white/[0.85] px-2.5 py-1.5 text-[11px] text-zinc-700 dark:border-white/10 dark:bg-white/[0.06] dark:text-zinc-200 sm:px-3 sm:text-xs">Python</div>
                <div class="rounded-full border border-black/10 bg-white/[0.85] px-2.5 py-1.5 text-[11px] text-zinc-700 dark:border-white/10 dark:bg-white/[0.06] dark:text-zinc-200 sm:px-3 sm:text-xs">Ruby</div>
                <div class="rounded-full border border-black/10 bg-white/[0.85] px-2.5 py-1.5 text-[11px] text-zinc-700 dark:border-white/10 dark:bg-white/[0.06] dark:text-zinc-200 sm:px-3 sm:text-xs">Go</div>
            </div>
            <div class="flex flex-col items-center gap-2">
                <div class="h-0.5 w-8 bg-zinc-300 dark:bg-white/15"></div>
                <div class="rounded-full bg-zinc-950 px-2.5 py-1 text-[10px] font-medium uppercase tracking-[0.18em] text-white dark:bg-[#dd7627] dark:text-black sm:px-3 sm:text-[11px]">ffi</div>
                <div class="h-0.5 w-8 bg-zinc-300 dark:bg-white/15"></div>
            </div>
            <div class="min-w-0 rounded-[24px] border border-black/10 bg-zinc-950 p-3 text-white shadow-[0_24px_65px_-40px_rgba(24,24,27,0.55)] dark:border-white/10 dark:bg-black sm:p-4">
                <div class="grid gap-2 sm:flex sm:items-center sm:justify-between sm:gap-3">
                    <p class="text-[10px] uppercase tracking-[0.22em] text-white/55">Proa render core</p>
                </div>
                <div class="mt-4 grid grid-cols-3 gap-2">
                    <div class="rounded-2xl border border-white/10 bg-white/[0.06] px-3 py-3"></div>
                    <div class="rounded-2xl border border-white/10 bg-white/[0.06] px-3 py-3"></div>
                    <div class="rounded-2xl border border-white/10 bg-white/[0.06] px-3 py-3"></div>
                </div>
                <div class="mt-4 h-16 rounded-[18px] bg-white/[0.06]"></div>
            </div>
        </div>
        <div class="rounded-full border border-black/10 bg-white/[0.88] px-4 py-2 text-xs text-zinc-600 dark:border-white/10 dark:bg-white/[0.06] dark:text-zinc-300">Axum-native when you want it. Shared library when you do not.</div>
    </div>
</div>"##;

#[allow(dead_code)]
const CONVERSION_LATENCY_GRAPH_HTML: &str = r##"<div class="mt-8 grid gap-4 lg:grid-cols-[minmax(0,1.18fr)_minmax(280px,0.82fr)]">
    <div class="overflow-hidden rounded-[26px] border border-black/10 bg-white/88 shadow-[0_28px_80px_-58px_rgba(24,24,27,0.38)] dark:border-white/10 dark:bg-zinc-950/70">
        <div class="flex items-center justify-between gap-4 border-b border-black/10 px-5 py-4 dark:border-white/10">
            <p class="font-mono text-[10px] uppercase tracking-[0.22em] text-zinc-500 dark:text-zinc-400">Load time -> session survival</p>
            <p class="font-mono text-[10px] uppercase tracking-[0.22em] text-[#9b5a24] dark:text-[#ffb070]">3s cliff</p>
        </div>
        <div class="relative h-[340px] px-4 pb-4 pt-5 sm:px-6">
            <svg viewBox="0 0 760 300" class="h-full w-full overflow-visible" fill="none" role="img" aria-label="Graph showing session survival falling as page load time rises">
                <defs>
                    <linearGradient id="conversionDrop" x1="0" x2="0" y1="0" y2="1">
                        <stop offset="0%" stop-color="#dd7627" stop-opacity="0.24" />
                        <stop offset="100%" stop-color="#dd7627" stop-opacity="0" />
                    </linearGradient>
                    <linearGradient id="slowZone" x1="0" x2="1" y1="0" y2="0">
                        <stop offset="0%" stop-color="#dd7627" stop-opacity="0.08" />
                        <stop offset="100%" stop-color="#ff5f57" stop-opacity="0.18" />
                    </linearGradient>
                </defs>
                <rect x="520" y="24" width="188" height="220" rx="18" fill="url(#slowZone)" />
                <g stroke="#18181b" stroke-opacity="0.1" stroke-width="1">
                    <path d="M72 24H708" />
                    <path d="M72 80H708" />
                    <path d="M72 136H708" />
                    <path d="M72 192H708" />
                    <path d="M72 244H708" />
                </g>
                <g fill="currentColor" class="text-zinc-400 dark:text-zinc-500" font-family="ui-monospace, SFMono-Regular, Menlo, monospace" font-size="11">
                    <text x="26" y="28">100%</text>
                    <text x="34" y="140">50%</text>
                    <text x="42" y="248">0%</text>
                    <text x="72" y="280">0s</text>
                    <text x="252" y="280">1s</text>
                    <text x="430" y="280">2s</text>
                    <text x="604" y="280">3s+</text>
                </g>
                <path d="M72 36 C168 42 224 58 300 82 C386 110 450 143 520 170 C592 198 650 220 708 232 L708 244 L72 244 Z" fill="url(#conversionDrop)" />
                <path d="M72 36 C168 42 224 58 300 82 C386 110 450 143 520 170 C592 198 650 220 708 232" stroke="#dd7627" stroke-width="4" stroke-linecap="round" />
                <path d="M520 24V244" stroke="#dd7627" stroke-opacity="0.42" stroke-width="1.5" stroke-dasharray="6 6" />
                <g fill="#f7f4ee" stroke="#dd7627" stroke-width="3">
                    <circle cx="72" cy="36" r="6" />
                    <circle cx="300" cy="82" r="6" />
                    <circle cx="520" cy="170" r="6" />
                    <circle cx="708" cy="232" r="6" />
                </g>
            </svg>
            <div class="absolute right-6 top-16 max-w-[180px] rounded-[18px] border border-[#dd7627]/35 bg-[#fff7ed]/95 p-4 shadow-[0_18px_48px_-36px_rgba(24,24,27,0.35)] dark:bg-[#1d140c]/95">
                <p class="font-mono text-[10px] uppercase tracking-[0.2em] text-[#9b5a24] dark:text-[#ffb070]">After 3 seconds</p>
                <p class="mt-2 font-display text-4xl font-semibold tracking-[-0.04em] text-zinc-950 dark:text-zinc-50">53%</p>
                <p class="mt-1 text-xs leading-5 text-zinc-600 dark:text-zinc-300">likely to abandon</p>
            </div>
        </div>
    </div>
    <div class="rounded-[26px] border border-zinc-950 bg-zinc-950 p-5 text-white shadow-[0_30px_90px_-62px_rgba(0,0,0,0.9)] dark:border-white/10">
        <p class="font-mono text-[10px] uppercase tracking-[0.22em] text-white/50">Route JavaScript</p>
        <div class="mt-5 space-y-5">
            <div>
                <div class="flex items-baseline justify-between gap-4">
                    <span class="text-sm text-white/65">Client-rendered route</span>
                    <span class="font-mono text-sm text-white">200 KB</span>
                </div>
                <div class="mt-2 h-4 rounded-full bg-white/10">
                    <div class="h-full rounded-full bg-[#ff5f57]" style="width: 100%;"></div>
                </div>
            </div>
            <div>
                <div class="flex items-baseline justify-between gap-4">
                    <span class="text-sm text-white/65">Proa route</span>
                    <span class="font-mono text-sm text-[#ffb070]">2.6 KB</span>
                </div>
                <div class="mt-2 h-4 rounded-full bg-white/10">
                    <div class="h-full min-w-[4px] rounded-full bg-[#dd7627]" style="width: 1.3%;"></div>
                </div>
            </div>
        </div>
        <div class="mt-7 border-t border-white/10 pt-5">
            <p class="font-display text-5xl font-semibold tracking-[-0.05em] text-[#ffb070]">~77x</p>
            <p class="mt-2 text-sm leading-6 text-white/68">less script to download, parse, compile, and hydrate on low-end devices.</p>
        </div>
    </div>
</div>"##;

#[allow(dead_code)]
const SIMPLIFICATION_CARDS: &[TextCard] = &[
    TextCard {
        title: "No ISR or PPR as survival tactics",
        detail: "Prerender when it helps the product, not because request-time rendering collapses under real traffic.",
    },
    TextCard {
        title: "No HTML page cache dependency",
        detail: "Fresh renders get cheap enough that cache invalidation stops being the architecture.",
    },
    TextCard {
        title: "No streaming shell to hide latency",
        detail: "Stream when the experience wants it, not because the renderer is too slow to send a full page.",
    },
    TextCard {
        title: "No dedicated SSR fleet or regen queue",
        detail: "A native render call replaces workers, warm pools, and background rebuild plumbing.",
    },
];

/// One header treatment for every band on the page.
///
/// The hero states the page's claim centered on the fold; each section below
/// repeats that shape one step down the scale, so an eyebrow, a heading, and
/// a lead paragraph read the same wherever they appear.
const BENCHMARKS_URL: &str = "https://github.com/Proa-Labs/proa-benchmarks";
const SHADCN_URL: &str = "https://ui.shadcn.com";
const LEAD_LINK: &str = "underline decoration-[#dd7627]/45 underline-offset-[6px] transition-colors hover:text-[#dd7627] hover:decoration-[#dd7627]";
const HERO_TITLE: &str = "text-balance font-display text-[42px] font-semibold leading-[0.95] tracking-[-0.045em] text-black dark:text-zinc-50 sm:text-[64px] lg:text-[80px]";
const HERO_LEAD: &str = "mt-7 max-w-[68ch] text-balance text-lg leading-[1.55] text-[#777169] dark:text-zinc-300 sm:text-xl sm:leading-[1.5] lg:text-[22px] lg:leading-[1.45]";
/// One shell for every band: the same vertical rhythm, the same measure, the
/// same ground. Every section sits on the one surface, so a hairline does the
/// separating rather than a change of colour.
const SECTION_SHELL: &str = "scroll-mt-24 border-t border-black/10 bg-[oklch(1_0_0)] py-16 dark:border-white/10 dark:bg-transparent sm:py-20 lg:py-24";
const SECTION_CONTAINER: &str = "mx-auto w-full max-w-[1400px] px-4 sm:px-6 lg:px-8";
const SECTION_HEADER: &str = "mx-auto flex max-w-[56rem] flex-col items-center text-center";
const SECTION_TITLE: &str = "mt-4 text-balance font-display text-[32px] font-semibold leading-[1.02] tracking-[-0.04em] text-black dark:text-zinc-50 sm:text-[44px] lg:text-[52px]";
const SECTION_LEAD: &str = "mt-5 max-w-[60ch] text-balance text-base leading-[1.6] text-[#777169] dark:text-zinc-300 sm:text-lg lg:max-w-[68ch] lg:text-xl lg:leading-[1.55]";
const SECTION_ACTIONS: &str = "mt-8 flex flex-wrap items-center justify-center gap-x-7 gap-y-4";
const CTA_PRIMARY: &str = "inline-flex items-center justify-center gap-2 rounded-full border border-zinc-950 bg-zinc-950 px-6 py-3 text-sm font-medium text-white transition-transform duration-200 hover:-translate-y-0.5 dark:border-[#dd7627] dark:bg-[#dd7627] dark:text-black";
const CTA_SECONDARY: &str = "inline-flex items-center gap-2 text-sm font-medium text-zinc-950 transition-colors hover:text-[#dd7627] dark:text-zinc-100 dark:hover:text-[#ffb070]";
const CTA_VIDEO: &str = "inline-flex min-h-[46px] items-center justify-center gap-2.5 whitespace-nowrap rounded-full border border-black/10 bg-white/80 px-6 py-3 text-sm font-medium text-zinc-950 transition-colors hover:border-[#dd7627]/60 hover:bg-[#dd7627]/[0.07] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#dd7627] dark:border-white/20 dark:bg-white/[0.04] dark:text-zinc-100 dark:hover:border-[#dd7627]/60 dark:hover:bg-[#dd7627]/10";
const BENCHMARK_CONTAINER: &str = "mx-auto w-full max-w-[1080px] px-4 sm:px-6 lg:px-8";
const BENCHMARK_TAB: &str = "rounded-[10px] px-2 py-3 text-sm font-medium text-[#777169] transition-colors hover:text-zinc-950 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#dd7627] data-[state=active]:bg-white data-[state=active]:text-zinc-950 dark:text-zinc-400 dark:hover:text-zinc-100 dark:data-[state=active]:bg-zinc-950 dark:data-[state=active]:text-zinc-50 sm:px-4";
const BENCHMARK_PANEL: &str = "overflow-x-auto rounded-[20px] border border-black/10 bg-[#fcfcfb] p-3 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#dd7627] dark:border-white/10 sm:p-5";

const COMPOSABLE_SOURCE_HTML: &str = r##"<pre class="overflow-x-auto rounded-[20px] border border-white/10 bg-[#07080a] p-4 font-mono text-[12px] leading-6 text-[#d7dde8] shadow-[0_30px_90px_-60px_rgba(0,0,0,0.9)]"><code class="block whitespace-pre"><span class="text-[#8ec5ff]">ProductCard</span><span class="text-white/45">::</span><span class="text-[#ffd166]">new</span>(product)</code></pre>"##;

const COMPOSABLE_MD_RENDER_HTML: &str = r##"<pre class="overflow-x-auto rounded-[20px] border border-[#dd7627]/35 bg-[#07080a] p-4 font-mono text-[12px] leading-6 text-[#d7dde8] shadow-[0_30px_90px_-60px_rgba(0,0,0,0.9)]"><code class="block whitespace-pre"><span class="text-[#ffb070]">impl</span>&lt;L: <span class="text-[#d8b4fe]">DataLoader</span>&gt; <span class="text-[#d8b4fe]">MdRenderSync</span>&lt;L&gt; <span class="text-[#ffb070]">for</span> <span class="text-[#8ec5ff]">ProductCard</span> {
    <span class="text-[#ffb070]">fn</span> <span class="text-[#ffd166]">render_md</span>(self, cx: &amp;mut <span class="text-[#8ec5ff]">WebContext</span>&lt;L&gt;) -&gt; <span class="text-[#8ec5ff]">Result</span>&lt;(), <span class="text-[#8ec5ff]">WriteError</span>&gt; {
        <span class="text-[#ff8a8a]">md_sync!</span> { <span class="text-[#a7e3a1]">r###&quot;
            # </span>{self.product.name}<span class="text-[#a7e3a1]">

            </span>{<span data-flow-node="md-child-origin" class="inline-block rounded-[7px] border border-[#dd7627]/55 bg-[#dd7627]/10 px-1.5 py-0.5 shadow-[0_0_0_1px_rgba(221,118,39,0.08)]"><span class="text-[#8ec5ff]">Link</span><span class="text-white/45">::</span><span class="text-[#ffd166]">new</span>(<span class="text-[#a7e3a1]">&quot;Buy now&quot;</span>, self.product.url)</span>}<span class="text-[#a7e3a1]">
            &quot;###</span>
        }
        .<span class="text-[#ffd166]">render_md</span>(cx)
    }
}</code></pre>"##;

const COMPOSABLE_WEB_RENDER_HTML: &str = r##"<pre class="overflow-x-auto rounded-[20px] border border-[#dd7627]/35 bg-[#07080a] p-4 font-mono text-[12px] leading-6 text-[#d7dde8] shadow-[0_30px_90px_-60px_rgba(0,0,0,0.9)]"><code class="block whitespace-pre"><span class="text-[#ffb070]">impl</span>&lt;L: <span class="text-[#d8b4fe]">DataLoader</span>&gt; <span class="text-[#d8b4fe]">WebRenderSync</span>&lt;L&gt; <span class="text-[#ffb070]">for</span> <span class="text-[#8ec5ff]">ProductCard</span> {
    <span class="text-[#ffb070]">fn</span> <span class="text-[#ffd166]">render</span>(self, cx: &amp;mut <span class="text-[#8ec5ff]">WebContext</span>&lt;L&gt;) -&gt; <span class="text-[#8ec5ff]">Result</span>&lt;(), <span class="text-[#8ec5ff]">WriteError</span>&gt; {
        <span class="text-[#ff8a8a]">html_sync!</span> {
            &lt;article data-product-card&gt;
                &lt;h2&gt;{self.product.name}&lt;/h2&gt;
                {<span data-flow-node="web-child-origin" class="inline-block rounded-[7px] border border-[#dd7627]/55 bg-[#dd7627]/10 px-1.5 py-0.5 shadow-[0_0_0_1px_rgba(221,118,39,0.08)]"><span class="text-[#8ec5ff]">Link</span><span class="text-white/45">::</span><span class="text-[#ffd166]">new</span>(<span class="text-[#a7e3a1]">&quot;Buy now&quot;</span>, self.product.url)</span>}
            &lt;/article&gt;
        }
        .<span class="text-[#ffd166]">render</span>(cx)
    }
}</code></pre>"##;

const COMPOSABLE_MD_CHILD_HTML: &str = r##"<pre class="overflow-x-auto rounded-[20px] border border-white/10 bg-[#07080a] p-4 font-mono text-[12px] leading-6 text-[#d7dde8] shadow-[0_30px_90px_-60px_rgba(0,0,0,0.9)]"><code class="block whitespace-pre"><span class="text-[#ff8a8a]">md_sync!</span> { <span class="text-[#a7e3a1]">r###&quot;
    [</span>{self.label}<span class="text-[#a7e3a1]">](</span>{self.href}<span class="text-[#a7e3a1]">)
    &quot;###</span>
}</code></pre>"##;

const COMPOSABLE_WEB_CHILD_HTML: &str = r##"<pre class="overflow-x-auto rounded-[20px] border border-white/10 bg-[#07080a] p-4 font-mono text-[12px] leading-6 text-[#d7dde8] shadow-[0_30px_90px_-60px_rgba(0,0,0,0.9)]"><code class="block whitespace-pre"><span class="text-[#ff8a8a]">html_sync!</span> {
    &lt;a href={self.href} data-link&gt;
        {self.label}
    &lt;/a&gt;
}</code></pre>"##;

const COMPOSABLE_CONNECTORS_HTML: &str = r##"<svg data-composable-connectors class="pointer-events-none absolute inset-0 z-20 h-full w-full overflow-visible text-[#dd7627] dark:text-[#ff9d5a]" viewBox="0 0 1 1" preserveAspectRatio="none" fill="none" aria-hidden="true">
    <defs>
        <marker id="composable-arrowhead" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="strokeWidth">
            <path d="M1.5 1 L7 4 L1.5 7 Z" fill="currentColor" />
        </marker>
    </defs>
    <g opacity="0.82" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke">
        <path data-flow-path="source-md" d="" marker-end="url(#composable-arrowhead)" pathLength="1" stroke-dasharray="1" stroke-dashoffset="1" style="transition: stroke-dashoffset 560ms cubic-bezier(0.45,0,0.2,1);" />
        <path data-flow-path="source-web" d="" marker-end="url(#composable-arrowhead)" pathLength="1" stroke-dasharray="1" stroke-dashoffset="1" style="transition: stroke-dashoffset 560ms cubic-bezier(0.45,0,0.2,1) 80ms;" />
        <path data-flow-path="md-child" d="" marker-end="url(#composable-arrowhead)" pathLength="1" stroke-dasharray="1" stroke-dashoffset="1" style="transition: stroke-dashoffset 360ms cubic-bezier(0.45,0,0.2,1) 260ms;" />
        <path data-flow-path="web-child" d="" marker-end="url(#composable-arrowhead)" pathLength="1" stroke-dasharray="1" stroke-dashoffset="1" style="transition: stroke-dashoffset 360ms cubic-bezier(0.45,0,0.2,1) 320ms;" />
    </g>
</svg>"##;

struct ArrowRightIcon;
struct ArrowUpRightIcon;
struct PlayIcon;
struct ComposableSourceCode;
struct ComposableMdRenderCode;
struct ComposableWebRenderCode;
struct ComposableMdChildCode;
struct ComposableWebChildCode;
struct ComposableConnectorLayer;
#[allow(dead_code)]
struct ConversionLatencyGraph;
struct CapabilityIllustration {
    variant: CapabilityVariant,
}
struct InterfaceLayerHero;
struct Direction18Hero;
struct Direction18HeroPreview;
struct BenchmarkHighlights;
struct HeroTravelPanel;
struct HeroRepoPanel;
struct HeroSentryPanel;
struct PenCircle;
const COMPOSABLE_CONNECTORS_SCRIPT: &str = r#"
(function() {
  function round(value) {
    return Math.round(value * 10) / 10;
  }

  function rect(rootRect, node) {
    var box = node.getBoundingClientRect();
    return {
      left: round(box.left - rootRect.left),
      right: round(box.right - rootRect.left),
      top: round(box.top - rootRect.top),
      bottom: round(box.bottom - rootRect.top),
      midY: round(box.top + box.height / 2 - rootRect.top)
    };
  }

  function elbow(from, to) {
    var gap = Math.max(18, (to.left - from.right) / 2);
    var bend = round(from.right + gap);
    return 'M ' + from.right + ' ' + from.midY + ' H ' + bend + ' V ' + to.midY + ' H ' + to.left;
  }

  function horizontal(from, to) {
    return 'M ' + from.right + ' ' + from.midY + ' H ' + to.left;
  }

  function entryY(to) {
    // Enter beside the card's first code line rather than its middle, so the
    // arrow reads as "this impl applies to the object" instead of pointing at
    // whichever line happens to sit at mid-height.
    return Math.min(to.midY, round(to.top + 22));
  }

  function branch(railX, fromY, to) {
    return 'M ' + railX + ' ' + fromY + ' V ' + entryY(to) + ' H ' + to.left;
  }

  function rail(outer, inner) {
    // Hug the left of the gutter so the arrowhead (11px at stroke width 2)
    // fits between the rail and the card edge instead of straddling the rail.
    return round(outer.left + Math.min(3, (inner.left - outer.left) / 2));
  }

  function setPath(svg, name, d) {
    var path = svg.querySelector('[data-flow-path=\"' + name + '\"]');
    if (!path) return;
    path.setAttribute('d', d);
    path.style.strokeDashoffset = '0';
  }

  function draw(root) {
    var svg = root.querySelector('[data-composable-connectors]');
    if (!svg) return;

    var nodes = {
      source: root.querySelector('[data-flow-node=\"source\"]'),
      mdRender: root.querySelector('[data-flow-node=\"md-render\"]'),
      mdChildOrigin: root.querySelector('[data-flow-node=\"md-child-origin\"]'),
      mdChild: root.querySelector('[data-flow-node=\"md-child\"]'),
      webRender: root.querySelector('[data-flow-node=\"web-render\"]'),
      webChildOrigin: root.querySelector('[data-flow-node=\"web-child-origin\"]'),
      webChild: root.querySelector('[data-flow-node=\"web-child\"]')
    };

    if (!nodes.source || !nodes.mdRender || !nodes.mdChild || !nodes.webRender || !nodes.webChild) {
      return;
    }

    var rootRect = root.getBoundingClientRect();
    svg.setAttribute('viewBox', '0 0 ' + round(rootRect.width) + ' ' + round(rootRect.height));

    var source = rect(rootRect, nodes.source);
    var mdRender = rect(rootRect, nodes.mdRender);
    var mdChildOrigin = nodes.mdChildOrigin ? rect(rootRect, nodes.mdChildOrigin) : mdRender;
    var mdChild = rect(rootRect, nodes.mdChild);
    var webRender = rect(rootRect, nodes.webRender);
    var webChildOrigin = nodes.webChildOrigin ? rect(rootRect, nodes.webChildOrigin) : webRender;
    var webChild = rect(rootRect, nodes.webChild);

    if (mdRender.top >= source.bottom) {
      // Below lg the grid stacks into one column, so draw a top-down tree:
      // one rail drops from the source through the gutter left of both
      // target cards, and each target drops a rail to its own child. The
      // second source branch starts where the first turned, so the rail is
      // drawn once instead of twice on top of itself.
      var sourceRail = rail(source, mdRender);
      setPath(svg, 'source-md', branch(sourceRail, source.bottom, mdRender));
      setPath(svg, 'source-web', branch(sourceRail, entryY(mdRender), webRender));
      setPath(svg, 'md-child', branch(rail(mdRender, mdChild), mdRender.bottom, mdChild));
      setPath(svg, 'web-child', branch(rail(webRender, webChild), webRender.bottom, webChild));
      return;
    }

    setPath(svg, 'source-md', elbow(source, mdRender));
    setPath(svg, 'source-web', elbow(source, webRender));
    setPath(svg, 'md-child', horizontal(mdChildOrigin, mdChild));
    setPath(svg, 'web-child', horizontal(webChildOrigin, webChild));
  }

  function drawAll() {
    document.querySelectorAll('[data-composable-flow]').forEach(draw);
  }

  function init() {
    if ('ResizeObserver' in window) {
      // Observer callbacks run right after the browser's own layout, so the
      // geometry reads below are free instead of forcing a synchronous page
      // layout before first paint. Rewriting the connector paths never
      // resizes an observed node, so drawing here cannot loop.
      var observer = new ResizeObserver(drawAll);
      document.querySelectorAll('[data-composable-flow], [data-flow-node]').forEach(function(node) {
        observer.observe(node);
      });
    } else {
      drawAll();
      window.addEventListener('resize', drawAll);
    }

    // A font swap can move the inline child origins without resizing a node.
    // `loadingdone` fires only when a font actually loads; `fonts.ready`
    // resolves immediately when none are pending, which would force layout.
    if (document.fonts && document.fonts.addEventListener) {
      document.fonts.addEventListener('loadingdone', function() {
        window.requestAnimationFrame(drawAll);
      });
    }
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init, { once: true });
  } else {
    init();
  }
})();
"#;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PenCircle {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        // The stroke used to be drawn by an IntersectionObserver flipping
        // `stroke-dashoffset`. It sits in the above-the-fold hero, so it was
        // always intersecting on load; a CSS keyframe with the same delay and
        // easing draws it without any script.
        html_sync! {
            <svg
                id="pen-circle"
                class="pointer-events-none absolute left-1/2 top-1/2 h-[150%] w-[250%] -translate-x-1/2 -translate-y-1/2 overflow-visible sm:h-[210%] sm:w-[150%]"
                viewBox="0 0 200 100"
                fill="none"
                preserveAspectRatio="none"
                aria-hidden="true"
            >
                <path
                    class="pen-circle-stroke"
                    d="M141 24 C112 8, 64 6, 38 18 C12 30, 14 62, 44 76 C78 92, 142 90, 168 72 C188 58, 184 30, 150 19 C128 12, 96 12, 78 18"
                    stroke="#e8412b"
                    stroke-width="2.6"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    pathLength="100"
                />
            </svg>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ArrowRightIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                viewBox="0 0 24 24"
                class="size-4"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
            >
                <path d="M5 12h14" />
                <path d="m12 5 7 7-7 7" />
            </svg>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PlayIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                data-component="play-icon"
                data-play-icon
                viewBox="0 0 24 24"
                class="size-4 shrink-0 text-[#dd7627] dark:text-[#ffb070]"
                fill="currentColor"
                aria-hidden="true"
                focusable="false"
            >
                <path d="M8 5.5a1 1 0 0 1 1.5-.86l10 6.5a1 1 0 0 1 0 1.72l-10 6.5A1 1 0 0 1 8 18.5z" />
            </svg>
        }
        .render(cx)
    }
}

/// The opening statement of the page: one centered claim, one supporting
/// sentence, and the quickstart or introduction video for first-time visitors.
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for InterfaceLayerHero {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section
                data-hero="interface-layer"
                class="bg-[oklch(1_0_0)] px-4 pb-16 pt-20 dark:bg-transparent sm:px-6 sm:pb-24 sm:pt-28 lg:px-8"
            >
                <div class="mx-auto flex max-w-[1000px] flex-col items-center text-center">
                    <h1 class={HERO_TITLE}>"The web framework for people and agents."</h1>
                    <p class={HERO_LEAD}>
                        "An "
                        <a
                            href={BENCHMARKS_URL}
                            rel="noreferrer noopener"
                            target="_blank"
                            class={LEAD_LINK}
                        >
                            "extremely performant,"
                        </a>
                        " batteries included Rust web framework built for people and agents, with composable, multiformat components, lean JavaScript, and fast server rendering."
                    </p>
                    <div class="mt-8 flex flex-wrap items-center justify-center gap-3 sm:gap-4">
                        <a
                            href="/docs"
                            class={CTA_PRIMARY}
                        >
                            "Get started"
                            {ArrowRightIcon}
                        </a>
                        <a
                            href="https://youtu.be/kGhNsD772gg?si=s-YcVTx78kJ1M8o6"
                            data-video-cta="intro"
                            target="_blank"
                            rel="noopener noreferrer"
                            aria-label="Watch intro on YouTube (opens in a new tab)"
                            class={CTA_VIDEO}
                        >
                            {PlayIcon}
                            "Watch intro"
                        </a>
                    </div>
                </div>
            </section>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Direction18Hero {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section
                id="performance"
                class={SECTION_SHELL}
                aria-labelledby="performance-heading"
            >
                <div data-hero-inner class={SECTION_CONTAINER}>
                    <div class={SECTION_HEADER}>
                        <h2 id="performance-heading" class={SECTION_TITLE}>
                            "Build fast, "
                            <span class="text-[#dd7627]">"information-dense apps."</span>
                        </h2>
                        <p class={SECTION_LEAD}>
                            "Explore re-creations of GitHub, Sentry, and Airbnb, built in Proa with fast server rendering and selective interactivity."
                        </p>
                        <div class={SECTION_ACTIONS}>
                            <a
                                href="/ecosystem/apps#reference-showcases"
                                target="_blank"
                                rel="noopener noreferrer"
                                class={CTA_PRIMARY}
                            >
                                "View Showcase"
                                {ArrowRightIcon}
                            </a>
                            <a
                                href={BENCHMARKS_URL}
                                target="_blank"
                                rel="noopener noreferrer"
                                class={CTA_SECONDARY}
                            >
                                "Benchmarks"
                                {ArrowUpRightIcon}
                            </a>
                        </div>
                    </div>
                    <div class="mt-12 min-w-0 sm:mt-14">{Direction18HeroPreview}</div>
                    <p class="mx-auto mt-5 max-w-[70ch] text-center text-sm leading-relaxed text-zinc-600 dark:text-zinc-400">
                        "Independent demos. Not affiliated with or endorsed by GitHub, Sentry, or Airbnb."
                    </p>
                </div>
            </section>
        }
        .render(cx)
    }
}

/// Original benchmark SVGs, served locally and switched by one tab signal.
#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BenchmarkHighlights {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal("render");
        let render_tab = rsjs::node_ref::<rsjs::HtmlElement>();
        let http_tab = rsjs::node_ref::<rsjs::HtmlElement>();
        let load_tab = rsjs::node_ref::<rsjs::HtmlElement>();
        let full_size_icon = ArrowUpRightIcon;
        let methodology_icon = ArrowUpRightIcon;
        let select_render = rsjs::event_handler(|_: rsjs::Event| selected.set("render"));
        let key_render = rsjs::event_handler(|event: rsjs::KeyboardEvent| {
            if event.key() == "ArrowRight" {
                event.prevent_default();
                selected.set("http");
                http_tab.focus();
            }
            if event.key() == "ArrowLeft" {
                event.prevent_default();
                selected.set("load");
                load_tab.focus();
            }
            if event.key() == "Home" {
                event.prevent_default();
                selected.set("render");
                render_tab.focus();
            }
            if event.key() == "End" {
                event.prevent_default();
                selected.set("load");
                load_tab.focus();
            }
        });
        let select_http = rsjs::event_handler(|_: rsjs::Event| selected.set("http"));
        let key_http = rsjs::event_handler(|event: rsjs::KeyboardEvent| {
            if event.key() == "ArrowRight" {
                event.prevent_default();
                selected.set("load");
                load_tab.focus();
            }
            if event.key() == "ArrowLeft" {
                event.prevent_default();
                selected.set("render");
                render_tab.focus();
            }
            if event.key() == "Home" {
                event.prevent_default();
                selected.set("render");
                render_tab.focus();
            }
            if event.key() == "End" {
                event.prevent_default();
                selected.set("load");
                load_tab.focus();
            }
        });
        let select_load = rsjs::event_handler(|_: rsjs::Event| selected.set("load"));
        let key_load = rsjs::event_handler(|event: rsjs::KeyboardEvent| {
            if event.key() == "ArrowRight" {
                event.prevent_default();
                selected.set("render");
                render_tab.focus();
            }
            if event.key() == "ArrowLeft" {
                event.prevent_default();
                selected.set("http");
                http_tab.focus();
            }
            if event.key() == "Home" {
                event.prevent_default();
                selected.set("render");
                render_tab.focus();
            }
            if event.key() == "End" {
                event.prevent_default();
                selected.set("load");
                load_tab.focus();
            }
        });

        html_sync! {
            <section
                id="benchmark-highlights"
                data-component="benchmark-highlights"
                class={SECTION_SHELL}
                aria-labelledby="benchmark-highlights-heading"
            >
                <div class={BENCHMARK_CONTAINER}>
                    <div class={SECTION_HEADER}>
                        <h2 id="benchmark-highlights-heading" class={SECTION_TITLE}>
                            "The numbers behind the speed."
                        </h2>
                    </div>
                    <div
                        role="tablist"
                        aria-label="Benchmark charts"
                        class="mx-auto mb-5 mt-12 grid w-full max-w-[42rem] grid-cols-3 gap-1 rounded-[14px] border border-black/10 bg-[#efedea] p-1 dark:border-white/10 dark:bg-white/5 sm:mt-14"
                    >
                        <button
                            ref={render_tab}
                            id="benchmark-tab-render"
                            type="button"
                            role="tab"
                            aria-controls="benchmark-panel-render"
                            aria_selected={selected.get() == "render"}
                            tabindex={if selected.get() == "render" { "0" } else { "-1" }}
                            data_state={if selected.get() == "render" { "active" } else { "inactive" }}
                            class={BENCHMARK_TAB}
                            on_click={select_render()}
                            on_keydown={key_render()}
                        >
                            "Render speed"
                        </button>
                        <button
                            ref={http_tab}
                            id="benchmark-tab-http"
                            type="button"
                            role="tab"
                            aria-controls="benchmark-panel-http"
                            aria_selected={selected.get() == "http"}
                            tabindex={if selected.get() == "http" { "0" } else { "-1" }}
                            data_state={if selected.get() == "http" { "active" } else { "inactive" }}
                            class={BENCHMARK_TAB}
                            on_click={select_http()}
                            on_keydown={key_http()}
                        >
                            "HTTP capacity"
                        </button>
                        <button
                            ref={load_tab}
                            id="benchmark-tab-load"
                            type="button"
                            role="tab"
                            aria-controls="benchmark-panel-load"
                            aria_selected={selected.get() == "load"}
                            tabindex={if selected.get() == "load" { "0" } else { "-1" }}
                            data_state={if selected.get() == "load" { "active" } else { "inactive" }}
                            class={BENCHMARK_TAB}
                            on_click={select_load()}
                            on_keydown={key_load()}
                        >
                            "Suspense under load"
                        </button>
                    </div>
                    <div
                        id="benchmark-panel-render"
                        role="tabpanel"
                        aria-labelledby="benchmark-tab-render"
                        hidden={selected.get() != "render"}
                        tabindex="0"
                        class={BENCHMARK_PANEL}
                    >
                        <div class="min-w-[800px]">
                            <img
                                src="/static/images/benchmarks/01-canonical-scoreboard.svg"
                                alt="Server-render latency across 17 renderers and three workloads. Proa leads the component frameworks; React takes 13.2 to 35.4 times as long."
                                width="980"
                                height="480"
                                loading="lazy"
                                class="block h-auto w-full"
                            />
                        </div>
                    </div>
                    <div
                        id="benchmark-panel-http"
                        role="tabpanel"
                        aria-labelledby="benchmark-tab-http"
                        hidden={selected.get() != "http"}
                        tabindex="0"
                        class={BENCHMARK_PANEL}
                    >
                        <div class="min-w-[800px]">
                            <img
                                src="/static/images/benchmarks/21-canonical-pages-http.svg"
                                alt="HTTP throughput, CPU per response, and peak memory for Proa, React, Marko, and Go. Proa serves 16,100 product-grid responses per second with 9.4 MiB peak memory."
                                width="980"
                                height="615"
                                loading="lazy"
                                class="block h-auto w-full"
                            />
                        </div>
                    </div>
                    <div
                        id="benchmark-panel-load"
                        role="tabpanel"
                        aria-labelledby="benchmark-tab-load"
                        hidden={selected.get() != "load"}
                        tabindex="0"
                        class={BENCHMARK_PANEL}
                    >
                        <div class="min-w-[800px]">
                            <img
                                src="/static/images/benchmarks/20-suspense-load.svg"
                                alt="Suspense streaming under increasing load: throughput, memory, and response latency for Proa, Marko, and React. Proa sustains about 5,000 responses per second before queueing rises at 6,000 offered requests per second."
                                width="980"
                                height="743"
                                loading="lazy"
                                class="block h-auto w-full"
                            />
                        </div>
                    </div>
                    <div class="mt-5 flex flex-wrap items-center justify-between gap-x-6 gap-y-4">
                        <p class="text-sm leading-6 text-zinc-500 dark:text-zinc-400">
                            "Apple M4 Max · September 2026"
                            <span class="block lg:hidden">
                                "Scroll horizontally to explore each chart."
                            </span>
                        </p>
                        <div class="flex flex-wrap items-center gap-x-6 gap-y-3">
                            <a
                                href={if selected.get() == "http" { "/static/images/benchmarks/21-canonical-pages-http.svg" } else if selected.get() == "load" { "/static/images/benchmarks/20-suspense-load.svg" } else { "/static/images/benchmarks/01-canonical-scoreboard.svg" }}
                                target="_blank"
                                rel="noopener noreferrer"
                                class={CTA_SECONDARY}
                            >
                                "Open full-size SVG"
                                {full_size_icon}
                            </a>
                            <a
                                href={BENCHMARKS_URL}
                                target="_blank"
                                rel="noopener noreferrer"
                                class={CTA_PRIMARY}
                            >
                                "View benchmarks"
                                {methodology_icon}
                            </a>
                        </div>
                    </div>
                </div>
            </section>
        }
        .render(cx)
    }
}

/// The hero preview tab strip, its three page previews, and the benchmark
/// metrics that track the selected tab.
///
/// An rsjs island rather than hand-written JS. The metrics used to be a
/// sibling component that a script rewrote through `querySelector` +
/// `textContent`, kept in sync by a `MutationObserver` on the tab root. One
/// signal owns the selection now, so the panel, the tab contract, and the
/// metric readouts all derive from the same value and there is nothing to
/// observe.
#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Direction18HeroPreview {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal("repo");

        let travel_tab = rsjs::node_ref::<rsjs::HtmlElement>();
        let repo_tab = rsjs::node_ref::<rsjs::HtmlElement>();
        let sentry_tab = rsjs::node_ref::<rsjs::HtmlElement>();

        // Composed in the prelude: an analyzed slot reads a bare struct
        // literal as a component edge, and these are ordinary Proa components.
        let pen_circle = PenCircle;
        let travel_panel = HeroTravelPanel;
        let repo_panel = HeroRepoPanel;
        let sentry_panel = HeroSentryPanel;

        html_sync! {
            <div>
                <div
                    data-component="marketing-tabs"
                    data-tabs-root
                    data_value={value.get()}
                    class="proa-hero-tabs"
                >
                    <div
                        role="tablist"
                        aria-label="Benchmark page previews"
                        class="mb-3 flex h-auto w-full gap-1 !rounded-[14px] border border-black/10 bg-[#efedea] p-1 text-[#777169] dark:border-white/10 dark:bg-white/5 dark:text-zinc-400"
                    >
                        <button
                            ref={repo_tab}
                            id="hero-tab-repo"
                            type="button"
                            role="tab"
                            data-tabs-trigger
                            data-value="repo"
                            data_state={if value.get() == "repo" { "active" } else { "inactive" }}
                            aria-controls="hero-panel-repo"
                            aria_selected={if value.get() == "repo" { true } else { false }}
                            tabindex={if value.get() == "repo" { "0" } else { "-1" }}
                            class="flex-1 !rounded-[12px] px-2 py-2 text-center text-[13px] font-medium text-[#777169] shadow-none transition-colors data-[state=active]:bg-white data-[state=active]:text-zinc-950 data-[state=active]:shadow-none dark:text-zinc-400 dark:data-[state=active]:bg-zinc-950 dark:data-[state=active]:text-zinc-50 sm:px-3"
                            on_click={|_| value.set("repo")}
                            on_keydown={|event: rsjs::KeyboardEvent| {
                                if event.key() == "ArrowRight" {
                                    event.prevent_default();
                                    value.set("sentry");
                                    sentry_tab.focus();
                                }
                                if event.key() == "ArrowLeft" {
                                    event.prevent_default();
                                    value.set("travel");
                                    travel_tab.focus();
                                }
                                if event.key() == "Home" {
                                    event.prevent_default();
                                    value.set("repo");
                                    repo_tab.focus();
                                }
                                if event.key() == "End" {
                                    event.prevent_default();
                                    value.set("travel");
                                    travel_tab.focus();
                                }
                            }}
                        >
                            "GitHub"
                        </button>
                        <button
                            ref={sentry_tab}
                            id="hero-tab-sentry"
                            type="button"
                            role="tab"
                            data-tabs-trigger
                            data-value="sentry"
                            data_state={if value.get() == "sentry" { "active" } else { "inactive" }}
                            aria-controls="hero-panel-sentry"
                            aria_selected={if value.get() == "sentry" { true } else { false }}
                            tabindex={if value.get() == "sentry" { "0" } else { "-1" }}
                            class="flex-1 !rounded-[12px] px-2 py-2 text-center text-[13px] font-medium text-[#777169] shadow-none transition-colors data-[state=active]:bg-white data-[state=active]:text-zinc-950 data-[state=active]:shadow-none dark:text-zinc-400 dark:data-[state=active]:bg-zinc-950 dark:data-[state=active]:text-zinc-50 sm:px-3"
                            on_click={|_| value.set("sentry")}
                            on_keydown={|event: rsjs::KeyboardEvent| {
                                if event.key() == "ArrowRight" {
                                    event.prevent_default();
                                    value.set("travel");
                                    travel_tab.focus();
                                }
                                if event.key() == "ArrowLeft" {
                                    event.prevent_default();
                                    value.set("repo");
                                    repo_tab.focus();
                                }
                                if event.key() == "Home" {
                                    event.prevent_default();
                                    value.set("repo");
                                    repo_tab.focus();
                                }
                                if event.key() == "End" {
                                    event.prevent_default();
                                    value.set("travel");
                                    travel_tab.focus();
                                }
                            }}
                        >
                            "Sentry"
                        </button>
                        <button
                            ref={travel_tab}
                            id="hero-tab-travel"
                            type="button"
                            role="tab"
                            data-tabs-trigger
                            data-value="travel"
                            data_state={if value.get() == "travel" { "active" } else { "inactive" }}
                            aria-controls="hero-panel-travel"
                            aria_selected={if value.get() == "travel" { true } else { false }}
                            tabindex={if value.get() == "travel" { "0" } else { "-1" }}
                            class="flex-1 !rounded-[12px] px-2 py-2 text-center text-[13px] font-medium text-[#777169] shadow-none transition-colors data-[state=active]:bg-white data-[state=active]:text-zinc-950 data-[state=active]:shadow-none dark:text-zinc-400 dark:data-[state=active]:bg-zinc-950 dark:data-[state=active]:text-zinc-50 sm:px-3"
                            on_click={|_| value.set("travel")}
                            on_keydown={|event: rsjs::KeyboardEvent| {
                                if event.key() == "ArrowRight" {
                                    event.prevent_default();
                                    value.set("repo");
                                    repo_tab.focus();
                                }
                                if event.key() == "ArrowLeft" {
                                    event.prevent_default();
                                    value.set("sentry");
                                    sentry_tab.focus();
                                }
                                if event.key() == "Home" {
                                    event.prevent_default();
                                    value.set("repo");
                                    repo_tab.focus();
                                }
                                if event.key() == "End" {
                                    event.prevent_default();
                                    value.set("travel");
                                    travel_tab.focus();
                                }
                            }}
                        >
                            "Airbnb"
                        </button>
                    </div>
                    <div
                        id="hero-panel-repo"
                        role="tabpanel"
                        data-tabs-content
                        data-value="repo"
                        data_state={if value.get() == "repo" { "active" } else { "inactive" }}
                        aria-labelledby="hero-tab-repo"
                        hidden={if value.get() == "repo" { false } else { true }}
                        class="rounded-[20px] bg-white p-3 shadow-[rgba(0,0,0,0.40)_0_0_1px_0,rgba(0,0,0,0.04)_0_1px_1px_0,rgba(0,0,0,0.04)_0_2px_4px_0,rgba(0,0,0,0.03)_0_12px_28px_-12px] dark:bg-white/[0.08] dark:shadow-[0_34px_90px_-62px_rgba(0,0,0,0.9)] dark:ring-1 dark:ring-white/10 sm:p-4"
                    >
                        {repo_panel}
                    </div>
                    <div
                        id="hero-panel-sentry"
                        role="tabpanel"
                        data-tabs-content
                        data-value="sentry"
                        data_state={if value.get() == "sentry" { "active" } else { "inactive" }}
                        aria-labelledby="hero-tab-sentry"
                        hidden={if value.get() == "sentry" { false } else { true }}
                        class="rounded-[20px] bg-white p-3 shadow-[rgba(0,0,0,0.40)_0_0_1px_0,rgba(0,0,0,0.04)_0_1px_1px_0,rgba(0,0,0,0.04)_0_2px_4px_0,rgba(0,0,0,0.03)_0_12px_28px_-12px] dark:bg-white/[0.08] dark:shadow-[0_34px_90px_-62px_rgba(0,0,0,0.9)] dark:ring-1 dark:ring-white/10 sm:p-4"
                    >
                        {sentry_panel}
                    </div>
                    <div
                        id="hero-panel-travel"
                        role="tabpanel"
                        data-tabs-content
                        data-value="travel"
                        data_state={if value.get() == "travel" { "active" } else { "inactive" }}
                        aria-labelledby="hero-tab-travel"
                        hidden={if value.get() == "travel" { false } else { true }}
                        class="rounded-[20px] bg-white p-3 shadow-[rgba(0,0,0,0.40)_0_0_1px_0,rgba(0,0,0,0.04)_0_1px_1px_0,rgba(0,0,0,0.04)_0_2px_4px_0,rgba(0,0,0,0.03)_0_12px_28px_-12px] dark:bg-white/[0.08] dark:shadow-[0_34px_90px_-62px_rgba(0,0,0,0.9)] dark:ring-1 dark:ring-white/10 sm:p-4"
                    >
                        {travel_panel}
                    </div>
                </div>
                <div
                    data-hero-metrics
                    data_active_tab={value.get()}
                    class="mt-4 grid rounded-[14px] border border-black/10 bg-white dark:border-white/10 dark:bg-zinc-950 sm:grid-cols-3"
                >
                    <div class="px-5 py-4">
                        <p
                            data-hero-metric-value="proa"
                            class="font-mono text-[22px] font-medium tracking-[-0.02em] text-[#dd7627]"
                        >
                            {if value.get() == "travel" { "11.5 us" } else if value.get() == "repo" { "23.5 us" } else { "155 us" }}
                        </p>
                        <p
                            data-hero-metric-label="proa"
                            class="mt-1 font-mono text-[11px] tracking-[0.02em] text-[#a59f97]"
                        >
                            "Proa render p50"
                        </p>
                    </div>
                    <div class="border-t border-black/10 px-5 py-4 dark:border-white/10 sm:border-l sm:border-t-0">
                        <p
                            data-hero-metric-value="peer"
                            class="font-mono text-[22px] font-medium tracking-[-0.02em] text-[#777169] dark:text-zinc-300"
                        >
                            {if value.get() == "travel" { "598 us" } else if value.get() == "repo" { "5.56 ms" } else { "25.4 ms" }}
                        </p>
                        <p
                            data-hero-metric-label="peer"
                            class="mt-1 font-mono text-[11px] tracking-[0.02em] text-[#a59f97]"
                        >
                            "React renderToString p50"
                        </p>
                    </div>
                    <div class="relative border-t border-black/10 px-5 py-4 dark:border-white/10 sm:border-l sm:border-t-0">
                        // Below `sm` the cells stack full width, so a circle sized
                        // against the cell is far wider than the number it marks.
                        // This wrapper is a containing block only on mobile, where
                        // it shrink-wraps the value; from `sm` up it goes static so
                        // the circle resolves against the cell exactly as before.
                        <div class="relative inline-block sm:static">
                            {pen_circle}
                            <p
                                data-hero-metric-value="mult"
                                class="relative z-10 font-mono text-[22px] font-medium tracking-[-0.02em] text-[#dd7627] drop-shadow-[0_0_14px_rgba(221,118,39,0.85)]"
                            >
                                {if value.get() == "travel" { "52x" } else if value.get() == "repo" { "237x" } else { "163x" }}
                            </p>
                            <p
                                data-hero-metric-label="mult"
                                class="relative z-10 mt-1 font-mono text-[11px] tracking-[0.02em] text-[#a59f97]"
                            >
                                "faster"
                            </p>
                        </div>
                    </div>
                </div>
                <p class="mt-2 font-mono text-[11px] leading-relaxed text-[#8f877e] dark:text-zinc-500">
                    "Warm production p50s measured on an 8-vCPU AMD EPYC-Milan VPS."
                </p>
            </div>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for HeroTravelPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="rounded-[14px] border border-black/10 bg-white p-4 text-zinc-950 dark:border-white/10 dark:bg-zinc-900 dark:text-zinc-50">
                <div
                    class="grid h-[150px] grid-cols-[2fr_1fr] grid-rows-2 gap-2"
                    aria-label="Airbnb listing photos"
                >
                    <div class="row-span-2 overflow-hidden rounded-[10px] bg-[#f3ede6]">
                        <img
                            src="/static/images/airbnb-preview-exterior-420.webp"
                            srcset="/static/images/airbnb-preview-exterior-420.webp 420w, /static/images/airbnb-preview-exterior-720.webp 720w"
                            sizes="(max-width: 640px) 58vw, 420px"
                            alt="Beachfront home exterior below coastal hills"
                            width="420"
                            height="327"
                            loading="eager"
                            fetchpriority="high"
                            decoding="async"
                            class="h-full w-full object-cover"
                        />
                    </div>
                    <div class="overflow-hidden rounded-[10px] bg-[#eef0f2]">
                        <img
                            src="/static/images/airbnb-preview-deck-210.webp"
                            srcset="/static/images/airbnb-preview-deck-210.webp 210w, /static/images/airbnb-preview-deck-420.webp 420w"
                            sizes="(max-width: 640px) 29vw, 210px"
                            alt="Oceanfront deck with outdoor fire feature"
                            width="210"
                            height="164"
                            loading="eager"
                            decoding="async"
                            class="h-full w-full object-cover"
                        />
                    </div>
                    <div class="overflow-hidden rounded-[10px] bg-[#f1efe8]">
                        <img
                            src="/static/images/airbnb-preview-bedroom-210.webp"
                            srcset="/static/images/airbnb-preview-bedroom-210.webp 210w, /static/images/airbnb-preview-bedroom-420.webp 420w"
                            sizes="(max-width: 640px) 29vw, 210px"
                            alt="Ocean-view living room with fireplace"
                            width="210"
                            height="164"
                            loading="eager"
                            decoding="async"
                            class="h-full w-full object-cover"
                        />
                    </div>
                </div>
                <div class="mt-3 flex items-start justify-between gap-4">
                    <div>
                        <p class="text-lg font-semibold">"Malibu Beach House"</p>
                        <p class="mt-1 text-xs text-[#57544e] dark:text-zinc-400">
                            "Entire home · 4.92 rating · Malibu, California"
                        </p>
                    </div>
                    <p class="whitespace-nowrap text-right">
                        <span class="text-lg font-semibold">"$350"</span>
                        <span class="ml-1 text-[11px] text-[#8a857d] dark:text-zinc-500">
                            "night"
                        </span>
                    </p>
                </div>
                <div class="relative mt-3 rounded-xl border border-black/10 bg-[#faf9f7] p-3 outline outline-[1.5px] outline-[#FF385C] outline-offset-[3px] dark:border-white/10 dark:bg-white/5">
                    <span class="absolute -top-2 left-3 rounded-[5px] bg-[#FF385C] px-2 py-0.5 font-mono text-[9px] text-white">
                        "{DatePicker {}}"
                    </span>
                    <div class="flex gap-1.5">
                        <div class="flex h-10 flex-1 flex-col items-center justify-center rounded-lg bg-black/[0.035]">
                            <span class="font-mono text-[13px] font-medium">"12"</span>
                            <span class="font-mono text-[8px] uppercase text-black/40 dark:text-white/50">
                                "Mon"
                            </span>
                        </div>
                        <div class="flex h-10 flex-1 flex-col items-center justify-center rounded-lg bg-[#FF385C] text-white">
                            <span class="font-mono text-[13px] font-medium">"13"</span>
                            <span class="font-mono text-[8px] uppercase text-white/80">"Tue"</span>
                        </div>
                        <div class="flex h-10 flex-1 flex-col items-center justify-center rounded-lg bg-[#FF385C]/15">
                            <span class="font-mono text-[13px] font-medium">"14"</span>
                            <span class="font-mono text-[8px] uppercase text-black/40 dark:text-white/50">
                                "Wed"
                            </span>
                        </div>
                        <div class="flex h-10 flex-1 flex-col items-center justify-center rounded-lg bg-[#FF385C]/15">
                            <span class="font-mono text-[13px] font-medium">"15"</span>
                            <span class="font-mono text-[8px] uppercase text-black/40 dark:text-white/50">
                                "Thu"
                            </span>
                        </div>
                        <div class="flex h-10 flex-1 flex-col items-center justify-center rounded-lg bg-[#FF385C]/15">
                            <span class="font-mono text-[13px] font-medium">"16"</span>
                            <span class="font-mono text-[8px] uppercase text-black/40 dark:text-white/50">
                                "Fri"
                            </span>
                        </div>
                        <div class="flex h-10 flex-1 flex-col items-center justify-center rounded-lg bg-[#FF385C] text-white">
                            <span class="font-mono text-[13px] font-medium">"17"</span>
                            <span class="font-mono text-[8px] uppercase text-white/80">"Sat"</span>
                        </div>
                        <div class="flex h-10 flex-1 flex-col items-center justify-center rounded-lg bg-black/[0.035]">
                            <span class="font-mono text-[13px] font-medium">"18"</span>
                            <span class="font-mono text-[8px] uppercase text-black/40 dark:text-white/50">
                                "Sun"
                            </span>
                        </div>
                    </div>
                </div>
                <div class="relative mt-4 flex items-center justify-between rounded-xl border border-black/10 bg-white px-3 py-3 shadow-[0_10px_30px_-22px_rgba(0,0,0,0.4)] outline outline-[1.5px] outline-[#FF385C] outline-offset-[3px] dark:border-white/10 dark:bg-zinc-950">
                    <span class="absolute -top-2 left-3 rounded-[5px] bg-[#FF385C] px-2 py-0.5 font-mono text-[9px] text-white">
                        "{BookingBar {}}"
                    </span>
                    <p class="text-[13px]">
                        <span class="text-base font-semibold">"$700"</span>
                        " - 2 nights - all fees"
                    </p>
                    <button
                        type="button"
                        class="rounded-[10px] bg-gradient-to-r from-[#E61E4D] via-[#E31C5F] to-[#D70466] px-5 py-2.5 text-[13px] font-semibold text-white"
                    >
                        "Reserve"
                    </button>
                </div>
            </div>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for HeroRepoPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div
                data-github-preview
                class="overflow-hidden rounded-[14px] border border-[#d0d7de] bg-white text-[#24292f] shadow-sm dark:border-white/10"
            >
                <div class="flex h-10 items-center gap-2 border-b border-[#d8dee4] bg-white px-3 text-[12px] text-[#57606a]">
                    <button
                        type="button"
                        class="inline-flex size-7 items-center justify-center rounded-md border border-transparent text-[#57606a]"
                    >
                        <svg
                            viewBox="0 0 16 16"
                            class="size-4"
                            fill="currentColor"
                            aria-hidden="true"
                        >
                            <path d="M1 2.75A.75.75 0 0 1 1.75 2h12.5a.75.75 0 0 1 0 1.5H1.75A.75.75 0 0 1 1 2.75Zm0 5A.75.75 0 0 1 1.75 7h12.5a.75.75 0 0 1 0 1.5H1.75A.75.75 0 0 1 1 7.75ZM1.75 12h12.5a.75.75 0 0 1 0 1.5H1.75a.75.75 0 0 1 0-1.5Z" />
                        </svg>
                    </button>
                    <svg
                        viewBox="0 0 24 24"
                        class="size-6 shrink-0 text-[#24292f]"
                        fill="currentColor"
                        aria-hidden="true"
                    >
                        <path d="M10.226 17.284c-2.965-.36-5.054-2.493-5.054-5.256 0-1.123.404-2.336 1.078-3.144-.292-.741-.247-2.314.09-2.965.898-.112 2.111.36 2.83 1.01.853-.269 1.752-.404 2.853-.404 1.1 0 1.999.135 2.807.382.696-.629 1.932-1.1 2.83-.988.315.606.36 2.179.067 2.942.72.854 1.101 2 1.101 3.167 0 2.763-2.089 4.852-5.098 5.234.763.494 1.28 1.572 1.28 2.807v2.336c0 .674.561 1.056 1.235.786 4.066-1.55 7.255-5.615 7.255-10.646C23.5 6.188 18.334 1 11.978 1 5.62 1 .5 6.188.5 12.545c0 4.986 3.167 9.12 7.435 10.669.606.225 1.19-.18 1.19-.786V20.63a2.9 2.9 0 0 1-1.078.224c-1.483 0-2.359-.808-2.987-2.313-.247-.607-.517-.966-1.034-1.033-.27-.023-.359-.135-.359-.27 0-.27.45-.471.898-.471.652 0 1.213.404 1.797 1.235.45.651.921.943 1.483.943.561 0 .92-.202 1.437-.719.382-.381.674-.718.944-.943" />
                    </svg>
                    <div class="min-w-0 text-[13px]">
                        <span class="text-[#57606a]">"oven-sh"</span>
                        <span class="px-1 text-[#57606a]">"/"</span>
                        <span class="font-semibold text-[#24292f]">"bun"</span>
                    </div>
                    <button
                        type="button"
                        class="ml-auto hidden h-7 min-w-[150px] items-center gap-2 rounded-md border border-[#d0d7de] bg-[#f6f8fa] px-2 text-left text-[#57606a] sm:flex"
                    >
                        <svg
                            viewBox="0 0 16 16"
                            class="size-3.5"
                            fill="currentColor"
                            aria-hidden="true"
                        >
                            <path d="M10.68 11.74a6 6 0 1 1 1.06-1.06l3.29 3.29a.75.75 0 0 1-1.06 1.06l-3.29-3.29ZM6.5 11a4.5 4.5 0 1 0 0-9 4.5 4.5 0 0 0 0 9Z" />
                        </svg>
                        <span class="truncate">"Type / to search"</span>
                    </button>
                    <span class="size-6 shrink-0 rounded-full bg-[linear-gradient(135deg,#8b949e,#d0d7de)]"></span>
                </div>
                <div class="flex min-h-9 items-center gap-3 overflow-hidden border-b border-[#d8dee4] bg-white px-3 text-[11px] text-[#57606a]">
                    <span class="inline-flex h-9 items-center gap-1 border-b-2 border-transparent">
                        "Code"
                    </span>
                    <span class="inline-flex h-9 items-center gap-1 border-b-2 border-transparent">
                        "Issues"
                        <span class="rounded-full bg-[#afb8c133] px-1.5">"5k+"</span>
                    </span>
                    <span class="inline-flex h-9 items-center gap-1 border-b-2 border-[#fd8c73] font-semibold text-[#24292f]">
                        "Pull requests"
                        <span class="rounded-full bg-[#afb8c133] px-1.5 font-normal">"1.9k"</span>
                    </span>
                    <span class="hidden h-9 items-center border-b-2 border-transparent sm:inline-flex">
                        "Actions"
                    </span>
                    <span class="hidden h-9 items-center border-b-2 border-transparent sm:inline-flex">
                        "Security"
                    </span>
                    <span class="hidden h-9 items-center border-b-2 border-transparent sm:inline-flex">
                        "Insights"
                    </span>
                </div>
                <div class="border-b border-[#d8dee4] bg-white px-3 pt-3">
                    <div class="flex items-start justify-between gap-3">
                        <div class="min-w-0">
                            <h3 class="truncate text-[18px] font-normal leading-6 text-[#24292f]">
                                "[tracking] Zig/C++ fork: Rust->Zig port progress "
                                <span class="text-[#57606a]">"#31136"</span>
                            </h3>
                            <p class="mt-1 flex flex-wrap items-center gap-1 text-[11px] text-[#57606a]">
                                <span class="inline-flex items-center gap-1 rounded-full bg-[#cf222e] px-2 py-0.5 font-semibold text-white">
                                    "Closed"
                                </span>
                                <strong>"MaurerAnton"</strong>
                                <span>"wants to merge 8 commits into"</span>
                                <code class="rounded-md bg-[#ddf4ff] px-1.5 py-0.5 text-[#0969da]">
                                    "oven-sh:main"
                                </code>
                            </p>
                        </div>
                        <button
                            type="button"
                            class="hidden rounded-md border border-[#1f883d] bg-[#1f883d] px-2.5 py-1 text-[11px] font-semibold text-white sm:inline-flex"
                        >
                            "New issue"
                        </button>
                    </div>
                    <div class="mt-3 flex items-end gap-0 overflow-hidden text-[11px] text-[#57606a]">
                        <span class="inline-flex h-8 items-center gap-1 border border-transparent px-2">
                            "Conversation"
                            <span class="rounded-full bg-[#afb8c133] px-1.5">"9"</span>
                        </span>
                        <span class="inline-flex h-8 items-center gap-1 border border-transparent px-2">
                            "Commits"
                            <span class="rounded-full bg-[#afb8c133] px-1.5">"8"</span>
                        </span>
                        <span class="hidden h-8 items-center gap-1 border border-transparent px-2 sm:inline-flex">
                            "Checks"
                            <span class="rounded-full bg-[#afb8c133] px-1.5">"1"</span>
                        </span>
                        <span class="-mb-px inline-flex h-8 items-center gap-1 rounded-t-md border border-[#d0d7de] border-b-white bg-white px-2 font-medium text-[#24292f]">
                            "Files changed"
                            <span class="rounded-full bg-[#afb8c133] px-1.5 font-normal">"16"</span>
                        </span>
                        <span class="ml-auto hidden items-center gap-2 pb-2 font-semibold sm:flex">
                            <span class="text-[#1a7f37]">"+461"</span>
                            <span class="text-[#cf222e]">"-10"</span>
                            <span class="grid grid-cols-5 gap-0.5">
                                <span class="size-1.5 rounded-[1px] bg-[#2da44e]"></span>
                                <span class="size-1.5 rounded-[1px] bg-[#2da44e]"></span>
                                <span class="size-1.5 rounded-[1px] bg-[#2da44e]"></span>
                                <span class="size-1.5 rounded-[1px] bg-[#2da44e]"></span>
                                <span class="size-1.5 rounded-[1px] bg-[#8c959f]"></span>
                            </span>
                        </span>
                    </div>
                </div>
                <div class="grid min-h-[236px] bg-white sm:grid-cols-[150px_minmax(0,1fr)]">
                    <aside class="hidden border-r border-[#d8dee4] bg-white p-2 text-[10.5px] text-[#57606a] sm:block">
                        <label class="relative block">
                            <span class="absolute left-2 top-1.5 text-[#57606a]">"⌕"</span>
                            <input
                                type="search"
                                placeholder="Filter changed files"
                                class="h-7 w-full rounded-md border border-[#d0d7de] bg-white pl-6 pr-2 text-[11px] outline-none"
                            />
                        </label>
                        <div class="mt-2 space-y-0.5">
                            <p class="rounded-md px-2 py-1 text-[#24292f]">"src"</p>
                            <p class="rounded-md px-2 py-1 text-[#24292f]">"install"</p>
                            <p class="rounded-md bg-[#f6f8fa] px-2 py-1 text-[#24292f]">
                                "PackageJSONEditor.zig"
                            </p>
                            <p class="rounded-md px-2 py-1">"events.ts"</p>
                            <p class="rounded-md px-2 py-1">"lexer.zig"</p>
                            <p class="rounded-md px-2 py-1">"bun-add.test.ts"</p>
                        </div>
                    </aside>
                    <div class="min-w-0 bg-[#f6f8fa] p-2">
                        <div class="overflow-hidden rounded-md border border-[#d0d7de] bg-white">
                            <div class="flex min-h-9 items-center gap-2 border-b border-[#d0d7de] bg-[#f6f8fa] px-2 text-[11px] text-[#57606a]">
                                <span class="font-semibold text-[#24292f]">
                                    "src/install/PackageManager/PackageJSONEditor.zig"
                                </span>
                                <span class="ml-auto text-[#1a7f37]">"+3"</span>
                                <span class="text-[#cf222e]">"-1"</span>
                            </div>
                            <div class="font-mono text-[10.5px] leading-[18px] text-[#24292f]">
                                <div class="grid grid-cols-[34px_34px_minmax(0,1fr)] bg-[#ddf4ff]">
                                    <span class="border-r border-[#b6e3ff] px-2 text-right text-[#57606a]"></span>
                                    <span class="border-r border-[#b6e3ff] px-2 text-right text-[#57606a]"></span>
                                    <span class="truncate px-2 text-[#57606a]">
                                        "@@ -459,7 +459,9 @@ pub fn edit("
                                    </span>
                                </div>
                                <div class="grid grid-cols-[34px_34px_minmax(0,1fr)]">
                                    <span class="border-r border-[#d8dee4] px-2 text-right text-[#57606a]">
                                        "459"
                                    </span>
                                    <span class="border-r border-[#d8dee4] px-2 text-right text-[#57606a]">
                                        "459"
                                    </span>
                                    <span class="truncate px-2">
                                        "                                }"
                                    </span>
                                </div>
                                <div class="grid grid-cols-[34px_34px_minmax(0,1fr)]">
                                    <span class="border-r border-[#d8dee4] px-2 text-right text-[#57606a]">
                                        "460"
                                    </span>
                                    <span class="border-r border-[#d8dee4] px-2 text-right text-[#57606a]">
                                        "460"
                                    </span>
                                    <span class="truncate px-2">
                                        "                                break;"
                                    </span>
                                </div>
                                <div class="grid grid-cols-[34px_34px_minmax(0,1fr)] bg-[#ffebe9]">
                                    <span class="border-r border-[#ffcecb] bg-[#ffcecb] px-2 text-right">
                                        "462"
                                    </span>
                                    <span class="border-r border-[#ffcecb] px-2 text-right"></span>
                                    <span class="truncate px-2">
                                        "-                                if (request.version.tag == .github or request.version.tag == .git) {"
                                    </span>
                                </div>
                                <div class="grid grid-cols-[34px_34px_minmax(0,1fr)] bg-[#dafbe1]">
                                    <span class="border-r border-[#aceebb] px-2 text-right"></span>
                                    <span class="border-r border-[#aceebb] bg-[#aceebb] px-2 text-right">
                                        "462"
                                    </span>
                                    <span class="truncate px-2">
                                        "+                                if (request.e_string == null and !request.is_aliased and"
                                    </span>
                                </div>
                                <div class="grid grid-cols-[34px_34px_minmax(0,1fr)] bg-[#dafbe1]">
                                    <span class="border-r border-[#aceebb] px-2 text-right"></span>
                                    <span class="border-r border-[#aceebb] bg-[#aceebb] px-2 text-right">
                                        "463"
                                    </span>
                                    <span class="truncate px-2">
                                        "+                                    (request.version.tag == .github or request.version.tag == .git or request.version.tag == .tarball))"
                                    </span>
                                </div>
                                <div class="grid grid-cols-[34px_34px_minmax(0,1fr)]">
                                    <span class="border-r border-[#d8dee4] px-2 text-right text-[#57606a]">
                                        "463"
                                    </span>
                                    <span class="border-r border-[#d8dee4] px-2 text-right text-[#57606a]">
                                        "465"
                                    </span>
                                    <span class="truncate px-2">
                                        "                                    for (query.expr.data.e_object.properties.slice()) |item| {"
                                    </span>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for HeroSentryPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div
                data-sentry-preview
                class="min-h-[390px] overflow-hidden rounded-[14px] border border-[#ded7e5] bg-white text-[#2f2936] shadow-sm"
            >
                <header class="flex h-10 items-center gap-2 border-b border-[#e5dfea] bg-white px-3 text-[11px] text-[#6b6174]">
                    <span class="grid size-6 place-items-center rounded-md bg-[#6c5fc7] text-[9px] font-bold text-white">
                        "SD"
                    </span>
                    <span>"Traces"</span>
                    <span class="text-[#aaa0b2]">"/"</span>
                    <span class="size-3 rounded-full bg-[#7b6fd0]"></span>
                    <strong class="min-w-0 truncate text-[#342a3f]">
                        "3a6f53d4f70e4b37b0f14c9b"
                    </strong>
                    <button
                        type="button"
                        class="ml-auto hidden rounded-md border border-[#d9d1e0] bg-[#f8f6fa] px-2.5 py-1 font-medium text-[#554a60] sm:inline-flex"
                    >
                        "Ask Seer"
                    </button>
                </header>
                <div class="grid min-h-[350px] grid-cols-[42px_minmax(0,1fr)]">
                    <aside
                        aria-label="Sentry preview navigation"
                        class="flex flex-col items-center gap-3 border-r border-[#e5dfea] bg-[#f7f5f9] py-3 text-[11px] text-[#81768b]"
                    >
                        <span class="grid size-6 place-items-center rounded-md bg-[#ede9f1]">
                            "!"
                        </span>
                        <span class="grid size-6 place-items-center rounded-md bg-[#6c5fc7] font-semibold text-white">
                            "⌁"
                        </span>
                        <span class="grid size-6 place-items-center">"▦"</span>
                        <span class="grid size-6 place-items-center">"◫"</span>
                        <span class="mt-auto grid size-6 place-items-center rounded-full bg-[#342a3f] text-[8px] font-bold text-white">
                            "PX"
                        </span>
                    </aside>
                    <div class="min-w-0">
                        <section class="border-b border-[#e5dfea] px-3 py-2.5">
                            <div class="flex items-start justify-between gap-3">
                                <div class="min-w-0">
                                    <p class="truncate text-[14px] font-semibold text-[#2f2936]">
                                        "GET /api/0/organizations/proa/events/"
                                    </p>
                                    <p class="mt-0.5 text-[10px] text-[#81768b]">
                                        "production · React · Chrome"
                                    </p>
                                </div>
                                <span class="rounded-full bg-[#ece8f7] px-2 py-0.5 text-[9px] font-semibold text-[#574ca5]">
                                    "1 replay"
                                </span>
                            </div>
                            <dl class="mt-2 grid grid-cols-5 gap-1 text-[9px] text-[#81768b]">
                                <div>
                                    <dt>"Issues"</dt>
                                    <dd class="mt-0.5 font-semibold text-[#6c5fc7]">"1"</dd>
                                </div>
                                <div>
                                    <dt>"Spans"</dt>
                                    <dd class="mt-0.5 font-semibold text-[#342a3f]">"18"</dd>
                                </div>
                                <div>
                                    <dt>"Age"</dt>
                                    <dd class="mt-0.5 font-semibold text-[#342a3f]">"4m"</dd>
                                </div>
                                <div>
                                    <dt>"Duration"</dt>
                                    <dd class="mt-0.5 font-semibold text-[#342a3f]">"238ms"</dd>
                                </div>
                                <div>
                                    <dt>"Logs"</dt>
                                    <dd class="mt-0.5 font-semibold text-[#342a3f]">"400"</dd>
                                </div>
                            </dl>
                        </section>
                        <div class="flex h-9 items-end justify-between border-b border-[#e5dfea] px-3 text-[10px] text-[#756a80]">
                            <div class="flex h-full items-end gap-4">
                                <span class="pb-2">"Waterfall"</span>
                                <span class="border-b-2 border-[#6c5fc7] pb-2 font-semibold text-[#342a3f]">
                                    "Logs"
                                </span>
                            </div>
                            <div class="hidden gap-2 pb-2 font-mono text-[8px] sm:flex">
                                <span>"LCP 108ms"</span>
                                <span>"INP 64ms"</span>
                                <span>"TTFB 12ms"</span>
                            </div>
                        </div>
                        <div class="flex h-10 items-center gap-2 border-b border-[#e5dfea] bg-[#fbfafc] px-2.5">
                            <label class="flex h-6 min-w-0 flex-1 items-center gap-1.5 rounded-md border border-[#dcd5e2] bg-white px-2 text-[9px] text-[#918698]">
                                <span>"⌕"</span>
                                <span class="truncate">"Search logs for this event"</span>
                            </label>
                            <span class="hidden rounded-md border border-[#dcd5e2] bg-white px-2 py-1 text-[8px] font-medium text-[#574d61] sm:inline-flex">
                                "Auto-refresh"
                            </span>
                        </div>
                        <div class="divide-y divide-[#eeeaf1] font-mono text-[9px] leading-4">
                            <div class="grid grid-cols-[52px_44px_minmax(0,1fr)] gap-2 px-2.5 py-2">
                                <span class="text-[#928899]">"09:41:08"</span>
                                <span class="text-[#267a5d]">"info"</span>
                                <span class="truncate text-[#403748]">
                                    "request completed status=200 duration=17ms"
                                </span>
                            </div>
                            <div class="grid grid-cols-[52px_44px_minmax(0,1fr)] gap-2 bg-[#fcfbfd] px-2.5 py-2">
                                <span class="text-[#928899]">"09:41:08"</span>
                                <span class="text-[#267a5d]">"info"</span>
                                <span class="truncate text-[#403748]">
                                    "cache hit key=organization:proa"
                                </span>
                            </div>
                            <div class="grid grid-cols-[52px_44px_minmax(0,1fr)] gap-2 px-2.5 py-2">
                                <span class="text-[#928899]">"09:41:08"</span>
                                <span class="text-[#b2671e]">"warn"</span>
                                <span class="truncate text-[#403748]">
                                    "slow query detected db.system=postgresql"
                                </span>
                            </div>
                            <div class="grid grid-cols-[52px_44px_minmax(0,1fr)] gap-2 bg-[#fcfbfd] px-2.5 py-2">
                                <span class="text-[#928899]">"09:41:08"</span>
                                <span class="text-[#267a5d]">"info"</span>
                                <span class="truncate text-[#403748]">
                                    "span finished op=http.server status=ok"
                                </span>
                            </div>
                            <div class="grid grid-cols-[52px_44px_minmax(0,1fr)] gap-2 px-2.5 py-2">
                                <span class="text-[#928899]">"09:41:09"</span>
                                <span class="text-[#267a5d]">"info"</span>
                                <span class="truncate text-[#403748]">
                                    "trace export complete batch.size=64"
                                </span>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ArrowUpRightIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                viewBox="0 0 24 24"
                class="size-4"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
            >
                <path d="M7 17 17 7" />
                <path d="M7 7h10v10" />
            </svg>
        }
        .render(cx)
    }
}

#[allow(dead_code)]
struct BarView {
    class: &'static str,
    width: BarWidth,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BarView {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        match self.width {
            BarWidth::Pct1 => html_sync! {
                <div class={self.class} style="width: 1%;"></div>
            }
            .render(cx),
            BarWidth::Pct3 => html_sync! {
                <div class={self.class} style="width: 3%;"></div>
            }
            .render(cx),
            BarWidth::Pct4 => html_sync! {
                <div class={self.class} style="width: 4%;"></div>
            }
            .render(cx),
            BarWidth::Pct12 => html_sync! {
                <div class={self.class} style="width: 12%;"></div>
            }
            .render(cx),
            BarWidth::Pct72 => html_sync! {
                <div class={self.class} style="width: 72%;"></div>
            }
            .render(cx),
            BarWidth::Pct86 => html_sync! {
                <div class={self.class} style="width: 86%;"></div>
            }
            .render(cx),
            BarWidth::Full => html_sync! {
                <div class={self.class} style="width: 100%;"></div>
            }
            .render(cx),
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComposableSourceCode {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        StaticRaw(COMPOSABLE_SOURCE_HTML).render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComposableMdRenderCode {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        StaticRaw(COMPOSABLE_MD_RENDER_HTML).render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComposableWebRenderCode {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        StaticRaw(COMPOSABLE_WEB_RENDER_HTML).render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComposableMdChildCode {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        StaticRaw(COMPOSABLE_MD_CHILD_HTML).render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComposableWebChildCode {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        StaticRaw(COMPOSABLE_WEB_CHILD_HTML).render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComposableConnectorLayer {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        StaticRaw(COMPOSABLE_CONNECTORS_HTML).render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ConversionLatencyGraph {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        StaticRaw(CONVERSION_LATENCY_GRAPH_HTML).render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for CapabilityIllustration {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        match self.variant {
            CapabilityVariant::Framework => StaticRaw(CAPABILITY_FRAMEWORK_HTML).render(cx),
            CapabilityVariant::Islands => StaticRaw(CAPABILITY_ISLANDS_HTML).render(cx),
            CapabilityVariant::Integration => StaticRaw(CAPABILITY_INTEGRATION_HTML).render(cx),
        }
    }
}

/*
struct UseCaseCardView {
    card: &'static TextCard,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for UseCaseCardView {

    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let card = self.card;
        html_sync! {
            <div class="rounded-[24px] border border-white/10 bg-white/[0.045] p-5">
                <p class="text-[10px] uppercase tracking-[0.22em] text-white/55">"High-traffic surface"</p>
                <h3 class="mt-3 font-display text-2xl font-semibold tracking-[-0.03em] text-white">{card.title}</h3>
                <p class="mt-3 text-sm leading-7 text-white/65">{card.detail}</p>
            </div>
        }
        .render(cx)
    }
}
*/

#[allow(dead_code)]
struct FrameworkRowView {
    row: &'static FrameworkRow,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for FrameworkRowView {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let row = self.row;
        html_sync! {
            <div>
                <div class="mb-2 flex items-end justify-between gap-4">
                    <div>
                        <p class="text-sm font-medium">{row.name}</p>
                        <p class="text-xs text-zinc-500 dark:text-zinc-400">{row.runtime}</p>
                    </div>
                    <p class="font-display text-xl font-semibold tracking-[-0.04em] tabular-nums">
                        {row.latency}
                    </p>
                </div>
                <div class="h-3 rounded-full bg-zinc-200/80 dark:bg-white/10">
                    {BarView {
                        class: "h-full rounded-full bg-[var(--color-brand)]",
                        width: row.width,
                    }}
                </div>
            </div>
        }
        .render(cx)
    }
}

struct CapabilitySupportView {
    card: &'static TextCard,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for CapabilitySupportView {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let card = self.card;
        html_sync! {
            <div class="rounded-[24px] border border-black/10 bg-[#f5f1e8]/80 p-4 dark:border-white/10 dark:bg-white/5">
                <h4 class="font-display text-xl font-semibold tracking-[-0.03em]">{card.title}</h4>
                <p class="mt-2 text-sm leading-7 text-zinc-600 dark:text-zinc-300">{card.detail}</p>
            </div>
        }
        .render(cx)
    }
}

struct CapabilityColumnView {
    column: &'static CapabilityColumn,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for CapabilityColumnView {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let column = self.column;
        html_sync! {
            <div class="flex flex-col gap-4">
                <div class="rounded-[30px] border border-black/10 bg-white/86 p-5 shadow-[0_28px_80px_-58px_rgba(24,24,27,0.42)] dark:border-white/10 dark:bg-zinc-900/82 dark:shadow-[0_34px_90px_-62px_rgba(0,0,0,0.9)]">
                    {CapabilityIllustration { variant: column.variant }}
                    <p class="mt-5 text-[11px] uppercase tracking-[0.22em] text-zinc-600 dark:text-zinc-400">
                        {column.label}
                    </p>
                    <h3 class="mt-3 text-balance font-display text-2xl font-semibold tracking-[-0.03em]">
                        {column.title}
                    </h3>
                    <p class="mt-3 text-sm leading-7 text-zinc-600 dark:text-zinc-300">
                        {column.detail}
                    </p>
                </div>
                {for item in column.supporting {
                    CapabilitySupportView { card: item }
                }}
            </div>
        }
        .render(cx)
    }
}

#[allow(dead_code)]
struct SimplificationCardView {
    card: &'static TextCard,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for SimplificationCardView {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let card = self.card;
        html_sync! {
            <div class="rounded-[24px] border border-white/10 bg-white/[0.045] p-5">
                <p class="text-[11px] uppercase tracking-[0.18em] text-white/55">"Optional now"</p>
                <h3 class="mt-3 font-display text-2xl font-semibold tracking-[-0.03em] text-white">
                    {card.title}
                </h3>
                <p class="mt-3 text-sm leading-7 text-white/65">{card.detail}</p>
            </div>
        }
        .render(cx)
    }
}

/// The source rail's inert, server-rendered component previews are imported as
/// static HTML so this workspace does not depend on the downstream UI library.
/// Refresh the complete rail with `scripts/import-proa-ui-rail.mjs`.
struct ProaUiSection;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ProaUiSection {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section
                id="proa-ui-components"
                data-proa-ui
                aria-labelledby="proa-ui-heading"
                class={SECTION_SHELL}
            >
                <link
                    rel="stylesheet"
                    href={concat!("/static/proa-ui-rail.css?v=", env!("MARKETING_ASSET_VERSION"))}
                />
                <div class={SECTION_CONTAINER}>
                    <div class={SECTION_HEADER}>
                        <h2 id="proa-ui-heading" class={SECTION_TITLE}>
                            "Build your design system."
                        </h2>
                        <p class={SECTION_LEAD}>
                            "Proa UI offers "
                            <a
                                href={SHADCN_URL}
                                target="_blank"
                                rel="noopener noreferrer"
                                class={LEAD_LINK}
                            >
                                "Shadcn"
                            </a>
                            " style Proa components. Source in your repo, in typed Rust. Theme it, compose it, ship it."
                        </p>
                        <div class={SECTION_ACTIONS}>
                            <a
                                href="https://ui.proa.so/components"
                                target="_blank"
                                rel="noopener noreferrer"
                                class={CTA_PRIMARY}
                            >
                                "Browse Components"
                                {ArrowUpRightIcon}
                            </a>
                        </div>
                    </div>
                </div>
                {StaticRaw(include_str!("home/proa_ui_rail.html"))}
            </section>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for HomePage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div data-page="home" class="overflow-hidden">
                {InterfaceLayerHero}
                <section
                    id="render-targets"
                    class={SECTION_SHELL}
                >
                    <div class={SECTION_CONTAINER}>
                        <div class={SECTION_HEADER}>
                            <h2 class={SECTION_TITLE}>"One object. Every render target."</h2>
                            <p class={SECTION_LEAD}>
                                "HTML, Markdown, or another target, from one typed component."
                            </p>
                            <div class={SECTION_ACTIONS}>
                                <a
                                    href="https://youtu.be/pJx84jdBDKU?si=z3pK3vvzYmSo-Sz_"
                                    data-video-cta="render-targets"
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    aria-label="Watch multi-target rendering on YouTube (opens in a new tab)"
                                    class={CTA_VIDEO}
                                >
                                    {PlayIcon}
                                    "Watch multi-target rendering"
                                </a>
                            </div>
                        </div>
                        <div
                            data-composable-flow
                            class="relative isolate mt-12 grid gap-3 sm:mt-14 lg:grid-cols-[minmax(190px,0.65fr)_minmax(0,1.25fr)_minmax(0,0.9fr)] lg:grid-rows-2 lg:items-center lg:gap-x-16 lg:gap-y-12"
                        >
                            {ComposableConnectorLayer}
                            <div
                                data-flow-node="source"
                                class="relative z-10 min-w-0 lg:row-span-2 lg:self-center"
                            >
                                {ComposableSourceCode}
                            </div>
                            <div data-flow-node="md-render" class="relative z-10 min-w-0">
                                {ComposableMdRenderCode}
                            </div>
                            <div data-flow-node="md-child" class="relative z-10 min-w-0">
                                {ComposableMdChildCode}
                            </div>
                            <div data-flow-node="web-render" class="relative z-10 min-w-0">
                                {ComposableWebRenderCode}
                            </div>
                            <div data-flow-node="web-child" class="relative z-10 min-w-0">
                                {ComposableWebChildCode}
                            </div>
                        </div>
                    </div>
                </section>
                {Direction18Hero}
                {BenchmarkHighlights}
                {ProaUiSection}
                <script>{StaticRaw(COMPOSABLE_CONNECTORS_SCRIPT)}</script>
                <section class={SECTION_SHELL}>
                    <div class={SECTION_CONTAINER}>
                        <div class={SECTION_HEADER}>
                            <h2 class={SECTION_TITLE}>
                                "Everything people want from a modern web app, without the render tax underneath it."
                            </h2>
                            <p class={SECTION_LEAD}>
                                "A component system and a framework surface, not a templating engine."
                            </p>
                        </div>
                        <div class="mt-12 grid gap-4 sm:mt-14 lg:grid-cols-3">
                            {for column in CAPABILITY_COLUMNS {
                                CapabilityColumnView { column }
                            }}
                        </div>
                    </div>
                </section>
                <section class={SECTION_SHELL}>
                    <div class={SECTION_CONTAINER}>
                        <div class="rounded-[36px] border border-black/10 bg-[#f4f0e8] p-6 dark:border-white/10 dark:bg-[#111215] sm:p-10">
                            <div class={SECTION_HEADER}>
                                <h2 class={SECTION_TITLE}>"Start building with Proa."</h2>
                                <div class={SECTION_ACTIONS}>
                                    <a href="/docs" class={CTA_PRIMARY}>
                                        "Read the quickstart"
                                        {ArrowRightIcon}
                                    </a>
                                </div>
                            </div>
                        </div>
                    </div>
                </section>
            </div>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::Direction18HeroPreview;
    use proa_core::{RenderBuffer, WebContext, WebRenderSync};

    #[test]
    fn proa_ui_rail_preserves_inert_previews_and_catalog_links() {
        let mut cx = WebContext::new();
        super::ProaUiSection.render(&mut cx).unwrap();
        let html = cx.into_output().into_string();

        assert_eq!(html.matches("class=\"home-rail-item").count(), 11);
        assert_eq!(html.matches(" inert").count(), 11);
        assert_eq!(html.matches("class=\"home-rail-name\"").count(), 11);
        assert_eq!(
            html.matches("href=\"https://ui.proa.so/components").count(),
            12
        );
        assert_eq!(html.matches("target=\"_blank\"").count(), 13);
        assert_eq!(html.matches("rel=\"noopener noreferrer\"").count(), 13);
        assert!(html.contains("https://ui.shadcn.com"));
        assert!(html.contains("tabindex=\"0\""));
        assert!(html.contains("aria-describedby=\"home-rail-help\""));
        assert!(html.contains("id=\"home-rail-help\""));
        assert!(!html.contains("data-rsjs-"));
        assert!(!html.contains("<script"));
    }

    #[test]
    fn hero_preview_preserves_branded_panels_and_tab_contract() {
        let mut output = RenderBuffer::new();
        let mut cx = WebContext::with_buffer(&mut output);

        Direction18HeroPreview.render(&mut cx).unwrap();

        let html = output.into_string();
        assert_eq!(html.matches(r#"role="tab""#).count(), 3);
        assert_eq!(html.matches(r#"role="tabpanel""#).count(), 3);
        assert_eq!(html.matches(r#"aria-selected="true""#).count(), 1);
        assert_eq!(html.matches(r#"aria-selected="false""#).count(), 2);

        for (tab_id, panel_id) in [
            ("hero-tab-repo", "hero-panel-repo"),
            ("hero-tab-sentry", "hero-panel-sentry"),
            ("hero-tab-travel", "hero-panel-travel"),
        ] {
            assert!(html.contains(&format!(r#"id="{tab_id}""#)));
            assert!(html.contains(&format!(r#"aria-controls="{panel_id}""#)));
            assert!(html.contains(&format!(r#"id="{panel_id}""#)));
            assert!(html.contains(&format!(r#"aria-labelledby="{tab_id}""#)));
        }

        let strip = ["GitHub", "Sentry", "Airbnb"].map(|label| {
            html.find(&format!(">{label}<"))
                .unwrap_or_else(|| panic!("tab `{label}` renders"))
        });
        assert!(
            strip[0] < strip[1] && strip[1] < strip[2],
            "hero tabs must render GitHub, Sentry, Airbnb in that order"
        );

        assert!(html.contains("/static/images/airbnb-preview-"));
        assert!(html.contains("data-github-preview"));
        assert!(html.contains("data-sentry-preview"));
        assert!(!html.contains("/static/images/nike/"));
        assert!(!html.contains("/static/images/synthetic/"));

        // The metrics moved inside this island so they can derive from the
        // same signal as the tabs. Server render is the first tab, GitHub.
        assert!(html.contains("data-hero-metrics"));
        assert!(html.contains(r#"data-active-tab="repo""#));
        assert_eq!(html.matches("data-hero-metric-value=").count(), 3);
        for metric in ["proa", "peer", "mult"] {
            assert!(html.contains(&format!(r#"data-hero-metric-value="{metric}""#)));
        }
    }

    #[test]
    fn benchmark_charts_preserve_tabs_and_local_svg_links() {
        let mut cx = WebContext::new();
        super::BenchmarkHighlights.render(&mut cx).unwrap();
        let html = cx.into_output().into_string();

        assert_eq!(html.matches(r#"role="tab""#).count(), 3);
        assert_eq!(html.matches(r#"role="tabpanel""#).count(), 3);
        assert_eq!(html.matches(r#"aria-selected="true""#).count(), 1);
        assert_eq!(html.matches(r#"aria-selected="false""#).count(), 2);
        assert_eq!(html.matches(" hidden").count(), 2);
        assert_eq!(html.matches("<img ").count(), 3);

        for (tab, asset) in [
            ("render", "01-canonical-scoreboard.svg"),
            ("http", "21-canonical-pages-http.svg"),
            ("load", "20-suspense-load.svg"),
        ] {
            assert!(html.contains(&format!(r#"id="benchmark-tab-{tab}""#)));
            assert!(html.contains(&format!(r#"aria-controls="benchmark-panel-{tab}""#)));
            assert!(html.contains(&format!(r#"id="benchmark-panel-{tab}""#)));
            assert!(html.contains(&format!(r#"aria-labelledby="benchmark-tab-{tab}""#)));
            assert!(html.contains(&format!(r#"src="/static/images/benchmarks/{asset}""#)));
        }

        assert!(html.contains(r#"href="/static/images/benchmarks/01-canonical-scoreboard.svg""#));
        assert!(html.contains("data-rsjs-island-root"));
    }

    /// The tab widgets were driven by `HERO_TABS_SCRIPT`, a hand-written DOM
    /// script. They are rsjs islands now, so the page must ship the runtime
    /// and their island modules or the tabs are inert.
    #[test]
    fn home_page_ships_hydration_for_its_tab_islands() {
        let html = crate::layout::render_page(
            crate::layout::PageMeta {
                title: "Home",
                description: "Home",
                path: "/",
            },
            super::HomePage,
        )
        .0;

        assert!(
            html.contains(r#"<script type="module" src="/rsjs-runtime.js""#),
            "home page must load the rsjs runtime"
        );
        // Search modal, hero tabs, and benchmark chart tabs.
        assert!(
            html.matches("/rsjs/island/").count() >= 3,
            "expected an island module per hydrated island, got: {}",
            html.matches("/rsjs/island/").count()
        );

        // The hero tab root still renders its server-side contract.
        assert_eq!(html.matches("data-tabs-root").count(), 1);
        assert!(html.contains("proa-hero-tabs"));

        // The replaced scripts are gone.
        assert!(!html.contains("metricsByTab"));
        assert!(!html.contains("MutationObserver"));
        assert!(!html.contains("IntersectionObserver"));
    }

    #[test]
    fn homepage_metadata_fits_social_preview_limits() {
        use futures::FutureExt;

        let html = super::handler()
            .now_or_never()
            .expect("homepage renders without I/O")
            .0;
        let head = html.split_once("</head>").unwrap().0;
        let title = head
            .split_once("<title>")
            .unwrap()
            .1
            .split_once("</title>")
            .unwrap()
            .0;
        let description = head
            .split_once(r#"name="description" content=""#)
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0;
        assert!(!title.is_empty() && title.chars().count() <= 70);
        // SEO auditors join every `<title>` in the document, inline SVG
        // titles included, into one over-long page title.
        assert_eq!(html.matches("<title>").count(), 1);
        assert!(head.contains(
            r#"<link rel="apple-touch-icon" sizes="180x180" href="/static/apple-touch-icon.png?v="#
        ));
        assert!(!description.is_empty() && description.chars().count() <= 125);
        for name in ["og:description", "twitter:description"] {
            let tag = head.split_once(&format!(r#"name="{name}""#)).unwrap().1;
            let content = tag
                .split_once(r#"content=""#)
                .unwrap()
                .1
                .split_once('"')
                .unwrap()
                .0;
            assert_eq!(content, description);
        }
        for name in ["og:title", "twitter:title"] {
            let tag = head.split_once(&format!(r#"name="{name}""#)).unwrap().1;
            let content = tag
                .split_once(r#"content=""#)
                .unwrap()
                .1
                .split_once('"')
                .unwrap()
                .0;
            assert_eq!(content, title);
        }
    }

    /// The centered hero opens the page, with video links scoped to the intro
    /// and render-target sections before the performance content.
    #[test]
    fn interface_layer_hero_opens_the_page() {
        let html = crate::layout::render_page(
            crate::layout::PageMeta {
                title: "Home",
                description: "Home",
                path: "/",
            },
            super::HomePage,
        )
        .0;

        assert_eq!(html.matches("<h1").count(), 1);
        assert!(html.contains(r#"data-hero="interface-layer""#));
        assert!(html.contains(r#"id="render-targets""#));

        let hero = html
            .find(r#"data-hero="interface-layer""#)
            .expect("statement hero renders");
        let benchmark_hero = html
            .find("data-hero-inner")
            .expect("benchmark hero renders");
        let render_targets = html.find("id=\"render-targets\"").unwrap();
        let benchmark_highlights = html.find("id=\"benchmark-highlights\"").unwrap();
        let ui_components = html.find("id=\"proa-ui-components\"").unwrap();
        assert!(
            render_targets < benchmark_hero
                && benchmark_hero < benchmark_highlights
                && benchmark_highlights < ui_components,
            "render targets, then the two performance sections, then the design system rail"
        );
        assert!(
            hero < benchmark_hero,
            "the statement hero must render above the benchmark hero"
        );

        let hero_section = &html[hero..render_targets];
        let render_section = &html[render_targets..benchmark_hero];
        let intro_video = "https://youtu.be/kGhNsD772gg?si=s-YcVTx78kJ1M8o6";
        let render_video = "https://youtu.be/pJx84jdBDKU?si=z3pK3vvzYmSo-Sz_";
        assert!(
            hero_section.find("href=\"/docs\"").unwrap() < hero_section.find(intro_video).unwrap()
        );
        assert!(render_section.find("</p>").unwrap() < render_section.find(render_video).unwrap());
        assert!(
            render_section.find(render_video).unwrap()
                < render_section.find("data-composable-flow").unwrap()
        );
        assert!(!hero_section.contains(render_video));
        assert!(!render_section.contains(intro_video));

        for (section, url) in [(hero_section, intro_video), (render_section, render_video)] {
            assert_eq!(html.matches(url).count(), 1);
            let link = section
                .split_once(url)
                .unwrap()
                .1
                .split_once("</a>")
                .unwrap()
                .0;
            assert!(link.contains(r#"target="_blank""#));
            assert!(link.contains(r#"rel="noopener noreferrer""#));
            assert!(link.contains("opens in a new tab"));
            assert!(link.contains(r#"aria-hidden="true""#));
            assert!(link.contains(r#"focusable="false""#));
            assert!(link.contains("data-play-icon"));
        }
    }
}
