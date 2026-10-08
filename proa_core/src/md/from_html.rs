// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{future::Future, rc::Rc, sync::LazyLock};

use htmd::{options::BulletListMarker, options::Options, HtmlToMarkdown};
use markup5ever_rcdom::NodeData;

mod list;
mod table;

use crate::{
    DataLoader, MdRender, MdRenderSync, RenderOutcome, WebContext, WebRenderSync, WriteBuf,
    WriteError,
};

/// An explicit HTML-to-Markdown adapter, created by [`to_markdown`].
///
/// The wrapped value renders HTML into the standard [`crate::RenderBuffer`],
/// even when the destination Markdown writer is a custom buffer. The capture
/// shares the live loader and request state, but isolates content modes,
/// Markdown formatting, and streaming hooks. Rendering errors leave the
/// destination output untouched; request-state mutations survive.
///
/// Conversion allocates an HTML buffer, a DOM, and a Markdown string. Use a
/// handwritten [`MdRenderSync`] implementation when allocation-free rendering
/// or a component-specific representation matters.
///
/// This adapter always uses HTML, including for children that also implement a
/// Markdown trait. Compose those children separately in `md_sync!` to use
/// their Markdown representations. Styling and interactive behavior are not
/// preserved. Metadata elements, scripts, noscript, styles, templates, SVG,
/// and elements with hidden or aria-hidden="true" attributes are omitted.
/// CSS is not evaluated.
/// The converted fragment has no leading or trailing newline; author fragment
/// separators in the surrounding Markdown template.
/// Simple grids become GFM pipe tables; complex tables retain rows/cells as
/// nested lists rather than preserving span geometry. Most attributes and
/// control state are not synthesized. This is not a lossless HTML round trip.
#[derive(Debug, Clone, Copy)]
pub struct ToMarkdown<T>(T);

/// Convert a synchronous HTML component or fragment into a Markdown child.
///
/// Available with the `html-to-markdown` feature. The adapter implements both
/// [`MdRenderSync`] and [`MdRender`], so it works in `md_sync!` and `md!`.
/// It does not execute async HTML components.
///
/// ```
/// use proa_core::{to_markdown, DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteError};
/// use proa_macros::html_sync;
///
/// struct Article<'a> { title: &'a str }
/// impl<L: DataLoader> WebRenderSync<L> for Article<'_> {
///     fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
///         html_sync! { <h2>{self.title}</h2> }.render(cx)
///     }
/// }
/// impl<L: DataLoader> MdRenderSync<L> for Article<'_> {
///     fn render_md(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
///         to_markdown(self).render_md(cx)
///     }
/// }
/// let mut cx = WebContext::new();
/// Article { title: "Hello" }.render_md(&mut cx)?;
/// assert_eq!(cx.into_output().into_string(), "## Hello");
/// # Ok::<(), WriteError>(())
/// ```
#[inline]
pub const fn to_markdown<T>(value: T) -> ToMarkdown<T> {
    ToMarkdown(value)
}

// proa-perf-ok(scope): explicit opt-in DOM conversion intentionally allocates outside the HTML fast path
impl<L, B, T> MdRenderSync<L, B> for ToMarkdown<T>
where
    L: DataLoader,
    B: WriteBuf,
    T: WebRenderSync<L>,
{
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let output = cx.capture_html_for_markdown(self.0)?;
        let html = String::from_utf8(output.into_vec())
            .map_err(|_| WriteError::render("HTML-to-Markdown input is not valid UTF-8"))?;
        let converter = &*CONVERTER;
        let tree = converter
            .html_to_tree(&html)
            .map_err(|error| WriteError::render(error.to_string()))?;
        prepare_dom(&tree, false);
        let markdown = converter.tree_to_markdown(&tree);
        cx.write_md_trusted(&markdown)
    }
}

// proa-perf-ok(scope): DOM preparation preserves literal text during opt-in conversion
fn prepare_dom(node: &Rc<htmd::Node>, in_code: bool) {
    let in_code = match &node.data {
        NodeData::Element { name, .. } => in_code || matches!(name.local.as_ref(), "pre" | "code"),
        NodeData::Text { contents } if !in_code => {
            let mut text = contents.borrow_mut();
            if let Some(escaped) = escape_literal_entities(&text, true) {
                *text = escaped.into();
            }
            in_code
        }
        _ => in_code,
    };
    let mut children = node.children.borrow_mut();
    children.retain(|child| match &child.data {
        NodeData::Element { name, attrs, .. } => {
            !matches!(
                name.local.as_ref(),
                "head"
                    | "title"
                    | "meta"
                    | "link"
                    | "base"
                    | "script"
                    | "noscript"
                    | "style"
                    | "template"
                    | "svg"
            ) && !attrs.borrow().iter().any(|attr| {
                attr.name.local.as_ref() == "hidden"
                    || (attr.name.local.as_ref() == "aria-hidden"
                        && attr.value.eq_ignore_ascii_case("true"))
            })
        }
        _ => true,
    });
    for child in children.iter() {
        prepare_dom(child, in_code);
    }
}

