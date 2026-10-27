// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, MdRenderSync, WebContext, WriteBuf, WriteError};
use proa_macros::md_sync;

use crate::utils::document::Url;

pub struct AgentIndex<'a> {
    pub origin: &'a str,
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for AgentIndex<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let origin = self.origin;
        md_sync! { r###"
            # Proa

            > Proa is a Rust web framework for developers building server-rendered websites, documentation, and interactive applications. Rust templates compile into optimized HTML; RSJS compiles browser behavior into small JavaScript islands.

            ## When to use Proa

            Use Proa when building a Rust website, composing typed HTML or Markdown, adding streaming server rendering, or moving a performance-sensitive route from an existing application. Use the documentation tools below to find the current APIs before writing code. Proa is a framework you run in your own application; this website's API searches and reads documentation, and does not host or deploy your applications.

            ## Start here

            - [Proa quickstart]({Url(origin)}/docs/index.md): Install the official CLI, scaffold a site, and run it locally.
            - [Proa documentation index]({Url(origin)}/docs/llms.txt): A focused index of every public documentation page.
            - [Proa agent setup]({Url(origin)}/docs/agents.md): Repository instructions and context selection for coding agents.
            - [Proa CLI]({Url(origin)}/docs/cli.md): Site generation, build, formatting, linting, and diagnostics.
            - [Proa source repository](https://github.com/proa-labs/proa): Source code, issues, release downloads, and licensing.

            ## Programmatic access

            - [Proa API and MCP documentation]({Url(origin)}/docs/api.md): Authentication, tool descriptions, pagination, errors, retries, and examples.
            - [OpenAPI specification]({Url(origin)}/openapi.json): Typed contracts for the public documentation API.
            - [MCP server card]({Url(origin)}/mcp/server-card): Connect to {Url(origin)}/mcp using Streamable HTTP. Search with search_docs, then read a returned page with get_doc. Documentation is also available through resources/list and resources/read.
            - [Authentication]({Url(origin)}/auth.md): Public, read-only access; no account, API key, or access token is required.
            - [Agent resource catalog]({Url(origin)}/.well-known/ard.json): Discovery links for APIs, MCP, and skills.
            - [Agent skills]({Url(origin)}/.well-known/agent-skills/index.json): Component, island, and performance-review workflows for a Proa source checkout. The instructions reference files and scripts in the source repository.

            ## Optional

            - [Full documentation corpus]({Url(origin)}/llms-full.txt): All public documentation as one large text response. Prefer individual pages for focused work.
            - [About Proa]({Url(origin)}/about): Project purpose and publisher.
            - [Contact Proa]({Url(origin)}/contact): Community, issue reporting, privacy, and licensing contacts.
            - [Privacy]({Url(origin)}/privacy): Website privacy policy.
            - [Sitemap]({Url(origin)}/sitemap.xml): Public HTML pages.
            "###
        }.render_md(cx)
    }
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
    fn preview_origin_reaches_links_and_fenced_request_examples() {
        let origin = "https://preview.example.test:8443";
        let index = render_markdown(AgentIndex { origin });
        assert!(index.contains("](https://preview.example.test:8443/openapi.json)"));
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
}
