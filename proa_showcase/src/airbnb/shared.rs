// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared assets and render helpers for the standalone Airbnb showcase pages.
#![allow(unused_must_use)]

use proa_core::{StaticRaw, WebContext, WebRenderSync, WriteBuf};
use proa_macros::{html_sync, include_css};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const AIRBNB_LOGO_SVG: &str = r##"<svg viewBox="0 0 64 64" width="32" height="32" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><path d="M60.9 45.487l-.966-2.305-1.475-3.27-.062-.062a661.83 661.83 0 0 0-14.15-28.957l-.198-.384-1.524-3.073a18.4 18.4 0 0 0-2.305-3.52A10.35 10.35 0 0 0 32.027 0a10.76 10.76 0 0 0-8.203 3.84 22.1 22.1 0 0 0-2.305 3.52l-1.735 3.395c-4.956 9.615-9.74 19.342-14.163 28.957l-.062.124c-.384 1.053-.892 2.13-1.413 3.284-.322.702-.644 1.47-.966 2.305a14.4 14.4 0 0 0-.768 6.914 13.63 13.63 0 0 0 8.327 10.631 13.16 13.16 0 0 0 5.192 1.028 14.57 14.57 0 0 0 1.66-.124 16.93 16.93 0 0 0 6.406-2.18 32.44 32.44 0 0 0 7.943-6.666 33.62 33.62 0 0 0 7.943 6.666 16.92 16.92 0 0 0 6.406 2.18c.55.073 1.105.114 1.66.124 1.783.018 3.55-.332 5.192-1.028a13.63 13.63 0 0 0 8.327-10.631 12.11 12.11 0 0 0-.582-6.852zM32.026 48.82c-3.457-4.362-5.7-8.45-6.468-11.92-.314-1.277-.38-2.6-.198-3.903.127-.965.48-1.886 1.028-2.7a6.79 6.79 0 0 1 5.638-2.825c2.236-.086 4.362.974 5.638 2.813a6.17 6.17 0 0 1 1.028 2.69 10.3 10.3 0 0 1-.198 3.903c-.768 3.395-3 7.435-6.468 11.92zm25.562 3c-.5 3.337-2.7 6.166-5.836 7.435a9.7 9.7 0 0 1-4.857.706 12.6 12.6 0 0 1-4.87-1.66 29.91 29.91 0 0 1-7.298-6.195c4.225-5.192 6.8-9.913 7.757-14.163a16.11 16.11 0 0 0 .322-5.452c-.238-1.567-.832-3.06-1.735-4.362-2.062-2.942-5.453-4.666-9.045-4.597-3.572-.046-6.942 1.65-9.033 4.547-.903 1.303-1.497 2.794-1.735 4.362a13.31 13.31 0 0 0 .322 5.452c.966 4.225 3.593 9.033 7.757 14.225a28.79 28.79 0 0 1-7.298 6.195 12.6 12.6 0 0 1-4.882 1.71 10.26 10.26 0 0 1-4.87-.644C9.16 58.12 6.94 55.292 6.45 51.954a10.61 10.61 0 0 1 .582-4.956c.198-.644.508-1.24.83-2.044.446-1.028.966-2.12 1.475-3.2l.062-.124c4.424-9.54 9.157-19.28 14.1-28.772l.186-.458 1.536-2.95a14.05 14.05 0 0 1 1.846-2.838 6.73 6.73 0 0 1 10.247 0 13.87 13.87 0 0 1 1.747 2.813l1.536 2.95.186.384c4.87 9.553 9.628 19.28 14.04 28.834v.062c.508 1.028.966 2.18 1.475 3.2.322.768.644 1.413.83 2.044a10.81 10.81 0 0 1 .446 4.956z" fill="#FF385C" fill-rule="evenodd"/></svg>"##;
#[deprecated(note = "use AIRBNB_LOGO_SVG for the restored branded showcase")]
pub const COAST_STAYS_MARK: &str = "/images/airbnb/external/search-homes.png";
pub const GLOBE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" aria-hidden="true" role="presentation" focusable="false" style="display:block;height:16px;width:16px;fill:currentcolor"><path d="M8 .25a7.77 7.77 0 0 1 7.75 7.78 7.75 7.75 0 0 1-7.52 7.72h-.25A7.75 7.75 0 0 1 .25 8.24v-.25A7.75 7.75 0 0 1 8 .25zm1.95 8.5h-3.9c.15 2.9 1.17 5.34 1.88 5.5H8c.68 0 1.72-2.37 1.93-5.23zm4.26 0h-2.76c-.09 1.96-.53 3.78-1.18 5.08A6.26 6.26 0 0 0 14.17 9zm-9.67 0H1.8a6.26 6.26 0 0 0 3.94 5.08 12.59 12.59 0 0 1-1.16-4.7l-.03-.38zm1.2-6.58-.12.05a6.26 6.26 0 0 0-3.83 5.03h2.75c.09-1.83.48-3.54 1.06-4.81zm2.25-.42c-.7 0-1.78 2.51-1.94 5.5h3.9c-.15-2.9-1.18-5.34-1.89-5.5h-.07zm2.28.43.03.05a12.95 12.95 0 0 1 1.15 5.02h2.75a6.28 6.28 0 0 0-3.93-5.07z"></path></svg>"##;
pub const HEART_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="filter:drop-shadow(0 1px 2px rgba(0,0,0,.4))"><path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/></svg>"##;

