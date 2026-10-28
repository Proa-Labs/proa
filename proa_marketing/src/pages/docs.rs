// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::utils::document::Url;
use axum::{
    extract::Path,
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use proa_core::{DataLoader, MdRenderSync, WebContext, WriteBuf, WriteError};
use proa_docs::Docs;
use proa_macros::md_sync;

use super::collection::response_for_collection_path;
use crate::utils::routes::collection_route_path;

static DOCS: Docs = proa_docs::include_docs!("docs");

pub(crate) fn docs() -> &'static Docs {
    &DOCS
}

pub async fn handler(headers: HeaderMap) -> Response {
    response_for_path("/docs", &headers)
}

pub async fn path_handler(headers: HeaderMap, Path(path): Path<String>) -> Response {
    let path = collection_route_path("/docs", &path);
    response_for_path(&path, &headers)
}

fn response_for_path(path: &str, headers: &HeaderMap) -> Response {
    if let Some(destination) = proa_ui_redirect_url(path) {
        return Redirect::permanent(&destination).into_response();
    }

    response_for_collection_path(&DOCS, path, "Docs", headers)
}

fn proa_ui_redirect_url(path: &str) -> Option<String> {
    if path == "/docs/ui" {
        return Some("https://ui.proa.so/docs".to_string());
    }

    path.strip_prefix("/docs/ui/")
        .map(|suffix| format!("https://ui.proa.so/docs/{suffix}"))
}

pub struct Authentication<'a> {
    pub origin: &'a str,
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Authentication<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let origin = self.origin;
        md_sync! { r###"
            # Proa public documentation authentication

            ## Discover

            The documentation API on {Url(origin)} and the Streamable HTTP MCP endpoint at {Url(origin)}/mcp are public and read-only. Their specification is at [openapi.json]({Url(origin)}/openapi.json), and usage examples are in the [API guide]({Url(origin)}/docs/api.md).

            ## Choose a method

            Use ordinary HTTPS without an Authorization header. No registration, API key, OAuth grant, identity assertion, access token, or cookie is needed. These interfaces only read published documentation. They do not access accounts, change projects, deploy applications, or charge money.

            ## Make a request

            {RequestExamples { origin }}

            For MCP, configure a Streamable HTTP connection to {Url(origin)}/mcp without credentials. Initialize the connection, list tools, and call search_docs or get_doc. Browser clients must originate from this deployment; non-browser clients may omit the Origin header.

            ## Errors and retries

            The REST API returns application/problem+json errors with status, code, detail, and a link to the API guide. A 400 means the query, cursor, limit, or path is invalid; a 404 means the requested public document or route is absent. A 429 includes Retry-After in seconds. Respect the advertised shared per-instance request budget and retry after that delay. GET requests and the documentation tools are read-only and safe to retry.

            ## Revocation

            There are no public API credentials to revoke. Remove the MCP connection from your client to stop using it. Authentication for applications you build with Proa is controlled by those applications, not by this documentation website. Never send application secrets or personal data as documentation search queries.
            "###
        }.render_md(cx)
    }
}

struct RequestExamples<'a> {
    origin: &'a str,
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for RequestExamples<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        // Token composition deliberately interpolates URLs inside a code fence.
        // Raw Markdown templates keep expressions inside fences literal.
        md_sync! {
            "```bash\n"
            "curl '" {Url(self.origin)} "/api/v1/search?q=streaming&limit=5'\n"
            "curl '" {Url(self.origin)} "/api/v1/docs/page?path=/docs/core/streaming-ssr'\n"
            "curl -H 'Accept: text/markdown' '" {Url(self.origin)} "/'\n"
            "```"
        }
        .render_md(cx)
    }
}

