// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::{routes, site};

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
    response_for_path(routes::DOCS, &headers)
}

pub async fn path_handler(headers: HeaderMap, Path(path): Path<String>) -> Response {
    let path = collection_route_path(routes::DOCS, &path);
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
        return Some(site::UI_DOCS_URL.to_string());
    }

    path.strip_prefix("/docs/ui/")
        .map(|suffix| format!("{}/{suffix}", site::UI_DOCS_URL))
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

            The documentation API on {Url(origin)} and the Streamable HTTP MCP endpoint at {Url(origin)}{Url(routes::MCP)} are public and read-only. Their specification is at [openapi.json]({Url(origin)}{Url(routes::OPENAPI)}), and usage examples are in the [API guide]({Url(origin)}{Url(routes::API_GUIDE_MARKDOWN)}).

            ## Choose a method

            Use ordinary HTTPS without an Authorization header. No registration, API key, OAuth grant, identity assertion, access token, or cookie is needed. These interfaces only read published documentation. They do not access accounts, change projects, deploy applications, or charge money.

            ## Make a request

            {RequestExamples { origin }}

            For MCP, configure a Streamable HTTP connection to {Url(origin)}{Url(routes::MCP)} without credentials. Initialize the connection, list tools, and call search_docs or get_doc. Browser clients must originate from this deployment; non-browser clients may omit the Origin header.

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
        // A template keeps slots inside a code fence literal, so the fence
        // delimiters and each command line are separate inline templates and
        // the URLs interpolate.
        (
            md_sync! { "```bash\n" },
            md_sync! { "curl '{Url(self.origin)}{Url(routes::API_SEARCH)}?q=streaming&limit=5'\n" },
            md_sync! { "curl '{Url(self.origin)}{Url(routes::API_PAGE)}?path=/docs/core/streaming-ssr'\n" },
            md_sync! { "curl -H 'Accept: text/markdown' '{Url(self.origin)}/'\n" },
            md_sync! { "```" },
        )
            .render_md(cx)
    }
}

pub struct DocumentationIndex<'a> {
    pub origin: &'a str,
    pub pages: &'a [proa_docs::DocPage],
    pub section: Option<(&'a str, &'a str)>,
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for DocumentationIndex<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            # Proa {self.section.map(|(title, _)| (title, " "))}documentation

            > Public documentation for the Proa Rust web framework. Fetch the linked Markdown for focused context.

            ## Pages

            {for page in self.pages.iter().filter(|page| {
                !page.frontmatter.hidden && self.section.is_none_or(|(_, prefix)| {
                    page.route_path == prefix || page.route_path.strip_prefix(prefix).is_some_and(|tail| tail.starts_with('/'))
                })
            }) {
                let description = page.description.unwrap_or("Proa documentation");
                md_sync! { "- [{page.title}]({Url(self.origin)}{Url(page.markdown_path)}): {description}\n" }
            }}"###
        }.render_md(cx)
    }
}