pub const DOCUMENT_HEAD_START_RAW: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1.0"/>
<meta name="turbo-refresh-method" content="morph"/>
<meta name="turbo-refresh-scroll" content="preserve"/>
<script type="importmap">{"imports":{"react":"https://esm.sh/react@19","react/jsx-runtime":"https://esm.sh/react@19/jsx-runtime","react-dom":"https://esm.sh/react-dom@19","react-dom/client":"https://esm.sh/react-dom@19/client"}}</script>
"##;
const DOCUMENT_HEAD_START: &str = DOCUMENT_HEAD_START_RAW;
pub const DOCUMENT_HEAD_END_RAW: &str = r##"
<script type="module">import * as Turbo from "https://esm.sh/@hotwired/turbo@8.0.12";</script>
<script src="https://cdn.tailwindcss.com"></script>
"##;
const DOCUMENT_HEAD_END: &str = DOCUMENT_HEAD_END_RAW;
pub const DOCUMENT_END_RAW: &str = "</body></html>";
const DOCUMENT_END: &str = DOCUMENT_END_RAW;
pub const BODY_OPEN_RAW: &str = "</head><body class=\"bg-white text-[#222] air-demo-page\">";
const BODY_OPEN: &str = BODY_OPEN_RAW;
pub const SHARED_CSS_RAW: &str = include_css!("src/airbnb/shared.css");
const SHARED_CSS: &str = SHARED_CSS_RAW;

fn resolve_asset_path(public_path: &str) -> Option<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent()?;
    match public_path {
        "/proa.css" => Some(
            workspace_root
                .join("proa_marketing")
                .join("public")
                .join("styles.css"),
        ),
        // `/proa.js` is an inline response owned by the showcase web boundary.
        // Its source file therefore provides the cache-busting timestamp.
        "/proa.js" => Some(workspace_root.join("proa_showcase_web").join("src/lib.rs")),
        _ => public_path.strip_prefix("/assets/").map(|path| {
            workspace_root
                .join("proa_showcase_web")
                .join("public/assets")
                .join(path)
        }),
    }
}

fn asset_version_token(public_path: &str) -> Option<u128> {
    let path = resolve_asset_path(public_path)?;
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_millis())
}

pub fn versioned_asset_path(public_path: &str) -> String {
    match asset_version_token(public_path) {
        Some(version) => format!("{public_path}?v={version}"),
        None => public_path.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)] // Asset tests stay beside asset path helpers.
mod asset_tests {
    use super::{resolve_asset_path, versioned_asset_path};

    #[test]
    fn resolves_assets_from_their_workspace_owners() {
        for public_path in [
            "/proa.css",
            "/proa.js",
            "/assets/airbnb-showcase.js",
            "/assets/DatePickerIsland.js",
            "/assets/BookingCardIsland.js",
        ] {
            let resolved = resolve_asset_path(public_path).expect("known showcase asset");
            assert!(resolved.is_file(), "missing {}", resolved.display());
            assert!(versioned_asset_path(public_path).contains("?v="));
        }
    }
}

pub fn render_document_start<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    title_parts: &[&str],
) {
    render_document_start_with_assets(
        cx,
        title_parts,
        "/proa.css",
        "/proa.js",
        "/assets/airbnb-showcase.js",
    );
}

