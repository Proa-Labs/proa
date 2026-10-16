// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use rsjs::{rsjs, signal};

use super::recipe::base;

/// The sticky ORIGIN header. The markdown chip, theme toggle, and the
/// machine-readable overlay dialog are one rsjs island (`HeaderTools`).
pub struct SiteHeader {
    pub query: String,
}

impl<L: DataLoader + 'static> proa_core::WebRender<L> for SiteHeader {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            proa_macros::html! {
                <header data-site-header class={base::HEADER}>
                    <a
                        href="#top"
                        class="flex items-baseline gap-[14px] text-inherit"
                    >
                        <span class={base::WORDMARK}>"ORIGIN"</span>
                    </a>
                    <div class="flex items-center gap-[8px] pl-[20px] border-l border-line">
                        <img
                            src={cx.request().url("/static/proa-logo.svg")}
                            width="20"
                            height="20"
                            alt=""
                            class="block"
                        />
                        <span class="font-display text-[10px] font-semibold tracking-[0.14em] uppercase text-brand">
                            "Built with Proa"
                        </span>
                    </div>
                    <div class="mr-auto"></div>
                    {proa_stream::Suspense::new(
                        HeaderToolsBoundary {
                            query: self.query.clone(), // proa-perf-ok: one short clone per request for the fallback chrome.
                        },
                        HeaderTools {
                            md_route: format!("{}/site/{}.md", cx.request().base_path().as_str(), self.query), // proa-perf-ok: one short string per request.
                            document: None,
                        },
                    )}
                </header>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

/// Streams the header tools without blocking the shell: the island awaits
/// the shared dossier key, so it joins the panes' single RIPEstat fan-out.
struct HeaderToolsBoundary {
    query: String,
}

impl<L: DataLoader> proa_core::WebRender<L> for HeaderToolsBoundary {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let dossier = cx
                .load_typed::<crate::model::Dossier>(crate::loader::dossier_key(&self.query))
                .await
                .ok();
            let host = dossier
                .as_ref()
                .and_then(|d| d.subject.domain.clone())
                .unwrap_or_else(|| self.query.clone());
            // proa-perf-ok(scope): overlay strings once per request.
            let md_text = crate::components::markdown_string_with_base_path(
                &self.query,
                dossier,
                cx.request().base_path(),
            );
            // proa-perf-ok: guided demo page; small per-request strings.
            let md_bytes = format!("{:.1} KB", md_text.len() as f64 / 1024.0);
            let md_route = format!("{}/site/{host}.md", cx.request().base_path().as_str()); // proa-perf-ok: guided demo page; small per-request strings.
            proa_core::WebRenderSync::render(
                HeaderTools {
                    md_route,
                    document: Some((md_text, md_bytes)),
                },
                cx,
            )
        })
    }
}

/// Header tools: the markdown overlay is the vendored proa-ui Dialog (the
/// runtime in proa-free.js handles open, close, focus, and escape) and the
/// theme switch is the vendored ThemeToggle. The document body is optional:
/// the Suspense fallback renders this same chrome with `None`, so the
/// header never blinks out while a cold lookup resolves. ORIGIN's skin
/// overrides the vendored recipe classes with `!` because both land at
/// equal specificity and stylesheet order would otherwise decide.
struct HeaderTools {
    md_route: String,
    document: Option<(String, String)>,
}

impl<L: DataLoader> WebRenderSync<L> for HeaderTools {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        WebRenderSync::render(
            proa_macros::html_sync! {
                <div data-header-tools class="flex items-center gap-[20px]">
                    {crate::components::Dialog {
                        default_open: false,
                        id: Some("md-overlay"),
                        trigger: Some(crate::components::DialogTrigger {
                            class: Some("hv-bg-ink gap-[8px]! border-0! rounded-none! shadow-none! h-auto! [font-family:inherit] bg-brand! text-bg! hover:text-bg! font-semibold! text-[10px]! tracking-[0.12em] uppercase py-[4px]! px-[10px]!"),
                            children: Some("Markdown"),
                        }),
                        content: Some(crate::components::DialogContent {
                            size: None,
                            class: Some("w-[min(1040px,calc(100vw_-_44px))]! max-w-none! gap-0! rounded-none! bg-bg2! border-line-strong! text-ink p-0!"),
                            show_close: false,
                            children: Some(MarkdownOverlayBody {
                                md_route: self.md_route,
                                document: self.document,
                            }),
                        }),
                    }}
                    {crate::components::ThemeToggle {
                        aria_label: "Toggle theme",
                        class: Some("border border-line-soft text-ink3 hover:text-ink"),
                    }}
                </div>
            },
            cx,
        )
    }
}

