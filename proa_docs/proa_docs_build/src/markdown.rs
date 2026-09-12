// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::{config::MarkdownPipeline, typescript::TypeTableRequest};

use latex2mathml::{latex_to_mathml, DisplayStyle};

#[derive(Debug, Clone)]
pub struct RenderedMarkdown {
    pub html: String,
    pub toc: Vec<TocEntry>,
    pub hashes: Vec<String>,
    pub search_text: String,
    pub links: Vec<LinkReference>,
    pub type_table_requests: Vec<TypeTableRequest>,
}

#[derive(Debug, Clone)]
pub struct TocEntry {
    pub depth: u8,
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct LinkReference {
    pub href: String,
}

// proa-perf-ok(scope): docs markdown is compiled into trusted static HTML at build time; dynamic text is escaped before insertion
pub fn render_markdown(
    markdown: &str,
    source_path: &Path,
    pipeline: &MarkdownPipeline,
) -> RenderedMarkdown {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    if pipeline.math.is_enabled() {
        options.insert(Options::ENABLE_MATH);
    }

    let parser = Parser::new_ext(markdown, options);
    let mut html = String::new();
    let mut toc = Vec::new();
    let mut hashes = Vec::new();
    let mut search_text = String::new();
    let mut links = Vec::new();
    let mut type_table_requests = Vec::new();
    let mut components = crate::components::ComponentState::default();

    let mut in_heading = false;
    let mut heading_level = HeadingLevel::H1;
    let mut heading_text = String::new();
    let mut heading_ids = Vec::<String>::new();

    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();
    let mut in_table_head = false;

    for event in parser {
        if in_code_block {
            match event {
                Event::Text(text) => code_buf.push_str(&text),
                Event::End(TagEnd::CodeBlock) => {
                    crate::components::CodeBlock::new(&code_buf, &code_lang, source_path, pipeline)
                        .emit_html(&mut html);
                    append_search_text(&mut search_text, &code_buf);
                    code_buf.clear();
                    code_lang.clear();
                    in_code_block = false;
                }
                _ => {}
            }
            continue;
        }

        if in_heading {
            match event {
                Event::Text(text) | Event::Code(text) => {
                    heading_text.push_str(&text);
                }
                Event::SoftBreak | Event::HardBreak => heading_text.push(' '),
                Event::End(TagEnd::Heading(level)) => {
                    let id = unique_slug(&heading_text, &mut heading_ids);
                    let depth = heading_depth(level);
                    hashes.push(id.clone());
                    if depth > 1 {
                        toc.push(TocEntry {
                            depth,
                            id: id.clone(),
                            title: heading_text.clone(),
                        });
                    }
                    append_search_text(&mut search_text, &heading_text);
                    html.push('<');
                    let _ = write!(html, "{tag} id=\"", tag = heading_tag(level));
                    escape_attr(&id, &mut html);
                    html.push_str("\">");
                    escape_text(&heading_text, &mut html);
                    let _ = write!(html, "</{tag}", tag = heading_tag(level));
                    html.push('>');
                    heading_text.clear();
                    in_heading = false;
                }
                _ => {}
            }
            continue;
        }

        match event {
            Event::Start(Tag::Paragraph) => html.push_str("<p>"),
            Event::End(TagEnd::Paragraph) => html.push_str("</p>"),
            Event::Start(Tag::Heading { level, .. }) => {
                in_heading = true;
                heading_level = level;
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                let id = unique_slug(&heading_text, &mut heading_ids);
                let depth = heading_depth(heading_level);
                hashes.push(id.clone());
                if depth > 1 {
                    toc.push(TocEntry {
                        depth,
                        id: id.clone(),
                        title: heading_text.clone(),
                    });
                }
                html.push('<');
                let _ = write!(html, "{tag} id=\"", tag = heading_tag(heading_level));
                escape_attr(&id, &mut html);
                html.push_str("\">");
                escape_text(&heading_text, &mut html);
                let _ = write!(html, "</{tag}", tag = heading_tag(heading_level));
                html.push('>');
                heading_text.clear();
                in_heading = false;
            }
            Event::Start(Tag::BlockQuote(_)) => html.push_str("<blockquote>"),
            Event::End(TagEnd::BlockQuote(_)) => html.push_str("</blockquote>"),
            Event::Start(Tag::CodeBlock(kind)) => {
                if let CodeBlockKind::Fenced(lang) = kind {
                    code_lang = lang.to_string();
                }
                in_code_block = true;
            }
            Event::Start(Tag::List(None)) => html.push_str("<ul>"),
            Event::End(TagEnd::List(false)) => html.push_str("</ul>"),
            Event::Start(Tag::List(Some(_))) => html.push_str("<ol>"),
            Event::End(TagEnd::List(true)) => html.push_str("</ol>"),
            Event::Start(Tag::Item) => html.push_str("<li>"),
            Event::End(TagEnd::Item) => html.push_str("</li>"),
            Event::Start(Tag::Table(_)) => html.push_str("<table>"),
            Event::End(TagEnd::Table) => html.push_str("</table>"),
            Event::Start(Tag::TableHead) => {
                in_table_head = true;
                html.push_str("<thead><tr>");
            }
            Event::End(TagEnd::TableHead) => {
                html.push_str("</tr></thead>");
                in_table_head = false;
            }
            Event::Start(Tag::TableRow) => html.push_str("<tr>"),
            Event::End(TagEnd::TableRow) => html.push_str("</tr>"),
            Event::Start(Tag::TableCell) => {
                if in_table_head {
                    html.push_str("<th>");
                } else {
                    html.push_str("<td>");
                }
            }
            Event::End(TagEnd::TableCell) => {
                if in_table_head {
                    html.push_str("</th>");
                } else {
                    html.push_str("</td>");
                }
            }
            Event::Start(Tag::Emphasis) => html.push_str("<em>"),
            Event::End(TagEnd::Emphasis) => html.push_str("</em>"),
            Event::Start(Tag::Strong) => html.push_str("<strong>"),
            Event::End(TagEnd::Strong) => html.push_str("</strong>"),
            Event::Start(Tag::Strikethrough) => html.push_str("<del>"),
            Event::End(TagEnd::Strikethrough) => html.push_str("</del>"),
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                links.push(LinkReference {
                    href: dest_url.to_string(),
                });
                html.push_str("<a href=\"");
                escape_attr(safe_url(&dest_url), &mut html);
                html.push('"');
                if !title.is_empty() {
                    html.push_str(" title=\"");
                    escape_attr(&title, &mut html);
                    html.push('"');
                }
                html.push('>');
            }
            Event::End(TagEnd::Link) => html.push_str("</a>"),
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                links.push(LinkReference {
                    href: dest_url.to_string(),
                });
                html.push_str("<img src=\"");
                escape_attr(safe_url(&dest_url), &mut html);
                html.push_str("\" alt=\"");
                escape_attr(&title, &mut html);
                html.push_str("\" />");
            }
            Event::End(TagEnd::Image) => {}
            Event::Text(text) => {
                append_search_text(&mut search_text, &text);
                escape_text(&text, &mut html);
            }
            Event::Code(code) => {
                append_search_text(&mut search_text, &code);
                html.push_str("<code>");
                escape_text(&code, &mut html);
                html.push_str("</code>");
            }
            Event::SoftBreak => {
                search_text.push(' ');
                html.push('\n');
            }
            Event::HardBreak => html.push_str("<br />"),
            Event::Rule => html.push_str("<hr />"),
            Event::TaskListMarker(checked) => {
                if checked {
                    html.push_str("<input type=\"checkbox\" checked disabled />");
                } else {
                    html.push_str("<input type=\"checkbox\" disabled />");
                }
            }
            Event::InlineHtml(raw) => {
                if let Some(request) =
                    crate::components::parse_type_table(&raw, source_path, pipeline)
                {
                    type_table_requests.push(request);
                    crate::components::render_type_table_placeholder(&mut html, &raw);
                } else if crate::components::render_tag(&raw, &mut components, &mut html) {
                } else {
                    escape_text(&raw, &mut html);
                }
            }
            Event::Html(raw)
                if !crate::components::render_tag(&raw, &mut components, &mut html) =>
            {
                // Block-level raw HTML is otherwise dropped; container
                // components are the one shape we recognise and render.
                warn_dropped_container_children(&raw, source_path);
            }
            Event::Html(_) => {}
            Event::InlineMath(tex) => {
                append_search_text(&mut search_text, &tex);
                render_math(&mut html, &tex, DisplayStyle::Inline, source_path);
            }
            Event::DisplayMath(tex) => {
                append_search_text(&mut search_text, &tex);
                render_math(&mut html, &tex, DisplayStyle::Block, source_path);
            }
            _ => {}
        }
    }

    if let Some(unclosed) = components.unclosed() {
        println!(
            "cargo:warning=docs components: {}: <{unclosed}> was opened but never closed; the emitted HTML will be unbalanced",
            source_path.display()
        );
    }

    for (src, reason) in components.invalid_demos() {
        println!(
            "cargo:warning=docs components: {}: <Demo src=\"{src}\"> was dropped: {reason}",
            source_path.display()
        );
    }
    for count in components.overflowing_tabs() {
        println!(
            "cargo:warning=docs components: {}: a <Tabs> block declares {count} tabs but the stylesheet only switches between {}; extra panels stay visible",
            source_path.display(),
            crate::components::MAX_STYLED_TABS
        );
    }
    for count in components.overflowing_explorers() {
        println!(
            "cargo:warning=docs components: {}: a <CodeExplorer> lists {count} files but the stylesheet only switches between {}; extra files stay visible",
            source_path.display(),
            crate::components::MAX_STYLED_TABS
        );
    }

    RenderedMarkdown {
        html,
        toc,
        hashes,
        search_text: search_text.trim().to_string(),
        links,
        type_table_requests,
    }
}

