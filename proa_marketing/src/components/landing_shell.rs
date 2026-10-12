// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

use super::brand::ProaBrand;
use super::search_trigger::SearchTrigger;
use super::theme_toggle::ThemeToggle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketingNavVariant {
    Landing,
    Docs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketingNavActive {
    None,
    Docs,
    Ecosystem,
}

impl MarketingNavActive {
    pub fn for_path(path: &str) -> Self {
        if path == "/docs" || path.starts_with("/docs/") {
            Self::Docs
        } else if path == "/ecosystem" || path.starts_with("/ecosystem/") {
            Self::Ecosystem
        } else {
            Self::None
        }
    }
}

pub struct MarketingNav {
    pub variant: MarketingNavVariant,
    pub active: MarketingNavActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EcosystemNavKind {
    Apps,
    DeveloperTools,
}

#[derive(Debug, Clone, Copy)]
struct EcosystemNavEntry {
    href: &'static str,
    title: &'static str,
    description: &'static str,
    kind: EcosystemNavKind,
}

const ECOSYSTEM_NAV_ITEMS: &[EcosystemNavEntry] = &[
    EcosystemNavEntry {
        href: "/ecosystem/apps",
        title: "Apps",
        description: "Products shipped on Proa",
        kind: EcosystemNavKind::Apps,
    },
    EcosystemNavEntry {
        href: "/ecosystem/developer-tools",
        title: "Developer tools",
        description: "Components, adapters, and more",
        kind: EcosystemNavKind::DeveloperTools,
    },
    // Registry navigation stays hidden until the package list is ready.
];

impl MarketingNavVariant {
    const fn nav_class(self) -> &'static str {
        match self {
            Self::Landing => {
                "marketing-nav marketing-nav-landing hidden items-center gap-1 text-sm md:flex"
            }
            Self::Docs => "marketing-nav marketing-nav-docs docs-header-nav",
        }
    }

    const fn link_class(self) -> &'static str {
        match self {
            Self::Landing | Self::Docs => "marketing-nav-link",
        }
    }

    const fn dropdown_root_class(self) -> &'static str {
        match self {
            Self::Landing | Self::Docs => "marketing-nav-dropdown",
        }
    }

    const fn trigger_class(self) -> &'static str {
        match self {
            Self::Landing | Self::Docs => "marketing-nav-trigger-shell",
        }
    }

    const fn trigger_link_class(self, active: bool) -> &'static str {
        match (self, active) {
            (_, true) => "marketing-nav-link marketing-nav-trigger marketing-nav-trigger-active",
            (_, false) => "marketing-nav-link marketing-nav-trigger",
        }
    }

    const fn chevron_class(self) -> &'static str {
        match self {
            Self::Landing | Self::Docs => "marketing-nav-chevron",
        }
    }

    const fn dropdown_content_class(self) -> &'static str {
        match self {
            Self::Landing | Self::Docs => "marketing-nav-dropdown-content",
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MarketingNav {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let nav_class = self.variant.nav_class();
        html_sync! {
            <nav class={nav_class} aria-label="Main navigation">
                {MarketingNavLink {
                    href: "/docs",
                    label: "Docs",
                    active: self.active == MarketingNavActive::Docs,
                    variant: self.variant,
                }}
                {MarketingEcosystemNav {
                    active: self.active == MarketingNavActive::Ecosystem,
                    variant: self.variant,
                }}
                {MarketingNavLink {
                    href: "https://ui.proa.so",
                    label: "Proa UI",
                    active: false,
                    variant: self.variant,
                }}
            </nav>
        }
        .render(cx)
    }
}

struct MarketingNavLink {
    href: &'static str,
    label: &'static str,
    active: bool,
    variant: MarketingNavVariant,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MarketingNavLink {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let class = self.variant.link_class();
        let active = self.active.then_some("true");
        html_sync! {
            <a href={self.href} class={class} data-active={active}>{self.label}</a>
        }
        .render(cx)
    }
}

