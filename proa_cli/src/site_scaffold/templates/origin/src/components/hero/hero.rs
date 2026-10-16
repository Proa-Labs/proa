// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

use crate::components::{Button, ButtonType};
use crate::model::SITES;

use super::recipe::base;

/// The hero: headline, lookup form, try-chips, and the radar. The lookup is a
/// plain GET form so tracing works before any client script runs.
pub struct Hero {
    pub query: String,
    pub active_host: Option<&'static str>,
}

impl<L: DataLoader> WebRenderSync<L> for Hero {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <section
                id="top"
                data-hero
                data-screen-label="Hero"
                class="relative overflow-hidden border-b border-line"
            >
                <div class="absolute inset-0 bg-[image:linear-gradient(var(--grid)_1px,transparent_1px),linear-gradient(90deg,var(--grid)_1px,transparent_1px)] [background-size:64px_64px] [mask-image:radial-gradient(120%_90%_at_72%_18%,#000_20%,transparent_75%)]"></div>
                <div
                    aria-hidden="true"
                    data-hero-radar
                    class="absolute top-[40px] right-[60px] w-[400px] h-[400px] pointer-events-none"
                >
                    <div class="absolute inset-0 rounded-full border border-[color-mix(in_srgb,var(--cyan)_16%,transparent)]"></div>
                    <div class="absolute inset-[58px] rounded-full border border-[color-mix(in_srgb,var(--cyan)_13%,transparent)]"></div>
                    <div class="absolute inset-[116px] rounded-full border border-[color-mix(in_srgb,var(--cyan)_11%,transparent)]"></div>
                    <div class="absolute inset-[172px] rounded-full border border-[color-mix(in_srgb,var(--cyan)_22%,transparent)]"></div>
                    <div class="absolute left-0 right-0 top-[50%] h-[1px] bg-[color-mix(in_srgb,var(--cyan)_10%,transparent)]"></div>
                    <div class="absolute top-0 bottom-0 left-[50%] w-[1px] bg-[color-mix(in_srgb,var(--cyan)_10%,transparent)]"></div>
                    <div class="absolute inset-0 rounded-full bg-[image:conic-gradient(from_0deg,transparent_0deg,transparent_300deg,color-mix(in_srgb,var(--cyan)_16%,transparent)_352deg,color-mix(in_srgb,var(--cyan)_42%,transparent)_360deg)] animate-[org-spin_5.5s_linear_infinite]"></div>
                    {Blips}
                </div>
                <div class="relative pt-[76px] px-[22px] pb-[52px]">
                    <h1 class="font-display font-bold text-[92px] leading-[0.92] tracking-[-0.045em] m-0 max-w-[16em]">
                        "SEE HOW THE INTERNET"
                        <br />
                        "REACHES ANY "
                        <span class="text-cyan">"WEBSITE"</span>
                        "."
                    </h1>
                    <p class="max-w-[66ch] mt-[30px] mx-0 mb-0 text-[13px] leading-[1.8] text-ink2 text-pretty">
                        "Enter a website or IP address to see which company connects it to the internet, how traffic reaches it, and whether anything unusual has recently changed."
                    </p>
                    <form
                        method="get"
                        action={cx.request().url("/")}
                        class="flex items-stretch mt-[40px] max-w-[720px] border border-line-strong"
                    >
                        <div class="flex items-center py-0 px-[14px] text-[10px] tracking-[0.14em] text-ink4 border-r border-line">
                            "LOOK UP"
                        </div>
                        <input
                            name="q"
                            value={self.query.as_str()}
                            placeholder="github.com"
                            autocomplete="off"
                            class="flex-1 bg-transparent border-0 outline-0 py-[17px] px-[16px] text-ink font-mono text-[14px]"
                        />
                        {Button {
                            button_type: ButtonType::Submit,
                            class: Some("origin-trace hv-bg-cyan"),
                            children: Some("TRACE"),
                            ..Button::default()
                        }}
                    </form>
                    <div class="flex items-center gap-[10px] mt-[12px]">
                        <span class="text-[10px] tracking-[0.14em] text-ink5 uppercase">"Try"</span>
                        {Chips { active: self.active_host }}
                    </div>
                </div>
            </section>
        }
        .render(cx)
    }
}

struct Blips;
impl<L: DataLoader> WebRenderSync<L> for Blips {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for class in base::BLIPS {
            html_sync! {
                <div class={*class}></div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct Chips {
    active: Option<&'static str>,
}
impl<L: DataLoader> WebRenderSync<L> for Chips {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for site in SITES {
            let chip = if Some(site.host) == self.active {
                base::CHIP_ACTIVE
            } else {
                base::CHIP
            };
            let classes = proa_core::ClassList::<2>::new().add("hv-cyan").add(chip);
            // proa-perf-ok: four short hrefs per request on a demo page.
            let href = format!("/?q={}", site.host);
            html_sync! {
                <a class={classes} href={cx.request().url(href.as_str())}>{site.host}</a>
            }
            .render(cx)?;
        }
        Ok(())
    }
}
