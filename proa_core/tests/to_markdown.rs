// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![cfg(feature = "html-to-markdown")]

use std::{cell::Cell, panic::AssertUnwindSafe, rc::Rc, sync::Arc};

use proa_core::{
    shared::escape::EscapeDialect, to_markdown, ContentMode, DataLoader, MdRenderSync, NoLoader,
    PendingBoundary, RenderMode, StreamBoundaryRecorder, StreamChunkFlusher, WebContext,
    WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::{html_sync, md, md_sync};
use pulldown_cmark::{Event, Options, Parser, Tag};

fn markdown<T: WebRenderSync>(value: T) -> String {
    let mut cx = WebContext::new();
    to_markdown(value).render_md(&mut cx).unwrap();
    cx.into_output().into_string()
}

fn text_content(source: &str) -> String {
    Parser::new_ext(source, Options::ENABLE_TABLES)
        .filter_map(|event| match event {
            Event::Text(text) | Event::Code(text) => Some(text.into_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn fragment_converts_semantic_html_without_styling() {
    let title = "Proa & Rust";
    let body = "Compiled templates";
    let url = "/docs?target=rust&mode=sync";
    let output = markdown(html_sync! {
        <article class="rounded-lg" data-card>
            <h2>{title}</h2>
            <p>
                {body}
                " with "
                <strong>"fast rendering"</strong>
                "."
            </p>
            <a href={url}>"Learn more"</a>
        </article>
    });
    assert_eq!(
        output,
        "## Proa & Rust\n\nCompiled templates with **fast rendering**&#46;\n\n[Learn more](/docs?target=rust&mode=sync)"
    );
}

struct Article<'a>(&'a str);
impl<L: DataLoader> WebRenderSync<L> for Article<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <h2>{self.0}</h2>
        }
        .render(cx)
    }
}
impl<L: DataLoader> MdRenderSync<L> for Article<'_> {
    fn render_md(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        to_markdown(self).render_md(cx)
    }
}

#[test]
fn default_writer_component_can_delegate_without_recursing() {
    let title = String::from("Borrowed article");
    let mut cx = WebContext::new();
    Article(&title).render_md(&mut cx).unwrap();
    assert_eq!(cx.into_output().into_string(), "## Borrowed article");
}

#[test]
fn default_writer_component_can_target_a_custom_markdown_writer() {
    let mut cx = WebContext::with_buffer(Vec::new());
    to_markdown(Article("Custom writer"))
        .render_md(&mut cx)
        .unwrap();
    assert_eq!(cx.into_output(), b"## Custom writer");
}

#[test]
fn source_can_read_a_custom_loader_and_update_request_metadata() {
    struct Loader(String);
    impl DataLoader for Loader {
        fn load(
            &self,
            _: &proa_core::LoadRequest,
        ) -> impl std::future::Future<Output = Result<proa_core::LoadValue, proa_core::LoadError>> + Send
        {
            std::future::ready(Err(proa_core::LoadError::NoLoader))
        }
    }
    struct LoadedHeading;
    impl WebRenderSync<Loader> for LoadedHeading {
        fn render(self, cx: &mut WebContext<Loader>) -> Result<(), WriteError> {
            let loader = Arc::clone(cx.loader());
            cx.head_mut().set_title(loader.0.clone());
            html_sync! {
                <h2>{loader.0.as_str()}</h2>
            }
            .render(cx)
        }
    }
    let loader = Arc::new(Loader(String::from("Request-specific title")));
    let mut cx = WebContext::with_loader(loader.clone());
    to_markdown(LoadedHeading).render_md(&mut cx).unwrap();
    assert!(Arc::ptr_eq(cx.loader(), &loader));
    assert_eq!(cx.head().title().as_deref(), Some("Request-specific title"));
    assert_eq!(cx.into_output().into_string(), "## Request-specific title");
}

#[test]
fn dynamic_text_retains_literal_semantics() {
    let inputs = [
        "*not emphasis* [not a link](https://example.com)",
        "<script>alert(1)</script> &amp; <img src=x>",
        "&copy; &#65; &#x41;",
        "<![CDATA[literal text]]>",
        "# not a heading",
        "- not a list",
        "1. not a list",
        "\\ backslash",
        "日本語 café 🦀",
    ];
    for input in inputs {
        let output = markdown(html_sync! {
            <p>{input}</p>
        });
        assert_eq!(text_content(&output), input, "Markdown: {output}");
        assert!(!Parser::new(&output)
            .any(|event| matches!(event, Event::Html(_) | Event::InlineHtml(_))));
    }
}