struct MarketingEcosystemNav {
    active: bool,
    variant: MarketingNavVariant,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MarketingEcosystemNav {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let root_class = self.variant.dropdown_root_class();
        let trigger_class = self.variant.trigger_class();
        let content_class = self.variant.dropdown_content_class();
        html_sync! {
            <div class={root_class}>
                <details data-dropdown-root data-component="marketing-ecosystem-menu">
                    <summary data-dropdown-trigger class={trigger_class}>
                        {MarketingEcosystemTrigger {
                            variant: self.variant,
                            active: self.active,
                        }}
                    </summary>
                    <div data-dropdown-content class={content_class}>{MarketingEcosystemPanel}</div>
                </details>
            </div>
        }
        .render(cx)
    }
}

struct MarketingEcosystemTrigger {
    variant: MarketingNavVariant,
    active: bool,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MarketingEcosystemTrigger {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let link_class = self.variant.trigger_link_class(self.active);
        let chevron_class = self.variant.chevron_class();
        html_sync! {
            <span class={link_class}>
                <span>"Ecosystem"</span>
                <svg
                    viewBox="0 0 20 20"
                    class={chevron_class}
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.8"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path d="m6 8 4 4 4-4" />
                </svg>
            </span>
        }
        .render(cx)
    }
}

struct MarketingEcosystemPanel;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MarketingEcosystemPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            {for item in ECOSYSTEM_NAV_ITEMS {
                MarketingEcosystemItem {
                    item: *item,
                }
            }}
        }
        .render(cx)
    }
}

struct MarketingEcosystemItem {
    item: EcosystemNavEntry,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MarketingEcosystemItem {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div data-dropdown-item class="marketing-nav-dropdown-item">
                {MarketingEcosystemLink {
                    item: self.item,
                }}
            </div>
        }
        .render(cx)
    }
}

struct MarketingEcosystemLink {
    item: EcosystemNavEntry,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MarketingEcosystemLink {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <a href={self.item.href} class="marketing-nav-dropdown-link">
                <span class="marketing-nav-dropdown-icon" aria-hidden="true">
                    {MarketingEcosystemBadge { kind: self.item.kind }}
                </span>
                <span class="marketing-nav-dropdown-copy">
                    <span class="marketing-nav-dropdown-title">{self.item.title}</span>
                    <span class="marketing-nav-dropdown-description">{self.item.description}</span>
                </span>
            </a>
        }
        .render(cx)
    }
}

struct MarketingEcosystemBadge {
    kind: EcosystemNavKind,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MarketingEcosystemBadge {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let label = match self.kind {
            EcosystemNavKind::Apps => "app",
            EcosystemNavKind::DeveloperTools => "dev",
        };
        html_sync! {
            {label}
        }
        .render(cx)
    }
}