// proa-perf-ok(scope): literal entity preservation allocates only for affected DOM text/attributes
fn escape_literal_entities(text: &str, adjacent_text: bool) -> Option<String> {
    // html5ever already decoded entities. htmd leaves ampersands untouched,
    // so Markdown would decode entity-shaped literal values a second time.
    // Escape possible prefixes, preserving ordinary ampersands and code.
    let is_entity = |index: usize| {
        let following = &text.as_bytes()[index + 1..];
        if adjacent_text {
            // Adjacent inline nodes can complete a prefix from this text node.
            return following
                .first()
                .is_none_or(|byte| byte.is_ascii_alphanumeric() || *byte == b'#');
        }
        let len = following
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'#')
            .count();
        len > 0 && following.get(len) == Some(&b';')
    };
    // htmd escapes block markers within one node, but inline HTML can
    // split a marker from its following space. Entities stay literal until
    // after Markdown block parsing, including across styled inline nodes.
    let is_marker = |index: usize, ch: char| {
        adjacent_text
            && (matches!(ch, '~' | '|')
                || (matches!(ch, '#' | '+' | '-' | '.' | ')' | '!')
                    && text[..index]
                        .bytes()
                        .all(|byte| byte.is_ascii_whitespace() || byte.is_ascii_digit())))
    };
    if !text
        .char_indices()
        .any(|(index, ch)| (ch == '&' && is_entity(index)) || is_marker(index, ch))
    {
        return None;
    }
    let mut escaped = String::with_capacity(text.len());
    for (index, ch) in text.char_indices() {
        if ch == '&' && is_entity(index) {
            escaped.push_str("&amp;");
        } else if is_marker(index, ch) {
            escaped.push_str(match ch {
                '#' => "&#35;",
                '+' => "&#43;",
                '-' => "&#45;",
                '.' => "&#46;",
                ')' => "&#41;",
                '!' => "&#33;",
                '~' => "&#126;",
                '|' => "&#124;",
                _ => unreachable!(),
            });
        } else {
            escaped.push(ch);
        }
    }
    Some(escaped)
}

