// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::render_to_string;
use proa_macros::{md, md_sync};
fn r(node: impl proa_core::Render) -> String {
    render_to_string(node)
}
#[test]
fn template_static_only() {
    let out = r(md_sync!("# Hello World\n\nSome body.\n"));
    assert_eq!(out, "# Hello World\n\nSome body.\n");
}
#[test]
fn template_leading_newline_stripped() {
    let out = r(md_sync!(
        "
# Title
"
    ));
    assert_eq!(out, "# Title\n");
}

#[test]
fn raw_template_is_the_canonical_document_form() {
    let title = "Markdown rendering";
    let body = "Typed body";
    let out = r(md_sync! { r###"
        # {title}

        {body}
        "###
    });

    assert_eq!(out, "# Markdown rendering\n\nTyped body\n");
}

#[test]
fn raw_template_preserves_braces_in_fenced_code() {
    let title = "Example";
    let out = r(md_sync! { r###"
        # {title}

        ```rust
        struct Card { title: &'static str }

        md_sync! { r#"{literal}"# }
        ```

        After `{inline}`.
        "###
    });

    assert_eq!(
        out,
        "# Example\n\n```rust\nstruct Card { title: &'static str }\n\nmd_sync! { r#\"{literal}\"# }\n```\n\nAfter `{inline}`.\n"
    );
}

#[test]
fn raw_template_preserves_braces_in_tilde_fences() {
    let value = "rendered";
    let out = r(md_sync! { r###"
        Before {value}.

        ~~~rust
        Thing { field: value }
        ~~~

        After {value}.
        "###
    });

    assert_eq!(
        out,
        "Before rendered.\n\n~~~rust\nThing { field: value }\n~~~\n\nAfter rendered.\n"
    );
}

#[test]
fn raw_template_composes_control_flow_without_escaped_newlines() {
    let items = ["alpha", "beta"];
    let out = r(md_sync! { r###"
        ## Items

        {for item in items {
        - {item}
        }}
        "###
    });

    assert_eq!(out, "## Items\n\n- alpha\n- beta\n\n");
}
#[test]
fn template_interpolation() {
    let name = "Rust";
    let out = r(md_sync!("Hello {name}!"));
    assert_eq!(out, "Hello Rust!");
}
#[test]
fn template_field_access() {
    struct Data {
        title: &'static str,
        count: u32,
    }
    let data = Data {
        title: "Page",
        count: 42,
    };
    let out = r(md_sync!("# {data.title} ({data.count})"));
    assert_eq!(out, "# Page (42)");
}
#[test]
fn template_for_loop() {
    let items = ["alpha", "beta", "gamma"];
    let out = r(md_sync!(
        "
{for item in items.iter() {
- {item}
}}
"
    ));
    assert_eq!(out, "- alpha\n- beta\n- gamma\n\n");
}
#[test]
fn template_if_else() {
    let x = 5;
    let out = r(md_sync!("{if x > 0 {positive} else {negative}}"));
    assert_eq!(out, "positive");
    let y = -1;
    let out2 = r(md_sync!("{if y > 0 {positive} else {negative}}"));
    assert_eq!(out2, "negative");
}
#[test]
fn template_if_without_else() {
    let show = true;
    let out = r(md_sync!("start{if show { visible}}end"));
    assert_eq!(out, "start visibleend");
}
#[test]
fn template_escaped_braces() {
    let out = r(md_sync!("Use {{expr}} for interpolation"));
    assert_eq!(out, "Use {expr} for interpolation");
}
#[test]
fn template_unicode() {
    let title = "Malibu";
    let out = r(md_sync!("# {title} — Beach House · 5 reviews"));
    assert_eq!(out, "# Malibu — Beach House · 5 reviews");
}
#[test]
fn template_nested_for() {
    let groups: Vec<(&str, Vec<&str>)> = vec![
        ("Fruits", vec!["apple", "banana"]),
        ("Vegs", vec!["carrot"]),
    ];
    let out = r(md_sync!(
        "
{for (name, items) in groups.iter() {
## {name}
{for item in items.iter() {
- {item}
}}}}
"
    ));
    assert_eq!(out, "## Fruits\n- apple\n- banana\n## Vegs\n- carrot\n\n");
}
#[test]
fn template_dedents_layout_indentation() {
    struct Product {
        title: &'static str,
        description: &'static str,
    }
    let product = Product {
        title: "Trail Runner",
        description: "Light and fast",
    };
    // Template aligned with the surrounding code, closing quote on its own line.
    let out = r(md_sync! { "
        ## {product.title}

        {product.description}
        "
    });
    assert_eq!(out, "## Trail Runner\n\nLight and fast\n");
}