pub struct LandingHeader {
    pub active: MarketingNavActive,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LandingHeader {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <header class="sticky top-0 z-50 border-b border-black/10 bg-[#fdfcfc]/85 backdrop-blur-xl dark:border-white/10 dark:bg-zinc-950/75">
                <div
                    data-marketing-header-inner
                    class="mx-auto flex max-w-[1400px] items-center justify-between gap-6 px-4 py-3 sm:px-6 lg:px-8"
                >
                    <div class="flex min-w-0 items-center gap-8 lg:gap-10">
                        <a href="/" class="shrink-0 text-zinc-950 dark:text-zinc-100">
                            {ProaBrand}
                        </a>
                        {MarketingNav {
                            variant: MarketingNavVariant::Landing,
                            active: self.active,
                        }}
                    </div>
                    <div class="flex shrink-0 items-center gap-2">
                        {SearchTrigger::new()}
                        {ThemeToggle::new()}
                        <a
                            href="https://github.com/proa-labs/proa"
                            rel="noreferrer noopener"
                            target="_blank"
                            aria-label="GitHub"
                            class="inline-flex h-9 w-9 min-w-9 flex-none items-center justify-center rounded-full border border-black/10 bg-white/70 p-0 text-zinc-600 transition-colors hover:bg-white hover:text-zinc-950 dark:border-white/10 dark:bg-white/5 dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-zinc-50 [&_svg]:h-5 [&_svg]:w-5 [&_svg]:shrink-0"
                        >
                            <svg
                                role="img"
                                viewBox="0 0 24 24"
                                fill="currentColor"
                                aria-hidden="true"
                            >
                                <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
                            </svg>
                        </a>
                        <a
                            href="https://proa.so/discord"
                            rel="noreferrer noopener"
                            target="_blank"
                            aria-label="Discord"
                            class="inline-flex h-9 w-9 min-w-9 flex-none items-center justify-center rounded-full border border-black/10 bg-white/70 p-0 text-zinc-600 transition-colors hover:bg-white hover:text-zinc-950 dark:border-white/10 dark:bg-white/5 dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-zinc-50 [&_svg]:h-5 [&_svg]:w-5 [&_svg]:shrink-0"
                        >
                            <svg
                                role="img"
                                viewBox="0 0 24 24"
                                fill="currentColor"
                                aria-hidden="true"
                            >
                                <path d="M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z" />
                            </svg>
                        </a>
                        <a
                            href="https://www.reddit.com/r/proa_rs/"
                            rel="noreferrer noopener"
                            target="_blank"
                            aria-label="Reddit"
                            class="inline-flex h-9 w-9 min-w-9 flex-none items-center justify-center rounded-full border border-black/10 bg-white/70 p-0 text-zinc-600 transition-colors hover:bg-white hover:text-zinc-950 dark:border-white/10 dark:bg-white/5 dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-zinc-50 [&_svg]:h-5 [&_svg]:w-5 [&_svg]:shrink-0"
                        >
                            <svg
                                role="img"
                                viewBox="0 0 24 24"
                                fill="currentColor"
                                aria-hidden="true"
                            >
                                <path d="M12 0C5.373 0 0 5.373 0 12c0 3.314 1.343 6.314 3.515 8.485l-2.286 2.286C.775 23.225 1.097 24 1.738 24H12c6.627 0 12-5.373 12-12S18.627 0 12 0Zm4.388 3.199c1.104 0 1.999.895 1.999 1.999 0 1.105-.895 2-1.999 2-.946 0-1.739-.657-1.947-1.539v.002c-1.147.162-2.032 1.15-2.032 2.341v.007c1.776.067 3.4.567 4.686 1.363.473-.363 1.064-.58 1.707-.58 1.547 0 2.802 1.254 2.802 2.802 0 1.117-.655 2.081-1.601 2.531-.088 3.256-3.637 5.876-7.997 5.876-4.361 0-7.905-2.617-7.998-5.87-.954-.447-1.614-1.415-1.614-2.538 0-1.548 1.255-2.802 2.803-2.802.645 0 1.239.218 1.712.585 1.275-.79 2.881-1.291 4.64-1.365v-.01c0-1.663 1.263-3.034 2.88-3.207.188-.911.993-1.595 1.959-1.595Zm-8.085 8.376c-.784 0-1.459.78-1.506 1.797-.047 1.016.64 1.429 1.426 1.429.786 0 1.371-.369 1.418-1.385.047-1.017-.553-1.841-1.338-1.841Zm7.406 0c-.786 0-1.385.824-1.338 1.841.047 1.017.634 1.385 1.418 1.385.785 0 1.473-.413 1.426-1.429-.046-1.017-.721-1.797-1.506-1.797Zm-3.703 4.013c-.974 0-1.907.048-2.77.135-.147.015-.241.168-.183.305.483 1.154 1.622 1.964 2.953 1.964 1.33 0 2.47-.81 2.953-1.964.057-.137-.037-.29-.184-.305-.863-.087-1.795-.135-2.769-.135Z" />
                            </svg>
                        </a>
                    </div>
                </div>
            </header>
        }
        .render(cx)
    }
}