// proa-perf-ok(scope): docs math is compiled once into trusted static MathML at build time; fallback output is escaped
fn render_math(html: &mut String, tex: &str, style: DisplayStyle, source_path: &Path) {
    match latex_to_mathml(tex, style) {
        Ok(mathml) => normalize_mathml(&mathml, html),
        Err(error) => {
            println!(
                "cargo:warning=docs math: {}: `{tex}` failed to render ({error}); emitting source text",
                source_path.display()
            );
            if matches!(style, DisplayStyle::Block) {
                html.push_str("<pre><code>");
                escape_text(tex, html);
                html.push_str("</code></pre>");
            } else {
                html.push_str("<code>");
                escape_text(tex, html);
                html.push_str("</code>");
            }
        }
    }
}

/// `latex2mathml` writes operator and text characters into element content
/// without escaping, so `$a < b$` would emit a bare `<` and truncate the
/// article. Every element it emits starts with `m`, which lets a single
/// pass re-escape content characters while leaving tags and numeric
/// character references intact.
fn normalize_mathml(mathml: &str, out: &mut String) {
    let mut chars = mathml.char_indices().peekable();
    let mut in_tag = false;
    while let Some((index, ch)) = chars.next() {
        if in_tag {
            out.push(ch);
            if ch == '>' {
                in_tag = false;
            }
            continue;
        }
        match ch {
            '<' => {
                if matches!(chars.peek(), Some((_, '/' | 'm'))) {
                    in_tag = true;
                    out.push('<');
                } else {
                    out.push_str("&lt;");
                }
            }
            '>' => out.push_str("&gt;"),
            '&' => {
                if is_character_reference(&mathml[index..]) {
                    out.push('&');
                } else {
                    out.push_str("&amp;");
                }
            }
            _ => out.push(ch),
        }
    }
}