#[test]
fn template_dedent_matches_flush_left_output() {
    let title = "Report";
    let aligned = r(md_sync! { "
        # {title}

        Body.
        "
    });
    let flush = r(md_sync!(
        "
# {title}

Body.
"
    ));
    assert_eq!(aligned, flush);
}

#[test]
fn template_dedent_preserves_relative_indentation() {
    let items = ["alpha", "beta"];
    let out = r(md_sync! { "
        # Nested

        - top
          - {items[0]}
          - {items[1]}
        "
    });
    assert_eq!(out, "# Nested\n\n- top\n  - alpha\n  - beta\n");
}

#[test]
fn template_dedent_keeps_deeper_indentation_as_content() {
    // Only the common prefix is layout; the extra four spaces stay an
    // indented code block.
    let out = r(md_sync! { "
        Paragraph.

            code_line();
        "
    });
    assert_eq!(out, "Paragraph.\n\n    code_line();\n");
}

#[test]
fn template_dedent_applies_inside_control_flow() {
    let items = ["alpha", "beta"];
    let out = r(md_sync! { "
        # Items

        {for item in items.iter() {
        - {item}
        }}
        "
    });
    // The newline after `}}` is part of the template, as in `template_nested_for`.
    assert_eq!(out, "# Items\n\n- alpha\n- beta\n\n");
}

#[test]
fn template_dedent_ignores_blank_line_whitespace() {
    let out = r(md_sync! { "
            # Title
        \u{20}
            Body.
        "
    });
    assert_eq!(out, "# Title\n\nBody.\n");
}

#[test]
fn template_single_line_keeps_leading_whitespace() {
    // No opening newline means no layout to strip.
    let out = r(md_sync!("    indented content"));
    assert_eq!(out, "    indented content");
}

#[test]
fn template_mixed_tabs_and_spaces_are_left_alone() {
    let out = r(md_sync!(
        "
\t# Tabbed
    # Spaced
"
    ));
    assert_eq!(out, "\t# Tabbed\n    # Spaced\n");
}

#[test]
fn braced_component_mode_basic() {
    let name = "World";
    let out = r(md_sync! {
        "# Hello " { name } "\n"
    });
    assert_eq!(out, "# Hello World\n");
}

#[test]
fn braced_source_lines_insert_block_boundaries() {
    let title = "Hello";
    let body = "World";
    let out = r(md_sync! {
        "# " { title }
        { body }
    });

    assert_eq!(out, "# Hello\n\nWorld");
}

#[test]
fn braced_source_lines_complete_existing_line_breaks() {
    let out = r(md_sync! {
        "First\n"
        "Second"
    });

    assert_eq!(out, "First\n\nSecond");
}

#[test]
fn multiline_raw_strings_dedent_source_indentation() {
    let out = r(md_sync! {
        r"---
        title: Markdown rendering
        description: Render typed Rust values to Markdown.
        ---"
    });

    assert_eq!(
        out,
        "---\ntitle: Markdown rendering\ndescription: Render typed Rust values to Markdown.\n---"
    );
}

#[test]
fn multiline_raw_strings_preserve_relative_indentation() {
    let out = r(md_sync! {
        r"## Features

        - Fast
          - No runtime parsing
        - Typed"
    });

    assert_eq!(
        out,
        "## Features\n\n- Fast\n  - No runtime parsing\n- Typed"
    );
}

