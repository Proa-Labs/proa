// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Edit this file, then run from the Proa workspace root:
//!
//! cargo run -p mcp-incident-desk --example markdown_playground
//! cargo test -p mcp-incident-desk --example markdown_playground
//!
//! The runner writes playground.md, playground.html, and a local SVG image to
//! the system temporary directory. Pass an output directory as the first arg.
//! The HTML preview enables tables, tasks, strikethrough, footnotes, definition
//! lists, and GFM alerts. Math is preserved as TeX, without a typesetting engine.
//!
//! Raw templates accept ordinary Markdown on Rust editions 2021 and 2024.
//! Rust expressions use {...}; {{...}} writes literal braces outside code.
//! Code spans and fenced code preserve braces literally.
//! Install the Proa editor extension for Rust navigation in slots.

use std::{future::Future, path::PathBuf};

use proa_core::{
    render_md_to_string_async, to_md_async, MdRenderSync, RenderOutcome, WebContext, WriteBuf,
    WriteError,
};
use proa_macros::{md, md_sync};
use pulldown_cmark::{html, Options, Parser};

// Cmd+click these fields from the templates below, or try changing their types.
#[derive(Clone, Copy)]
struct Person<'a> {
    name: &'a str,
    role: &'a str,
    website: &'a str,
}

#[derive(Clone, Copy)]
struct Feature<'a> {
    name: &'a str,
    checks: u32,
    ready: bool,
}

/// A custom synchronous component with a nested custom link component.
struct PersonCard<'a>(Person<'a>);

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for PersonCard<'_> {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            ### {self.0.name}

            **Role:** {self.0.role}

            {MarkdownLink { label: "Website", href: self.0.website }}

            "###
        }
        .render_md(cx)
    }
}

/// URL destinations use the URL writer, independently of label escaping.
struct MarkdownLink<'a> {
    label: &'a str,
    href: &'a str,
}

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for MarkdownLink<'_> {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.md_link(self.label, self.href)
    }
}

/// The core inline-code writer chooses a delimiter that fits the dynamic text.
struct InlineCode<'a>(&'a str);

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for InlineCode<'_> {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.write_md_inline_code(self.0)
    }
}

struct FeatureTable<'a>(&'a [Feature<'a>]);

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for FeatureTable<'_> {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            | Feature | Checks | Ready |
            | :--- | ---: | :---: |
            {for feature in self.0 {
            | {feature.name} | {feature.checks} | {(if feature.ready { "Yes" } else { "No" })} |
            }}
            "###
        }
        .render_md(cx)
    }
}

struct ConditionalNote(bool);

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for ConditionalNote {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            {if self.0 {
            > [!NOTE]
            > The optional details are enabled.
            } else {
            Optional details are hidden.
            }}
            "###
        }
        .render_md(cx)
    }
}

/// A real asynchronous custom component: it returns Pending and yields before
/// producing its Markdown. This uses the current MdRender trait signature.
struct AsyncRelease<'a> {
    version: &'a str,
    checks: u32,
}

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> proa_core::MdRender<Loader, B>
    for AsyncRelease<'_>
{
    fn render_md(
        self,
        cx: &mut WebContext<Loader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            tokio::task::yield_now().await;
            let content = md_sync! { r###"
                ### Release {self.version}

                Completed **{self.checks}** checks after awaiting.

                "###
            };
            MdRenderSync::render_md(content, cx)
        })
    }
}

