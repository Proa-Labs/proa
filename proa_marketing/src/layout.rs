// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::response::Html;
use proa_core::shared::attr_value::display_attr;
use proa_core::text::text;
use proa_core::{
    raw_text_ok, render_in_context_async, DataLoader, NoLoader, RenderBuffer, RenderOutcome,
    StaticRaw, WebContext, WebRender, WebRenderSync, WriteBuf, WriteError,
};
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
        meta,
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
        meta,
        content,
    })
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

struct Document<C> {
    shell: Shell,
    meta: PageMeta,
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
                <meta name="proa-route" content={self.meta.path} />
                <link
                    rel="canonical"
                    href={display_attr(format_args!("{origin}{}", self.meta.path))}
                />
                <meta name="og:site_name" property="og:site_name" content="Proa" />
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
                <meta name="twitter:site" content="@drewhariri" />
                <meta name="twitter:title" content={self.meta.title} />
                <meta name="twitter:description" content={self.meta.description} />
                <meta
                    name="twitter:image"
                    content={display_attr(format_args!("{origin}{SOCIAL_IMAGE_SRC}"))}
                />
                <meta name="twitter:image:alt" content={SOCIAL_IMAGE_ALT} />
                {google_analytics()}
            },
            cx,
        )
    }
}

/// The `<head>` both shells share. Only the docs enhancement script differs.
struct PageHead {
    shell: Shell,
    meta: PageMeta,
}

impl<L, B> WebRenderSync<L, B> for PageHead
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
            <meta charset="utf-8" />
            <meta name="viewport" content="width=device-width, initial-scale=1" />
            {DocumentMetadata::new(self.meta)}
            <link rel="icon" href="/static/favicon.ico" />
            <link rel="icon" type="image/svg+xml" href="/static/favicon.svg" />
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
    meta: PageMeta,
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
                        active: MarketingNavActive::for_path(self.meta.path),
                    }}
                    <main id="main-content">{self.content}</main>
                    {LandingFooter}
                </div>
            },
            cx,
        )
    }
}