pub struct DocumentationIndex<'a> {
    pub origin: &'a str,
    pub pages: &'a [proa_docs::DocPage],
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for DocumentationIndex<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            # Proa documentation

            > Public documentation for the Proa Rust web framework. Fetch the linked Markdown for focused context.

            ## Pages

            {for page in self.pages.iter().filter(|page| !page.frontmatter.hidden) {
            - [{page.title}]({Url(self.origin)}{Url(page.markdown_path)}): {page.description.unwrap_or("Proa documentation")}
            }}"###
        }.render_md(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::document::render_markdown;

    #[test]
    fn preview_origin_reaches_auth_links_and_fenced_request_examples() {
        let origin = "https://preview.example.test:8443";
        let auth = render_markdown(Authentication { origin });
        let examples = auth
            .split_once("```bash\n")
            .unwrap()
            .1
            .split_once("```")
            .unwrap()
            .0;
        assert_eq!(examples.matches(origin).count(), 3);
        assert!(!examples.contains("{Url("));
        assert!(auth.contains("](https://preview.example.test:8443/docs/api.md)"));
    }

    #[test]
    fn documentation_index_escapes_runtime_metadata_and_destinations() {
        let mut page = crate::pages::docs::docs()
            .pages
            .iter()
            .find(|page| !page.frontmatter.hidden)
            .copied()
            .unwrap();
        page.title = "[Title]";
        page.description = Some("*Description*");
        page.markdown_path = "/docs/a (b).md";
        let output = render_markdown(DocumentationIndex {
            origin: "https://preview.example.test",
            pages: &[page],
        });
        assert!(output.contains(r"[\[Title\]]"));
        assert!(output.contains(r"\*Description\*"));
        assert!(output.contains(r"](https://preview.example.test/docs/a%20\(b\).md)"));
        assert_eq!(output.matches("\n- [").count(), 1);
    }

    #[test]
    fn every_page_advertises_its_markdown_representation() {
        // Style 11.2: an agent that fetched the HTML should learn the
        // readable source exists without guessing the URL.
        for page in docs().pages {
            assert!(
                page.markdown_path.ends_with(".md"),
                "{} has markdown_path {}, which is not a .md URL",
                page.slug,
                page.markdown_path
            );
        }
    }

    #[test]
    fn components_overview_is_not_published() {
        assert!(docs().find_page("proa-docs/components").is_none());
    }

    #[test]
    fn tutorial_code_fences_are_highlighted_at_build_time() {
        let page = docs().find_page("guides/tutorial").expect("tutorial guide");
        assert!(page.article_html.contains("data-lang=\"rust\""));
        assert!(page.article_html.contains("class=\"language-rust\""));
        assert!(page.article_html.contains("class=\"hl-keyword\""));
        assert!(page.article_html.contains("class=\"hl-string\""));
        assert!(page.markdown.contains("```rust\n"));
        assert!(!page.markdown.contains("hl-keyword"));
    }

    #[test]
    fn filename_code_fences_render_their_header_at_build_time() {
        let page = docs().find_page("guides/testing").expect("testing guide");

        assert!(page
            .markdown
            .contains("```rust filename=\"src/components/badge/badge.rs\"\n"));
        assert!(page
            .article_html
            .contains("data-filename=\"src/components/badge/badge.rs\""));
        assert!(page
            .article_html
            .contains("data-code-filename>src/components/badge/badge.rs</span>"));
        assert!(page.article_html.contains("data-code-header"));
        assert!(page.article_html.contains("data-code-icon=\"rust\""));
        assert!(page.article_html.contains("data-code-copy"));
    }

    /// An unrecognised or malformed `<Demo>` is dropped silently by the docs
    /// pipeline, which would leave the tutorial with a paragraph pointing at a
    /// demo that is not there. Pin the frame and the route it targets.
    #[test]
    fn the_tutorial_embeds_its_live_demo() {
        let page = docs().find_page("guides/tutorial").expect("tutorial guide");
        assert!(page
            .article_html
            .contains(r#"<iframe class="proa-docs-demo-frame" src="/demos/quantity-stepper""#));
        assert!(page.article_html.contains(r#"title="Quantity stepper""#));
    }

    /// The tutorial's listings live in `<CodeExplorer>` blocks. If a tag stops
    /// being recognised the pipeline drops it silently, and the page degrades
    /// into every file's code stacked end to end with no tabs. Pin the tab
    /// strip, the panels, and the fact that highlighted code still reaches
    /// them.
    #[test]
    fn the_tutorial_groups_its_listings_into_file_explorers() {
        let page = docs().find_page("guides/tutorial").expect("tutorial guide");
        let html = &page.article_html;

        assert!(
            html.contains(r#"class="proa-docs-tabs proa-docs-explorer""#),
            "{html}"
        );
        assert!(html.contains(
            r#"class="proa-docs-explorer-path" aria-label="File: src/data/products.rs""#
        ));
        assert!(html.contains(r#"class="proa-docs-tabs-tab proa-docs-explorer-file""#));
        assert!(html.contains(r#"data-code-icon="rust""#));
        assert!(html.contains(">products.rs</span></label>"));
        assert!(html.contains(
            ">src</span><span class=\"proa-docs-explorer-path-separator\" aria-hidden=\"true\">&gt;</span><span class=\"proa-docs-explorer-path-segment\">data</span><span class=\"proa-docs-explorer-path-separator\" aria-hidden=\"true\">&gt;</span><span class=\"proa-docs-explorer-path-segment\">products.rs</span>"
        ));
        assert!(
            !html.contains(">src/data/products.rs</span>"),
            "the shared directory belongs in the breadcrumb, not on every tab"
        );

        // The radios have to precede the tab strip and the panels, or the
        // stylesheet's sibling combinators never switch anything.
        let radio = html
            .find(r#"class="proa-docs-tabs-radio""#)
            .expect("explorer radios");
        let list = html
            .find(r#"class="proa-docs-tabs-list""#)
            .expect("explorer tabs");
        assert!(
            radio < list,
            "radios must be spliced in ahead of the tab strip"
        );

        // Children flow through the normal pipeline, so they keep highlighting.
        let panel = html
            .split(r#"aria-label="src/data/products.rs">"#)
            .nth(1)
            .expect("products.rs panel");
        assert!(panel.contains("hl-keyword"), "panel lost its highlighting");
    }

    #[test]
    fn selected_code_lines_are_compiled_into_stable_html_hooks() {
        let page = docs()
            .find_page("proa-docs/_retired/markdown-pipeline")
            .expect("markdown pipeline guide");
        assert!(page.markdown.contains("```rust {2,4-5}\n"));
        assert!(page.article_html.contains("data-code-lines"));
        assert!(page
            .article_html
            .contains("data-code-line=\"2\" data-highlighted-line"));
        assert!(page
            .article_html
            .contains("data-code-line=\"4\" data-highlighted-line"));
        assert!(page
            .article_html
            .contains("data-code-line=\"5\" data-highlighted-line"));
    }
}
