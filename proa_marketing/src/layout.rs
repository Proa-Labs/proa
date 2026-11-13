// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::{routes, site};

mod structured_data;

use axum::response::{Html, IntoResponse, Response};
use proa_core::shared::attr_value::display_attr;
use proa_core::text::text;
use proa_core::{
    raw_text_ok, render_in_context_async, DataLoader, HeadConfig, HeadMedia, HeadUrlPolicy,
    NoLoader, OpenGraph, RenderBuffer, RenderOutcome, StaticRaw, Twitter, TwitterCard, WebContext,
    WebRender, WebRenderSync, WriteBuf, WriteError,
};
use proa_framework_core::{FrameworkBody, FrameworkRequestContext, RouteResponse};
use proa_macros::{html, html_sync};

use crate::{analytics::google_analytics, utils::site_url::public_site_url};

use crate::components::{
    landing_shell::{LandingFooter, LandingHeader, MarketingNavActive},
    search::SearchModal,
};

#[derive(Debug, Clone, Copy)]
pub struct PageMeta {
    pub title: &'static str,
    pub description: &'static str,
    pub path: &'static str,
}

/// Render the marketing shell: landing header, `<main>`, landing footer.
pub fn render_page<C>(meta: PageMeta, content: C) -> Html<String>
where
    C: WebRenderSync<NoLoader, RenderBuffer> + Send + 'static,
{
    render_chained_html(Document {
        shell: Shell::Marketing,
        path: meta.path,
        metadata: DocumentMetadata::new(meta),
        managed: false,
        content,
    })
}

/// Render the docs shell: no landing chrome, the docs enhancement script, and
/// the content owning its own layout.
pub fn render_docs_page<C>(meta: PageMeta, content: C) -> Html<String>
where
    C: WebRenderSync<NoLoader, RenderBuffer> + Send + 'static,
{
    render_chained_html(Document {
        shell: Shell::Docs,
        path: meta.path,
        metadata: DocumentMetadata::new(meta),
        managed: false,
        content,
    })
}

/// Render a Page that owns its metadata through `render_head`.
pub fn render_managed_page<C>(path: &'static str, content: C) -> Response
where
    C: WebRenderSync<NoLoader, RenderBuffer> + Send + 'static,
{
    static HEAD: std::sync::OnceLock<HeadConfig> = std::sync::OnceLock::new();
    let head = HEAD.get_or_init(|| {
        let origin = public_site_url();
        HeadConfig::default()
            .with_urls(HeadUrlPolicy::new(origin, "/").expect("valid public site origin"))
            .with_defaults(|head| {
                let mut image = HeadMedia::new(SOCIAL_IMAGE_SRC)
                    .mime_type("image/jpeg")
                    .alt(SOCIAL_IMAGE_ALT)
                    .dimensions(1536, 1024);
                if origin.starts_with("https://") {
                    image = image.secure_url(SOCIAL_IMAGE_SRC);
                }
                head.open_graph(
                    OpenGraph::new()
                        .site_name(site::NAME)
                        .locale("en_US")
                        .kind("website")
                        .images([image]),
                )?;
                head.twitter(
                    Twitter::new()
                        .card(TwitterCard::SummaryLargeImage)
                        .site(site::SOCIAL_HANDLE),
                )
            })
            .expect("valid shared marketing metadata")
    });
    let response = futures::executor::block_on(
        RouteResponse::ssr_async(Document {
            shell: Shell::Marketing,
            path,
            metadata: ManagedMetadata { path },
            managed: true,
            content,
        })
        .with_head(head.clone())
        .into_framework_response(&FrameworkRequestContext::default()),
    )
    .expect("render managed marketing page");
    let FrameworkBody::Full(body) = response.body else {
        unreachable!("buffered page produces a complete response")
    };
    (response.status, response.headers, body).into_response()
}

/// Drive any async root to a finished HTML response.
///
/// This is the one place a render becomes a `String`. Pages with their own
/// document shell (demos, the ecosystem microsites) call it directly instead
/// of going through [`render_page`] / [`render_docs_page`].
///
/// `block_on` is safe here rather than making the whole call chain async:
/// nothing in a marketing render awaits I/O, so the future completes on its
/// first poll and never parks the worker thread. The async trait is only in
/// play because rsjs islands render through [`WebRender`].
pub(crate) fn render_chained_html<R>(root: R) -> Html<String>
where
    R: WebRender<NoLoader, RenderBuffer>,
{
    let out = futures::executor::block_on(async {
        let mut cx = WebContext::new();
        render_in_context_async(&mut cx, root).await.unwrap();
        cx.into_output()
    });
    Html(out.into_string())
}

/// Which chrome a page wears. Both shells share one `<head>` and one `<body>`
/// tail; this is the whole of what differs between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shell {
    Marketing,
    Docs,
}

