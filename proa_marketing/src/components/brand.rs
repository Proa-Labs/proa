// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

pub struct ProaBrand;

pub struct ProaLogoMark {
    pub class: &'static str,
}

impl Default for ProaLogoMark {
    fn default() -> Self {
        Self { class: "size-5" }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ProaLogoMark {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <svg
                viewBox="0 0 64 64"
                class={self.class}
                fill="none"
                xmlns="http://www.w3.org/2000/svg"
                aria-hidden="true"
            >
                <path
                    d="M13 36.5C17.6 25.7 25.5 18 36.2 13L34.8 33.5L13 36.5Z"
                    fill="var(--color-brand)"
                />
                <path
                    d="M34.6 14.5L30.9 44.6"
                    stroke="currentColor"
                    stroke-width="2.8"
                    stroke-linecap="round"
                />
                <path
                    d="M10 39.5C16.2 39.8 23.4 39.2 31.3 37.9C41.2 36.3 49 33.7 54 30.2C50.9 39.8 42.3 46.5 30 47.8C18.7 49 11.5 46 10 39.5Z"
                    fill="currentColor"
                />
                <path
                    d="M23.5 50.6L29.4 43.8"
                    stroke="currentColor"
                    stroke-width="2.4"
                    stroke-linecap="round"
                />
                <path
                    d="M40.9 49.7L35.3 42.4"
                    stroke="currentColor"
                    stroke-width="2.4"
                    stroke-linecap="round"
                />
                <path
                    d="M19.8 52.2C23.3 51.7 27.2 51.2 31.4 50.7C36.1 50.1 39.8 49.4 42.8 48.5C40.7 54 35.5 56.5 27.4 56C24 55.8 21.4 54.5 19.8 52.2Z"
                    fill="currentColor"
                    opacity="0.6"
                />
            </svg>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ProaBrand {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <span
                class="flex items-center gap-1 text-sm font-medium tracking-tight"
                data-proa-brand
            >
                <svg
                    viewBox="0 0 64 64"
                    class="size-9 shrink-0"
                    role="img"
                    aria-label="Proa"
                    fill="none"
                    xmlns="http://www.w3.org/2000/svg"
                >
                    <title>"Proa"</title>
                    <path
                        d="M13 36.5C17.6 25.7 25.5 18 36.2 13L34.8 33.5L13 36.5Z"
                        fill="var(--color-brand)"
                    />
                    <path
                        d="M34.6 14.5L30.9 44.6"
                        stroke="currentColor"
                        stroke-width="2.8"
                        stroke-linecap="round"
                    />
                    <path
                        d="M10 39.5C16.2 39.8 23.4 39.2 31.3 37.9C41.2 36.3 49 33.7 54 30.2C50.9 39.8 42.3 46.5 30 47.8C18.7 49 11.5 46 10 39.5Z"
                        fill="currentColor"
                    />
                    <path
                        d="M23.5 50.6L29.4 43.8"
                        stroke="currentColor"
                        stroke-width="2.4"
                        stroke-linecap="round"
                    />
                    <path
                        d="M40.9 49.7L35.3 42.4"
                        stroke="currentColor"
                        stroke-width="2.4"
                        stroke-linecap="round"
                    />
                    <path
                        d="M19.8 52.2C23.3 51.7 27.2 51.2 31.4 50.7C36.1 50.1 39.8 49.4 42.8 48.5C40.7 54 35.5 56.5 27.4 56C24 55.8 21.4 54.5 19.8 52.2Z"
                        fill="currentColor"
                        opacity="0.6"
                    />
                </svg>
                <span class="font-display text-base font-semibold tracking-tight">"Proa"</span>
            </span>
        }
        .render(cx)
    }
}
