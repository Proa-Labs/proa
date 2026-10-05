// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![cfg(feature = "html-to-markdown")]

use proa_core::{to_markdown, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;
use proptest::prelude::*;
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::collections::BTreeSet;

struct Html<'a>(&'a str);
impl WebRenderSync for Html<'_> {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.out.extend(self.0.as_bytes())
    }
}
fn markdown<T: WebRenderSync>(node: T) -> String {
    let mut cx = WebContext::new();
    to_markdown(node).render_md(&mut cx).unwrap();
    cx.into_output().into_string()
}
fn events(source: &str) -> Vec<Event<'_>> {
    Parser::new_ext(
        source,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
    .collect()
}
fn text(source: &str) -> String {
    events(source)
        .into_iter()
        .filter_map(|event| match event {
            Event::Text(value) | Event::Code(value) => Some(value.into_string()),
            _ => None,
        })
        .collect()
}
fn no_html(source: &str) {
    assert!(
        !events(source)
            .iter()
            .any(|event| matches!(event, Event::Html(_) | Event::InlineHtml(_))),
        "{source}"
    );
}
fn table_cells(source: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    for event in events(source) {
        match event {
            Event::Start(Tag::TableHead | Tag::TableRow) => row.clear(),
            Event::Start(Tag::TableCell) => cell.clear(),
            Event::Text(value) | Event::Code(value) => cell.push_str(&value),
            Event::End(pulldown_cmark::TagEnd::TableCell) => row.push(cell.clone()),
            Event::End(pulldown_cmark::TagEnd::TableHead | pulldown_cmark::TagEnd::TableRow) => {
                rows.push(row.clone())
            }
            _ => {}
        }
    }
    rows
}

#[test]
fn tables_preserve_mixed_cells_footers_captions_and_ragged_rows() {
    let output = markdown(Html("<table><caption>Release status</caption><thead><tr><th>Name</th><td>Status</td></tr></thead><tbody><tr><th scope='row'>API</th><td>Ready</td><td>Extra</td></tr><tr><td>Worker</td></tr></tbody><tfoot><tr><th>Total</th><td>2</td></tr></tfoot></table>"));
    assert_eq!(
        table_cells(&output),
        [
            vec!["Name", "Status", ""],
            vec!["API", "Ready", "Extra"],
            vec!["Worker", "", ""],
            vec!["Total", "2", ""]
        ]
    );
    assert!(output.starts_with("Release status\n\n"), "{output}");
    no_html(&output);
}

#[test]
fn headerless_and_empty_tables_have_valid_gfm_shapes() {
    for (source, expected) in [
        ("<table><tr><td>A</td><td>B</td></tr><tr><td>C</td><td>D</td></tr></table>", vec![vec!["", ""], vec!["A", "B"], vec!["C", "D"]]),
        ("<table><thead><tr><th></th><th></th></tr></thead><tbody><tr><td></td><td></td></tr></tbody></table>", vec![vec!["", ""], vec!["", ""]]),
        ("<table><tr><th>Only header</th></tr></table>", vec![vec!["Only header"]]),
        ("<table><tr><th scope='row'>Row</th><td>Value</td></tr></table>", vec![vec!["", ""], vec!["Row", "Value"]]),
        ("<table><tr><th scope='ROWGROUP'>Row</th><td>Value</td></tr></table>", vec![vec!["", ""], vec!["Row", "Value"]]),
    ] {
        let output = markdown(Html(source));
        assert_eq!(table_cells(&output), expected, "{output}");
        no_html(&output);
    }
    assert_eq!(markdown(Html("<table></table>")), "");
}

#[test]
fn table_pipes_backslashes_entities_links_images_and_code_round_trip() {
    let value = "a|b \\| c &copy; `ticks`";
    let url = "/a|b?literal=&copy;";
    let output = markdown(html_sync! {
        <table>
            <tr>
                <th>"Text"</th>
                <th>"Code"</th>
                <th>"Link"</th>
                <th>"Image"</th>
            </tr>
            <tr>
                <td>{value}</td>
                <td>
                    <code>{value}</code>
                </td>
                <td>
                    <a href={url}>{value}</a>
                </td>
                <td>
                    <img src={url} alt={value} />
                </td>
            </tr>
        </table>
    });
    assert_eq!(table_cells(&output)[1], vec![value; 4], "{output}");
    for event in events(&output) {
        if let Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) = event {
            assert_eq!(dest_url.as_ref(), url, "{output}");
        }
    }
    no_html(&output);
}