impl Shell {
    const fn body_class(self) -> &'static str {
        match self {
            Self::Marketing => {
                "min-h-screen bg-[#f7f4ee] text-zinc-950 antialiased dark:bg-[#0b0b0d] dark:text-zinc-100"
            }
            Self::Docs => "docs-body min-h-screen antialiased",
        }
    }

    const fn is_docs(self) -> bool {
        matches!(self, Self::Docs)
    }
}

struct Document<C, M> {
    shell: Shell,
    path: &'static str,
    metadata: M,
    managed: bool,
    content: C,
}

/// Resolve the theme before first paint.
///
/// This one has to stay a blocking inline script. It must run before the
/// first paint or the page flashes the wrong theme, and an rsjs island ships
/// as `<script type="module">`, which is deferred by definition. It also
/// needs `matchMedia`, which the rsjs browser registry does not expose.
///
/// The flip side (the toggle button) is an rsjs island: see
/// [`crate::components::theme_toggle::ThemeToggle`]. Both agree on the same
/// contract, the `dark` class on `<html>`, and `color-scheme` follows from
/// that class in CSS rather than being set here as an inline style.
pub(crate) const THEME_BOOT: &str = r#"
(function() {
  try {
    var saved = localStorage.getItem('theme');
    var dark = saved === 'dark' || (!saved && window.matchMedia('(prefers-color-scheme: dark)').matches);
    document.documentElement.classList.toggle('dark', dark);
  } catch (_) {}
})();
"#;

pub(crate) const STYLESHEET_HREF: &str =
    concat!("/static/styles.css?v=", env!("MARKETING_ASSET_VERSION"));
const SOCIAL_IMAGE_ALT: &str =
    "Proa: a person and a robot in a boat with an orange sail, traveling through streams of HTML and Markdown.";
const SOCIAL_IMAGE_SRC: &str = concat!("/static/og.jpg?v=", env!("MARKETING_ASSET_VERSION"));
/// iOS fills transparent touch icons with black, so this PNG is opaque.
pub(crate) const APPLE_TOUCH_ICON_HREF: &str = concat!(
    "/static/apple-touch-icon.png?v=",
    env!("MARKETING_ASSET_VERSION")
);
pub(crate) const DOCS_SCRIPT_SRC: &str =
    concat!("/static/docs.js?v=", env!("MARKETING_ASSET_VERSION"));
/// Orama-backed search for the modal on every shell. A module script, so it
/// is deferred and its `?v=` carries through to the modules it imports.
pub(crate) const SEARCH_SCRIPT_SRC: &str =
    concat!("/static/search.mjs?v=", env!("MARKETING_ASSET_VERSION"));
const AGENT_TOOLS_SCRIPT_SRC: &str = concat!(
    "/static/agent-tools.mjs?v=",
    env!("MARKETING_ASSET_VERSION")
);

/// Emit the rsjs runtime and island module tags for every hydrated island
/// this response has rendered so far.
///
/// Island asset ids differ between build modes (standalone descriptor ids
/// vs `cargo proa` linked-owner ids), so the tags must come from the
/// response-local artifact snapshot resolved through the linked manifest's
/// page planner, never from filtering the manifest by struct name. Place
/// this after the last island in `<body>`: islands rendered later would be
/// missed by the snapshot.
pub(crate) struct RsjsRuntimeTags;