/// Render document start with pre-resolved asset paths (no filesystem I/O).
pub fn render_document_start_with_assets<B: WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    title_parts: &[&str],
    proa_css: &str,
    proa_js: &str,
    showcase_js: &str,
) {
    let full_title = title_parts.concat();
    html_sync! {
        {StaticRaw(DOCUMENT_HEAD_START)}
        <link rel="stylesheet" href={proa_css} />
        <script src={proa_js} defer></script>
        <script type="module" src={showcase_js}></script>
        {StaticRaw(DOCUMENT_HEAD_END)}
        <title>{proa_core::raw_text_ok(full_title.as_str())}</title>
        <style>{StaticRaw(SHARED_CSS)}</style>
        {StaticRaw(BODY_OPEN)}
    }
    .render(cx);
}

pub fn render_header<B: WriteBuf, L: ::proa_core::DataLoader>(cx: &mut WebContext<L, B>) {
    html_sync! {
        <header class="air-topbar">
            <div class="air-shell">
                <div class="air-topbar-inner grid grid-cols-[auto_1fr_auto] items-center gap-5">
                    <a
                        href="/showcase/airbnb/proa"
                        class="flex items-center gap-2.5 text-[#FF385C] font-semibold text-[28px] leading-none tracking-[-0.02em]"
                    >
                        {StaticRaw(AIRBNB_LOGO_SVG)}
                        <span class="text-[30px] leading-none">"airbnb"</span>
                    </a>
                    <div class="flex justify-center">
                        <a
                            href="/showcase/airbnb/proa/search"
                            class="air-search-pill hidden md:flex items-center h-12 rounded-full border border-[var(--air-line)] bg-white px-2"
                        >
                            <span class="px-4 text-sm font-semibold">"Anywhere"</span>
                            <span class="h-6 w-px bg-[var(--air-line)]"></span>
                            <span class="px-4 text-sm font-semibold">"Any week"</span>
                            <span class="h-6 w-px bg-[var(--air-line)]"></span>
                            <span class="px-4 text-sm text-[var(--air-muted)]">"Add guests"</span>
                            <span class="ml-1 flex h-8 w-8 items-center justify-center rounded-full bg-[#FF385C] text-white">
                                <svg
                                    viewBox="0 0 32 32"
                                    xmlns="http://www.w3.org/2000/svg"
                                    style="display:block;fill:none;height:12px;width:12px;stroke:currentColor;stroke-width:5.333;overflow:visible"
                                    aria-hidden="true"
                                    role="presentation"
                                    focusable="false"
                                >
                                    <path d="m13 24c6.0751 0 11-4.9249 11-11 0-6.0751-4.9249-11-11-11-6.0751 0-11 4.9249-11 11 0 6.0751 4.9249 11 11 11zm8-3 10 10" />
                                </svg>
                            </span>
                        </a>
                        <a
                            href="/showcase/airbnb/proa/search"
                            class="air-search-pill flex md:hidden items-center gap-3 rounded-full border border-[var(--air-line)] bg-white px-4 py-3"
                        >
                            <span class="flex h-8 w-8 items-center justify-center rounded-full bg-[#FF385C] text-white">
                                <svg
                                    viewBox="0 0 32 32"
                                    xmlns="http://www.w3.org/2000/svg"
                                    style="display:block;fill:none;height:12px;width:12px;stroke:currentColor;stroke-width:5.333;overflow:visible"
                                    aria-hidden="true"
                                    role="presentation"
                                    focusable="false"
                                >
                                    <path d="m13 24c6.0751 0 11-4.9249 11-11 0-6.0751-4.9249-11-11-11-6.0751 0-11 4.9249-11 11 0 6.0751 4.9249 11 11 11zm8-3 10 10" />
                                </svg>
                            </span>
                            <span class="text-sm font-semibold">"Start your search"</span>
                        </a>
                    </div>
                    <div class="flex items-center justify-end gap-2">
                        <a
                            href="/showcase/airbnb/proa"
                            class="hidden md:block rounded-full px-4 py-3 text-sm font-semibold hover:bg-[var(--air-soft)]"
                        >
                            "Airbnb your home"
                        </a>
                        <button
                            class="hidden md:flex h-10 w-10 items-center justify-center rounded-full hover:bg-[var(--air-soft)]"
                            aria-label="Language"
                        >
                            {StaticRaw(GLOBE_SVG)}
                        </button>
                        <button
                            class="air-search-pill flex items-center gap-3 rounded-full border border-[var(--air-line)] bg-white pl-3 pr-2.5 py-1.5"
                            aria-label="Search"
                        >
                            <svg
                                xmlns="http://www.w3.org/2000/svg"
                                width="16"
                                height="16"
                                viewBox="0 0 32 32"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="3"
                                stroke-linecap="round"
                                aria-hidden="true"
                            >
                                <line x1="6" y1="9" x2="26" y2="9" />
                                <line x1="6" y1="16" x2="26" y2="16" />
                                <line x1="6" y1="23" x2="26" y2="23" />
                            </svg>
                            <div class="flex h-8 w-8 items-center justify-center rounded-full bg-[#6A6A6A] text-white">
                                <svg
                                    xmlns="http://www.w3.org/2000/svg"
                                    width="15"
                                    height="15"
                                    viewBox="0 0 24 24"
                                    fill="currentColor"
                                    aria-hidden="true"
                                >
                                    <path d="M12 12c2.209 0 4-1.791 4-4S14.209 4 12 4s-4 1.791-4 4 1.791 4 4 4zm0 2c-2.673 0-8 1.327-8 3.963V20h16v-2.037C20 15.327 14.673 14 12 14z" />
                                </svg>
                            </div>
                        </button>
                    </div>
                </div>
            </div>
        </header>
    }
    .render(cx);
}