#[test]
fn complex_tables_keep_all_cells_and_block_structure_in_linear_fallback() {
    for source in [
        "<table><thead><tr><th colspan='2'>Group</th></tr><tr><th>A</th><th>B</th></tr></thead><tr><td>1</td><td>2</td></tr></table>",
        "<table><thead><tr><th>Group</th><th>A</th></tr><tr><th>B</th><th>1</th></tr></thead><tr><td>2</td></tr></table>",
        "<table><tr><th>A</th><th>B</th></tr><tr><td colspan='2'>Group</td></tr><tr><td>1</td><td>2</td></tr></table>",
        "<table><tr><th>A</th><th>B</th></tr><tr><td rowspan='2'>Group</td><td>1</td></tr><tr><td>2</td></tr></table>",
        "<table><tr><th>A</th><th>B</th></tr><tr><td rowspan='0'>Group</td><td>1</td></tr><tr><td>2</td></tr></table>",
    ] {
        let output = markdown(Html(source));
        for token in ["Group", "A", "B", "1", "2"] { assert!(text(&output).contains(token), "{output}"); }
        assert!(!events(&output).iter().any(|event| matches!(event, Event::Start(Tag::Table(_)))), "{output}");
        assert!(events(&output).iter().any(|event| matches!(event, Event::Start(Tag::List(Some(1))))), "{output}");
        no_html(&output);
    }
    let output = markdown(Html("<table><tr><th>Header</th></tr><tr><td><p>Paragraph</p><ul><li>Item</li></ul><pre><code>line1\nline2\n</code></pre></td></tr></table>"));
    assert_eq!(text(&output), "HeaderParagraphItemline1\nline2\n");
    assert!(events(&output)
        .iter()
        .any(|event| matches!(event, Event::Start(Tag::CodeBlock(_)))));
    no_html(&output);
}

#[test]
fn complex_cells_preserve_breaks_custom_elements_and_unit_spans_stay_tabular() {
    for (content, expected) in [
        ("First<br>Second", "FirstSecond"),
        ("<x-card>Custom</x-card>", "Custom"),
        (
            "<hgroup><h2>Heading</h2><p>Body</p></hgroup>",
            "HeadingBody",
        ),
    ] {
        let source = format!("<table><tr><th>H</th></tr><tr><td>{content}</td></tr></table>");
        let output = markdown(Html(&source));
        assert_eq!(text(&output), format!("H{expected}"), "{output}");
        assert!(
            !events(&output)
                .iter()
                .any(|event| matches!(event, Event::Start(Tag::Table(_)))),
            "{output}"
        );
        if content.contains("<br>") {
            assert!(
                events(&output)
                    .iter()
                    .any(|event| matches!(event, Event::HardBreak)),
                "{output}"
            );
        }
        no_html(&output);
    }
    let output = markdown(Html(
        "<table><tr><th>H</th></tr><tr><td rowspan='1' colspan='1'>Value</td></tr></table>",
    ));
    assert_eq!(table_cells(&output), [vec!["H"], vec!["Value"]]);
    assert_eq!(
        markdown(Html("<table><caption>Only caption</caption></table>")),
        "Only caption"
    );
}