fn is_character_reference(rest: &str) -> bool {
    let body = rest.strip_prefix('&').unwrap_or(rest);
    let body = body.strip_prefix('#').unwrap_or(body);
    match body.find(';') {
        Some(end) if end > 0 => body[..end].chars().all(|ch| ch.is_ascii_alphanumeric()),
        _ => false,
    }
}

fn unique_slug(text: &str, existing: &mut Vec<String>) -> String {
    let base = slugify(text);
    let mut candidate = if base.is_empty() {
        "section".to_string()
    } else {
        base
    };
    if !existing.iter().any(|id| id == &candidate) {
        existing.push(candidate.clone());
        return candidate;
    }

    let base = candidate;
    let mut index = 2;
    loop {
        candidate = format!("{base}-{index}");
        if !existing.iter().any(|id| id == &candidate) {
            existing.push(candidate.clone());
            return candidate;
        }
        index += 1;
    }
}

pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    if last_dash {
        out.pop();
    }
    out
}

fn heading_tag(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "h1",
        HeadingLevel::H2 => "h2",
        HeadingLevel::H3 => "h3",
        HeadingLevel::H4 => "h4",
        HeadingLevel::H5 => "h5",
        HeadingLevel::H6 => "h6",
    }
}

fn heading_depth(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn append_search_text(search_text: &mut String, text: &str) {
    if !search_text.is_empty() {
        search_text.push(' ');
    }
    search_text.push_str(text.trim());
}

fn safe_url(url: &str) -> &str {
    let trimmed = url.trim_start();
    let lower = trimmed
        .bytes()
        .take_while(|byte| *byte != b':')
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if lower == b"javascript" || lower == b"vbscript" {
        ""
    } else {
        url
    }
}

pub fn escape_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

pub fn escape_attr(text: &str, out: &mut String) {
    escape_text(text, out);
}

/// Warn when a dropped raw-HTML block carried a container tag plus content.
///
/// Block-level raw HTML is dropped by design, but a block shaped like
/// `<Callout>\ntext\n</Callout>` means the author's prose is disappearing. MDX
/// lowering normally splits those tags into their own blocks; this catches the
/// case where it did not run, so the loss is loud instead of silent.
fn warn_dropped_container_children(raw: &str, source_path: &Path) {
    let Some(tag) = dropped_container_tag(raw) else {
        return;
    };
    println!(
        "cargo:warning=docs components: {}: content inside {tag} was dropped; put a blank line after the opening tag and before the closing tag so its children are parsed as Markdown",
        source_path.display()
    );
}

/// The container tag opening a dropped raw-HTML block that also carried
/// content, or `None` when the block is ordinary raw HTML.
fn dropped_container_tag(raw: &str) -> Option<&str> {
    let mut lines = raw.lines().map(str::trim).filter(|line| !line.is_empty());
    let tag = lines
        .next()
        .filter(|line| crate::components::is_container_tag(line))?;
    // A tag on its own is handled by `render_tag`; a tag plus content is the
    // shape whose children disappear.
    lines.next()?;
    Some(tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CodeHighlight, MarkdownPipeline, MathMode, MermaidConfig};
    use serde_json::Value as JsonValue;
    use std::fs;

    #[test]
    fn renders_heading_ids_and_toc() {
        let rendered = render_markdown(
            "# Intro\n\n## Install\n\nHello",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default(),
        );
        assert!(rendered.html.contains("<h1 id=\"intro\">Intro</h1>"));
        assert!(rendered.html.contains("<h2 id=\"install\">Install</h2>"));
        assert_eq!(rendered.toc[0].depth, 2);
        assert_eq!(rendered.toc[0].id, "install");
        assert_eq!(
            rendered.hashes,
            vec!["intro".to_string(), "install".to_string()]
        );
    }

    #[test]
    fn renders_mermaid_shell() {
        let rendered = render_markdown(
            "```mermaid\ngraph TD; A-->B;\n```",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().mermaid(MermaidConfig::new()),
        );
        assert!(rendered.html.contains("data-proa-docs-mermaid"));
        assert!(rendered.html.contains("graph TD; A--&gt;B;"));
    }

    #[test]
    fn renders_escaped_json_code_block_when_highlighting_is_requested() {
        let rendered = render_markdown(
            "```json\n{\"title\":\"<Proa>\",\"enabled\":true}\n```",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered.html.contains("data-code-block"));
        assert!(rendered.html.contains("data-code-highlight=\"syntect\""));
        assert!(rendered.html.contains("data-lang=\"json\""));
        assert!(rendered.html.contains("data-code-copy"));
        assert!(rendered.html.contains("class=\"language-json\""));
        assert!(rendered.html.contains("class=\"hl-string\""));
        assert!(rendered.html.contains("class=\"hl-constant-builtin\""));
        assert!(!rendered.html.contains("style="));
        assert!(rendered.html.contains("&lt;Proa&gt;"));
        assert!(!rendered.html.contains("<Proa>"));
    }

    #[test]
    fn highlights_rust_semantically_without_changing_copy_text() {
        let code = concat!(
            "pub fn render() {\n",
            "    let message = \"</span><script>alert('x')</script>&λ\"; // greeting\n",
            "}\n",
        );
        let rendered = render_markdown(
            &format!("```rust\n{code}```"),
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered.html.contains("class=\"hl-keyword\""));
        assert!(rendered.html.contains("class=\"hl-string\""));
        assert!(rendered.html.contains("class=\"hl-comment\""));
        assert!(!rendered.html.contains("<script>"));
        assert!(!rendered.html.contains("style="));

        let code_html = code_contents(&rendered.html);
        let mut escaped = String::new();
        escape_text(code, &mut escaped);
        assert_eq!(strip_highlight_spans(code_html), escaped);
    }

    #[test]
    fn highlights_component_structs_inside_html_macro_slots() {
        let code = concat!(
            "html_sync! {\n",
            "    <article>\n",
            "        {ProductCard { product }}\n",
            "    </article>\n",
            "}\n",
        );
        let rendered = render_markdown(
            &format!("```rust\n{code}```"),
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered
            .html
            .contains("<span class=\"hl-type\">ProductCard</span>"));
        assert!(!rendered
            .html
            .contains("<span class=\"hl-type\">product</span>"));

        let code_html = code_contents(&rendered.html);
        let mut escaped = String::new();
        escape_text(code, &mut escaped);
        assert_eq!(strip_highlight_spans(code_html), escaped);
    }

    #[test]
    fn highlights_rust_inside_markdown_template_control_flow() {
        let code = concat!(
            "let summary = md_sync! {\"\n",
            "    # {title}\n",
            "\n",
            "    ## Features\n",
            "\n",
            "    {for feature in features.iter() {\n",
            "    - {feature}\n",
            "    }}\n",
            "    {if in_stock {In stock} else {Backordered}}\n",
            "\"};\n",
        );
        let rendered = render_markdown(
            &format!("```rust\n{code}```"),
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered
            .html
            .contains("<span class=\"hl-keyword\">for</span>"));
        assert!(rendered
            .html
            .contains("<span class=\"hl-keyword\">if</span>"));
        assert!(rendered
            .html
            .contains("<span class=\"hl-keyword\">else</span>"));
        assert!(rendered
            .html
            .contains("<span class=\"hl-function-builtin\">iter</span>"));

        let code_html = code_contents(&rendered.html);
        let mut escaped = String::new();
        escape_text(code, &mut escaped);
        assert_eq!(strip_highlight_spans(code_html), escaped);
    }

    #[test]
    fn highlights_selected_code_lines_without_changing_copy_text() {
        let code = concat!(
            "pub fn render() {\n",
            "    let message = r#\"first λ\n",
            "second\"#;\n",
            "    println!(\"{message}\");\n",
            "}\n",
        );
        let rendered = render_markdown(
            &format!("```rust {{2,4-5}}\n{code}```"),
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered.html.contains("data-code-lines"));
        assert!(rendered
            .html
            .contains("<span data-code-line=\"1\"><span class=\"hl-keyword\""));
        assert!(rendered
            .html
            .contains("<span data-code-line=\"2\" data-highlighted-line>"));
        assert!(rendered
            .html
            .contains("<span data-code-line=\"4\" data-highlighted-line>"));
        assert!(rendered
            .html
            .contains("<span data-code-line=\"5\" data-highlighted-line>"));
        assert!(!rendered
            .html
            .contains("<span data-code-line=\"3\" data-highlighted-line>"));
        assert_eq!(rendered.html.matches(" data-highlighted-line").count(), 3);

        let code_html = code_contents(&rendered.html);
        assert_eq!(
            code_html.matches("<span").count(),
            code_html.matches("</span>").count()
        );
        let mut escaped = String::new();
        escape_text(code, &mut escaped);
        assert_eq!(strip_span_tags(code_html), escaped);
    }

    #[test]
    fn selected_lines_work_for_plain_escaped_code() {
        let code = "first <tag>\nsecond & final\n";
        let rendered = render_markdown(
            &format!("```text {{2}}\n{code}```"),
            Path::new("docs/index.md"),
            &MarkdownPipeline::default(),
        );

        assert!(!rendered.html.contains("data-code-highlight"));
        assert!(rendered.html.contains(
            "<span data-code-line=\"2\" data-highlighted-line>second &amp; final</span>"
        ));
        let mut escaped = String::new();
        escape_text(code, &mut escaped);
        assert_eq!(strip_span_tags(code_contents(&rendered.html)), escaped);
    }

    #[test]
    fn shell_alias_uses_the_bundled_bash_syntax() {
        let rendered = render_markdown(
            "```shell\nif true; then echo \"ok\"; fi\n```",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered.html.contains("class=\"language-shell\""));
        assert!(rendered.html.contains("class=\"hl-keyword\""));
        assert!(rendered.html.contains("class=\"hl-string\""));
    }

    #[test]
    fn unknown_language_falls_back_to_escaped_plain_code() {
        let rendered = render_markdown(
            "```not-a-real-language\nif x < y { \"<script>\" }\n```",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered
            .html
            .contains("class=\"language-not-a-real-language\""));
        assert!(!rendered.html.contains("class=\"hl-"));
        assert!(rendered
            .html
            .contains("if x &lt; y { &quot;&lt;script&gt;&quot; }"));
        assert!(!rendered.html.contains("<script>"));
    }

    #[test]
    fn renders_plain_code_block_with_copy_button() {
        let rendered = render_markdown(
            "```text\njust <text> & a quote: \"\n```",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default(),
        );

        assert!(rendered.html.contains("data-code-block"));
        assert!(!rendered.html.contains("data-code-highlight"));
        assert!(rendered.html.contains("data-lang=\"text\""));
        assert!(rendered.html.contains("data-code-copy"));
        assert!(!rendered.html.contains("data-code-header"));
        assert!(!rendered.html.contains("data-filename="));
        assert!(rendered.html.contains("<code class=\"language-text\">"));
        assert!(!rendered.html.contains("class=\"hl-"));
        assert!(rendered
            .html
            .contains("just &lt;text&gt; &amp; a quote: &quot;"));
    }

    #[test]
    fn renders_an_escaped_filename_header_without_changing_code_output() {
        let code = "fn hero() { println!(\"<ready>\"); }\n";
        let rendered = render_markdown(
            &format!("```rust filename=\"src/components/<hero>& card.rs\" {{1}}\n{code}```"),
            Path::new("docs/index.mdx"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered
            .html
            .contains("data-filename=\"src/components/&lt;hero&gt;&amp; card.rs\""));
        assert!(rendered.html.contains("data-code-header"));
        assert!(rendered.html.contains("data-code-file-icon"));
        assert!(rendered.html.contains("data-code-icon=\"rust\""));
        assert!(rendered
            .html
            .contains("data-code-filename>src/components/&lt;hero&gt;&amp; card.rs</span>"));
        let header = rendered
            .html
            .split_once("data-code-header")
            .and_then(|(_, rest)| rest.split_once("</div>"))
            .map(|(header, _)| header)
            .expect("filename header");
        assert!(header.contains("data-code-copy"));

        let code_html = code_contents(&rendered.html);
        let mut escaped = String::new();
        escape_text(code, &mut escaped);
        assert_eq!(strip_span_tags(code_html), escaped);
    }

    #[test]
    fn renders_language_icons_for_aliases_and_a_generic_fallback() {
        for (language, expected_icon) in [
            ("ts", "typescript"),
            ("tsx", "react"),
            ("bash", "shell"),
            ("html", "html"),
            ("py", "python"),
            ("yml", "yaml"),
            ("mdx", "markdown"),
            ("dockerfile", "docker"),
            ("unknown", "file"),
        ] {
            let rendered = render_markdown(
                &format!("```{language} filename=\"example\"\ncontent\n```"),
                Path::new("docs/index.mdx"),
                &MarkdownPipeline::default(),
            );

            assert!(
                rendered
                    .html
                    .contains(&format!("data-code-icon=\"{expected_icon}\"")),
                "expected {language} to select {expected_icon}: {}",
                rendered.html
            );
        }
    }

    #[test]
    fn leaves_unlabeled_code_block_unclassified() {
        let rendered = render_markdown(
            "```\ninterface User { name: \"<script>\" }\n```",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered.html.contains("data-code-block"));
        assert!(!rendered.html.contains("data-lang="));
        assert!(!rendered.html.contains("class=\"language-"));
        assert!(!rendered.html.contains("class=\"hl-"));
        assert!(rendered.html.contains("&quot;&lt;script&gt;&quot;"));
        assert!(!rendered.html.contains("<script>"));
    }

    #[test]
    fn rejects_unsafe_language_class_tokens() {
        let rendered = render_markdown(
            "```rust<script>\nlet answer = 42;\n```",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered.html.contains("data-lang=\"rust&lt;script&gt;\""));
        assert!(!rendered.html.contains("class=\"language-"));
        assert!(!rendered.html.contains("class=\"hl-"));
        assert!(!rendered.html.contains("<script>"));
    }

    #[test]
    fn renders_inline_and_display_math_as_mathml() {
        let rendered = render_markdown(
            "Euler: $e^{i\\pi} + 1 = 0$\n\n$$x = \\frac{a}{b}$$",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().math(MathMode::MathMl),
        );

        assert!(rendered.html.contains("display=\"inline\""));
        assert!(rendered.html.contains("display=\"block\""));
        assert!(rendered.html.contains("<mfrac>"));
        assert!(rendered.search_text.contains("e^{i\\pi} + 1 = 0"));
    }

    #[test]
    fn math_escapes_comparison_operators() {
        let rendered = render_markdown(
            "$a < b > c$ and $x \\& y$",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().math(MathMode::MathMl),
        );

        assert!(rendered.html.contains("&lt;"));
        assert!(rendered.html.contains("&gt;"));
        assert!(!rendered.html.contains("<mo><"));
        assert!(!rendered.html.contains("<mo>></mo>"));
    }

    #[test]
    fn invalid_math_falls_back_to_escaped_source() {
        let rendered = render_markdown(
            "$\\begin{bogus}x\\end{bogus}$",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().math(MathMode::MathMl),
        );

        assert!(!rendered.html.contains("<math"));
        assert!(rendered.html.contains("<code>"));
        assert!(rendered.html.contains("\\begin{bogus}"));
    }

    #[test]
    fn disabled_math_keeps_dollar_text_literal() {
        let rendered = render_markdown(
            "Plans run $5 to $20 per month.",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default(),
        );

        assert!(!rendered.html.contains("<math"));
        assert!(rendered.html.contains("$5 to $20"));
    }

    #[test]
    fn renders_file_tree_code_fence_as_structured_html() {
        let rendered = render_markdown(
            "```proa-filetree\napp/\n  src/\n    main.rs\n  Cargo.toml\n```",
            Path::new("docs/index.md"),
            &MarkdownPipeline::default().code_highlighting(CodeHighlight::new()),
        );

        assert!(rendered.html.contains("data-proa-docs-filetree"));
        assert!(rendered.html.contains("data-kind=\"folder\""));
        assert!(rendered.html.contains("aria-expanded=\"true\""));
        assert!(rendered.html.contains("data-kind=\"file\""));
        assert!(rendered.html.contains("<code>main.rs</code>"));
        assert!(rendered.search_text.contains("Cargo.toml"));
    }

    const COMPAT_FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/markdown_compat"
    );
    #[test]
    fn component_free_legacy_contract_is_exact() {
        let source = fs::read_to_string(format!("{COMPAT_FIXTURES}/component_free.md")).unwrap();
        let legacy =
            fs::read_to_string(format!("{COMPAT_FIXTURES}/component_free.legacy.md")).unwrap();
        let expected_body =
            fs::read_to_string(format!("{COMPAT_FIXTURES}/component_free.body.md")).unwrap();
        let expected_html =
            fs::read_to_string(format!("{COMPAT_FIXTURES}/component_free.html")).unwrap();
        let expected_metadata: JsonValue = serde_json::from_str(
            &fs::read_to_string(format!("{COMPAT_FIXTURES}/component_free.metadata.json")).unwrap(),
        )
        .unwrap();

        let (_, body) = crate::frontmatter::split_frontmatter(&source).unwrap();
        assert_eq!(
            source, legacy,
            "legacy .md endpoint keeps full processed source"
        );
        assert_eq!(body, expected_body, "rendered body excludes frontmatter");

        let rendered = render_markdown(
            body,
            Path::new("docs/component-free.md"),
            &MarkdownPipeline::default(),
        );
        assert_eq!(rendered.html, golden_payload(&expected_html));

        let actual_metadata = serde_json::json!({
            "hashes": rendered.hashes,
            "toc": rendered.toc.into_iter().map(|entry| serde_json::json!({
                "depth": entry.depth,
                "id": entry.id,
                "title": entry.title,
            })).collect::<Vec<_>>(),
            "links": rendered.links.into_iter().map(|link| link.href).collect::<Vec<_>>(),
            "type_table_requests": rendered.type_table_requests.len(),
        });
        assert_eq!(actual_metadata, expected_metadata);
    }

    #[test]
    fn legacy_raw_html_and_type_table_prefix_behavior_is_exact() {
        let source = fs::read_to_string(format!("{COMPAT_FIXTURES}/legacy_html.md")).unwrap();
        let expected = fs::read_to_string(format!("{COMPAT_FIXTURES}/legacy_html.html")).unwrap();
        let rendered = render_markdown(
            &source,
            Path::new("docs/legacy-html.md"),
            &MarkdownPipeline::default(),
        );

        assert_eq!(rendered.html, golden_payload(&expected));
        assert_eq!(rendered.type_table_requests.len(), 1);
        assert_eq!(
            rendered.type_table_requests[0].path.as_deref(),
            Some("./Button.tsx")
        );
        assert_eq!(
            rendered.type_table_requests[0].name.as_deref(),
            Some("ButtonProps")
        );
    }

    fn golden_payload(golden: &str) -> &str {
        golden.strip_suffix('\n').unwrap_or(golden)
    }

    fn code_contents(html: &str) -> &str {
        let code = html.find("<code").expect("code element");
        let start = code + html[code..].find('>').expect("code start tag") + 1;
        let end = html[start..].find("</code>").expect("code end tag") + start;
        &html[start..end]
    }

    fn strip_highlight_spans(html: &str) -> String {
        let mut plain = String::with_capacity(html.len());
        let mut rest = html;
        while let Some(start) = rest.find("<span class=\"hl-") {
            plain.push_str(&rest[..start]);
            let end = rest[start..].find('>').expect("highlight span start") + start;
            rest = &rest[end + 1..];
        }
        plain.push_str(rest);
        plain.replace("</span>", "")
    }

    fn strip_span_tags(html: &str) -> String {
        let mut plain = String::with_capacity(html.len());
        let mut rest = html;
        while let Some(start) = rest.find("<span") {
            plain.push_str(&rest[..start]);
            let end = rest[start..].find('>').expect("span start") + start;
            rest = &rest[end + 1..];
        }
        plain.push_str(rest);
        plain.replace("</span>", "")
    }

    /// Render the way the compiler does: MDX lowering, then Markdown.
    fn render_mdx(source: &str) -> RenderedMarkdown {
        let lowered =
            crate::mdx::lower_mdx(source, &crate::config::MdxConfig::proa()).expect("mdx lowering");
        render_markdown(
            &lowered,
            Path::new("docs/index.mdx"),
            &MarkdownPipeline::default(),
        )
    }

    /// A container written the way authors actually write it, with the
    /// children on the line after the opening tag, must keep those children.
    #[test]
    fn container_children_survive_without_surrounding_blank_lines() {
        let rendered = render_mdx("<Callout>\nEnable it in `build.rs` first.\n</Callout>");
        assert!(
            rendered.html.contains("Enable it in"),
            "callout children were dropped: {}",
            rendered.html
        );
        assert!(
            rendered.html.contains("proa-docs-callout"),
            "callout wrapper missing: {}",
            rendered.html
        );
    }

    /// The children are Markdown, not raw text: links and code spans render.
    #[test]
    fn container_children_are_parsed_as_markdown() {
        let rendered = render_mdx(
            "<Callout>\nSee [the docs](https://example.com) and `cargo run`.\n</Callout>",
        );
        assert!(
            rendered.html.contains("<a href=\"https://example.com\">"),
            "link was not parsed: {}",
            rendered.html
        );
        assert!(
            rendered.html.contains("<code>cargo run</code>"),
            "code span was not parsed: {}",
            rendered.html
        );
    }

    #[test]
    fn container_children_are_searchable() {
        let rendered = render_mdx("<Callout>\nEnable it in `build.rs` first.\n</Callout>");
        assert!(
            rendered.search_text.contains("Enable it in"),
            "callout children missing from search text: {}",
            rendered.search_text
        );
    }

    /// The blank-line form authors may already have written keeps working, and
    /// does not gain extra vertical space.
    #[test]
    fn container_children_with_explicit_blank_lines_still_render() {
        let rendered = render_mdx("<Callout>\n\nEnable it in `build.rs` first.\n\n</Callout>");
        assert!(
            rendered.html.contains("Enable it in"),
            "callout children were dropped: {}",
            rendered.html
        );
    }

    /// Nested containers keep their structure: each tag becomes its own block.
    #[test]
    fn nested_containers_keep_their_children() {
        let rendered = render_mdx(
            "<Steps>\n<Step>\nInstall the CLI.\n</Step>\n<Step>\nRun it.\n</Step>\n</Steps>",
        );
        assert!(
            rendered.html.contains("Install the CLI."),
            "first step lost: {}",
            rendered.html
        );
        assert!(
            rendered.html.contains("Run it."),
            "second step lost: {}",
            rendered.html
        );
    }

    /// The safety net: if lowering ever stops splitting the tags, the loss is
    /// reported instead of happening silently.
    #[test]
    fn dropped_container_children_are_detected() {
        assert_eq!(
            dropped_container_tag("<Callout>\nEnable it first.\n</Callout>\n"),
            Some("<Callout>")
        );
        assert_eq!(
            dropped_container_tag("<Tabs data-items=\"a,b\">\nBody.\n"),
            Some("<Tabs data-items=\"a,b\">")
        );
    }

    #[test]
    fn a_lone_container_tag_is_not_reported_as_dropped() {
        // The normal, lowered shape: `render_tag` handles it.
        assert_eq!(dropped_container_tag("<Callout>\n"), None);
        assert_eq!(dropped_container_tag("</Callout>\n"), None);
    }

    #[test]
    fn ordinary_raw_html_is_not_reported_as_dropped() {
        assert_eq!(
            dropped_container_tag("<div class=\"x\">\ntext\n</div>\n"),
            None
        );
    }

    /// Container tags inside fenced code are documentation, not markup.
    #[test]
    fn container_tags_inside_code_fences_are_untouched() {
        let rendered = render_mdx("```mdx\n<Callout>\nDocumented usage.\n</Callout>\n```");
        assert!(
            !rendered.html.contains("proa-docs-callout"),
            "fenced example was rendered as a real callout: {}",
            rendered.html
        );
        assert!(
            rendered.html.contains("&lt;Callout&gt;"),
            "fenced example lost its tags: {}",
            rendered.html
        );
    }
}