#[test]
fn braced_multiline_raw_strings_dedent_before_composed_children() {
    let body = "Body";
    let out = r(md_sync! {
        r"---
        title: Markdown rendering
        ---"
        { body }
    });

    assert_eq!(out, "---\ntitle: Markdown rendering\n---\n\nBody");
}

#[test]
fn escaped_newlines_preserve_intentional_indentation() {
    let out = r(md_sync! { "Paragraph\n    indented code" });

    assert_eq!(out, "Paragraph\n    indented code");
}

#[test]
fn braced_source_lines_do_not_break_inline_contexts() {
    use proa_core::WebContext;
    use proa_core::{MdRenderSync, WriteBuf};

    struct InlinePair;

    impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for InlinePair {
        fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), proa_core::WriteError> {
            let previous_inline = cx.md_set_inline(true);
            let result = md_sync! {
                "Before"
                "After"
            }
            .render_md(cx);
            cx.md_set_inline(previous_inline);
            result
        }
    }

    let mut buf = Vec::new();
    InlinePair
        .render_md(&mut WebContext::with_buffer(&mut buf))
        .unwrap();
    assert_eq!(String::from_utf8(buf).unwrap(), "BeforeAfter");
}

struct DelayedMd(&'static str);

impl<Loader: ::proa_core::DataLoader, B: proa_core::WriteBuf> proa_core::MdRender<Loader, B>
    for DelayedMd
{
    fn render_md(
        self,
        cx: &mut proa_core::WebContext<Loader, B>,
    ) -> proa_core::RenderOutcome<
        impl core::future::Future<Output = Result<(), proa_core::WriteError>>,
    > {
        proa_core::RenderOutcome::Pending(async move {
            tokio::task::yield_now().await;
            cx.write_md_text(self.0)
        })
    }
}

struct SyncOnlyMd(&'static str);

impl<Loader: ::proa_core::DataLoader, B: proa_core::WriteBuf> proa_core::MdRenderSync<Loader, B>
    for SyncOnlyMd
{
    fn render_md(
        self,
        cx: &mut proa_core::WebContext<Loader, B>,
    ) -> Result<(), proa_core::WriteError> {
        cx.write_md_text(self.0)
    }
}

#[tokio::test]
async fn async_md_composes_async_and_immediate_children() {
    let count = 3u32;
    let out = proa_core::render_md_to_string_async(md! { r###"
        Result: {DelayedMd("ready")} ({count})
        "###
    })
    .await;

    assert_eq!(out, "Result: ready (3)\n");
}

#[tokio::test]
async fn async_md_accepts_explicitly_lifted_sync_children() {
    let out = proa_core::render_md_to_string_async(md! {
        "Wrapped: " { proa_core::to_md_async(SyncOnlyMd("sync")) }
    })
    .await;

    assert_eq!(out, "Wrapped: sync");
}

#[tokio::test]
async fn async_md_supports_template_control_flow() {
    let items = ["alpha".to_owned(), "beta".to_owned()];
    let show_footer = true;
    let out = proa_core::render_md_to_string_async(md!("
{for item in items.iter() {
- {item}
}}
{if show_footer {Done}}
"))
    .await;

    assert_eq!(out, "- alpha\n- beta\n\nDone\n");
}

#[tokio::test]
async fn async_md_bridges_to_async_web_rendering() {
    let out = proa_core::render_to_string_async(md!("# {DelayedMd(\"Async title\")}\n")).await;

    assert_eq!(out, "# Async title\n");
}

#[tokio::test]
async fn async_md_braced_mode_composes_children() {
    let out = proa_core::render_md_to_string_async(md! { "Before " { DelayedMd("after") } }).await;

    assert_eq!(out, "Before after");
}

#[tokio::test]
async fn async_md_braced_source_lines_insert_block_boundaries() {
    let out = proa_core::render_md_to_string_async(md! {
        "Before"
        { DelayedMd("after") }
    })
    .await;

    assert_eq!(out, "Before\n\nafter");
}

#[test]
fn md_renders_via_web_render() {
    use proa_core::{WebContext, WebRenderSync, WriteBuf};
    struct Page {
        title: &'static str,
    }
    impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Page {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
            md_sync! { "# " { self.title } "\n" }.render(cx)
        }
    }
    let out = render_to_string(Page { title: "Hello" });
    assert_eq!(out, "# Hello\n");
}
#[test]
fn dual_trait_component() {
    use proa_core::render::MdRenderSync;
    use proa_core::{WebContext, WebRenderSync, WriteBuf};
    use proa_macros::html_sync;
    struct Card {
        title: &'static str,
        body: &'static str,
    }
    impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Card {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
            html_sync! {
                <div data-card>
                    <h2>{self.title}</h2>
                    <p>{self.body}</p>
                </div>
            }
            .render(cx)
        }
    }
    impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Card {
        fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), proa_core::WriteError> {
            md_sync! {
                "## " { self.title }
                { self.body } "\n"
            }
            .render_md(cx)?;
            Ok(())
        }
    }
    let html_out = render_to_string(Card {
        title: "Hello",
        body: "World",
    });
    assert!(html_out.contains("<h2>"));
    assert!(html_out.contains("Hello"));
    let mut buf = Vec::new();
    {
        let mut cx = WebContext::with_buffer(&mut buf);
        Card {
            title: "Hello",
            body: "World",
        }
        .render_md(&mut cx)
        .unwrap();
    }
    let md_out = String::from_utf8(buf).unwrap();
    assert_eq!(md_out, "## Hello\n\nWorld\n");
}
#[test]
fn md_composes_md_render_children() {
    use proa_core::render::MdRenderSync;
    use proa_core::{WebContext, WebRenderSync, WriteBuf};
    use proa_macros::html_sync;
    struct Card {
        title: &'static str,
        body: &'static str,
    }
    impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Card {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
            html_sync! {
                <div data-card>
                    <h2>{self.title}</h2>
                    <p>{self.body}</p>
                </div>
            }
            .render(cx)
        }
    }
    impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Card {
        fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), proa_core::WriteError> {
            md_sync! {
                "## " { self.title }
                { self.body } "\n"
            }
            .render_md(cx)?;
            Ok(())
        }
    }
    struct Page {
        featured: Card,
        nearby: Vec<Card>,
    }
    impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Page {
        fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), proa_core::WriteError> {
            md_sync! {
                "# Featured"
                { self.featured }
                "# Nearby"
                { for card in self.nearby {
                    card
                }}
            }
            .render_md(cx)?;
            Ok(())
        }
    }
    let page = Page {
        featured: Card {
            title: "Beach House",
            body: "Ocean view",
        },
        nearby: vec![
            Card {
                title: "Mountain Cabin",
                body: "Forest trails",
            },
            Card {
                title: "City Loft",
                body: "Downtown",
            },
        ],
    };
    let mut buf = Vec::new();
    {
        let mut cx = WebContext::with_buffer(&mut buf);
        page.render_md(&mut cx).unwrap();
    }
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("# Featured"), "out: {out}");
    assert!(out.contains("## Beach House"), "out: {out}");
    assert!(out.contains("Ocean view"), "out: {out}");
    assert!(out.contains("# Nearby"), "out: {out}");
    assert!(out.contains("## Mountain Cabin"), "out: {out}");
    assert!(out.contains("## City Loft"), "out: {out}");
    assert!(out.contains("Downtown"), "out: {out}");
}

