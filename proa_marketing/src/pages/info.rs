// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::{routes, site};

use axum::{
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
};
use proa_core::{DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{content_part, html_sync, md_sync};

use crate::layout::{render_page, PageMeta};

fn contact_content() -> (PageMeta, InfoPage) {
    (PageMeta {
        title: "Contact Proa: Support, community, and licensing",
        description: const_format::concatcp!("Find Proa's community, report a reproducible issue, and contact ", site::ORGANIZATION_NAME, " about privacy or licensing."),
        path: routes::CONTACT,
    }, InfoPage {
        title: "Contact Proa",
        paragraphs: &[
            "For help building with Proa, start with the quickstart and troubleshooting documentation. The guides cover component rendering, RSJS islands, application routing, deployment, and compiler diagnostics. Search the documentation using the website search or the public documentation API to find the relevant reference.",
            const_format::concatcp!("For reproducible bugs or documentation issues, use the issue tracker in the official ", site::GITHUB_REPOSITORY_SLUG, " GitHub repository. Include the Proa version, operating system, relevant compiler output, and a minimal example when possible. Remove secrets, credentials, and private application data before posting a public issue."),
            "The Proa community is also available through the Discord link below. Use community channels for questions and discussion; they are public or shared spaces, so avoid including confidential project information. The repository is the source of truth for code, releases, and contribution guidance.",
            const_format::concatcp!("For privacy questions or requests about this website, email ", site::PRIVACY_EMAIL, " and consult the published privacy policy. For licensing questions and questions about the website terms, email ", site::LEGAL_EMAIL, ". These contacts are operated by ", site::ORGANIZATION, ", a maintainer of Proa."),
        ],
        links: &[
            ("/docs/guides/troubleshooting", "Troubleshooting guide"),
            (site::ISSUES_URL, "Proa issue tracker"),
            (site::DISCORD_URL, "Proa Discord community"),
            (site::PRIVACY_MAILTO, site::PRIVACY_EMAIL),
            (site::LEGAL_MAILTO, site::LEGAL_EMAIL),
            (routes::PRIVACY, "Privacy policy"),
            (routes::TERMS, "Website terms"),
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
                    {for &paragraph in self.paragraphs {
                        html_sync! { <p>{paragraph}</p> }
                    }}
                </div>
                <ul class="mt-8 space-y-3">
                    {for &(href, label) in self.links {
                        html_sync! { <li><a href={content_part!("link-url", href)} class="underline underline-offset-4">{label}</a></li> }
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

            {for &paragraph in self.paragraphs {
                md_sync! { "{paragraph}\n\n" }
            }}{for &(href, label) in self.links {
                let href = content_part!("link-url", Url(href));
                md_sync! { "- [{label}]({href})\n" }
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

pub async fn contact(headers: HeaderMap) -> Response {
    let (meta, page) = contact_content();
    page_response(meta, page, crate::utils::http::accepts_markdown(&headers))
}

pub async fn contact_markdown() -> Response {
    let (meta, page) = contact_content();
    page_response(meta, page, true)
}