#[test]
fn large_tables_and_visibility_flags_do_not_drop_visible_rows() {
    let mut source = String::from("<table><tr><th>Index</th><th>Value</th></tr>");
    for row in 0..500 {
        source.push_str(&format!("<tr><td>{row}</td><td>Value{row}</td></tr>"));
    }
    source.push_str("</table>");
    let output = markdown(Html(&source));
    let cells = table_cells(&output);
    assert_eq!(cells.len(), 501);
    assert_eq!(cells[500], ["499", "Value499"]);
    assert!(output.len() > proa_core::DEFAULT_CHAINED_BUMP_PAGE_SIZE);
    let mut source = String::from("<table>");
    let mut expected = String::new();
    for row in 0..150 {
        source.push_str(&format!("<tr><td><p>Value{row}</p></td></tr>"));
        expected.push_str(&format!("Value{row}"));
    }
    source.push_str("</table>");
    let output = markdown(Html(&source));
    assert_eq!(text(&output), expected, "{output}");
    assert_eq!(
        events(&output)
            .iter()
            .filter(|event| matches!(event, Event::Start(Tag::List(Some(_)))))
            .count(),
        1,
        "{output}"
    );
    assert_eq!(
        events(&output)
            .iter()
            .filter(|event| matches!(event, Event::Start(Tag::List(None))))
            .count(),
        150,
        "{output}"
    );
    let output = markdown(Html("<p>Before<span hidden='false'>Hidden</span><span aria-hidden='false'>Visible</span><span aria-hidden='TRUE'>Hidden</span>After</p>"));
    assert_eq!(text(&output), "BeforeVisibleAfter");
    no_html(&output);
}

#[test]
fn nested_tables_and_tables_in_lists_or_quotes_preserve_context() {
    let output = markdown(Html("<table><tr><th>Outer</th></tr><tr><td><table><tr><th>Inner</th></tr><tr><td>Value</td></tr></table></td></tr></table>"));
    assert_eq!(text(&output), "OuterInnerValue");
    assert_eq!(
        events(&output)
            .iter()
            .filter(|event| matches!(event, Event::Start(Tag::Table(_))))
            .count(),
        1
    );
    for wrapper in ["blockquote", "li"] {
        let source = if wrapper == "li" {
            "<ul><li><table><tr><th>H</th></tr><tr><td>Value</td></tr></table></li></ul>"
        } else {
            "<blockquote><table><tr><th>H</th></tr><tr><td>Value</td></tr></table></blockquote>"
        };
        let output = markdown(Html(source));
        assert_eq!(table_cells(&output), [vec!["H"], vec!["Value"]], "{output}");
        no_html(&output);
    }
}

#[test]
fn malformed_html_is_repaired_and_hidden_table_content_is_excluded() {
    let output = markdown(Html(
        "<table><tr><th>Header<tr><td>Visible<td hidden>Secret</table><p>After",
    ));
    assert_eq!(text(&output), "HeaderVisibleAfter");
    assert_eq!(table_cells(&output), [vec!["Header"], vec!["Visible"]]);
    no_html(&output);
    let output = markdown(Html("<!-- comment --><!DOCTYPE html><p>Before<script>secret</script><style>secret</style><template>secret</template><svg><text>secret</text></svg><span aria-hidden='TRUE'>secret</span>After</p>"));
    assert_eq!(text(&output), "BeforeAfter");
    no_html(&output);
}

#[test]
fn noscript_subtrees_do_not_leak_text_or_escaped_html() {
    for source in [
        "<p>Hi</p><noscript><img src=\"/p.gif\"></noscript>",
        "<p>Hi</p><noscript>Fallback text</noscript>",
        "<p>Hi<noscript><strong>Fallback</strong></noscript> there</p>",
    ] {
        let output = markdown(Html(source));
        let expected = if source.contains(" there") {
            "Hi there"
        } else {
            "Hi"
        };
        assert_eq!(output, expected, "{source}");
        no_html(&output);
    }
}

#[test]
fn metadata_is_omitted_even_when_parsed_inside_the_body() {
    for metadata in [
        "<title>Page title</title>",
        "<meta name='description' content='Secret'>",
        "<link rel='stylesheet' href='/secret.css'>",
        "<base href='/secret/'>",
    ] {
        let source = format!("<p>Body</p>{metadata}<p>After</p>");
        let output = markdown(Html(&source));
        assert_eq!(output, "Body\n\nAfter", "{source}");
        no_html(&output);
    }
}

