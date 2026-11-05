// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::OnceLock;

use axum::{
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
};
use proa_core::{DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{content_part, html_sync, md_sync, omit};

use crate::{
    data::{routes, site},
    layout::{render_page, PageMeta},
    utils::document::{render_markdown, Url},
};

pub(crate) const META: PageMeta = PageMeta {
    title: const_format::concatcp!("Team: ", site::ORGANIZATION_NAME),
    description: const_format::concatcp!(
        "Meet the founder behind ",
        site::ORGANIZATION_NAME,
        " and the Proa Rust web framework."
    ),
    path: routes::TEAM,
};

const TEAM: TeamPage = TeamPage {
    title: "Meet the team",
    intro: const_format::concatcp!(
        "The people behind Proa. ",
        site::ORGANIZATION,
        " builds the Proa Rust web framework."
    ),
    founder: TeamMember {
        name: "Andrew Hariri",
        role: "Founder",
        organization: site::ORGANIZATION,
        portrait: "/static/images/team/andrew-hariri.jpg",
        links: &[
            ("Website", "https://hariri.dev/"),
            ("GitHub", "https://github.com/hariria"),
            ("X", site::SOCIAL_URL),
        ],
    },
};

#[derive(Clone, Copy)]
struct TeamPage {
    title: &'static str,
    intro: &'static str,
    founder: TeamMember,
}

#[derive(Clone, Copy)]
pub(crate) struct TeamMember {
    pub(crate) name: &'static str,
    pub(crate) role: &'static str,
    pub(crate) organization: &'static str,
    pub(crate) portrait: &'static str,
    pub(crate) links: &'static [(&'static str, &'static str)],
}

pub(crate) fn founder() -> TeamMember {
    TEAM.founder
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TeamPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <article
                data-team-page
                class="mx-auto max-w-[1400px] px-4 py-16 sm:px-6 sm:py-24 lg:px-8"
            >
                <h1 class="text-4xl font-semibold tracking-tight sm:text-5xl">{self.title}</h1>
                <p class="mt-6 max-w-2xl text-lg leading-8 text-zinc-600 dark:text-zinc-300">
                    {self.intro}
                </p>
                {self.founder}
            </article>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TeamMember {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div data-team-member class="mt-8 grid gap-8 sm:grid-cols-[200px_1fr] sm:gap-12">
                <img
                    src={content_part!("portrait", self.portrait)}
                    alt={content_part!("name", self.name)}
                    width="460"
                    height="460"
                    class="aspect-square w-48 rounded-2xl object-cover sm:w-full"
                />
                <div>
                    <h2 class="text-2xl font-semibold tracking-tight">
                        {content_part!("name", self.name)}
                    </h2>
                    <p class="mt-2 text-base font-medium">{self.role}", "{self.organization}</p>
                    <ul class="mt-6 flex flex-wrap gap-6 text-sm font-medium">
                        {for (label, href) in self.links {
                            html_sync! {
                                <li>
                                    <a href={content_part!("profile-url", *href)} rel="me" class="inline-flex items-center gap-2 underline decoration-black/20 underline-offset-4 transition-colors hover:text-brand dark:decoration-white/30">
                                        {content_part!("profile-icon", html_sync! {
                                            <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false" class="size-4 shrink-0">
                                                {match *label {
                                                    "GitHub" => html_sync! {
                                                        <path fill="currentColor" d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
                                                    },
                                                    "X" => html_sync! {
                                                        <path fill="currentColor" d="M14.234 10.162 22.977 0h-2.072l-7.591 8.824L7.251 0H.258l9.168 13.343L.258 24H2.33l8.016-9.318L16.749 24h6.993zm-2.837 3.299-.929-1.329L3.076 1.56h3.182l5.965 8.532.929 1.329 7.754 11.09h-3.182z" />
                                                    },
                                                    _ => html_sync! {
                                                        <g fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
                                                            <circle cx="12" cy="12" r="9" />
                                                            <ellipse cx="12" cy="12" rx="4" ry="9" />
                                                            <path d="M3 12h18" />
                                                        </g>
                                                    },
                                                }}
                                            </svg>
                                        })}
                                        {content_part!("profile-label", *label)}
                                    </a>
                                </li>
                            }
                        }}
                    </ul>
                </div>
            </div>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for TeamPage {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            # {self.title}

            {self.intro}

            {self.founder}
            "###
        }
        .render_md(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for TeamMember {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        omit!(
            "profile-icon",
            "Decorative icons accompany the shared link labels."
        );
        let name = content_part!("name", self.name);
        let portrait = content_part!("portrait", Url(self.portrait));
        md_sync! { r###"
            ## {name}

            {self.role}, {self.organization}

            ![{name}]({portrait})

            {for (label, href) in self.links {
                let label = content_part!("profile-label", *label);
                let href = content_part!("profile-url", Url(href));
                md_sync! { "- [{label}]({href})\n" }
            }}
            "###
        }
        .render_md(cx)
    }
}

pub(crate) fn markdown() -> &'static str {
    static MARKDOWN: OnceLock<String> = OnceLock::new();
    MARKDOWN.get_or_init(|| render_markdown(TEAM))
}

/// Read Proa's founding team and leadership as HTML or Markdown.
#[utoipa::path(get, path = routes::TEAM, operation_id = "getTeamPage",
    responses((status = 200, description = "Founding team and leadership in the requested representation. Responses vary by Accept.", content((String = "text/html"), (String = "text/markdown")))))]
pub async fn handler(headers: HeaderMap) -> Response {
    let mut response = if crate::utils::http::accepts_markdown(&headers) {
        markdown_handler().await
    } else {
        render_page(META, TEAM).into_response()
    };
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    response
}

/// Read the same team content through its explicit Markdown URL.
#[utoipa::path(get, path = routes::TEAM_MARKDOWN, operation_id = "getTeamMarkdown",
    responses((status = 200, description = "Founding team and leadership as Markdown.", body = String, content_type = "text/markdown")))]
pub async fn markdown_handler() -> Response {
    crate::endpoints::agent_discovery::text_response(markdown(), "text/markdown; charset=utf-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_profile_content_is_shared_and_escaped_in_both_formats() {
        let page = TeamPage {
            title: "People <&>",
            intro: "A changed introduction",
            founder: TeamMember {
                name: "Ada <Lovelace>",
                role: "Technical lead",
                organization: "Example & Co.",
                portrait: "/static/portrait.jpg",
                links: &[("Work [profile]", "https://example.test/a (b)")],
            },
        };
        let html = proa_core::render_to_string(page);
        let markdown = render_markdown(page);
        assert!(html.contains("People &lt;&amp;&gt;"));
        assert!(html.contains("Ada &lt;Lovelace&gt;"));
        assert!(!html.contains("Ada <Lovelace>"));
        assert!(markdown.contains(r"Ada \<Lovelace\>"));
        assert!(markdown.contains(r"[Work \[profile\]](https://example.test/a%20\(b\))"));
        for output in [&html, &markdown] {
            assert!(output.contains(page.intro));
            assert!(output.contains(page.founder.role));
            assert!(output.contains(page.founder.portrait));
        }
        assert!(markdown.find(page.intro).unwrap() < markdown.find("## Ada").unwrap());
    }
}