#[test]
fn image_alt_text_and_link_destinations_remain_literal() {
    let alt = "a [label] &copy; <tag>";
    let url = "/images?literal=&copy;";
    let output = markdown(html_sync! {
        <img src={url} alt={alt} />
        <a href={url}>"Link"</a>
    });
    assert_eq!(text_content(&output), format!("{alt}Link"), "{output}");
    let events: Vec<_> = Parser::new(&output).collect();
    assert!(events.iter().any(
        |e| matches!(e, Event::Start(Tag::Image { dest_url, .. }) if dest_url.as_ref() == url)
    ));
    assert!(events.iter().any(
        |e| matches!(e, Event::Start(Tag::Link { dest_url, .. }) if dest_url.as_ref() == url)
    ));
}

#[test]
fn nested_lists_links_images_and_quotes_keep_structure() {
    let label = "Guide";
    let output = markdown(html_sync! {
        <ul>
            <li>
                {label}
                <ul>
                    <li>
                        <a href="/nested">"Nested"</a>
                    </li>
                </ul>
            </li>
            <li>"Second"</li>
        </ul>
        <ol start="3">
            <li>"Third"</li>
            <li>"Fourth"</li>
        </ol>
        <blockquote>
            <p>
                "Quoted "
                <em>"text"</em>
            </p>
        </blockquote>
        <img src="/diagram.png" alt="Architecture diagram" />
    });
    let events: Vec<_> = Parser::new(&output).collect();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Start(Tag::List(_))))
            .count(),
        3
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Start(Tag::List(Some(3))))));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Start(Tag::BlockQuote(_)))));
    assert!(events.iter().any(|e| matches!(e, Event::Start(Tag::Image { dest_url, .. }) if dest_url.as_ref() == "/diagram.png")));
    assert!(events.iter().any(
        |e| matches!(e, Event::Start(Tag::Link { dest_url, .. }) if dest_url.as_ref() == "/nested")
    ));
    assert_eq!(
        text_content(&output),
        "GuideNestedSecondThirdFourthQuoted textArchitecture diagram"
    );
}

#[test]
// proa-perf-ok(scope): this fixture constructs source code to verify adaptive Markdown fences
fn tables_and_adaptive_code_fences_round_trip() {
    let fence = char::from(96).to_string().repeat(3);
    let code = format!("fn main() {{\n    println!(\"{fence} & <tag>\");\n}}\n");
    let cell = "a | b";
    let output = markdown(html_sync! {
        <pre>
            <code class="language-rust">{code.as_str()}</code>
        </pre>
        <table>
            <thead>
                <tr>
                    <th>"Name"</th>
                    <th>"Value"</th>
                </tr>
            </thead>
            <tbody>
                <tr>
                    <td>"Item"</td>
                    <td>{cell}</td>
                </tr>
            </tbody>
        </table>
    });
    let events: Vec<_> = Parser::new_ext(&output, Options::ENABLE_TABLES).collect();
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Start(Tag::CodeBlock(_)))));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Text(text) if text.as_ref() == code)));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Start(Tag::Table(_)))));
    assert_eq!(text_content(&output), format!("{code}NameValueItem{cell}"));
}

#[test]
fn non_content_nodes_are_omitted_and_empty_output_is_a_noop() {
    let visible = "Visible content";
    let output = markdown(html_sync! {
        <head>
            <title>"Metadata"</title>
        </head>
        <script>"window.internal = true;"</script>
        <style>"body { color: red; }"</style>
        <template>
            <p>"Template content"</p>
        </template>
        <svg>
            <title>"Decorative icon"</title>
        </svg>
        <div hidden>
            <h2>"Hidden heading"</h2>
        </div>
        <span aria-hidden="true">"Decorative label"</span>
        <p>{visible}</p>
    });
    assert_eq!(output, visible);
    let mut cx = WebContext::new();
    cx.write_md_text(visible).unwrap();
    to_markdown(html_sync! {
        <svg />
    })
    .render_md(&mut cx)
    .unwrap();
    cx.md_block_boundary().unwrap();
    assert_eq!(cx.into_output().into_string(), "Visible content\n\n");
}