impl<L, B> WebRenderSync<L, B> for RsjsRuntimeTags
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let artifacts = cx.rsjs_artifacts_snapshot();
        let plan = rsjs::manifest()
            .page_runtime_plan(artifacts.rendered_islands())
            .expect("every rendered rsjs island must resolve in the linked manifest");
        plan.write_html(&mut cx.out, None)
    }
}

/// Shared document and social metadata for every public page shell.
pub(crate) struct DocumentMetadata {
    meta: PageMeta,
    origin: &'static str,
}

impl DocumentMetadata {
    pub(crate) fn new(meta: PageMeta) -> Self {
        Self {
            meta,
            origin: public_site_url(),
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocumentMetadata {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let origin = self.origin;
        WebRenderSync::render(
            html_sync! {
                <title>{raw_text_ok(text(self.meta.title))}</title>
                <meta name="description" content={self.meta.description} />
                {SiteMetadata { path: self.meta.path }}
                <link
                    rel="canonical"
                    href={display_attr(format_args!("{origin}{}", self.meta.path))}
                />
                <meta name="og:site_name" property="og:site_name" content={site::NAME} />
                <meta name="og:locale" property="og:locale" content="en_US" />
                <meta name="og:type" property="og:type" content="website" />
                <meta name="og:title" property="og:title" content={self.meta.title} />
                <meta
                    name="og:description"
                    property="og:description"
                    content={self.meta.description}
                />
                <meta
                    name="og:url"
                    property="og:url"
                    content={display_attr(format_args!("{origin}{}", self.meta.path))}
                />
                <meta
                    name="og:image"
                    property="og:image"
                    content={display_attr(format_args!("{origin}{SOCIAL_IMAGE_SRC}"))}
                />
                {if origin.starts_with("https://") {
                    html_sync! {
                        <meta name="og:image:secure_url" property="og:image:secure_url" content={display_attr(format_args!("{origin}{SOCIAL_IMAGE_SRC}"))} />
                    }
                }}
                <meta name="og:image:type" property="og:image:type" content="image/jpeg" />
                <meta name="og:image:width" property="og:image:width" content="1536" />
                <meta name="og:image:height" property="og:image:height" content="1024" />
                <meta name="og:image:alt" property="og:image:alt" content={SOCIAL_IMAGE_ALT} />
                <meta name="twitter:card" content="summary_large_image" />
                <meta name="twitter:site" content={site::SOCIAL_HANDLE} />
                <meta name="twitter:title" content={self.meta.title} />
                <meta name="twitter:description" content={self.meta.description} />
                <meta
                    name="twitter:image"
                    content={display_attr(format_args!("{origin}{SOCIAL_IMAGE_SRC}"))}
                />
                <meta name="twitter:image:alt" content={SOCIAL_IMAGE_ALT} />
            },
            cx,
        )
    }
}

struct SiteMetadata {
    path: &'static str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for SiteMetadata {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                <meta name="proa-route" content={self.path} />
                <link rel="describedby" type="text/plain" href={routes::AGENT_INDEX} />
                <link
                    rel="service-desc"
                    type="application/vnd.oai.openapi+json"
                    href={routes::OPENAPI}
                />
                <link rel="ard" type="application/ai-catalog+json" href={routes::ARD_CATALOG} />
                {if self.path == routes::HOME {
                        html_sync! {
                            <link rel="alternate" type="text/markdown" href={routes::HOME_MARKDOWN} />
                            <script type="application/ld+json">{raw_text_ok(structured_data::document())}</script>
                        }
                    }}
                {if self.path == routes::TEAM {
                        html_sync! {
                            <link rel="alternate" type="text/markdown" href={routes::TEAM_MARKDOWN} />
                        }
                    }}
                {if let Some(data) = structured_data::for_documentation(self.path) {
                        html_sync! {
                            <script type="application/ld+json">{raw_text_ok(data)}</script>
                        }
                    }}
                <script type="module" src={AGENT_TOOLS_SCRIPT_SRC}></script>
                {google_analytics()}
            },
            cx,
        )
    }
}

struct ManagedMetadata {
    path: &'static str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ManagedMetadata {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                {cx.head()}
                {SiteMetadata { path: self.path }}
            },
            cx,
        )
    }
}

