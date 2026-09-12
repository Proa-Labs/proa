// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Server-rendered directory for the standalone showcase routes.

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;
#[allow(unused_imports)]
use rsjs::signal;
use rsjs_macros::rsjs;

pub struct ShowcaseIndexPage;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for ShowcaseIndexPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <title>"Proa Showcases"</title>
                    <script src="https://cdn.tailwindcss.com"></script>
                </head>
                <body class="min-h-screen bg-white text-[#222] antialiased">
                    {ShowcaseDirectory}
                </body>
            </html>
        }
        .render(cx)
    }
}

struct ShowcaseDirectory;

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for ShowcaseDirectory {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let proa_only = signal(false);

        html_sync! {
            <main data-showcase-index class="mx-auto max-w-3xl px-6 py-16">
                <header class="mb-10 flex flex-wrap items-end justify-between gap-5">
                    <div>
                        <h1 class="text-3xl font-bold tracking-tight">"Showcases"</h1>
                        <p class="mt-2 max-w-2xl text-sm leading-6 text-gray-500">
                            "Product recreations rendered with multiple SSR engines for comparison."
                        </p>
                    </div>
                    <button
                        type="button"
                        class="inline-flex min-h-9 items-center gap-2 rounded-full border border-zinc-200 bg-white px-3.5 text-xs font-semibold text-zinc-600 transition-colors hover:border-zinc-400 hover:bg-zinc-50 data-[active=true]:border-zinc-950 data-[active=true]:bg-zinc-950 data-[active=true]:text-white"
                        data_active={proa_only.get()}
                        aria_pressed={proa_only.get()}
                        on_click={|_| proa_only.update(|value| !value)}
                    >
                        <span class="size-2 rounded-full bg-[#e43717]"></span>
                        "Proa routes only"
                    </button>
                </header>
                <div class="grid gap-6">
                    <section class="rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm">
                        <div class="mb-1 flex items-center gap-2">
                            <h2 class="text-xl font-semibold">"Airbnb"</h2>
                            <span class="rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700">
                                "Live"
                            </span>
                        </div>
                        <p class="mb-5 text-sm text-gray-500">"Home, Search, and Listing pages."</p>
                        <div class="grid gap-4">
                            <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                                <span class="text-sm font-medium text-zinc-700">"Home"</span>
                                <div class="flex flex-wrap gap-2">
                                    <a
                                        href="/showcase/airbnb/proa"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full bg-[#e43717]"></span>
                                        "Proa"
                                    </a>
                                    <a
                                        hidden={proa_only.get()}
                                        href="/showcase/airbnb/node"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full bg-[#68a063]"></span>
                                        "Node"
                                    </a>
                                    <a
                                        hidden={proa_only.get()}
                                        href="/showcase/airbnb/bun"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full border border-[#e5c07b] bg-[#fbf0df]"></span>
                                        "Bun"
                                    </a>
                                </div>
                            </div>
                            <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                                <span class="text-sm font-medium text-zinc-700">"Search"</span>
                                <div class="flex flex-wrap gap-2">
                                    <a
                                        href="/showcase/airbnb/proa/search"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full bg-[#e43717]"></span>
                                        "Proa"
                                    </a>
                                    <a
                                        hidden={proa_only.get()}
                                        href="/showcase/airbnb/node/search"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full bg-[#68a063]"></span>
                                        "Node"
                                    </a>
                                    <a
                                        hidden={proa_only.get()}
                                        href="/showcase/airbnb/bun/search"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full border border-[#e5c07b] bg-[#fbf0df]"></span>
                                        "Bun"
                                    </a>
                                </div>
                            </div>
                            <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                                <span class="text-sm font-medium text-zinc-700">"Listing"</span>
                                <div class="flex flex-wrap gap-2">
                                    <a
                                        href="/showcase/airbnb/proa/listing"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full bg-[#e43717]"></span>
                                        "Proa"
                                    </a>
                                    <a
                                        hidden={proa_only.get()}
                                        href="/showcase/airbnb/node/listing"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full bg-[#68a063]"></span>
                                        "Node"
                                    </a>
                                    <a
                                        hidden={proa_only.get()}
                                        href="/showcase/airbnb/bun/listing"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full border border-[#e5c07b] bg-[#fbf0df]"></span>
                                        "Bun"
                                    </a>
                                </div>
                            </div>
                        </div>
                    </section>
                    <section class="rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm">
                        <div class="mb-1 flex items-center gap-2">
                            <h2 class="text-xl font-semibold">"Nike"</h2>
                            <span class="rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700">
                                "Live"
                            </span>
                        </div>
                        <p class="mb-5 text-sm text-gray-500">
                            "Catalog and Product Detail pages."
                        </p>
                        <div class="grid gap-4">
                            <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                                <span class="text-sm font-medium text-zinc-700">"Catalog"</span>
                                <div class="flex flex-wrap gap-2">
                                    <a
                                        href="/showcase/nike/proa"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full bg-[#e43717]"></span>
                                        "Proa"
                                    </a>
                                    <a
                                        hidden={proa_only.get()}
                                        href="/showcase/nike/node"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full bg-[#68a063]"></span>
                                        "Node"
                                    </a>
                                    <a
                                        hidden={proa_only.get()}
                                        href="/showcase/nike/bun"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full border border-[#e5c07b] bg-[#fbf0df]"></span>
                                        "Bun"
                                    </a>
                                </div>
                            </div>
                            <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                                <span class="text-sm font-medium text-zinc-700">"Product"</span>
                                <div class="flex flex-wrap gap-2">
                                    <a
                                        href="/showcase/nike/proa/product"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full bg-[#e43717]"></span>
                                        "Proa"
                                    </a>
                                    <a
                                        hidden={proa_only.get()}
                                        href="/showcase/nike/node/product"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full bg-[#68a063]"></span>
                                        "Node"
                                    </a>
                                    <a
                                        hidden={proa_only.get()}
                                        href="/showcase/nike/bun/product"
                                        class="inline-flex min-h-9 items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                                    >
                                        <span class="size-2 rounded-full border border-[#e5c07b] bg-[#fbf0df]"></span>
                                        "Bun"
                                    </a>
                                </div>
                            </div>
                        </div>
                    </section>
                    <section class="rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm">
                        <h2 class="text-lg font-semibold">"eBay search"</h2>
                        <p class="mb-5 mt-1 text-sm text-gray-500">
                            "Full-page eBay search workload with 60 production-shaped listings."
                        </p>
                        <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                            <span class="text-sm font-medium text-zinc-700">
                                "Search Results (portable field recorder)"
                            </span>
                            <a
                                href="/showcase/ebay/proa"
                                class="inline-flex min-h-9 w-fit items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                            >
                                <span class="size-2 rounded-full bg-[#e43717]"></span>
                                "Proa"
                            </a>
                        </div>
                    </section>
                    <section class="rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm">
                        <h2 class="text-lg font-semibold">"GitHub PR Diff"</h2>
                        <p class="mb-5 mt-1 text-sm text-gray-500">
                            "Large Files changed review surface modeled after oven-sh/bun#31136."
                        </p>
                        <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                            <span class="text-sm font-medium text-zinc-700">
                                "10k-line pull request review"
                            </span>
                            <a
                                href="/showcase/github-pr-diff"
                                class="inline-flex min-h-9 w-fit items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                            >
                                <span class="size-2 rounded-full bg-[#e43717]"></span>
                                "Proa"
                            </a>
                        </div>
                    </section>
                    <section class="rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm">
                        <h2 class="text-lg font-semibold">"Ops Dashboard"</h2>
                        <p class="mb-5 mt-1 text-sm text-gray-500">
                            "Internal-tool stress page with filters, table rows, row-action islands, and forms."
                        </p>
                        <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                            <span class="text-sm font-medium text-zinc-700">
                                "Escalation Command Center"
                            </span>
                            <a
                                href="/showcase/ops-dashboard"
                                class="inline-flex min-h-9 w-fit items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                            >
                                <span class="size-2 rounded-full bg-[#e43717]"></span>
                                "Proa"
                            </a>
                        </div>
                    </section>
                    <section class="rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm">
                        <h2 class="text-lg font-semibold">"Settings Console"</h2>
                        <p class="mb-5 mt-1 text-sm text-gray-500">
                            "Stateful settings UI stressing dynamic classes, tabs, ARIA state, and disabled actions."
                        </p>
                        <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                            <span class="text-sm font-medium text-zinc-700">"Admin Settings"</span>
                            <a
                                href="/showcase/settings-console"
                                class="inline-flex min-h-9 w-fit items-center gap-1.5 rounded-full border border-zinc-200 px-3 text-[13px] font-medium text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50"
                            >
                                <span class="size-2 rounded-full bg-[#e43717]"></span>
                                "Proa"
                            </a>
                        </div>
                    </section>
                </div>
                <p class="mt-8 text-center text-xs text-gray-400">
                    <a href="/playground" class="underline transition-colors hover:text-gray-600">
                        "Open Playground"
                    </a>
                    " for side-by-side editing and benchmarks."
                </p>
            </main>
        }
        .render(cx)
    }
}