impl<L, B, C> WebRender<L, B> for Document<C>
where
    B: WriteBuf,
    L: DataLoader,
    C: WebRenderSync<L, B> + Send + 'static,
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
                            meta: self.meta,
                        })}
                    </head>
                    <body class={self.shell.body_class()}>
                        {proa_core::to_async(PageBody {
                            shell: self.shell,
                            meta: self.meta,
                            content: self.content,
                        })}
                        {SearchModal}
                        // Must stay last in `<body>`: it snapshots the islands
                        // rendered so far, so anything after it ships no JS.
                        {proa_core::to_async(RsjsRuntimeTags)}
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

    const META: PageMeta = PageMeta {
        title: "Test",
        description: "Test page",
        path: "/",
    };

    #[test]
    fn document_metadata_emits_absolute_urls_and_escapes_page_values() {
        for (origin, path) in [
            ("https://proa.so", "/"),
            ("https://pr-123.example.test", "/docs/core/markdown"),
            ("http://localhost:5555", "/ecosystem/apps"),
        ] {
            let mut cx = WebContext::new();
            DocumentMetadata {
                origin,
                meta: PageMeta {
                    title: "Docs & <examples>",
                    description: "A \"quoted\" & <description>",
                    path,
                },
            }
            .render(&mut cx)
            .unwrap();
            let html = cx.into_output().into_string();
            let canonical = format!("{origin}{path}");
            let image = format!("{origin}{SOCIAL_IMAGE_SRC}");

            for tag in [
                format!(r#"<link rel="canonical" href="{canonical}""#),
                format!(r#"<meta name="og:url" property="og:url" content="{canonical}""#),
                format!(r#"<meta name="og:image" property="og:image" content="{image}""#),
                format!(r#"<meta name="twitter:image" content="{image}""#),
            ] {
                assert_eq!(html.matches(&tag).count(), 1, "{tag}");
            }
            assert_eq!(
                html.contains(r#"property="og:image:secure_url""#),
                origin.starts_with("https://")
            );
            assert!(html.contains("<title>Docs &amp; &lt;examples&gt;</title>"));
            assert!(html.contains(r#"property="og:title" content="Docs &amp; &lt;examples&gt;""#));
            assert!(html.contains(r#"name="twitter:description" content="A &quot;quoted&quot; &amp; &lt;description&gt;""#));
            assert!(!html.contains("<examples>"));
            assert!(!html.contains("<description>"));
        }
    }

    #[test]
    fn public_page_shells_share_social_image_metadata() {
        use futures::FutureExt;

        let jpeg = include_bytes!("../public/og.jpg");
        assert!(jpeg.len() <= 500_000, "social image must stay below 500 KB");
        assert_eq!(&jpeg[..2], b"\xff\xd8");
        // Read the JPEG frame dimensions so metadata follows the actual asset.
        let mut offset = 2;
        let (width, height) = loop {
            assert_eq!(jpeg[offset], 0xff);
            let marker = jpeg[offset + 1];
            assert_ne!(marker, 0xda, "frame dimensions must precede image data");
            if matches!(marker, 0xc0 | 0xc2) {
                let height = u16::from_be_bytes(jpeg[offset + 5..offset + 7].try_into().unwrap());
                let width = u16::from_be_bytes(jpeg[offset + 7..offset + 9].try_into().unwrap());
                break (width, height);
            }
            let length = u16::from_be_bytes(jpeg[offset + 2..offset + 4].try_into().unwrap());
            offset += 2 + usize::from(length);
        };
        let origin = public_site_url();
        for (path, html) in [
            ("/", render_page(META, proa_core::text::text("body")).0),
            (
                "/docs/core/markdown",
                render_docs_page(
                    PageMeta {
                        path: "/docs/core/markdown",
                        ..META
                    },
                    proa_core::text::text("body"),
                )
                .0,
            ),
            (
                "/ecosystem",
                crate::pages::ecosystem::handler()
                    .now_or_never()
                    .expect("ecosystem page renders without I/O")
                    .0,
            ),
        ] {
            let head = html.split_once("</head>").unwrap().0;
            for tag in [
                r#"property="og:site_name" content="Proa""#.to_owned(),
                r#"property="og:image:type" content="image/jpeg""#.to_owned(),
                format!(r#"property="og:url" content="{origin}{path}""#),
                format!(r#"rel="canonical" href="{origin}{path}""#),
                format!(r#"property="og:image" content="{origin}{SOCIAL_IMAGE_SRC}""#),
                format!(r#"name="twitter:image" content="{origin}{SOCIAL_IMAGE_SRC}""#),
                format!(r#"property="og:image:width" content="{width}""#),
                format!(r#"property="og:image:height" content="{height}""#),
                format!(r#"property="og:image:alt" content="{SOCIAL_IMAGE_ALT}""#),
                format!(r#"name="twitter:image:alt" content="{SOCIAL_IMAGE_ALT}""#),
            ] {
                assert_eq!(head.matches(&tag).count(), 1, "{path}: {tag}");
            }
        }
    }

    /// Regression test for the dead cmd+K search: the page shells must load
    /// the rsjs runtime and the search island module. The old implementation
    /// filtered the manifest by struct name, which matched nothing under the
    /// linked-owner asset ids `cargo proa` produces, so no scripts shipped
    /// and the modal never hydrated.
    #[test]
    fn page_shells_emit_rsjs_runtime_tags_for_the_search_modal() {
        for (shell, html) in [
            (
                "render_page",
                render_page(META, proa_core::text::text("body")).0,
            ),
            (
                "render_docs_page",
                render_docs_page(META, proa_core::text::text("body")).0,
            ),
        ] {
            assert!(
                html.contains(r#"<script type="module" src="/rsjs-runtime.js""#),
                "{shell} must emit the rsjs runtime script tag"
            );
            assert!(
                html.contains("/rsjs/island/"),
                "{shell} must emit the search island module script tag"
            );
            assert!(
                html.contains(r#"<script type="module" src="/static/search.mjs?v="#),
                "{shell} must load the Orama search module for the modal"
            );
        }
    }
}