#[test]
fn markdown_bridges_typed_html_with_normal_escaping() {
    use proa_core::WebContext;
    use proa_core::{MdRenderSync, WriteBuf};
    use proa_macros::html_sync;

    struct Callout<'a> {
        label: &'a str,
    }

    impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Callout<'_> {
        fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), proa_core::WriteError> {
            md_sync! { "Before\n\n" }.render_md(cx)?;
            cx.render_html_in_md(html_sync! {
                <strong>{self.label}</strong>
            })?;
            md_sync! { "\n\nAfter" }.render_md(cx)
        }
    }

    let mut buf = Vec::new();
    Callout { label: "<typed>" }
        .render_md(&mut WebContext::with_buffer(&mut buf))
        .unwrap();

    assert_eq!(
        String::from_utf8(buf).unwrap(),
        "Before\n\n<strong>&lt;typed&gt;</strong>\nAfter"
    );
}

#[test]
fn md_render_with_chained_bump_writer() {
    use proa_core::render::MdRenderSync;
    use proa_core::WebContext;
    use proa_core::{ChainedBumpWriter, WriteBuf};
    struct Article {
        title: &'static str,
        body: &'static str,
    }
    impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Article {
        fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), proa_core::WriteError> {
            md_sync! {
                "# " { self.title }
                { self.body } "\n"
            }
            .render_md(cx)?;
            Ok(())
        }
    }
    let mut buf = ChainedBumpWriter::<16>::new();
    {
        let mut cx = WebContext::with_buffer(&mut buf);
        Article {
            title: "Hello",
            body: "World",
        }
        .render_md(&mut cx)
        .unwrap();
    }
    let out = String::from_utf8(buf.to_vec()).unwrap();
    assert_eq!(out, "# Hello\n\nWorld\n");
    assert!(buf.page_count() >= 1);
}
#[test]
fn md_render_chained_bump_large_output() {
    use proa_core::render::MdRenderSync;
    use proa_core::WebContext;
    use proa_core::{ChainedBumpWriter, WriteBuf};
    struct Listing {
        items: Vec<&'static str>,
    }
    impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Listing {
        fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), proa_core::WriteError> {
            md_sync! {
                "# Items"
                { for item in self.items.iter() {
                    md_sync! { "- " { item } "\n" }
                }}
            }
            .render_md(cx)?;
            Ok(())
        }
    }
    let listing = Listing {
        items: vec!["alpha", "beta", "gamma", "delta", "epsilon"],
    };
    let mut buf = ChainedBumpWriter::<32>::new();
    {
        let mut cx = WebContext::with_buffer(&mut buf);
        listing.render_md(&mut cx).unwrap();
    }
    let out = String::from_utf8(buf.to_vec()).unwrap();
    assert_eq!(
        out,
        "# Items\n\n- alpha\n- beta\n- gamma\n- delta\n- epsilon\n"
    );
    assert!(
        buf.page_count() >= 2,
        "expected multi-page chain, got {} pages",
        buf.page_count()
    );
}