#[test]
fn every_builtin_html_and_svg_tag_has_an_explicit_conversion_fixture() {
    let mut covered = BTreeSet::new();
    let inline = [
        "abbr",
        "b",
        "bdi",
        "bdo",
        "cite",
        "data",
        "dfn",
        "em",
        "i",
        "kbd",
        "mark",
        "q",
        "rp",
        "rt",
        "ruby",
        "s",
        "samp",
        "small",
        "span",
        "strong",
        "sub",
        "sup",
        "time",
        "u",
        "var",
        "del",
        "ins",
        "label",
        "button",
        "output",
        "progress",
        "meter",
        "slot",
        "selectedcontent",
        "code",
        "audio",
        "video",
        "canvas",
        "map",
        "picture",
        "object",
        "iframe",
    ];
    let block = [
        "html",
        "body",
        "header",
        "footer",
        "nav",
        "main",
        "section",
        "article",
        "aside",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hgroup",
        "address",
        "div",
        "p",
        "pre",
        "blockquote",
        "figure",
        "figcaption",
        "form",
        "fieldset",
        "legend",
        "details",
        "summary",
        "dialog",
        "search",
        "textarea",
        "option",
        "optgroup",
        "datalist",
        "select",
    ];
    for tag in inline.into_iter().chain(block) {
        let value = format!("Token{tag}");
        let source = if tag == "option" {
            format!("<select><option>{value}</option></select>")
        } else if tag == "optgroup" {
            format!("<select><optgroup><option>{value}</option></optgroup></select>")
        } else if matches!(tag, "select" | "datalist") {
            format!("<{tag}><option>{value}</option></{tag}>")
        } else {
            format!("<{tag}>{value}</{tag}>")
        };
        let output = markdown(Html(&source));
        assert!(text(&output).starts_with(&value), "<{tag}>: {output}");
        no_html(&output);
        covered.insert(tag.to_owned());
    }
    for tag in ["ul", "ol", "menu"] {
        let source = format!("<{tag}><li>Token</li></{tag}>");
        assert_eq!(text(&markdown(Html(&source))), "Token");
        covered.insert(tag.to_owned());
        covered.insert("li".to_owned());
    }
    let output = markdown(Html("<dl><dt>Term</dt><dd>Definition</dd></dl>"));
    assert_eq!(text(&output), "TermDefinition");
    covered.extend(["dl", "dt", "dd"].map(str::to_owned));
    let output = markdown(Html(
        "<a href='/page'>Label</a><img src='/image' alt='Diagram'><br><hr>",
    ));
    assert_eq!(text(&output), "LabelDiagram");
    covered.extend(["a", "img", "br", "hr"].map(str::to_owned));
    let output = markdown(Html("<table><caption>Caption</caption><colgroup><col></colgroup><thead><tr><th>Header</th></tr></thead><tbody><tr><td>Body</td></tr></tbody><tfoot><tr><td>Footer</td></tr></tfoot></table>"));
    assert_eq!(text(&output), "CaptionHeaderBodyFooter");
    covered.extend(
        [
            "table", "caption", "colgroup", "col", "thead", "tbody", "tfoot", "tr", "th", "td",
        ]
        .map(str::to_owned),
    );
    for tag in ["input", "area", "embed", "param", "source", "track", "wbr"] {
        let source = format!("<p>Before<{tag}>After</p>");
        assert_eq!(text(&markdown(Html(&source))), "BeforeAfter", "{source}");
        covered.insert(tag.to_owned());
    }
    let output = markdown(Html("<html><head><title>Secret</title><meta name='x' content='secret'><link href='/secret'><base href='/secret'><style>secret</style></head><body>Visible<script>secret</script><noscript>secret</noscript><template><p>secret</p></template></body></html>"));
    assert_eq!(text(&output), "Visible");
    covered.extend(
        [
            "head", "title", "meta", "link", "base", "style", "script", "noscript", "template",
        ]
        .map(str::to_owned),
    );
    // Gate the fixtures against the native tag catalog, including SVG children
    // in their valid SVG context rather than as HTML lookalikes.
    let catalog = include_str!("../src/shared/tag.rs")
        .split("tag_static_bytes! {")
        .nth(1)
        .unwrap();
    let names: Vec<_> = catalog
        .lines()
        .filter_map(|line| line.split('"').nth(1))
        .collect();
    let svg = names.iter().position(|tag| *tag == "svg").unwrap();
    for tag in &names[svg..] {
        if *tag == "style" {
            continue;
        }
        let source = format!("<p>Before</p><svg><{tag}>Secret</{tag}></svg><p>After</p>");
        assert_eq!(text(&markdown(Html(&source))), "BeforeAfter", "{source}");
        covered.insert((*tag).to_owned());
    }
    for tag in names {
        assert!(covered.contains(tag), "missing explicit <{tag}> fixture");
    }
}

