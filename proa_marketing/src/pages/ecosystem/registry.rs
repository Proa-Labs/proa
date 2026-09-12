// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::response::Html;
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

use crate::layout::{render_chained_html, PageMeta};

use super::data::{ecosystem_data, EcosystemData, EcosystemFilter, RegistryRequest};
use super::layout::{EcosystemMockFooter, EcosystemStandaloneDocument};

pub(crate) fn render() -> Html<String> {
    // proa-perf-ok: top-level Axum response finalization owns the HTML String
    render_chained_html(EcosystemStandaloneDocument {
        meta: PageMeta {
            title: "Ecosystem Registry - Proa",
            description:
                "The Proa registry wishlist for components, adapters, data packages, and tooling.",
            path: "/ecosystem/registry",
        },
        content: EcosystemRegistryPage {
            data: ecosystem_data(),
        },
    })
}

struct EcosystemRegistryPage<'a> {
    data: &'a EcosystemData<'a>,
}

struct RegistryFilterButton<'a> {
    filter: &'a EcosystemFilter<'a>,
    active: bool,
}

struct RegistryRequestRow<'a> {
    request: &'a RegistryRequest<'a>,
}

struct RegistryRequestBadges<'a> {
    request: &'a RegistryRequest<'a>,
}

struct RegistryRequestDescription<'a> {
    request: &'a RegistryRequest<'a>,
}

struct RegistryRequestAction<'a> {
    request: &'a RegistryRequest<'a>,
}

struct RegistryDifficultyBadge<'a> {
    label: &'a str,
}