#[test]
fn template_named_args_substitute_field_access() {
    struct Stats {
        min: u32,
        max: u32,
    }
    let stats = Stats { min: 12, max: 95 };
    let count = 40u32;
    let out = r(md_sync! { r###"
        | Points | Min (ms) | Max (ms) |
        | --- | --- | --- |
        | {count} | {min} | {max} |
        "###,
        min = stats.min,
        max = stats.max,
    });
    assert_eq!(
        out,
        "| Points | Min (ms) | Max (ms) |\n| --- | --- | --- |\n| 40 | 12 | 95 |\n"
    );
}

#[test]
fn template_named_arg_used_twice_splices_twice() {
    let out = r(md_sync! { r###"
        {value} and {value}
        "###,
        value = 2 + 3
    });
    assert_eq!(out, "5 and 5\n");
}

#[test]
fn template_named_arg_shadows_like_named_local() {
    let value = "local";
    let _ = value;
    let out = r(md_sync! { r###"
        {value}
        "###,
        value = "named"
    });
    assert_eq!(out, "named\n");
}

#[tokio::test]
async fn async_template_supports_named_args() {
    struct Stats {
        median: u32,
    }
    let stats = Stats { median: 7 };
    let out = proa_core::render_md_to_string_async(md! { r###"
        Median: {median}
        "###,
        median = stats.median,
    })
    .await;
    assert_eq!(out, "Median: 7\n");
}

#[test]
fn template_spaced_placeholder_renders_like_bare() {
    let title = "Spaced";
    let out = r(md_sync! { r###"
        # { title }
        "###
    });
    assert_eq!(out, "# Spaced\n");
}