pub fn render_footer<B: WriteBuf, L: ::proa_core::DataLoader>(cx: &mut WebContext<L, B>) {
    html_sync! {
        // ── Disclaimer banner ──
        <div class="border-t border-b border-[#e2c56b] bg-[#fef9eb]">
            <div class="air-shell">
                <p class="py-3 text-xs text-[#7a6520] leading-relaxed text-center">
                    <strong>"Disclaimer:"</strong>
                    " This is an unofficial technology demo built with "
                    <a href="/" class="underline font-medium">"Proa UI"</a>
                    " to showcase server-rendered UI components. It is "
                    <strong>"not affiliated with, endorsed by, or connected to Airbnb, Inc."</strong>
                    " in any way. All Airbnb trademarks, logos, and design elements belong to their respective owners and are used here solely for illustrative purposes."
                </p>
            </div>
        </div>
        <footer class="border-t border-[var(--air-line)] bg-[var(--air-soft)]">
            <div class="air-shell">
                <div class="grid grid-cols-1 gap-8 py-12 text-sm md:grid-cols-3">
                    <div>
                        <p class="mb-3 font-semibold">"Support"</p>
                        <ul class="space-y-2 text-[var(--air-muted)]">
                            <li><a href="#support" class="hover:underline">"Help Center"</a></li>
                            <li><a href="#support" class="hover:underline">"AirCover"</a></li>
                            <li><a href="#support" class="hover:underline">"Cancellation options"</a></li>
                        </ul>
                    </div>
                    <div>
                        <p class="mb-3 font-semibold">"Hosting"</p>
                        <ul class="space-y-2 text-[var(--air-muted)]">
                            <li><a href="#hosting" class="hover:underline">"Airbnb your home"</a></li>
                            <li><a href="#hosting" class="hover:underline">"Hosting resources"</a></li>
                            <li><a href="#hosting" class="hover:underline">"Community forum"</a></li>
                        </ul>
                    </div>
                    <div>
                        <p class="mb-3 font-semibold">"Airbnb"</p>
                        <ul class="space-y-2 text-[var(--air-muted)]">
                            <li><a href="#about" class="hover:underline">"Newsroom"</a></li>
                            <li><a href="#about" class="hover:underline">"Careers"</a></li>
                            <li><a href="#about" class="hover:underline">"Investors"</a></li>
                        </ul>
                    </div>
                </div>
                <div class="flex flex-col gap-4 border-t border-[var(--air-line)] py-6 text-sm text-[var(--air-muted)] md:flex-row md:items-center md:justify-between">
                    <p>"© 2026 Proa Labs, Inc. · This is a demo, not a real Airbnb site · "
                       "Airbnb\u{00AE} is a registered trademark of Airbnb, Inc."</p>
                    <p>"Built with "<a href="/" class="underline">"Proa UI"</a>" (showcase demo)"</p>
                </div>
            </div>
        </footer>
    }
    .render(cx);
}

pub fn render_document_end<B: WriteBuf, L: ::proa_core::DataLoader>(cx: &mut WebContext<L, B>) {
    html_sync! {
        {StaticRaw(DOCUMENT_END)}
    }
    .render(cx);
}
