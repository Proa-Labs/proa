// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::{
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
};
use proa_core::{DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{content_part, html_sync, md_sync};

use crate::layout::{render_page, PageMeta};

fn about_content() -> (PageMeta, InfoPage) {
    (PageMeta {
        title: "About Proa: Rust web framework by Proa Labs",
        description: "What Proa builds, who it is for, and where to find its source, documentation, and community.",
        path: "/about",
    }, InfoPage {
        title: "About Proa",
        paragraphs: &[
            "Proa is a Rust web framework published by Proa Labs, Inc. It is built for developers creating server-rendered websites, documentation, and interactive applications. Proa compiles Rust-authored templates into optimized output and supports composable HTML and Markdown rendering.",
            "The framework combines typed components, request-scoped data loading, streaming server rendering, routing, and response metadata. RSJS islands let developers author browser state and event handlers in Rust, with the build producing the JavaScript used for interactivity. Applications can adopt Proa one route at a time alongside an existing stack.",
            "This website publishes the framework documentation, installation instructions, examples, and a playground. Its public search API and MCP server help coding agents find and read the same documentation available to people. These tools are read-only and do not require an account.",
            "Proa Labs operates from the United States. Source code, release downloads, contribution information, and licensing terms are maintained in the official Proa repository. Check the repository's licensing documents for the terms that apply to a particular package or release.",
        ],
        links: &[
            ("/docs", "Read the Proa quickstart"),
            ("https://github.com/proa-labs/proa", "Official Proa source repository"),
            ("/docs/api", "API and MCP documentation"),
            ("/contact", "Contact and community"),
        ],
    })
}

fn contact_content() -> (PageMeta, InfoPage) {
    (PageMeta {
        title: "Contact Proa: Support, community, and licensing",
        description: "Find Proa's community, report a reproducible issue, and contact Proa Labs about privacy or licensing.",
        path: "/contact",
    }, InfoPage {
        title: "Contact Proa",
        paragraphs: &[
            "For help building with Proa, start with the quickstart and troubleshooting documentation. The guides cover component rendering, RSJS islands, application routing, deployment, and compiler diagnostics. Search the documentation using the website search or the public documentation API to find the relevant reference.",
            "For reproducible bugs or documentation issues, use the issue tracker in the official proa-labs/proa GitHub repository. Include the Proa version, operating system, relevant compiler output, and a minimal example when possible. Remove secrets, credentials, and private application data before posting a public issue.",
            "The Proa community is also available through the Discord link below. Use community channels for questions and discussion; they are public or shared spaces, so avoid including confidential project information. The repository is the source of truth for code, releases, and contribution guidance.",
            "For privacy questions or requests about this website, email privacy@proa.so and consult the published privacy policy. For licensing questions and questions about the website terms, email legal@proa.so. These contacts are operated by Proa Labs, Inc., the publisher of Proa.",
        ],
        links: &[
            ("/docs/guides/troubleshooting", "Troubleshooting guide"),
            ("https://github.com/proa-labs/proa/issues", "Proa issue tracker"),
            ("https://proa.so/discord", "Proa Discord community"),
            ("mailto:privacy@proa.so", "privacy@proa.so"),
            ("mailto:legal@proa.so", "legal@proa.so"),
            ("/privacy", "Privacy policy"),
            ("/terms", "Website terms"),
        ],
    })
}

struct InfoPage {
    title: &'static str,
    paragraphs: &'static [&'static str],
    links: &'static [(&'static str, &'static str)],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for InfoPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <article data-info-page class="mx-auto max-w-3xl px-6 py-16">
                <h1 class="text-4xl font-semibold tracking-tight">{self.title}</h1>
                <div class="mt-8 flex flex-col gap-6 text-base leading-7 text-zinc-600 dark:text-zinc-300">
                    {for paragraph in self.paragraphs {
                        html_sync! { <p>{content_part!("paragraph", *paragraph)}</p> }
                    }}
                </div>
                <ul class="mt-8 space-y-3">
                    {for (href, label) in self.links {
                        html_sync! { <li><a href={content_part!("link-url", *href)} class="underline underline-offset-4">{content_part!("link-label", *label)}</a></li> }
                    }}
                </ul>
            </article>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for InfoPage {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        use crate::utils::document::Url;
        md_sync! { r###"
            # {self.title}

            {for paragraph in self.paragraphs {
            {content_part!("paragraph", *paragraph)}

            }}{for (href, label) in self.links {
            - [{content_part!("link-label", *label)}]({content_part!("link-url", Url(href))})
            }}"###
        }
        .render_md(cx)
    }
}

fn page_response(meta: PageMeta, page: InfoPage, markdown: bool) -> Response {
    let mut response = if markdown {
        (
            [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
            crate::utils::document::render_markdown(page),
        )
            .into_response()
    } else {
        render_page(meta, page).into_response()
    };
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    response
}

pub async fn about(headers: HeaderMap) -> Response {
    let (meta, page) = about_content();
    page_response(meta, page, crate::utils::http::accepts_markdown(&headers))
}

pub async fn contact(headers: HeaderMap) -> Response {
    let (meta, page) = contact_content();
    page_response(meta, page, crate::utils::http::accepts_markdown(&headers))
}

pub async fn about_markdown() -> Response {
    let (meta, page) = about_content();
    page_response(meta, page, true)
}

pub async fn contact_markdown() -> Response {
    let (meta, page) = contact_content();
    page_response(meta, page, true)
}