#[test]
fn custom_elements_math_and_form_attribute_limits_are_explicit() {
    assert_eq!(
        text(&markdown(Html(
            "<x-card><p>Custom</p></x-card><math><mi>x</mi><mo>+</mo><mn>1</mn></math>"
        ))),
        "Customx+1"
    );
    let output = markdown(Html("<form><label>Name<input value='Alice'></label><select><option>First</option><option selected>Second</option></select><button>Save</button></form>"));
    assert_eq!(text(&output), "NameFirstSecondSave");
    no_html(&output);
}

#[test]
fn inline_code_preserves_styled_text_edge_spaces_and_empty_spans() {
    let output = markdown(Html("<p>Before<code></code>After</p>"));
    assert_eq!(text(&output), "BeforeAfter");
    for value in [" ", "  padded  ", "a\nb", "`ticks`", "&copy; <tag>"] {
        let output = markdown(html_sync! {
            <code>
                <strong>{value}</strong>
            </code>
        });
        assert_eq!(text(&output), value.replace('\n', " "), "{output}");
        assert!(
            events(&output)
                .iter()
                .any(|event| matches!(event, Event::Code(_))),
            "{output}"
        );
        no_html(&output);
    }
}

#[test]
fn images_without_src_keep_alt_text_and_unsafe_urls_are_neutralized() {
    let output = markdown(Html("<img srcset='/small 1x, /large 2x' alt='Literal [label] &amp;copy;'><a>No destination</a><a href='javascript:alert(1)'>Unsafe link</a><img src='data:text/html,secret' alt='Unsafe image'>"));
    assert_eq!(
        text(&output),
        "Literal [label] &copy;No destinationUnsafe linkUnsafe image"
    );
    for event in events(&output) {
        if let Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) = event {
            assert_eq!(dest_url.as_ref(), "", "{output}");
        }
    }
    no_html(&output);
}

#[test]
fn line_breaks_horizontal_rules_and_nested_emphasis_keep_structure() {
    let output = markdown(Html("<p>First<br>Second</p><hr><p><strong>Bold <em>and italic</em></strong></p><blockquote><blockquote><p>Nested</p></blockquote></blockquote>"));
    let events = events(&output);
    assert!(events.iter().any(|event| matches!(event, Event::HardBreak)));
    assert!(events.iter().any(|event| matches!(event, Event::Rule)));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::Start(Tag::Strong))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::Start(Tag::Emphasis))));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Start(Tag::BlockQuote(_))))
            .count(),
        2
    );
    assert_eq!(text(&output), "FirstSecondBold and italicNested");
    no_html(&output);
}