#[test]
fn converting_a_parent_uses_child_html_while_explicit_composition_uses_child_markdown() {
    struct Child<'a>(&'a str);
    impl<L: DataLoader> WebRenderSync<L> for Child<'_> {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            html_sync! {
                <strong>{self.0}</strong>
            }
            .render(cx)
        }
    }
    impl<L: DataLoader> MdRenderSync<L> for Child<'_> {
        fn render_md(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            md_sync! { r###"
                > {self.0}
                "###
            }
            .render_md(cx)
        }
    }
    let input = "Custom semantics";
    assert_eq!(
        markdown(html_sync! { <div>{Child(input)}</div> }),
        "**Custom semantics**"
    );
    let mut cx = WebContext::new();
    md_sync! { r###"
        {to_markdown(Article("Automatic parent"))}

        {Child(input)}
        "###
    }
    .render_md(&mut cx)
    .unwrap();
    assert_eq!(
        cx.into_output().into_string(),
        "## Automatic parent\n\n> Custom semantics\n\n"
    );
}

#[test]
fn inline_code_and_fenced_code_do_not_decode_literal_entities_again() {
    let input = "&amp; &copy; <tag>";
    let output = markdown(html_sync! {
        <p>
            <code>{input}</code>
        </p>
        <pre>
            <code>{input}</code>
        </pre>
    });
    assert_eq!(text_content(&output), format!("{input}{input}\n"));
}

#[test]
fn entity_prefixes_split_across_inline_nodes_remain_literal() {
    let prefix = "&am";
    let suffix = "p;";
    let output = markdown(html_sync! {
        <p>
            {prefix}
            <span>{suffix}</span>
        </p>
        <p>
            "&"
            <span>"copy;"</span>
        </p>
    });
    assert_eq!(text_content(&output), "&amp;&copy;");
}

#[test]
// proa-perf-ok(scope): this fixture constructs preformatted source to verify code preservation
fn plain_preformatted_text_becomes_a_code_block() {
    let code = format!(
        "{}\n  indented &amp; <tag>\n  second line\n",
        char::from(96).to_string().repeat(3)
    );
    let output = markdown(html_sync! {
        <pre>{code.as_str()}</pre>
    });
    assert!(Parser::new(&output).any(|e| matches!(e, Event::Start(Tag::CodeBlock(_)))));
    assert_eq!(text_content(&output), code);
}

#[test]
fn explicit_markdown_children_can_be_composed_beside_converted_html() {
    let mut cx = WebContext::new();
    md_sync! { r###"
        Before

        {to_markdown(Article("Automatic"))}

        {proa_core::TrustedMarkdown("> Custom child")}
        "###
    }
    .render_md(&mut cx)
    .unwrap();
    assert_eq!(
        cx.into_output().into_string(),
        "Before\n\n## Automatic\n\n> Custom child\n"
    );
}

#[tokio::test]
async fn sync_adapter_composes_directly_in_async_markdown() {
    let output = proa_core::render_md_to_string_async(md! { r###"
        {to_markdown(Article("Async composition"))}
        "###
    })
    .await;
    assert_eq!(output, "## Async composition\n");
    let mut cx = WebContext::new();
    assert!(matches!(
        proa_core::MdRender::render_md(to_markdown(Article("Done")), &mut cx),
        proa_core::RenderOutcome::Done(Ok(()))
    ));
}

struct Hooks(Cell<usize>);
impl StreamChunkFlusher for Hooks {
    fn flush_chunk(&self, _: Vec<u8>) -> Result<(), WriteError> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}
impl StreamBoundaryRecorder for Hooks {
    fn next_boundary_id(&self) -> u64 {
        panic!("capture must not schedule HTML boundaries")
    }
    fn push_boundary(&self, _: PendingBoundary) {
        panic!("capture must not schedule HTML boundaries")
    }
}