struct RegistrySearchIcon;
struct RegistryVoteIcon;
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for RegistrySearchIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
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
        }
        .render(cx)
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for RegistryVoteIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.4"
                stroke-linecap="round"
                stroke-linejoin="round"
                class="text-[#7b746b] group-hover:text-[#dd7627]"
                aria-hidden="true"
            >
                <polyline points="18 15 12 9 6 15" />
            </svg>
        }
        .render(cx)
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for RegistryFilterButton<'_> {
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

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for RegistryDifficultyBadge<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <span class="rounded-full border border-[#00000014] px-2 py-0.5 font-mono text-[10px] text-[#a59f97]">
                {self.label}
            </span>
        }
        .render(cx)
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for RegistryRequestBadges<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let request = self.request;
        if request.claimed {
            html_sync! {
                <span class="rounded-full bg-[#f3ece2] px-2 py-0.5 font-mono text-[10px] text-[#7b746b]">
                    {request.tag}
                </span>
                <span class="inline-flex items-center gap-1.5 rounded-full bg-[#1b7a44] px-2 py-0.5 font-mono text-[10px] text-white">
                    <span class="h-1.5 w-1.5 rounded-full bg-white"></span>
                    "Claimed"
                </span>
            }
            .render(cx)
        } else if let Some(difficulty) = request.difficulty {
            html_sync! {
                <span class="rounded-full bg-[#f3ece2] px-2 py-0.5 font-mono text-[10px] text-[#7b746b]">
                    {request.tag}
                </span>
                {RegistryDifficultyBadge { label: difficulty }}
            }
            .render(cx)
        } else {
            html_sync! {
                <span class="rounded-full bg-[#f3ece2] px-2 py-0.5 font-mono text-[10px] text-[#7b746b]">
                    {request.tag}
                </span>
            }
            .render(cx)
        }
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for RegistryRequestDescription<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let request = self.request;
        if let Some(builder) = request.builder {
            html_sync! {
                <div class="mt-1 max-w-[60ch] text-[13.5px] text-[#7b746b]">
                    {request.description}
                    <span class="text-[#18181b]">{builder}</span>
                    {request.description_suffix}
                </div>
            }
            .render(cx)
        } else {
            html_sync! {
                <div class="mt-1 max-w-[60ch] text-[13.5px] text-[#7b746b]">
                    {request.description}
                </div>
            }
            .render(cx)
        }
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for RegistryRequestAction<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if self.request.claimed {
            html_sync! {
                <span class="shrink-0 px-4 py-2.5 font-mono text-[12px] text-[#a59f97] max-[560px]:ml-[76px]">
                    "In progress"
                </span>
            }
            .render(cx)
        } else {
            html_sync! {
                <a
                    href="https://github.com/proa-labs/proa"
                    rel="noreferrer noopener"
                    target="_blank"
                    class="shrink-0 rounded-full border border-[#00000014] px-4 py-2.5 font-mono text-[12px] text-[#18181b] transition-colors hover:border-[#18181b] max-[560px]:ml-[76px]"
                >
                    "Claim →"
                </a>
            }
            .render(cx)
        }
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for RegistryRequestRow<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let request = self.request;
        let row_class = if request.claimed {
            "registry-request flex items-center gap-5 bg-[#dd76270a] px-5 py-5 max-[560px]:flex-wrap"
        } else {
            "registry-request flex items-center gap-5 px-5 py-5 max-[560px]:flex-wrap"
        };

        html_sync! {
            <div data-cat={request.category} class={row_class}>
                <button
                    class="registry-vote group flex h-16 w-14 shrink-0 flex-col items-center justify-center gap-0.5 rounded-xl border border-[#00000014] transition-colors hover:border-[#dd7627] hover:bg-[#dd76270a]"
                    type="button"
                    aria-label="Upvote request"
                >
                    {RegistryVoteIcon}
                    <span class="registry-vote-count font-mono text-[13px] font-medium text-[#18181b]">
                        {request.votes}
                    </span>
                </button>
                <div class="min-w-0 flex-1">
                    <div class="flex flex-wrap items-center gap-2.5">
                        <span class="text-[16px] font-semibold tracking-[-0.01em]">
                            {request.name}
                        </span>
                        {RegistryRequestBadges { request }}
                    </div>
                    {RegistryRequestDescription { request }}
                </div>
                {RegistryRequestAction { request }}
            </div>
        }
        .render(cx)
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for EcosystemRegistryPage<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let request_count = self.data.registry_requests.len();

        html_sync! {
            <main>
                <section class="pb-10 pt-[68px]">
                    <div class="mx-auto max-w-[1400px] px-4 sm:px-6 lg:px-8">
                        <span class="mb-[22px] inline-flex items-center gap-[9px] font-mono text-[12px] uppercase tracking-[0.16em] text-[#7b746b]">
                            <span class="h-1.5 w-1.5 rounded-full bg-[#dd7627]"></span>
                            "Ecosystem · Registry"
                        </span>
                        <h1 class="max-w-[18ch] text-[52px] font-semibold leading-[0.98] tracking-[-0.04em] sm:text-[60px] lg:text-[68px]">
                            "Tools we'd love the ecosystem to "
                            <span class="text-[#dd7627]">"build."</span>
                        </h1>
                        <p class="mt-6 max-w-[54ch] text-[17px] leading-[1.55] text-[#7b746b]">
                            "An open wishlist of packages, adapters, and integrations the community is asking for. Upvote what you'd use, or claim one and ship it — we'll feature it in Developer tools."
                        </p>
                    </div>
                </section>
                <section class="pb-16 pt-2" id="registry">
                    <div class="mx-auto max-w-[1400px] px-4 sm:px-6 lg:px-8">
                        <div class="mb-7 flex flex-wrap items-center gap-x-2 gap-y-3">
                            <div id="registry-filter-bar" class="flex flex-wrap gap-2">
                                {for filter in self.data.registry_filters {
                                    RegistryFilterButton {
                                        filter,
                                        active: filter.key == "all",
                                    }.render(cx)?;
                                }}
                            </div>
                            <div class="ml-auto flex items-center gap-3 max-[560px]:ml-0 max-[560px]:w-full">
                                <div class="relative max-[560px]:flex-1">
                                    {RegistrySearchIcon}
                                    <input
                                        id="registry-search"
                                        type="text"
                                        placeholder="Search requests…"
                                        autocomplete="off"
                                        class="w-[180px] rounded-full border border-[#00000014] bg-white py-2 pl-9 pr-3 font-mono text-[12px] text-[#18181b] placeholder:text-[#a59f97] focus:border-[#18181b] focus:outline-none max-[560px]:w-full"
                                    />
                                </div>
                                <span
                                    id="registry-count"
                                    class="whitespace-nowrap font-mono text-[12px] text-[#a59f97]"
                                >
                                    "Showing "
                                    {request_count}
                                    " of "
                                    {request_count}
                                </span>
                            </div>
                        </div>
                        <div
                            id="registry-list"
                            class="overflow-hidden rounded-2xl border border-[#00000014] bg-white divide-y divide-[#00000014]"
                        >
                            {for request in self.data.registry_requests {
                                RegistryRequestRow { request }.render(cx)?;
                            }}
                        </div>
                        <div id="registry-empty-state" class="hidden py-20 text-center">
                            <div class="font-mono text-[13px] text-[#7b746b]">
                                "No requests match that search."
                            </div>
                            <button
                                id="registry-clear-search"
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
                                "Missing a tool you need?"
                            </h2>
                            <p class="relative mb-7 text-[17px] text-[#ffffff99]">
                                "Open a request and let the community vote — or claim one and we'll help you ship it."
                            </p>
                            <div class="relative flex flex-wrap justify-center gap-3">
                                <a
                                    class="inline-flex items-center gap-[7px] whitespace-nowrap rounded-full border border-[#dd7627] bg-[#dd7627] px-[17px] py-[9px] text-sm font-medium text-[#1a0f06]"
                                    href="https://github.com/proa-labs/proa"
                                    rel="noreferrer noopener"
                                    target="_blank"
                                >
                                    "Request a tool"
                                </a>
                                <a
                                    class="inline-flex items-center gap-[7px] whitespace-nowrap rounded-full border border-[#ffffff26] bg-[#ffffff12] px-[17px] py-[9px] text-sm font-medium text-white"
                                    href="/docs"
                                >
                                    "Contributor guide"
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
