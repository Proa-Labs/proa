// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::sync::OnceLock;

use proa_core::{
    ClassList, DataLoader, Metadata, RenderOutcome, StaticRaw, WebContext, WebRender,
    WebRenderSync, WriteError,
};
use proa_fonts::{google_font, Font};
use proa_macros::html;

static SANS: Font = google_font!(
    "Instrument Sans",
    subsets = [latin],
    weights = [400, 500, 600, 700, 800, 900],
    variable = "--font-sans"
);
static MONO: Font = google_font!(
    "Azeret Mono",
    subsets = [latin],
    weights = [400, 500, 600, 700],
    variable = "--font-mono"
);

struct FontsHead;

impl<L: DataLoader> WebRenderSync<L> for FontsHead {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        SANS.head(cx)?;
        MONO.head(cx)
    }
}

/// Cache-busted asset URLs: ServeDir sends no Cache-Control, so browsers
/// apply heuristic freshness and can keep serving a stale copy after a
/// deploy. Keying the query string to the compiled-in byte length makes any
/// content change a new URL.
// proa-perf-ok(scope): one URL built once per process behind OnceLock.
fn js_src() -> &'static str {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(|| {
        format!(
            "/static/origin.js?v={}",
            include_bytes!("../../public/origin.js").len()
        )
    })
}

// proa-perf-ok(scope): one URL built once per process behind OnceLock.
fn css_href() -> &'static str {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(|| {
        format!(
            "/static/styles.css?v={}",
            include_bytes!("../../public/styles.css").len()
        )
    })
}

/// Runs before first paint: resolves the theme exactly as the proa-ui
/// runtime does (stored `theme` key, else the OS preference) and sets the
/// `dark` class plus `color-scheme` so a stored light theme never flashes
/// dark during a navigation.
const THEME_BOOTSTRAP: &str = "<script>(function(){try{var t=localStorage.getItem(\"theme\");var d=t?t===\"dark\":window.matchMedia(\"(prefers-color-scheme: dark)\").matches;var c=document.documentElement.classList;if(d){c.add(\"dark\")}else{c.remove(\"dark\")}document.documentElement.style.colorScheme=d?\"dark\":\"light\"}catch(e){}})()</script>";

pub struct RootDocument<C> {
    pub content: C,
}

impl<L, C> WebRender<L> for RootDocument<C>
where
    L: DataLoader,
    C: WebRender<L> + Send + 'static,
{
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            // ORIGIN is dark-first: the server ships `dark` and the head
            // bootstrap corrects it before first paint when the visitor has
            // chosen (or their OS prefers) light.
            let font_classes = ClassList::<3>::new()
                .add(SANS.class_name())
                .add(MONO.class_name())
                .add("dark");
            cx.metadata(Metadata::new()
                .title("ORIGIN — how the internet reaches websites")
                .description("ORIGIN shows how the internet reaches any website: the addresses, the networks, and the routes between them."))?;
            let favicon = cx.request().url("/static/favicon.svg");
            let stylesheet = cx.request().url(css_href());
            let ui_stylesheet = cx.request().url("/static/proa/proa-ui/proa-free.css");
            let ui_script = cx.request().url("/static/proa/proa-ui/proa-free.js");
            let script = cx.request().url(js_src());
            html! {
                <!DOCTYPE html>
                <html lang="en" class={font_classes}>
                    <head>
                        <meta charset="utf-8" />
                        <meta name="viewport" content="width=device-width, initial-scale=1" />
                        {cx.metadata.head()}
                        <meta name="theme-color" content="#0b0b0c" />
                        <link rel="preconnect" href="https://stat.ripe.net" />
                        <link rel="preconnect" href="https://ris-live.ripe.net" />
                        <link rel="icon" href={favicon} />
                        {StaticRaw(THEME_BOOTSTRAP)}
                        {proa_core::to_async(FontsHead)}
                        <link rel="stylesheet" href={stylesheet} />
                        <link rel="stylesheet" href={ui_stylesheet} />
                        <script type="module" src={ui_script}></script>
                        <script type="module" src={script}></script>
                    </head>
                    <body>{self.content}</body>
                </html>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