struct StateProbe {
    loader: Arc<NoLoader>,
    fail: bool,
    panic: bool,
}
impl WebRenderSync for StateProbe {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        assert!(Arc::ptr_eq(cx.loader(), &self.loader));
        assert_eq!(cx.locals().get::<usize>().as_deref(), Some(&7));
        assert_eq!(cx.current_mode(), ContentMode::Html);
        assert_eq!(cx.__proa_current_escape_dialect(), EscapeDialect::Html);
        assert_eq!(cx.render_mode(), RenderMode::Static);
        assert!(cx.stream_chunk_flusher().is_none());
        assert!(cx.stream_recorder().is_none());
        assert_eq!(cx.md_heading_offset(), 0);
        assert_eq!(cx.md_list_depth(), 0);
        assert!(!cx.md_inline());
        cx.locals_mut().insert(9usize);
        html_sync! {
            <p>"Captured"</p>
        }
        .render(cx)?;
        cx.flush_stream_chunk()?;
        cx.md_push_heading_offset(3);
        cx.md_push_list_depth();
        cx.md_set_inline(true);
        cx.write_md_trusted("\n\n")?;
        cx.enter_mode(ContentMode::Style);
        cx.__proa_enter_escape_dialect(EscapeDialect::Html);
        cx.set_render_mode(RenderMode::Agent);
        if self.panic {
            panic!("probe panic");
        }
        if self.fail {
            return Err(WriteError::InvalidAttribute("probe failure"));
        }
        Ok(())
    }
}

#[test]
fn capture_restores_formatting_hooks_and_loader_on_success_error_and_panic() {
    for outcome in 0..3 {
        let loader = Arc::new(NoLoader);
        let hooks = Rc::new(Hooks(Cell::new(0)));
        let flusher: Rc<dyn StreamChunkFlusher> = hooks.clone();
        let recorder: Rc<dyn StreamBoundaryRecorder> = hooks.clone();
        let mut cx = WebContext::with_loader(loader.clone());
        cx.locals_mut().insert(7usize);
        cx.write_md_text("Prefix").unwrap();
        cx.md_push_heading_offset(1);
        cx.md_push_list_depth();
        cx.md_set_inline(true);
        cx.enter_mode(ContentMode::Script);
        cx.__proa_enter_escape_dialect(EscapeDialect::Xml);
        cx.set_render_mode(RenderMode::ChunkedOutOfOrder);
        cx.set_stream_chunk_flusher(flusher.clone());
        cx.set_stream_recorder(recorder.clone());
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            to_markdown(StateProbe {
                loader: loader.clone(),
                fail: outcome == 1,
                panic: outcome == 2,
            })
            .render_md(&mut cx)
        }));
        match outcome {
            0 => assert!(matches!(result, Ok(Ok(())))),
            1 => assert!(matches!(
                result,
                Ok(Err(WriteError::InvalidAttribute("probe failure")))
            )),
            _ => assert!(result.is_err()),
        }
        assert!(Arc::ptr_eq(cx.loader(), &loader));
        assert_eq!(cx.locals().get::<usize>().as_deref(), Some(&9));
        assert_eq!(cx.current_mode(), ContentMode::Script);
        assert_eq!(cx.__proa_current_escape_dialect(), EscapeDialect::Xml);
        assert_eq!(cx.render_mode(), RenderMode::ChunkedOutOfOrder);
        assert_eq!(cx.md_heading_offset(), 1);
        assert_eq!(cx.md_list_depth(), 1);
        assert!(cx.md_inline());
        assert!(Rc::ptr_eq(cx.stream_chunk_flusher().unwrap(), &flusher));
        assert!(Rc::ptr_eq(cx.stream_recorder().unwrap(), &recorder));
        assert_eq!(hooks.0.get(), 0);
        cx.md_set_inline(false);
        cx.md_block_boundary().unwrap();
        let expected = if outcome == 0 {
            "PrefixCaptured\n\n"
        } else {
            "Prefix\n\n"
        };
        assert_eq!(cx.into_output().into_string(), expected);
    }
}

#[test]
fn invalid_utf8_is_an_error_without_panicking_or_writing_markdown() {
    struct Invalid;
    impl WebRenderSync for Invalid {
        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            cx.out.extend(&[0xff])
        }
    }
    let mut cx = WebContext::new();
    assert!(matches!(
        to_markdown(Invalid).render_md(&mut cx),
        Err(WriteError::Render(_))
    ));
    assert!(cx.out.is_empty());
}

#[test]
fn destination_write_errors_are_preserved() {
    let meter = proa_core::OutputMeter::new(2);
    let mut cx =
        WebContext::with_buffer(proa_core::MeteredWriteBuf::new(Vec::new(), meter.borrow()));
    assert!(matches!(
        to_markdown(Article("Too long")).render_md(&mut cx),
        Err(WriteError::OutputLimitExceeded { .. })
    ));
    assert!(cx.out.is_empty());
}