/// The overlay's body: header row with the GET link and close, then either
/// the markdown document or a loading note while the dossier streams.
struct MarkdownOverlayBody {
    md_route: String,
    document: Option<(String, String)>,
}

impl<L: DataLoader> WebRenderSync<L> for MarkdownOverlayBody {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let (md_text, md_bytes) = match &self.document {
            Some((text, bytes)) => (Some(text.as_str()), bytes.as_str()),
            None => (None, "…"),
        };
        WebRenderSync::render(
            proa_macros::html_sync! {
                <div>
                    <div class="flex items-center justify-between gap-[20px] py-[13px] px-[18px] border-b border-line">
                        <div class="flex items-baseline gap-[16px]">
                            <span class="text-[10px] tracking-[0.18em] text-ink4 uppercase">
                                "Machine-readable version"
                            </span>
                            <a
                                href={self.md_route.as_str()}
                                target="_blank"
                                rel="noopener"
                                class="hv-cyan text-[13px] text-brand"
                            >
                                "GET "
                                {self.md_route.as_str()}
                            </a>
                        </div>
                        <div class="flex items-center gap-[10px] text-[10px] tracking-[0.14em] uppercase">
                            <span class="text-ink4">{md_bytes}</span>
                            {md_text.map(|text| CopyMarkdown {
                                // proa-perf-ok: seeds the island's clipboard signal once per request.
                                md_text: text.to_string(),
                            })}
                            {crate::components::DialogClose {
                                class: Some("hv-bg-red bg-ink border-0 [font-family:inherit] text-bg py-[5px] px-[12px] font-semibold text-[10px] tracking-[0.14em] uppercase"),
                                children: Some("Close"),
                            }}
                        </div>
                    </div>
                    <div class="overflow-y-auto max-h-[calc(100vh_-_240px)] pt-[24px] px-[26px] pb-[40px]">
                        {match md_text {
                            Some(text) => proa_macros::html_sync! {
                                <pre
                                    data-md-text
                                    class="m-0 font-mono text-[12px] leading-[1.75] text-ink2 whitespace-pre-wrap text-pretty"
                                >
                                    {text}
                                </pre>
                            },
                            None => proa_macros::html_sync! {
                                <pre
                                    data-md-text
                                    class="m-0 font-mono text-[12px] leading-[1.75] text-ink4 whitespace-pre-wrap animate-[org-pulse_1.6s_ease-in-out_infinite]"
                                >
                                    "Rendering the markdown document…"
                                </pre>
                            },
                        }}
                    </div>
                    <div class="flex items-center gap-[20px] py-[11px] px-[18px] border-t border-line text-[10px] tracking-[0.1em] text-ink4 uppercase">
                        <span>"Same components, text output"</span>
                        <span class="text-cyan">"0 KB client javascript"</span>
                        <span>"one render pass, no scraper"</span>
                    </div>
                </div>
            },
            cx,
        )
    }
}

/// Clipboard needs a typed browser API, so the copy button stays an island.
struct CopyMarkdown {
    md_text: String,
}

// proa-perf-ok(scope): island signals take owned initial values; the one
// clone per render seeds the client-side markdown payload.
#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for CopyMarkdown {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let md = signal(self.md_text.clone());
        let copied = signal(false);
        proa_macros::html_sync! {
            <button
                type="button"
                class="hv-cyan border border-line-strong bg-transparent [font-family:inherit] py-[5px] px-[12px] cursor-pointer text-ink text-[10px] tracking-[0.14em] uppercase"
                data-md-copy
                on_click={|_| { rsjs::clipboard::write_text(md.get()); copied.set(true); }}
            >
                {if copied.get() { proa_macros::html_sync! {
                    <span>"Copied"</span>
                } } else { proa_macros::html_sync! {
                    <span>"Copy"</span>
                } }}
            </button>
        }
        .render(cx)
    }
}
