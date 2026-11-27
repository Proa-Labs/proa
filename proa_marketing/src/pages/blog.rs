// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{cmp::Reverse, sync::LazyLock};

use axum::{
    extract::Path,
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
};
use proa_core::{
    ComponentRole, DataLoader, HeadMetadata, MdRenderSync, StaticRaw, WebContext, WebRenderSync,
    WriteBuf, WriteError,
};
use proa_docs::{DocPage, Docs, DocsToc};
use proa_macros::{html_sync, md_sync};

use crate::{
    data::{routes, site},
    layout::{render_managed_page, PageMeta},
    utils::{
        document::{render_markdown, Url},
        http::accepts_markdown,
        routes::collection_route_path,
    },
};

use super::collection::{collection_response, insert_alternate_markdown_link};

pub(crate) const LABEL: &str = "Blog";
const TITLE: &str = const_format::concatcp!(site::NAME, " ", LABEL);
const INTRO: &str = "Release notes and engineering stories from the team building Proa.";
const ALL_POSTS: &str = "All posts";
const INDEX: PageMeta = PageMeta {
    title: TITLE,
    description: INTRO,
    path: routes::BLOG,
};
static BLOG: Docs = proa_docs::include_docs!("blog");

pub(crate) fn docs() -> &'static Docs {
    &BLOG
}

struct Post {
    page: &'static DocPage,
    author: &'static str,
    date: &'static str,
}

// The compiler checks the required fields and retains the authored frontmatter
// in the Markdown export. Read the two blog-specific fields once per process.
static POSTS: LazyLock<Vec<Post>> = LazyLock::new(|| {
    let mut posts: Vec<_> = BLOG
        .pages
        .iter()
        .map(|page| {
            let field = |name| {
                page.markdown
                    .lines()
                    .skip(1)
                    .take_while(|line| *line != "---")
                    .filter_map(|line| line.split_once(':'))
                    .find(|(key, _)| key.trim() == name)
                    .map(|(_, value)| value.trim().trim_matches(['"', '\'']))
                    .expect("blog frontmatter validated at build time")
            };
            Post {
                page,
                author: field("author"),
                date: field("date"),
            }
        })
        .collect();
    posts.sort_unstable_by_key(|post| (Reverse(post.date), post.page.slug));
    posts
});

fn visible_posts() -> impl Iterator<Item = &'static Post> {
    POSTS.iter().filter(|post| !post.page.frontmatter.hidden)
}

pub async fn handler(headers: HeaderMap) -> Response {
    if accepts_markdown(&headers) {
        return archive_markdown();
    }
    let mut response = render_managed_page(INDEX.path, BlogPage { post: None });
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    insert_alternate_markdown_link(&mut response, routes::BLOG_MARKDOWN);
    response
}

pub async fn markdown_handler() -> Response {
    archive_markdown()
}

fn archive_markdown() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
            (header::VARY, "Accept"),
        ],
        render_markdown(Archive),
    )
        .into_response()
}

pub async fn path_handler(headers: HeaderMap, Path(path): Path<String>) -> Response {
    let path = collection_route_path(routes::BLOG, &path);
    collection_response(docs(), &path, &headers, |page| {
        let post = POSTS
            .iter()
            .find(|post| post.page.slug == page.slug)
            .expect("compiled blog post");
        render_managed_page(page.route_path, BlogPage { post: Some(post) })
    })
}

struct BlogPage {
    post: Option<&'static Post>,
}

impl BlogPage {
    fn meta(&self) -> PageMeta {
        self.post.map_or(INDEX, |post| PageMeta {
            title: post.page.title,
            description: post.page.description.unwrap_or(INTRO),
            path: post.page.route_path,
        })
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BlogPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    fn render_head(&self, cx: &mut WebContext<L, B>) -> impl proa_core::SyncHeadOperation {
        let meta = self.meta();
        cx.head_mut().metadata(HeadMetadata {
            title: meta.title.into(),
            description: meta.description.into(),
            canonical: meta.path.into(),
            ..Default::default()
        })
    }

    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let current = self.meta().path;
        html_sync! {
            <div class="blog-layout" data-component="blog-page">
                <div class="blog-frame">
                    <aside class="blog-sidebar">
                        <p class="blog-sidebar-title">{LABEL}</p>
                        {PostNav { current }}
                    </aside>
                    <details class="blog-mobile-nav">
                        <summary>"Browse posts"</summary>
                        {PostNav { current }}
                    </details>
                    <div class="blog-content">
                        {match self.post {
                            Some(post) => html_sync! { {Article { post }} },
                            None => html_sync! { {Archive} },
                        }}
                    </div>
                </div>
            </div>
        }
        .render(cx)
    }
}

struct PostNav {
    current: &'static str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PostNav {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <nav class="blog-nav" data-component="blog-nav" aria-label="Blog posts">
                <ul>
                    <li>
                        <a
                            href={routes::BLOG}
                            aria-current={(self.current == routes::BLOG).then_some("page")}
                        >
                            {ALL_POSTS}
                        </a>
                    </li>
                    {for post in visible_posts() {
                        html_sync! {
                            <li>
                                <a href={post.page.route_path} aria-current={(self.current == post.page.route_path).then_some("page")}>
                                    {post.page.title}
                                </a>
                            </li>
                        }
                    }}
                </ul>
            </nav>
        }
        .render(cx)
    }
}

struct Archive;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Archive {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section data-component="blog-archive">
                <header class="blog-archive-header">
                    <h1 class="blog-heading">{TITLE}</h1>
                    <p class="blog-intro">{INTRO}</p>
                </header>
                <ul class="blog-posts">
                    {for post in visible_posts() {
                        html_sync! {
                            <li>
                                <a href={post.page.route_path} class="blog-card">
                                    <time datetime={post.date}>{post.date}</time>
                                    <h2>{post.page.title}</h2>
                                    <p>{post.page.description}</p>
                                    <span class="blog-card-arrow" aria-hidden="true">"→"</span>
                                </a>
                            </li>
                        }
                    }}
                </ul>
            </section>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Archive {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            # {TITLE}

            {INTRO}

            {for post in visible_posts() {
                md_sync! { "- [{post.page.title}]({Url(post.page.route_path)}) · {post.date}. {post.page.description}\n" }
            }}
            "###
        }
        .render_md(cx)
    }
}

struct Article {
    post: &'static Post,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Article {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let page = self.post.page;
        html_sync! {
            <article data-component="blog-article">
                <header class="blog-article-header">
                    <a class="blog-back" href={routes::BLOG}><span aria-hidden="true">"←"</span>{ALL_POSTS}</a>
                    <h1 class="blog-heading">{page.title}</h1>
                    <p class="blog-intro">{page.description}</p>
                    <div class="blog-byline">
                        <span>{self.post.author}</span>
                        <time datetime={self.post.date}>{self.post.date}</time>
                        <a href={page.markdown_path}>"Open Markdown"</a>
                    </div>
                </header>
                <div class="blog-reading">
                    {if page.frontmatter.toc != Some(false) && !page.toc.is_empty() {
                        html_sync! {
                            <aside class="blog-toc" aria-label="Article contents">
                                {DocsToc { items: page.toc }}
                            </aside>
                        }
                    }}
                    <div class="blog-prose prose">
                        // The MDX compiler renders and escapes the authored source at build time.
                        {StaticRaw(page.article_html)}
                    </div>
                </div>
            </article>
        }
        .render(cx)
    }
}
