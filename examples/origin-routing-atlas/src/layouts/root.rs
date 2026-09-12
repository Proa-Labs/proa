// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::sync::OnceLock;

use proa_core::{
    ClassList, DataLoader, RenderOutcome, UnsafeRaw, WebContext, WebRender, WriteError,
};
use proa_fonts::Font;
use proa_fonts_macros::google_font;
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

/// `Font::head` writes through a `NoLoader` context, while this page renders
/// under the RIPEstat loader; the head fragment is static per process, so it
/// is rendered once and embedded as pre-rendered trusted HTML.
fn fonts_head_html() -> &'static str {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut cx = proa_core::WebContext::new();
        SANS.head(&mut cx).expect("render sans font head");
        MONO.head(&mut cx).expect("render mono font head");
        cx.into_output().into_string()
    })
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

// proa-perf-ok(scope): the fonts head fragment is rendered once per
// process and cached; UnsafeRaw re-emits that trusted pre-rendered HTML.
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
            html! {
                <!DOCTYPE html>
                <html lang="en" class={font_classes}>
                    <head>
                        <meta charset="utf-8" />
                        <meta name="viewport" content="width=device-width, initial-scale=1" />
                        <meta
                            name="description"
                            content="ORIGIN shows how the internet reaches any website: the addresses, the networks, and the routes between them."
                        />
                        <meta name="theme-color" content="#0b0b0c" />
                        <title>"ORIGIN — how the internet reaches websites"</title>
                        <link rel="preconnect" href="https://stat.ripe.net" />
                        <link rel="preconnect" href="https://ris-live.ripe.net" />
                        <link rel="icon" href="/static/favicon.svg" />
                        // proa-perf-ok: a static trusted literal; it must run
                        // synchronously during head parsing so the stored theme
                        // applies before anything paints.
                        {UnsafeRaw(THEME_BOOTSTRAP)}
                        {UnsafeRaw(fonts_head_html())}
                        <link rel="stylesheet" href={css_href()} />
                        <link rel="stylesheet" href="/static/proa/proa-ui/proa-free.css" />
                        <script type="module" src="/static/proa/proa-ui/proa-free.js"></script>
                        <script type="module" src={js_src()}></script>
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
