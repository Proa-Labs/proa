// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::response::Html;
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

use crate::layout::{render_chained_html, PageMeta};

use super::data::{
    ecosystem_data, AppStatus, AppVisual, EcosystemApp, EcosystemData, EcosystemFilter,
};
use super::layout::{EcosystemMockFooter, EcosystemStandaloneDocument};

pub(crate) fn render(path: &'static str) -> Html<String> {
    // proa-perf-ok: top-level Axum response finalization owns the HTML String
    render_chained_html(EcosystemStandaloneDocument {
            meta: PageMeta {
                title: "Ecosystem Apps - Proa",
                description:
                    "Production applications built on Proa: booking platforms, realtime dashboards, storefronts, and agent consoles.",
                path,
            },
            content: EcosystemAppsPage {
                data: ecosystem_data(),
            },
        },
    )
}

const REFERENCE_SHOWCASE_VIDEO_URL: &str =
    "https://pub-4b6fd181929c45ccb532e271c8671f26.r2.dev/showcase.mp4";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlatformDownloadKind {
    AppleSilicon,
    Linux,
    Windows,
    Docker,
}

#[derive(Debug, Clone, Copy)]
struct PlatformDownloadOption {
    kind: PlatformDownloadKind,
    href: &'static str,
    action: &'static str,
}

const PLATFORM_DOWNLOAD_OPTIONS: &[PlatformDownloadOption] = &[
    PlatformDownloadOption {
        kind: PlatformDownloadKind::AppleSilicon,
        href: "https://pub-cf0f833f7be646a38360802f5e0c7829.r2.dev/playground/proa-playground-aarch64-apple-darwin.tar.gz",
        action: "Download for Apple Silicon",
    },
    PlatformDownloadOption {
        kind: PlatformDownloadKind::Linux,
        href: "https://pub-cf0f833f7be646a38360802f5e0c7829.r2.dev/playground/proa-playground-x86_64-unknown-linux-gnu.tar.gz",
        action: "Download for Linux",
    },
    PlatformDownloadOption {
        kind: PlatformDownloadKind::Windows,
        href: "https://pub-cf0f833f7be646a38360802f5e0c7829.r2.dev/playground/proa-playground-x86_64-pc-windows-msvc.zip",
        action: "Download for Windows",
    },
    PlatformDownloadOption {
        kind: PlatformDownloadKind::Docker,
        href: "/docs/ffi#docker",
        action: "Run with Docker",
    },
];

struct EcosystemAppsPage<'a> {
    data: &'a EcosystemData<'a>,
}

struct AppFilterButton<'a> {
    filter: &'a EcosystemFilter<'a>,
    active: bool,
}

struct EcosystemAppCard<'a> {
    app: &'a EcosystemApp<'a>,
}

struct EcosystemAppCardBody<'a> {
    app: &'a EcosystemApp<'a>,
}

struct AppVisualPreview<'a> {
    visual: AppVisual<'a>,
}

struct AppStatusBadge {
    status: AppStatus,
}