#[test]
fn a_large_capture_does_not_truncate_at_a_buffer_page_boundary() {
    let body = "content ".repeat(1500);
    let output = markdown(html_sync! {
        <p>{body.as_str()}</p>
    });
    assert_eq!(output.trim_end(), body.trim_end());
    assert!(output.len() > proa_core::DEFAULT_CHAINED_BUMP_PAGE_SIZE);
}

#[test]
fn inline_node_boundaries_cannot_create_markdown_structure() {
    for (prefix, suffix) in [
        ("#", " title"),
        ("-", " item"),
        ("1", ". item"),
        ("1", ") item"),
        ("!", ""),
        ("a ~~", "struck~~"),
    ] {
        let output = markdown(html_sync! {
            <p>
                <span class="first">{prefix}</span>
                <span class="second">{suffix}</span>
            </p>
        });
        assert_eq!(
            text_content(&output),
            format!("{prefix}{suffix}"),
            "{output}"
        );
        assert!(
            Parser::new_ext(&output, Options::ENABLE_STRIKETHROUGH).all(|event| !matches!(
                event,
                Event::Start(Tag::Heading { .. } | Tag::List(_) | Tag::Strikethrough)
            )),
            "{output}"
        );
    }
    let output = markdown(html_sync! {
        <p>
            "!"
            <a href="/page">"Link"</a>
        </p>
    });
    assert!(Parser::new(&output).any(|event| matches!(event, Event::Start(Tag::Link { .. }))));
    assert!(!Parser::new(&output).any(|event| matches!(event, Event::Start(Tag::Image { .. }))));
}

#[test]
fn destinations_and_titles_use_markdown_attribute_escaping() {
    for (url, expected) in [
        ("/a\\(b)", "/a\\(b)"),
        ("/a b<c>", "/a%20b%3Cc%3E"),
        ("<x>", "%3Cx%3E"),
        ("/a\nb", "/a%0Ab"),
        ("/literal?x=&copy;", "/literal?x=&copy;"),
    ] {
        let title = "literal \\\"title\\\" &copy;";
        let output = markdown(html_sync! {
            <a href={url} title={title}>"Link"</a>
            <img src={url} title={title} alt="a [label]" />
        });
        let events: Vec<_> = Parser::new(&output).collect();
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::Html(_) | Event::InlineHtml(_))),
            "{output}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Start(Tag::Link { .. } | Tag::Image { .. })))
                .count(),
            2,
            "{output}"
        );
        for event in events {
            if let Event::Start(
                Tag::Link {
                    dest_url,
                    title: actual,
                    ..
                }
                | Tag::Image {
                    dest_url,
                    title: actual,
                    ..
                },
            ) = event
            {
                assert_eq!(dest_url.as_ref(), expected, "{output}");
                assert_eq!(actual.as_ref(), title, "{output}");
            }
        }
    }
}

#[test]
fn highlighted_code_keeps_boundary_blank_lines_and_safe_language_fences() {
    let code = "\n\nlet value = `literal`;\n\n";
    for language in ["rust", "rust`injected"] {
        let class = format!("language-{language}");
        let output = markdown(html_sync! {
            <pre>
                <code class={class.as_str()}>
                    <span>{code}</span>
                </code>
            </pre>
        });
        let events: Vec<_> = Parser::new(&output).collect();
        assert_eq!(text_content(&output), code, "{output}");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Start(Tag::CodeBlock(_))))
                .count(),
            1,
            "{output}"
        );
    }
}

#[test]
fn link_spacing_and_image_labels_survive_custom_handlers() {
    let alt = "  a \\\"label\\\" &amp;  ";
    let title = "first\n\nsecond\tline";
    let output = markdown(html_sync! {
        <p>
            "Before"
            <a href="/page">" linked "</a>
            "after"
        </p>
        <img src="/image" alt={alt} title={title} />
    });
    assert_eq!(
        text_content(&output),
        format!("Before linked after{alt}"),
        "{output}"
    );
    assert!(
        Parser::new(&output).any(|event| matches!(event,
            Event::Start(Tag::Image { title, .. }) if title.as_ref() == "first  second line"
        )),
        "{output}"
    );
}