/// The `<head>` both shells share. Only the docs enhancement script differs.
struct PageHead<M> {
    shell: Shell,
    metadata: M,
}

impl<L, B, M> WebRenderSync<L, B> for PageHead<M>
where
    B: WriteBuf,
    L: DataLoader,
    M: WebRenderSync<L, B>,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
            <meta charset="utf-8" />
            <meta name="viewport" content="width=device-width, initial-scale=1" />
            {self.metadata}
            <link rel="icon" href={crate::static_assets::image_url("/static/favicon.ico")} />
            <link rel="icon" type="image/svg+xml" href={crate::static_assets::image_url("/static/favicon.svg")} />
            <link rel="apple-touch-icon" sizes="180x180" href={APPLE_TOUCH_ICON_HREF} />
            <link rel="stylesheet" href={STYLESHEET_HREF} />
            <script type="module" src={SEARCH_SCRIPT_SRC}></script>
            {if self.shell.is_docs() {
                WebRenderSync::render(
                    html_sync! {
                        <script src={DOCS_SCRIPT_SRC} defer></script>
                    },
                    cx,
                )?;
            }}
            // Must stay a blocking inline script in `<head>`: it decides the
            // theme before first paint, so a deferred module (an rsjs island)
            // would flash the wrong theme.
            <script>{StaticRaw(THEME_BOOT)}</script>
            },
            cx,
        )
    }
}

/// Everything inside `<body>` above the shared tail. The marketing shell wraps
/// the content in the landing chrome; the docs shell hands `<body>` straight to
/// the content, which owns its own layout.
struct PageBody<C> {
    shell: Shell,
    path: &'static str,
    content: C,
}

impl<L, B, C> WebRenderSync<L, B> for PageBody<C>
where
    B: WriteBuf,
    L: DataLoader,
    C: WebRenderSync<L, B>,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if self.shell.is_docs() {
            return self.content.render(cx);
        }

        WebRenderSync::render(
            html_sync! {
                <div class="landing-shell min-h-screen">
                    {LandingHeader {
                        active: MarketingNavActive::for_path(self.path),
                    }}
                    <main id="main-content">{self.content}</main>
                    {LandingFooter}
                </div>
            },
            cx,
        )
    }
}

impl<L, B, C, M> WebRender<L, B> for Document<C, M>
where
    B: WriteBuf,
    L: DataLoader,
    C: WebRenderSync<L, B> + Send + 'static,
    M: WebRenderSync<L, B>,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            html! {
                <!DOCTYPE html>
                <html lang="en">
                    <head>
                        {proa_core::to_async(PageHead {
                            shell: self.shell,
                            metadata: self.metadata,
                        })}
                    </head>
                    <body class={self.shell.body_class()}>
                        {proa_core::to_async(PageBody {
                            shell: self.shell,
                            path: self.path,
                            content: self.content,
                        })}
                        {SearchModal}
                        // Must stay last in `<body>`: it snapshots the islands
                        // rendered so far, so anything after it ships no JS.
                        {if !self.managed {
                            proa_core::to_async(RsjsRuntimeTags)
                        }}
                    </body>
                </html>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_shells_load_the_search_islands_runtime_and_modules() {
        let mut cx = WebContext::new();
        futures::executor::block_on(SearchModal.render(&mut cx).resolve()).unwrap();
        let search_assets: Vec<_> = cx.rsjs_artifacts_snapshot().rendered_islands().collect();
        assert!(
            !search_assets.is_empty(),
            "search must register client work"
        );

        let meta = PageMeta {
            title: "Test",
            description: "Test",
            path: "/",
        };
        for html in [
            render_page(meta, text("body")).0,
            render_docs_page(meta, text("body")).0,
        ] {
            assert!(html.contains(r#"<script type="module" src="/rsjs-runtime.js""#));
            assert!(html.contains(&format!(
                r#"<script type="module" src="{SEARCH_SCRIPT_SRC}""#
            )));
            for asset in &search_assets {
                assert!(html.contains(&format!(
                    r#"<script type="module" src="/rsjs/island/{asset}.js""#
                )));
            }
        }
    }
}