struct ReferenceShowcases;
struct PlatformDownloadCta;
struct PlatformDownloadMenuItem {
    option: PlatformDownloadOption,
}
struct PlatformDownloadBadge {
    kind: PlatformDownloadKind,
}
struct ShowcaseChevronDownIcon;
struct ShowcaseArrowUpRightIcon;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ShowcaseChevronDownIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                viewBox="0 0 20 20"
                class="size-3.5"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
            >
                <path d="m6 8 4 4 4-4" />
            </svg>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ShowcaseArrowUpRightIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                viewBox="0 0 24 24"
                class="size-3.5"
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

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PlatformDownloadBadge {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        match self.kind {
            PlatformDownloadKind::AppleSilicon => html_sync! {
                <span
                    class="inline-flex size-7 shrink-0 items-center justify-center rounded-full border border-[#00000014] bg-[#f3ece2] text-[#9b5a24] dark:border-white/10 dark:bg-white/5 dark:text-[#ffb070]"
                    aria-hidden="true"
                >
                    <svg viewBox="0 0 24 24" class="size-3.5" fill="currentColor">
                        <path d="M16.7 2.1c.1 1.1-.3 2.2-1.1 3.1-.8.9-1.9 1.4-2.9 1.3-.1-1.1.3-2.2 1.1-3 .8-.9 1.9-1.4 2.9-1.4Z" />
                        <path d="M20.3 17.2c-.4.9-.7 1.3-1.2 2.1-.8 1.2-1.9 2.8-3.3 2.8-1.2 0-1.6-.8-3.3-.8s-2.1.8-3.3.8c-1.4 0-2.5-1.5-3.3-2.7-2.3-3.4-2.5-7.4-1.1-9.5 1-1.5 2.5-2.4 3.9-2.4 1.5 0 2.4.8 3.6.8 1.2 0 1.9-.8 3.6-.8 1.3 0 2.7.7 3.7 2-3.2 1.8-2.7 6.2.1 7.7Z" />
                    </svg>
                </span>
            }
            .render(cx),
            PlatformDownloadKind::Linux => html_sync! {
                <span
                    class="inline-flex size-7 shrink-0 items-center justify-center rounded-full border border-[#00000014] bg-[#f3ece2] text-[#9b5a24] dark:border-white/10 dark:bg-white/5 dark:text-[#ffb070]"
                    aria-hidden="true"
                >
                    <svg
                        viewBox="0 0 24 24"
                        class="size-3.5"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.9"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path d="M4.8 6.2h14.4a1.8 1.8 0 0 1 1.8 1.8v8a1.8 1.8 0 0 1-1.8 1.8H4.8A1.8 1.8 0 0 1 3 16V8a1.8 1.8 0 0 1 1.8-1.8Z" />
                        <path d="m7.2 9.5 2.2 2.2-2.2 2.2" />
                        <path d="M11.3 14h4.8" />
                    </svg>
                </span>
            }
            .render(cx),
            PlatformDownloadKind::Windows => html_sync! {
                <span
                    class="inline-flex size-7 shrink-0 items-center justify-center rounded-full border border-[#00000014] bg-[#f3ece2] text-[#9b5a24] dark:border-white/10 dark:bg-white/5 dark:text-[#ffb070]"
                    aria-hidden="true"
                >
                    <svg viewBox="0 0 24 24" class="size-3.5" fill="currentColor">
                        <path d="M3.5 5.6 10.4 4.7v6.6H3.5V5.6Z" />
                        <path d="M11.4 4.6 20.5 3.3v8h-9.1V4.6Z" />
                        <path d="M3.5 12.3h6.9v6.7l-6.9-.9v-5.8Z" />
                        <path d="M11.4 12.3h9.1v8.1l-9.1-1.3v-6.8Z" />
                    </svg>
                </span>
            }
            .render(cx),
            PlatformDownloadKind::Docker => html_sync! {
                <span
                    class="inline-flex size-7 shrink-0 items-center justify-center rounded-full border border-[#00000014] bg-[#f3ece2] text-[#9b5a24] dark:border-white/10 dark:bg-white/5 dark:text-[#ffb070]"
                    aria-hidden="true"
                >
                    <svg
                        viewBox="0 0 24 24"
                        class="size-3.5"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.9"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path d="M5 10.5h14v5.8a2.2 2.2 0 0 1-2.2 2.2H7.2A2.2 2.2 0 0 1 5 16.3v-5.8Z" />
                        <path d="M7.5 7h3v3.5h-3Z" />
                        <path d="M10.5 7h3v3.5h-3Z" />
                        <path d="M13.5 7h3v3.5h-3Z" />
                        <path d="M10.5 3.5h3V7h-3Z" />
                    </svg>
                </span>
            }
            .render(cx),
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PlatformDownloadMenuItem {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let detail = match self.option.kind {
            PlatformDownloadKind::AppleSilicon => "aarch64-apple-darwin",
            PlatformDownloadKind::Linux => "x86_64 Linux tarball",
            PlatformDownloadKind::Windows => "Windows zip archive",
            PlatformDownloadKind::Docker => "Docker build guide",
        };
        html_sync! {
            <a
                href={self.option.href}
                class="flex items-center gap-2 rounded-xl px-2.5 py-1.5 text-xs text-[#57544e] outline-none transition-colors hover:bg-[#f7f4ee] hover:text-[#18181b] focus:bg-[#f7f4ee] focus:text-[#18181b] dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-zinc-50 dark:focus:bg-white/10 dark:focus:text-zinc-50"
            >
                {PlatformDownloadBadge { kind: self.option.kind }}
                <span class="min-w-0 flex-1">
                    <span class="block font-medium text-[#18181b] dark:text-zinc-50">
                        {self.option.action}
                    </span>
                    <span class="mt-0.5 block text-xs text-[#a59f97] dark:text-zinc-400">
                        {detail}
                    </span>
                </span>
            </a>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PlatformDownloadCta {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let default = PLATFORM_DOWNLOAD_OPTIONS[0];
        html_sync! {
            <div class="group relative inline-flex items-stretch overflow-visible rounded-full border border-[#00000014] bg-[#f7f4ee] text-[#18181b] shadow-[0_14px_36px_-26px_rgba(24,24,27,0.35)] dark:border-white/10 dark:bg-white/5 dark:text-zinc-100 dark:shadow-[0_18px_52px_-34px_rgba(0,0,0,0.85)]">
                <a
                    href={default.href}
                    class="inline-flex items-center justify-center gap-2 rounded-l-full px-3 py-1.5 text-xs font-medium leading-tight transition-colors hover:bg-[#f3ece2] dark:hover:bg-white/10"
                >
                    {PlatformDownloadBadge { kind: default.kind }}
                    <span class="flex flex-col items-start">
                        <span>"Download for "</span>
                        <span>"Apple Silicon"</span>
                    </span>
                </a>
                <div class="relative">
                    <button
                        type="button"
                        class="inline-flex h-full min-h-8 items-center justify-center rounded-r-full border-l border-[#00000014] px-2 transition-colors hover:bg-[#f3ece2] dark:border-white/10 dark:hover:bg-white/10"
                        aria-label="Choose another platform"
                    >
                        {ShowcaseChevronDownIcon}
                    </button>
                    <div class="invisible absolute right-0 top-[calc(100%-1px)] z-[80] w-60 translate-y-1 pt-2 opacity-0 transition-all group-hover:visible group-hover:translate-y-0 group-hover:opacity-100 group-focus-within:visible group-focus-within:translate-y-0 group-focus-within:opacity-100">
                        <div class="rounded-2xl border border-[#00000014] bg-white/96 p-2 shadow-[0_24px_80px_-45px_rgba(24,24,27,0.45)] backdrop-blur-xl dark:border-white/10 dark:bg-zinc-950/96 dark:shadow-[0_28px_90px_-52px_rgba(0,0,0,0.95)]">
                            <p class="px-3 pb-1.5 pt-1 font-mono text-[11px] uppercase tracking-[0.18em] text-[#a59f97] dark:text-zinc-500">
                                "Native binary"
                            </p>
                            {for option in PLATFORM_DOWNLOAD_OPTIONS.iter().filter(|option| option.kind != PlatformDownloadKind::Docker) {
                                PlatformDownloadMenuItem { option: *option }
                            }}
                            <div class="my-1.5 h-px bg-[#00000014] dark:bg-white/10"></div>
                            <p class="px-3 pb-1.5 pt-1 font-mono text-[11px] uppercase tracking-[0.18em] text-[#a59f97] dark:text-zinc-500">
                                "Container"
                            </p>
                            {PlatformDownloadMenuItem {
                                option: PLATFORM_DOWNLOAD_OPTIONS[3],
                            }}
                        </div>
                    </div>
                </div>
            </div>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ReferenceShowcases {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section class="pb-16 pt-2" id="reference-showcases">
                <div class="mx-auto max-w-[1400px] px-4 sm:px-6 lg:px-8">
                    <div class="grid overflow-visible rounded-[28px] border border-[#00000014] bg-white shadow-[0_30px_90px_-65px_rgba(24,24,27,0.45)] dark:border-white/10 dark:bg-[#111215] dark:shadow-[0_34px_100px_-58px_rgba(0,0,0,0.9)] lg:grid-cols-[minmax(0,1.12fr)_minmax(360px,0.88fr)]">
                        <div class="rounded-t-[27px] border-b border-[#00000014] bg-[#111215] p-3 dark:border-white/10 lg:rounded-l-[27px] lg:rounded-tr-none lg:border-b-0 lg:border-r">
                            <div class="overflow-hidden rounded-[22px] border border-white/10 bg-black">
                                <video
                                    src={REFERENCE_SHOWCASE_VIDEO_URL}
                                    autoplay="autoplay"
                                    loop="loop"
                                    muted="muted"
                                    playsinline="playsinline"
                                    controls="controls"
                                    preload="metadata"
                                    poster="/static/images/synthetic/ecosystem-travel.svg"
                                    class="aspect-video w-full bg-[#0b0b0d] object-cover"
                                >
                                    <source src={REFERENCE_SHOWCASE_VIDEO_URL} type="video/mp4" />
                                </video>
                            </div>
                        </div>
                        <div class="flex flex-col justify-center p-6 sm:p-8 lg:p-10">
                            <p class="font-mono text-[11px] uppercase tracking-[0.18em] text-[#9b5a24] dark:text-[#ffb070]">
                                "Reference showcases"
                            </p>
                            <h2 class="mt-4 max-w-[13ch] text-[36px] font-semibold leading-[0.98] tracking-[-0.04em] text-[#18181b] dark:text-zinc-50 sm:text-[46px]">
                                "Real pages rendered by Proa."
                            </h2>
                            <p class="mt-5 max-w-[54ch] text-[15px] leading-7 text-[#7b746b] dark:text-zinc-300 sm:text-[16px]">
                                "Reference recreations and toy benchmarks for comparing Proa with other frameworks. Not affiliated with the referenced brands."
                            </p>
                            <div class="mt-7 flex flex-wrap gap-3">
                                <a
                                    href="/showcase/airbnb/proa"
                                    class="inline-flex items-center gap-1.5 rounded-full border border-[#18181b] bg-[#18181b] px-3 py-1.5 text-xs font-medium text-white transition-transform hover:-translate-y-0.5"
                                >
                                    "Open Airbnb"
                                    {ShowcaseArrowUpRightIcon}
                                </a>
                                <a
                                    href="/showcase/nike/proa"
                                    class="inline-flex items-center gap-1.5 rounded-full border border-[#00000014] bg-[#f7f4ee] px-3 py-1.5 text-xs font-medium text-[#18181b] transition-colors hover:bg-[#f3ece2] dark:border-white/10 dark:bg-white/5 dark:text-zinc-100 dark:hover:bg-white/10"
                                >
                                    "Open Nike"
                                </a>
                                <a
                                    href="/showcase/ebay/proa"
                                    class="inline-flex items-center gap-1.5 rounded-full border border-[#00000014] bg-[#f7f4ee] px-3 py-1.5 text-xs font-medium text-[#18181b] transition-colors hover:bg-[#f3ece2] dark:border-white/10 dark:bg-white/5 dark:text-zinc-100 dark:hover:bg-white/10"
                                >
                                    "Open eBay"
                                </a>
                                <a
                                    href="/playground/airbnb/listing"
                                    class="inline-flex items-center gap-1.5 rounded-full border border-[#00000014] bg-[#f7f4ee] px-3 py-1.5 text-xs font-medium text-[#18181b] transition-colors hover:bg-[#f3ece2] dark:border-white/10 dark:bg-white/5 dark:text-zinc-100 dark:hover:bg-white/10"
                                >
                                    "Open Playground"
                                </a>
                                <a
                                    href="/playground/github-pr/github-pr-diff"
                                    class="inline-flex items-center gap-1.5 rounded-full border border-[#00000014] bg-[#f7f4ee] px-3 py-1.5 text-xs font-medium text-[#18181b] transition-colors hover:bg-[#f3ece2] dark:border-white/10 dark:bg-white/5 dark:text-zinc-100 dark:hover:bg-white/10"
                                >
                                    "GitHub"
                                </a>
                            </div>
                            <div class="mt-3">{PlatformDownloadCta}</div>
                        </div>
                    </div>
                </div>
            </section>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for AppFilterButton<'_> {
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

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for AppStatusBadge {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        match self.status {
            AppStatus::Live => html_sync! {
                <span class="inline-flex shrink-0 items-center gap-1.5 font-mono text-[10px] text-[#1b7a44]">
                    <span class="h-1.5 w-1.5 rounded-full bg-[#1b7a44]"></span>
                    "Live"
                </span>
            }
            .render(cx),
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for AppVisualPreview<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        match self.visual {
            AppVisual::Screenshot { src, alt } => html_sync! {
                <div class="h-[180px] overflow-hidden bg-[#f7f4ee] dark:bg-[#111215]">
                    <img
                        src={src}
                        alt={alt}
                        width="1440"
                        height="900"
                        loading="lazy"
                        decoding="async"
                        class="h-full w-full object-cover object-top"
                    />
                </div>
            }
            .render(cx),
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for EcosystemAppCardBody<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let app = self.app;
        html_sync! {
            {AppVisualPreview { visual: app.visual }}
            <div class="p-4">
                <div class="flex items-center justify-between gap-2">
                    <div
                        data-app-title
                        class="text-[15px] font-semibold text-[#18181b] dark:text-zinc-50"
                    >
                        {app.name}
                    </div>
                    {AppStatusBadge { status: app.status }}
                </div>
                <div
                    data-app-kind
                    class="mt-0.5 text-[13px] leading-5 text-[#7b746b] dark:text-zinc-400"
                >
                    {app.product_kind}
                </div>
            </div>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for EcosystemAppCard<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let app = self.app;
        if app.external {
            html_sync! {
                <a
                    href={app.href}
                    target="_blank"
                    rel="noreferrer noopener"
                    data-industry={app.industry}
                    class="ecosystem-app-card group overflow-hidden rounded-2xl border border-[#00000014] bg-white transition-transform hover:-translate-y-1 dark:border-white/10 dark:bg-[#111215]"
                >
                    {EcosystemAppCardBody { app }}
                </a>
            }
            .render(cx)
        } else {
            html_sync! {
                <a
                    href={app.href}
                    data-industry={app.industry}
                    class="ecosystem-app-card group overflow-hidden rounded-2xl border border-[#00000014] bg-white transition-transform hover:-translate-y-1 dark:border-white/10 dark:bg-[#111215]"
                >
                    {EcosystemAppCardBody { app }}
                </a>
            }
            .render(cx)
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for EcosystemAppsPage<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let app_count = self.data.apps.len();

        html_sync! {
            <main>
                <section class="pb-10 pt-[68px]">
                    <div class="mx-auto max-w-[1400px] px-4 sm:px-6 lg:px-8">
                        <span class="mb-[22px] inline-flex items-center gap-[9px] font-mono text-[12px] uppercase tracking-[0.16em] text-[#7b746b]">
                            <span class="h-1.5 w-1.5 rounded-full bg-[#dd7627]"></span>
                            "Ecosystem · Apps"
                        </span>
                        <h1 class="max-w-[18ch] text-[52px] font-semibold leading-[0.98] tracking-[-0.04em] sm:text-[60px] lg:text-[68px]">
                            "Apps and showcases built on "
                            <span class="text-[#dd7627]">"Proa."</span>
                        </h1>
                        <p class="mt-6 max-w-[52ch] text-[17px] leading-[1.55] text-[#7b746b] dark:text-zinc-300">
                            "Production surfaces and reference recreations rendered by Proa, from live sites to benchmarkable showcase pages."
                        </p>
                    </div>
                </section>
                {ReferenceShowcases}
                <section class="pb-16 pt-2" id="apps">
                    <div class="mx-auto max-w-[1400px] px-4 sm:px-6 lg:px-8">
                        <div class="mb-7 flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
                            <div>
                                <p class="font-mono text-[11px] uppercase tracking-[0.18em] text-[#9b5a24] dark:text-[#ffb070]">
                                    "App directory"
                                </p>
                                <h2 class="mt-2 text-[30px] font-semibold leading-none tracking-[-0.04em] sm:text-[40px]">
                                    "Sites and reference pages rendered by Proa."
                                </h2>
                            </div>
                            <p class="max-w-[46ch] text-sm leading-6 text-[#7b746b] dark:text-zinc-300">
                                "Filter the production site, playground console, and brand-reference recreations from the same ecosystem surface."
                            </p>
                        </div>
                        <div class="mb-7 flex flex-wrap items-center gap-x-2 gap-y-3">
                            <div id="filter-bar" class="flex flex-wrap gap-2 max-[560px]:w-full">
                                {for filter in self.data.app_filters {
                                    AppFilterButton {
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
                                        id="ecosystem-app-search"
                                        type="text"
                                        placeholder="Search sites…"
                                        autocomplete="off"
                                        class="w-[180px] rounded-full border border-[#00000014] bg-white py-2 pl-9 pr-3 font-mono text-[12px] text-[#18181b] placeholder:text-[#a59f97] focus:border-[#18181b] focus:outline-none max-[560px]:w-full dark:border-white/15 dark:bg-white/[0.06] dark:text-zinc-100 dark:placeholder:text-zinc-500 dark:focus:border-[#dd7627]"
                                    />
                                </div>
                                <span
                                    id="count"
                                    class="whitespace-nowrap font-mono text-[12px] text-[#a59f97]"
                                >
                                    "Showing "
                                    {app_count}
                                    " of "
                                    {app_count}
                                </span>
                            </div>
                        </div>
                        <div
                            id="showcase-grid"
                            class="grid grid-cols-3 gap-4 max-[920px]:grid-cols-2 max-[560px]:grid-cols-1"
                        >
                            {for app in self.data.apps {
                                EcosystemAppCard { app }
                            }}
                        </div>
                        <div id="empty-state" class="hidden py-20 text-center">
                            <div class="font-mono text-[13px] text-[#7b746b]">
                                "No sites match that search."
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
                                "Built something on Proa?"
                            </h2>
                            <p class="relative mb-7 text-[17px] text-[#ffffff99]">
                                "Add your site to the showcase — it takes one pull request to join the directory."
                            </p>
                            <div class="relative flex flex-wrap justify-center gap-3">
                                <a
                                    class="inline-flex items-center gap-[7px] whitespace-nowrap rounded-full border border-[#dd7627] bg-[#dd7627] px-[17px] py-[9px] text-sm font-medium text-[#1a0f06]"
                                    href="https://github.com/proa-labs/proa"
                                    rel="noreferrer noopener"
                                    target="_blank"
                                >
                                    "Add your project"
                                </a>
                                <a
                                    class="inline-flex items-center gap-[7px] whitespace-nowrap rounded-full border border-[#ffffff26] bg-[#ffffff12] px-[17px] py-[9px] text-sm font-medium text-white"
                                    href="/docs"
                                >
                                    "Submission guidelines"
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