pub struct LandingFooter;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LandingFooter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <footer
                data-marketing-footer
                class="border-t border-black/10 bg-[#fdfcfc]/70 dark:border-white/10 dark:bg-zinc-950/55"
            >
                <div class="mx-auto grid max-w-[1400px] gap-10 px-4 py-12 sm:px-6 lg:grid-cols-[minmax(0,1fr)_auto] lg:px-8">
                    <div class="max-w-md">
                        <a href="/" class="inline-flex text-zinc-950 dark:text-zinc-100">
                            {ProaBrand}
                        </a>
                        <p class="mt-4 text-sm leading-7 text-zinc-600 dark:text-zinc-300">
                            "The Rust web framework for people and agents. Build fast interfaces, from the server to the browser."
                        </p>
                        <nav
                            data-footer-socials
                            aria-label="Social links"
                            class="mt-5 flex items-center gap-2"
                        >
                            <a
                                href="https://github.com/proa-labs/proa"
                                rel="noreferrer noopener"
                                target="_blank"
                                aria-label="GitHub"
                                class="inline-flex h-9 w-9 items-center justify-center rounded-full border border-black/10 bg-white/70 text-zinc-600 transition-colors hover:bg-white hover:text-zinc-950 dark:border-white/10 dark:bg-white/5 dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-zinc-50 [&_svg]:h-5 [&_svg]:w-5"
                            >
                                <svg
                                    role="img"
                                    viewBox="0 0 24 24"
                                    fill="currentColor"
                                    aria-hidden="true"
                                >
                                    <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
                                </svg>
                            </a>
                            <a
                                href="https://proa.so/discord"
                                rel="noreferrer noopener"
                                target="_blank"
                                aria-label="Discord"
                                class="inline-flex h-9 w-9 items-center justify-center rounded-full border border-black/10 bg-white/70 text-zinc-600 transition-colors hover:bg-white hover:text-zinc-950 dark:border-white/10 dark:bg-white/5 dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-zinc-50 [&_svg]:h-5 [&_svg]:w-5"
                            >
                                <svg
                                    role="img"
                                    viewBox="0 0 24 24"
                                    fill="currentColor"
                                    aria-hidden="true"
                                >
                                    <path d="M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z" />
                                </svg>
                            </a>
                            <a
                                href="https://x.com/drewhariri"
                                rel="noreferrer noopener"
                                target="_blank"
                                aria-label="X"
                                class="inline-flex h-9 w-9 items-center justify-center rounded-full border border-black/10 bg-white/70 text-zinc-600 transition-colors hover:bg-white hover:text-zinc-950 dark:border-white/10 dark:bg-white/5 dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-zinc-50 [&_svg]:h-5 [&_svg]:w-5"
                            >
                                <svg
                                    role="img"
                                    viewBox="0 0 24 24"
                                    fill="currentColor"
                                    aria-hidden="true"
                                >
                                    <path d="M14.234 10.162 22.977 0h-2.072l-7.591 8.824L7.251 0H.258l9.168 13.343L.258 24H2.33l8.016-9.318L16.749 24h6.993zm-2.837 3.299-.929-1.329L3.076 1.56h3.182l5.965 8.532.929 1.329 7.754 11.09h-3.182z" />
                                </svg>
                            </a>
                            <a
                                href="https://www.reddit.com/r/proa_rs/"
                                rel="noreferrer noopener"
                                target="_blank"
                                aria-label="Reddit"
                                class="inline-flex h-9 w-9 items-center justify-center rounded-full border border-black/10 bg-white/70 text-zinc-600 transition-colors hover:bg-white hover:text-zinc-950 dark:border-white/10 dark:bg-white/5 dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-zinc-50 [&_svg]:h-5 [&_svg]:w-5"
                            >
                                <svg
                                    role="img"
                                    viewBox="0 0 24 24"
                                    fill="currentColor"
                                    aria-hidden="true"
                                >
                                    <path d="M12 0C5.373 0 0 5.373 0 12c0 3.314 1.343 6.314 3.515 8.485l-2.286 2.286C.775 23.225 1.097 24 1.738 24H12c6.627 0 12-5.373 12-12S18.627 0 12 0Zm4.388 3.199c1.104 0 1.999.895 1.999 1.999 0 1.105-.895 2-1.999 2-.946 0-1.739-.657-1.947-1.539v.002c-1.147.162-2.032 1.15-2.032 2.341v.007c1.776.067 3.4.567 4.686 1.363.473-.363 1.064-.58 1.707-.58 1.547 0 2.802 1.254 2.802 2.802 0 1.117-.655 2.081-1.601 2.531-.088 3.256-3.637 5.876-7.997 5.876-4.361 0-7.905-2.617-7.998-5.87-.954-.447-1.614-1.415-1.614-2.538 0-1.548 1.255-2.802 2.803-2.802.645 0 1.239.218 1.712.585 1.275-.79 2.881-1.291 4.64-1.365v-.01c0-1.663 1.263-3.034 2.88-3.207.188-.911.993-1.595 1.959-1.595Zm-8.085 8.376c-.784 0-1.459.78-1.506 1.797-.047 1.016.64 1.429 1.426 1.429.786 0 1.371-.369 1.418-1.385.047-1.017-.553-1.841-1.338-1.841Zm7.406 0c-.786 0-1.385.824-1.338 1.841.047 1.017.634 1.385 1.418 1.385.785 0 1.473-.413 1.426-1.429-.046-1.017-.721-1.797-1.506-1.797Zm-3.703 4.013c-.974 0-1.907.048-2.77.135-.147.015-.241.168-.183.305.483 1.154 1.622 1.964 2.953 1.964 1.33 0 2.47-.81 2.953-1.964.057-.137-.037-.29-.184-.305-.863-.087-1.795-.135-2.769-.135Z" />
                                </svg>
                            </a>
                        </nav>
                        // Temporarily hidden until the legal pages are ready.
                        /* <div class="mt-6 flex flex-col items-start gap-2 text-sm text-zinc-600 dark:text-zinc-400">
                            <a
                                href="/privacy"
                                class="transition-colors hover:text-zinc-950 dark:hover:text-zinc-50"
                            >
                                "Privacy Policy"
                            </a>
                            <a
                                href="/terms"
                                class="transition-colors hover:text-zinc-950 dark:hover:text-zinc-50"
                            >
                                "Terms and Conditions"
                            </a>
                        </div> */
                    </div>
                    <nav aria-label="Footer navigation" class="grid gap-8 text-sm sm:grid-cols-2">
                        <div>
                            <p class="font-medium text-zinc-950 dark:text-zinc-100">"Explore"</p>
                            <ul class="mt-4 space-y-3 text-zinc-600 dark:text-zinc-400">
                                <li>
                                    <a
                                        href="/ecosystem/apps"
                                        class="transition-colors hover:text-zinc-950 dark:hover:text-zinc-50"
                                    >
                                        "Apps"
                                    </a>
                                </li>
                                <li>
                                    <a
                                        href="/ecosystem"
                                        class="transition-colors hover:text-zinc-950 dark:hover:text-zinc-50"
                                    >
                                        "Ecosystem"
                                    </a>
                                </li>
                            </ul>
                        </div>
                        <div>
                            <p class="font-medium text-zinc-950 dark:text-zinc-100">"Resources"</p>
                            <ul class="mt-4 space-y-3 text-zinc-600 dark:text-zinc-400">
                                <li>
                                    <a
                                        href="/docs"
                                        class="transition-colors hover:text-zinc-950 dark:hover:text-zinc-50"
                                    >
                                        "Docs"
                                    </a>
                                </li>
                                <li>
                                    <a
                                        href="/sitemap.xml"
                                        class="transition-colors hover:text-zinc-950 dark:hover:text-zinc-50"
                                    >
                                        "Sitemap"
                                    </a>
                                </li>
                            </ul>
                        </div>
                    </nav>
                </div>
                <div class="border-t border-black/10 dark:border-white/10">
                    <div class="mx-auto flex max-w-[1400px] flex-col gap-3 px-4 py-5 text-xs text-zinc-500 dark:text-zinc-500 sm:flex-row sm:items-center sm:justify-between sm:px-6 lg:px-8">
                        <p>"© Proa Labs, Inc. All rights reserved."</p>
                        <p>"Rendered by Proa."</p>
                    </div>
                </div>
            </footer>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LandingFooter, LandingHeader, MarketingEcosystemNav, MarketingNav, MarketingNavActive,
        MarketingNavVariant,
    };
    use proa_core::{RenderBuffer, WebContext, WebRenderSync};

    #[test]
    fn marketing_nav_active_matches_top_level_routes() {
        assert_eq!(
            MarketingNavActive::for_path("/docs"),
            MarketingNavActive::Docs
        );
        assert_eq!(
            MarketingNavActive::for_path("/ecosystem"),
            MarketingNavActive::Ecosystem
        );
        assert_eq!(
            MarketingNavActive::for_path("/pricing"),
            MarketingNavActive::None
        );
        assert_eq!(
            MarketingNavActive::for_path("/blog"),
            MarketingNavActive::None
        );
    }

    #[test]
    fn marketing_nav_active_matches_nested_collection_routes() {
        assert_eq!(
            MarketingNavActive::for_path("/docs/ffi/search"),
            MarketingNavActive::Docs
        );
        assert_eq!(
            MarketingNavActive::for_path("/ecosystem/developer-tools"),
            MarketingNavActive::Ecosystem
        );
        assert_eq!(
            MarketingNavActive::for_path("/playground"),
            MarketingNavActive::None
        );
    }

    #[test]
    fn ecosystem_menu_uses_native_disclosure_markup() {
        let mut output = RenderBuffer::new();
        let mut cx = WebContext::with_buffer(&mut output);

        MarketingEcosystemNav {
            active: true,
            variant: MarketingNavVariant::Landing,
        }
        .render(&mut cx)
        .unwrap();

        let html = output.into_string();
        assert!(html.contains("<details"));
        assert!(html.contains("<summary"));
        assert!(html.contains(r#"data-component="marketing-ecosystem-menu""#));
        assert!(html.contains("data-dropdown-content"));
        assert_eq!(html.matches("data-dropdown-item").count(), 2);
    }

    #[test]
    fn both_header_navigation_variants_link_to_proa_ui() {
        for variant in [MarketingNavVariant::Landing, MarketingNavVariant::Docs] {
            let mut output = RenderBuffer::new();
            let mut cx = WebContext::with_buffer(&mut output);

            MarketingNav {
                variant,
                active: MarketingNavActive::Docs,
            }
            .render(&mut cx)
            .unwrap();

            let html = output.into_string();
            assert_eq!(html.matches(r#"href="https://ui.proa.so""#).count(), 1);
            assert!(html.contains(r#"href="https://ui.proa.so" class="marketing-nav-link">"#));
            assert!(html.contains(r#"href="/docs" class="marketing-nav-link" data-active="true">"#));
        }
    }

    #[test]
    fn landing_header_links_to_the_project_social_channels() {
        let mut output = RenderBuffer::new();
        let mut cx = WebContext::with_buffer(&mut output);

        LandingHeader {
            active: MarketingNavActive::None,
        }
        .render(&mut cx)
        .unwrap();

        let html = output.into_string();
        assert!(html.contains(
            r#"href="https://github.com/proa-labs/proa" rel="noreferrer noopener" target="_blank" aria-label="GitHub""#
        ));
        assert!(html.contains(
            r#"href="https://proa.so/discord" rel="noreferrer noopener" target="_blank" aria-label="Discord""#
        ));
        assert!(html.contains(
            r#"href="https://www.reddit.com/r/proa_rs/" rel="noreferrer noopener" target="_blank" aria-label="Reddit""#
        ));
    }

    #[test]
    fn landing_footer_links_to_all_project_social_channels() {
        let mut output = RenderBuffer::new();
        let mut cx = WebContext::with_buffer(&mut output);

        LandingFooter.render(&mut cx).unwrap();

        let html = output.into_string();
        assert!(html.contains(r#"data-footer-socials aria-label="Social links""#));
        for (href, label) in [
            ("https://github.com/proa-labs/proa", "GitHub"),
            ("https://proa.so/discord", "Discord"),
            ("https://x.com/drewhariri", "X"),
            ("https://www.reddit.com/r/proa_rs/", "Reddit"),
        ] {
            let link_contract = format!(
                r#"href="{href}" rel="noreferrer noopener" target="_blank" aria-label="{label}""#
            );
            assert!(html.contains(&link_contract));
        }
    }
}
