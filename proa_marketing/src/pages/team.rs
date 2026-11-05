// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::OnceLock;

use axum::{
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
};
use proa_core::{DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{content_part, html_sync, md_sync};

use crate::{
    data::{routes, site},
    layout::{render_page, PageMeta},
    utils::document::{render_markdown, Url},
};

pub(crate) const META: PageMeta = PageMeta {
    title: const_format::concatcp!("Team — ", site::ORGANIZATION_NAME),
    description: const_format::concatcp!(
        "Meet the founder behind ",
        site::ORGANIZATION_NAME,
        " and the Proa Rust web framework."
    ),
    path: routes::TEAM,
};

const TEAM: TeamPage = TeamPage {
    title: "Meet the team",
    intro: const_format::concatcp!("The people behind Proa. ", site::ORGANIZATION, " builds the Proa Rust web framework."),
    section: "Founders and leadership",
    founder: TeamMember {
        name: "Andrew Hariri",
        role: "Founder",
        organization: site::ORGANIZATION,
        bio: "Andrew builds Proa and leads its technical direction, from the template compiler and rendering engine to the developer tools and documentation.",
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
    section: &'static str,
    founder: TeamMember,
}

#[derive(Clone, Copy)]
pub(crate) struct TeamMember {
    pub(crate) name: &'static str,
    pub(crate) role: &'static str,
    pub(crate) organization: &'static str,
    pub(crate) bio: &'static str,
    pub(crate) portrait: &'static str,
    pub(crate) links: &'static [(&'static str, &'static str)],
}

pub(crate) fn founder() -> TeamMember {
    TEAM.founder
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TeamPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <article data-team-page class="mx-auto max-w-5xl px-6 py-16 sm:py-24">
                <h1 class="text-4xl font-semibold tracking-tight sm:text-5xl">{self.title}</h1>
                <p class="mt-6 max-w-2xl text-lg leading-8 text-zinc-600 dark:text-zinc-300">
                    {self.intro}
                </p>
                <section
                    aria-labelledby="team-leadership"
                    class="mt-16 border-t border-black/10 pt-10 dark:border-white/10"
                >
                    <h2
                        id="team-leadership"
                        class="text-sm font-medium uppercase tracking-widest text-zinc-500 dark:text-zinc-400"
                    >
                        {self.section}
                    </h2>
                    {self.founder}
                </section>
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
                    <h3 class="text-2xl font-semibold tracking-tight">
                        {content_part!("name", self.name)}
                    </h3>
                    <p class="mt-2 text-base font-medium">{self.role}", "{self.organization}</p>
                    <p class="mt-5 max-w-2xl text-base leading-7 text-zinc-600 dark:text-zinc-300">
                        {self.bio}
                    </p>
                    <ul class="mt-6 flex flex-wrap gap-6 text-sm font-medium">
                        {for (label, href) in self.links {
                            html_sync! {
                                <li><a href={content_part!("profile-url", *href)} rel="me" class="underline decoration-black/20 underline-offset-4 transition-colors hover:text-brand dark:decoration-white/30">{content_part!("profile-label", *label)}</a></li>
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

            ## {self.section}

            {self.founder}
            "###
        }
        .render_md(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for TeamMember {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let name = content_part!("name", self.name);
        let portrait = content_part!("portrait", Url(self.portrait));
        md_sync! { r###"
            ### {name}

            {self.role}, {self.organization}

            ![{name}]({portrait})

            {self.bio}

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
            section: "Leadership",
            founder: TeamMember {
                name: "Ada <Lovelace>",
                role: "Technical lead",
                organization: "Example & Co.",
                bio: "Builds [typed] interfaces.",
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
        assert!(markdown.contains(r"Builds \[typed\]"));
        assert!(markdown.contains(r"[Work \[profile\]](https://example.test/a%20\(b\))"));
        for output in [&html, &markdown] {
            assert!(output.contains(page.intro));
            assert!(output.contains(page.founder.role));
            assert!(output.contains(page.founder.portrait));
        }
        assert!(markdown.find("## Leadership").unwrap() < markdown.find("### Ada").unwrap());
    }
}