/// The main syntax playground, with ordinary Markdown in one raw template.
async fn document(show_details: bool) -> String {
    let author = Person {
        name: "Andrew Hariri",
        role: "Rust & Markdown",
        website: "https://proa.so/docs?from=markdown&mode=preview",
    };
    let features = [
        Feature {
            name: "Static Markdown",
            checks: 6,
            ready: true,
        },
        Feature {
            name: "Typed children",
            checks: 4,
            ready: true,
        },
        Feature {
            name: "Pipes | stay in one cell",
            checks: 2,
            ready: false,
        },
    ];
    let literal_text =
        "**not bold** <script>alert('no')</script> [not a link](https://example.com)";
    let optional_child = Some(InlineCode("Option<InlineCode>"));
    let absent_child: Option<InlineCode<'_>> = None;

    render_md_to_string_async(md! { r###"
        # Markdown playground

        A live document built from raw Markdown templates and typed Rust components.

        ## Headings

        ### Heading three

        #### Heading four

        ##### Heading five

        ###### Heading six

        Setext heading one
        ==================

        Setext heading two
        ------------------

        ## Paragraphs and inline formatting

        This is a soft line break.
        The same paragraph continues here.

        This is a hard line break.\
        This sentence starts on the next line.

        *Emphasis*, _also emphasis_, **strong**, __also strong__,
        ***strong emphasis***, and ~~strikethrough~~.

        Inline code: `let value = 42;`.
        Adaptive code: {to_md_async(InlineCode("a `backtick` and ``two``"))}.

        Punctuation: don't, "quoted", Rust · Proa — 🦀.
        Literal delimiters: {{literal_braces}}.
        Escaped Markdown: \*not italic\* and \[not a link\].

        ## Links and images

        [Inline link](https://proa.so "Proa homepage")
        and [reference link][proa].

        [Collapsed reference][] and [shortcut].

        Autolink: <https://proa.so>.
        Email: <hello@example.com>.

        Dynamic link: {to_md_async(MarkdownLink { label: author.name, href: author.website })}.

        ![Local Proa illustration](playground.svg "Local image with no network dependency")

        [proa]: https://proa.so "Proa reference"
        [Collapsed reference]: https://proa.so/docs
        [shortcut]: https://proa.so

        ## Lists

        - Unordered item
        - Item with **strong text**
          - Nested unordered item
          - Another nested item

        1. First ordered item
        2. Second ordered item
           1. Nested ordered item

        - [x] Completed task
        - [ ] Pending task

        ## Blockquotes and rules

        > A blockquote with **formatting**.
        >
        > > A nested blockquote.

        ---

        ## Tables

        | Left aligned | Centered | Right aligned |
        | :--- | :---: | ---: |
        | **Bold cell** | `code` | 42 |
        | [Link](https://proa.so) | A \| B | 100 |

        ### Table from custom structs

        {to_md_async(FeatureTable(&features))}

        ## Code blocks

        ~~~rust
        struct Example { count: u32 }
        fn main() { println!("hello"); }
        ~~~

        ```json
        {"ready": true, "items": [1, 2, 3]}
        ```

            indented_code();
            another_line();

        ## Custom components

        {to_md_async(PersonCard(author))}

        Present optional child: {to_md_async(optional_child)}.
        Absent optional child: before{to_md_async(absent_child)}after.

        {to_md_async(ConditionalNote(show_details))}

        ## Async components and expressions

        {AsyncRelease { version: "0.3.0", checks: 12 }}

        Awaited expression: {async {
            tokio::task::yield_now().await;
            "resolved without a network request"
        }.await}.

        ## Escaped dynamic text

        The next paragraph must stay literal text, including the apparent HTML:

        {literal_text}

        ## Extended Markdown

        A footnote reference.[^example]

        [^example]: Footnote **content**, with a [link](https://proa.so).

        Markdown
        : A document format with plain text source.

        Proa
        : Typed composition with compiled templates.

        Inline math: $E = mc^2$.

        $$
        a^2 + b^2 = c^2
        $$

        Math remains TeX in this preview; it does not load a math typesetter.

        ## Authored HTML

        <details>
        <summary>Expand the HTML example</summary>

        **Markdown inside an HTML details element**, separated by blank lines.

        </details>

        Text with <sub>subscript</sub>, <sup>superscript</sup>, and <kbd>Enter</kbd>.

        "###
    })
    .await
}

fn preview_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_GFM
        | Options::ENABLE_MATH
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let directory = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("proa-markdown-playground"));
    std::fs::create_dir_all(&directory)?;
    let markdown = document(true).await;
    let mut body = String::new();
    html::push_html(&mut body, Parser::new_ext(&markdown, preview_options()));
    std::fs::write(directory.join("playground.md"), &markdown)?;
    std::fs::write(
        directory.join("playground.html"),
        format!("{HTML_HEAD}{body}{HTML_END}"),
    )?;
    std::fs::write(directory.join("playground.svg"), IMAGE)?;
    println!("Markdown: {}", directory.join("playground.md").display());
    println!("Preview:  {}", directory.join("playground.html").display());
    Ok(())
}

const HTML_HEAD: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Proa Markdown playground</title>
<style>
:root{color-scheme:light dark;font-family:system-ui,sans-serif;color:#252525;background:#fbfbfa}
body{margin:0}main{max-width:860px;margin:auto;padding:48px 24px 100px;line-height:1.7}
nav{display:flex;gap:20px;font-size:14px;margin-bottom:32px}a{color:#a74913;text-underline-offset:3px}
h1,h2,h3,h4,h5,h6{line-height:1.25;letter-spacing:-.025em}h1{font-size:2.5rem}h2{margin-top:3rem;padding-bottom:10px;border-bottom:1px solid #ddd}
h3{font-size:1.35rem}h4{font-size:1.15rem}h5{font-size:1rem}h6{font-size:.9rem;color:#666}
pre,code,kbd{font-family:ui-monospace,monospace;font-size:.9em}code,kbd{background:#eeeae4;padding:2px 5px;border-radius:4px}
pre{background:#eeeae4;padding:20px;overflow-x:auto;border-radius:8px}pre code{padding:0;background:none}
table{width:100%;border-collapse:collapse;margin:24px 0}th,td{padding:10px 14px;border:1px solid #d8d4ce}th{background:#eeeae4}
blockquote{margin:24px 0;padding:1px 20px;border-left:3px solid #dd7627;background:#f5f0e9}
img{display:block;max-width:100%;border-radius:10px;margin:24px 0}hr{border:0;border-top:1px solid #d8d4ce;margin:32px 0}
li{margin:5px 0}details{border:1px solid #d8d4ce;border-radius:8px;padding:16px 20px}summary{cursor:pointer;font-weight:600}
dt{font-weight:600}dd{margin-bottom:12px}.footnote-definition{font-size:.9em}.math{font-family:ui-monospace,monospace}
@media(prefers-color-scheme:dark){:root{background:#191919;color:#e8e8e8}a{color:#ffb070}h2,th,td,hr,details{border-color:#444}pre,code,kbd,th{background:#292929}blockquote{background:#26221e}h6{color:#bbb}}
</style></head><body><main><nav><a href="playground.md">Markdown source</a><a href="#" onclick="window.print();return false">Print preview</a></nav>
"##;
const HTML_END: &str = "</main></body></html>";
const IMAGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="860" height="220" viewBox="0 0 860 220">
<rect width="860" height="220" rx="12" fill="#eeeae4"/><circle cx="110" cy="110" r="52" fill="#dd7627"/>
<path d="M88 135V85h24a17 17 0 0 1 0 34H88" stroke="#fff" stroke-width="9" fill="none"/>
<text x="195" y="105" font-family="system-ui,sans-serif" font-size="30" font-weight="600" fill="#252525">Proa Markdown playground</text>
<text x="195" y="145" font-family="system-ui,sans-serif" font-size="19" fill="#555">Local image · typed components · async rendering</text></svg>"##;

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, LinkType, Tag};

    #[tokio::test]
    async fn document_contains_real_markdown_elements() {
        let markdown = document(true).await;
        let events: Vec<_> = Parser::new_ext(&markdown, preview_options()).collect();
        for level in [
            HeadingLevel::H1,
            HeadingLevel::H2,
            HeadingLevel::H3,
            HeadingLevel::H4,
            HeadingLevel::H5,
            HeadingLevel::H6,
        ] {
            assert!(events.iter().any(|event| matches!(event, Event::Start(Tag::Heading { level: found, .. }) if *found == level)), "missing {level:?}");
        }
        for kind in [
            LinkType::Inline,
            LinkType::Reference,
            LinkType::Collapsed,
            LinkType::Shortcut,
            LinkType::Autolink,
            LinkType::Email,
        ] {
            assert!(events.iter().any(|event| matches!(event, Event::Start(Tag::Link { link_type, .. }) if *link_type == kind)), "missing {kind:?} link");
        }
        for tag in [
            Tag::Emphasis,
            Tag::Strong,
            Tag::Strikethrough,
            Tag::List(None),
            Tag::List(Some(1)),
            Tag::BlockQuote(None),
            Tag::DefinitionList,
            Tag::CodeBlock(CodeBlockKind::Indented),
        ] {
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, Event::Start(found) if *found == tag)),
                "missing {tag:?}"
            );
        }
        assert!(events.iter().any(|event| matches!(event, Event::Start(Tag::Table(align)) if align == &[Alignment::Left, Alignment::Center, Alignment::Right])));
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::Start(Tag::Image { .. }))));
        assert!(events.iter().any(|event| matches!(event, Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(language))) if language.as_ref() == "rust")));
        assert!(events.iter().any(|event| matches!(event, Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(language))) if language.as_ref() == "json")));
        assert!(events.iter().any(|event| matches!(event, Event::Code(_))));
        assert!(events.iter().any(|event| matches!(event, Event::HardBreak)));
        assert!(events.iter().any(|event| matches!(event, Event::SoftBreak)));
        assert!(events.iter().any(|event| matches!(event, Event::Rule)));
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::TaskListMarker(true))));
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::TaskListMarker(false))));
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::FootnoteReference(_))));
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::InlineMath(_))));
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::DisplayMath(_))));
    }

    #[test]
    fn dynamic_cells_do_not_create_extra_columns() {
        let rows = [Feature {
            name: "a | b",
            checks: 37,
            ready: true,
        }];
        let markdown = sync_markdown(FeatureTable(&rows));
        let events: Vec<_> = Parser::new_ext(&markdown, preview_options()).collect();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Start(Tag::TableCell)))
                .count(),
            6
        );
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::Text(text) if text.as_ref() == "37")));
        assert!(plain_text(&markdown).contains("a | b"));
    }

    #[test]
    fn components_escape_text_and_preserve_link_destinations() {
        let person = Person {
            name: "**name** <script>bad</script>",
            role: "[role](bad)",
            website: "https://example.com/a(b)?q=a%20b&x=1",
        };
        let markdown = sync_markdown(PersonCard(person));
        let events: Vec<_> = Parser::new_ext(&markdown, preview_options()).collect();
        assert!(plain_text(&markdown).contains(person.name));
        assert!(plain_text(&markdown).contains(person.role));
        assert!(!events
            .iter()
            .any(|event| matches!(event, Event::Html(_) | Event::InlineHtml(_))));
        assert!(events.iter().any(|event| matches!(event, Event::Start(Tag::Link { dest_url, .. }) if dest_url.as_ref() == person.website)), "{markdown}");
    }

    #[test]
    fn adaptive_code_and_optional_children_preserve_payloads() {
        let payload = "a `backtick` and ``two``";
        let markdown = sync_markdown(InlineCode(payload));
        assert!(Parser::new(&markdown)
            .any(|event| matches!(event, Event::Code(text) if text.as_ref() == payload)));
        assert_eq!(sync_markdown(None::<InlineCode<'_>>), "");
        assert_eq!(sync_markdown(Some(InlineCode(payload))), markdown);
    }

    #[test]
    fn conditional_component_selects_the_requested_branch() {
        let enabled = sync_markdown(ConditionalNote(true));
        let disabled = sync_markdown(ConditionalNote(false));
        assert!(Parser::new_ext(&enabled, preview_options())
            .any(|event| matches!(event, Event::Start(Tag::BlockQuote(Some(_))))));
        assert!(!Parser::new_ext(&disabled, preview_options())
            .any(|event| matches!(event, Event::Start(Tag::BlockQuote(_)))));
    }

    #[tokio::test]
    async fn custom_async_component_is_awaited() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        let result = proa_core::MdRender::render_md(
            AsyncRelease {
                version: "test-release",
                checks: 73,
            },
            &mut cx,
        );
        assert!(matches!(result, RenderOutcome::Pending(_)));
        result.resolve().await.unwrap();
        let markdown = String::from_utf8(out).unwrap();
        let text = plain_text(&markdown);
        assert!(text.contains("test-release"));
        assert!(text.contains("73"));
    }

    fn sync_markdown(child: impl MdRenderSync<::proa_core::NoLoader, Vec<u8>>) -> String {
        let mut bytes = Vec::new();
        child
            .render_md(&mut WebContext::with_buffer(&mut bytes))
            .unwrap();
        String::from_utf8(bytes).unwrap()
    }

    fn plain_text(markdown: &str) -> String {
        Parser::new_ext(markdown, preview_options())
            .filter_map(|event| match event {
                Event::Text(text) | Event::Code(text) => Some(text.into_string()),
                _ => None,
            })
            .collect()
    }
}