#[test]
fn ordered_lists_keep_zero_negative_reversed_and_per_item_ordinals() {
    let output = markdown(Html("<ol start='0'><li>Zero</li><li>One</li></ol>"));
    assert!(
        events(&output)
            .iter()
            .any(|event| matches!(event, Event::Start(Tag::List(Some(0))))),
        "{output}"
    );
    for (source, expected) in [
        (
            "<ol reversed><li>Third</li><li>Second</li><li>First</li></ol>",
            "3: Third2: Second1: First",
        ),
        (
            "<ol start='-2'><li>Negative</li><li>Next</li></ol>",
            "-2: Negative-1: Next",
        ),
        (
            "<ol><li value='9'>Nine</li><li>Ten</li><li value='2'>Two</li></ol>",
            "9: Nine10: Ten2: Two",
        ),
        (
            "<ol start='2147483647'><li>Large</li><li>Next</li></ol>",
            "2147483647: Large2147483648: Next",
        ),
    ] {
        let output = markdown(Html(source));
        assert_eq!(text(&output), expected, "{output}");
        assert!(
            events(&output)
                .iter()
                .any(|event| matches!(event, Event::Start(Tag::List(None)))),
            "{output}"
        );
        no_html(&output);
    }
    assert_eq!(markdown(Html("<ol></ol>")), "");
    let output = markdown(Html(
        "<ol start='  +3junk'><li>Item<ul><li>Nested</li></ul></li></ol>",
    ));
    assert!(
        events(&output)
            .iter()
            .any(|event| matches!(event, Event::Start(Tag::List(Some(3))))),
        "{output}"
    );
    assert_eq!(text(&output), "ItemNested");
    let output = markdown(Html("<ol start='10000' reversed><li><p>Parent</p><p>Second paragraph</p><ul><li>Nested</li></ul><pre><code>line1\nline2\n</code></pre></li><li>After</li></ol>"));
    assert_eq!(
        text(&output),
        "10000: ParentSecond paragraphNestedline1\nline2\n9999: After",
        "{output}"
    );
    assert_eq!(
        events(&output)
            .iter()
            .filter(|event| matches!(event, Event::Start(Tag::List(_))))
            .count(),
        2,
        "{output}"
    );
    assert!(
        events(&output).iter().any(|event| matches!(
            event,
            Event::Start(Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(_)))
        )),
        "{output}"
    );
    let output = markdown(Html("<ol><li><em>      Leading</em> text</li></ol>"));
    assert_eq!(text(&output), "Leading text");
    assert!(
        events(&output)
            .iter()
            .any(|event| matches!(event, Event::Start(Tag::Emphasis))),
        "{output}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 1024, failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct("tests/to_markdown_coverage.proptest-regressions"))), ..ProptestConfig::default() })]
    #[test]
    fn arbitrary_literal_unicode_text_cannot_create_html_or_lose_characters(
        chars in prop::collection::vec(any::<char>().prop_filter("non-control, non-whitespace", |ch| !ch.is_control() && !ch.is_whitespace()), 0..100)
    ) {
        let value: String = chars.into_iter().collect();
        let output = markdown(html_sync! { <p>{value.as_str()}</p> });
        prop_assert_eq!(text(&output), value);
        no_html(&output);
    }
    #[test]
    fn rectangular_tables_keep_every_dynamic_cell_and_column(
        columns in 1usize..9, rows in 0usize..9, explicit_header in any::<bool>(),
        value in "[a-zA-Z0-9&;|_*]{0,30}"
    ) {
        let mut source = String::from("<table>");
        let mut expected = vec![vec![String::new(); columns]];
        if explicit_header {
            source.push_str("<tr>");
            for (col, cell) in expected[0].iter_mut().enumerate() { source.push_str(&format!("<th>H{col}</th>")); *cell = format!("H{col}"); }
            source.push_str("</tr>");
        }
        for row in 0..rows {
            source.push_str("<tr>");
            let mut cells = Vec::new();
            for col in 0..columns {
                let cell = format!("R{row}C{col}{value}");
                let escaped = cell.replace('&', "&amp;");
                source.push_str(&format!("<td>{escaped}</td>")); cells.push(cell);
            }
            source.push_str("</tr>"); expected.push(cells);
        }
        source.push_str("</table>");
        if rows == 0 && !explicit_header { expected.clear(); }
        let output = markdown(Html(&source));
        prop_assert_eq!(table_cells(&output), expected);
        no_html(&output);
    }
    #[test]
    fn arbitrary_inline_code_in_tables_keeps_pipes_backslashes_and_backticks(
        chars in prop::collection::vec(proptest::sample::select(vec!['a', '|', '\\', '`', '&', ';', '*', '[', ']', '🦀', ' ']), 0..80)
    ) {
        let value = format!("X{}Y", chars.into_iter().collect::<String>());
        let output = markdown(html_sync! {
            <table><tr><th>"Code"</th></tr><tr><td><code>{value.as_str()}</code></td></tr></table>
        });
        prop_assert_eq!(&table_cells(&output)[1][0], &value);
        no_html(&output);
    }
}