// proa-perf-ok(scope): immediate synchronous adapter for async Markdown composition
impl<L, B, T> MdRender<L, B> for ToMarkdown<T>
where
    L: DataLoader,
    B: WriteBuf,
    T: WebRenderSync<L>,
{
    fn render_md(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}

// proa-perf-ok(scope): converter configuration allocates once when conversion is first requested
static CONVERTER: LazyLock<HtmlToMarkdown> = LazyLock::new(|| {
    HtmlToMarkdown::builder()
        .options(Options {
            bullet_list_marker: BulletListMarker::Dash,
            ul_bullet_spacing: 1,
            ol_number_spacing: 1,
            ..Options::default()
        })
        .add_handler(vec!["pre"], preformatted)
        .add_handler(vec!["code"], inline_code)
        .add_handler(vec!["a", "img"], link_or_image)
        .add_handler(vec!["table"], table::table)
        .add_handler(vec!["ol"], list::ordered_list)
        .build()
});

// proa-perf-ok(scope): preformatted HTML needs a buffered adaptive Markdown code fence
fn preformatted(
    _: &dyn htmd::element_handler::Handlers,
    element: htmd::Element<'_>,
) -> Option<htmd::element_handler::HandlerResult> {
    // Collect raw DOM text instead of walking inline handlers, which trim
    // boundary newlines from highlighted spans. Core fences also sanitize
    // language labels and cannot be closed by literal source content.
    let language = element
        .node
        .children
        .borrow()
        .iter()
        .find_map(|child| match &child.data {
            NodeData::Element { name, .. } if name.local.as_ref() == "code" => code_language(child),
            _ => None,
        })
        .or_else(|| code_language(element.node));
    let mut content = String::new();
    append_pre_text(element.node, &mut content);
    let mut output = Vec::new();
    output.extend_from_slice(b"\n\n");
    crate::md::escape::write_fenced_code(&mut output, language.as_deref().unwrap_or(""), &content)
        .expect("growable Markdown buffer");
    Some(
        String::from_utf8(output)
            .expect("UTF-8 Markdown escaping")
            .into(),
    )
}

// proa-perf-ok(scope): preformatted DOM text is collected for the buffered code-block conversion
fn append_pre_text(node: &htmd::Node, output: &mut String) {
    match &node.data {
        NodeData::Text { contents } => output.push_str(&contents.borrow()),
        NodeData::Element { name, .. } if name.local.as_ref() == "br" => output.push('\n'),
        _ => {}
    }
    for child in node.children.borrow().iter() {
        append_pre_text(child, output);
    }
}

// proa-perf-ok(scope): opt-in inline code captures literal DOM text before adaptive escaping
fn inline_code(
    _: &dyn htmd::element_handler::Handlers,
    element: htmd::Element<'_>,
) -> Option<htmd::element_handler::HandlerResult> {
    let mut content = String::new();
    append_pre_text(element.node, &mut content);
    if content.is_empty() {
        return Some(String::new().into());
    }
    let mut output = Vec::new();
    crate::md::escape::write_inline_code(&mut output, &content).expect("growable Markdown buffer");
    Some(
        String::from_utf8(output)
            .expect("UTF-8 Markdown escaping")
            .into(),
    )
}

// proa-perf-ok(scope): code language metadata is owned during buffered conversion
fn code_language(node: &htmd::Node) -> Option<String> {
    let NodeData::Element { attrs, .. } = &node.data else {
        return None;
    };
    attrs
        .borrow()
        .iter()
        .find(|attr| attr.name.local.as_ref() == "class")?
        .value
        .split_ascii_whitespace()
        .find_map(|class| class.strip_prefix("language-"))
        .map(str::to_owned)
}

// proa-perf-ok(scope): opt-in links and images use the core destination escaper
fn link_or_image(
    handlers: &dyn htmd::element_handler::Handlers,
    element: htmd::Element<'_>,
) -> Option<htmd::element_handler::HandlerResult> {
    let image = element.tag == "img";
    let attribute = |name: &str| {
        element
            .attrs
            .iter()
            .find(|attr| attr.name.local.as_ref() == name)
            .map(|attr| attr.value.as_ref())
    };
    let Some(destination) = attribute(if image { "src" } else { "href" }) else {
        return if image {
            let mut output = Vec::new();
            crate::md::escape::escape_md(&mut output, attribute("alt").unwrap_or(""))
                .expect("growable Markdown buffer");
            Some(
                String::from_utf8(output)
                    .expect("UTF-8 Markdown escaping")
                    .into(),
            )
        } else {
            Some(handlers.walk_children(element.node))
        };
    };
    let mut output = Vec::new();
    let content = if image {
        crate::md::escape::escape_md(&mut output, attribute("alt").unwrap_or(""))
            .expect("growable Markdown buffer");
        String::from_utf8(std::mem::take(&mut output)).expect("UTF-8 Markdown escaping")
    } else {
        handlers.walk_children(element.node).content
    };
    // Keep spaces outside the brackets so adjacent words do not concatenate.
    let label = if image {
        content.as_str()
    } else {
        content.trim_matches([' ', '\n', '\r', '\t'])
    };
    let leading = if image {
        0
    } else {
        content.len() - content.trim_start_matches([' ', '\n', '\r', '\t']).len()
    };
    output.extend_from_slice(&content.as_bytes()[..leading]);
    if image {
        output.push(b'!');
    }
    output.push(b'[');
    output.extend_from_slice(label.as_bytes());
    output.extend_from_slice(b"](");
    let escaped_destination = escape_literal_entities(destination, false);
    let mut url = Vec::new();
    crate::md::escape::write_link_destination(
        &mut url,
        escaped_destination.as_deref().unwrap_or(destination),
    )
    .expect("growable Markdown buffer");
    // A leading '<' selects CommonMark's angle-delimited destination form.
    // Encode both angle brackets so a literal HTML URL keeps its target.
    for byte in url {
        match byte {
            b'<' => output.extend_from_slice(b"%3C"),
            b'>' => output.extend_from_slice(b"%3E"),
            _ => output.push(byte),
        }
    }
    if let Some(title) = attribute("title") {
        output.extend_from_slice(b" \"");
        let title = escape_literal_entities(title, false).unwrap_or_else(|| title.to_owned());
        for ch in title.chars() {
            if matches!(ch, '\r' | '\n' | '\t') {
                output.push(b' ');
                continue;
            }
            if matches!(ch, '\\' | '"') {
                output.push(b'\\');
            }
            let mut bytes = [0; 4];
            output.extend_from_slice(ch.encode_utf8(&mut bytes).as_bytes());
        }
        output.push(b'"');
    }
    output.push(b')');
    let trailing = if image {
        content.len()
    } else {
        content
            .trim_end_matches([' ', '\n', '\r', '\t'])
            .len()
            .max(leading)
    };
    output.extend_from_slice(&content.as_bytes()[trailing..]);
    Some(
        String::from_utf8(output)
            .expect("UTF-8 Markdown escaping")
            .into(),
    )
}
