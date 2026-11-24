// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{future::Future, rc::Rc, sync::LazyLock};

use htmd::{
    element_handler::Context, node::Node, options::BulletListMarker, options::Options,
    HtmlToMarkdown,
};
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, Node as DomNode, NodeData, RcDom};

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
        let html = cx.capture_html_for_markdown(self.0)?.into_string();
        let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
        prepare_dom(&dom.document, false);
        let markdown = CONVERTER
            .convert(&dom.document)
            .map_err(|error| WriteError::render(error.to_string()))?;
        cx.write_md_trusted(&markdown)
    }
}

fn prepare_dom(node: &Handle, in_code: bool) {
    let in_code = in_code
        || matches!(&node.data, NodeData::Element { name, .. }
        if matches!(name.local.as_ref(), "pre" | "code"));
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
                &*attr.name.local == "hidden"
                    || (&*attr.name.local == "aria-hidden"
                        && attr.value.eq_ignore_ascii_case("true"))
            })
        }
        // The pure Markdown converter omits HTML comments.
        NodeData::Comment { .. } => false,
        _ => true,
    });
    for child in children.iter() {
        prepare_dom(child, in_code);
    }
    if in_code {
        return;
    }

    // A preceding inline node can end in digits. htmd cannot recognize the
    // ordered-list delimiter when the two occur in separate text nodes.
    let starts_with_delimiter = |child: &Handle| matches!(&child.data, NodeData::Text { contents } if contents.borrow().starts_with(['.', ')']));
    if let Some(first) = children.iter().position(starts_with_delimiter) {
        let tail = children.split_off(first);
        for child in tail {
            if let NodeData::Text { contents } = &child.data {
                let mut text = contents.borrow_mut();
                if text.starts_with(['.', ')']) {
                    let literal = DomNode::new(NodeData::Comment {
                        contents: text.subtendril(0, 1),
                    });
                    text.pop_front(1);
                    literal.parent.set(Some(Rc::downgrade(node)));
                    children.push(literal);
                }
            }
            children.push(child);
        }
    }
}

// proa-perf-ok(scope): Markdown link attributes allocate only when escaping an entity
fn escape_attribute_entities(text: &str) -> Option<String> {
    // The custom link handler writes attributes directly into Markdown. Preserve
    // entity-shaped literals that the HTML parser has already decoded once.
    let is_entity = |following: &[u8]| {
        let len = following
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'#')
            .count();
        len > 0 && following.get(len) == Some(&b';')
    };
    let mut matches = text
        .match_indices('&')
        .filter(|(index, _)| is_entity(&text.as_bytes()[index + 1..]))
        .peekable();
    matches.peek()?;
    let mut escaped = String::with_capacity(text.len());
    let mut start = 0;
    for (index, _) in matches {
        escaped.push_str(&text[start..index]);
        escaped.push_str("&amp;");
        start = index + 1;
    }
    escaped.push_str(&text[start..]);
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
        // prepare_dom removed authored comments. These nodes carry a single
        // literal delimiter which must survive Markdown block parsing.
        .comment_handler(
            |_: &dyn htmd::element_handler::Handlers, node: &Node<'_>, _: Context| match node
                .comment()?
            {
                "." => Some("&#46;".into()),
                ")" => Some("&#41;".into()),
                _ => None,
            },
        )
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
    element: &htmd::Element<'_>,
) -> Option<htmd::element_handler::HandlerResult> {
    // Collect raw DOM text instead of walking inline handlers, which trim
    // boundary newlines from highlighted spans. Core fences also sanitize
    // language labels and cannot be closed by literal source content.
    let language = element
        .node
        .children()
        .iter()
        .filter(|child| child.tag_name() == Some("code"))
        .find_map(|child| code_language(&child))
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
fn append_pre_text(node: &Node<'_>, output: &mut String) {
    if let Some(text) = node.text() {
        output.push_str(&text);
    } else if node.tag_name() == Some("br") {
        output.push('\n');
    }
    for child in node.children().iter() {
        append_pre_text(&child, output);
    }
}

// proa-perf-ok(scope): opt-in inline code captures literal DOM text before adaptive escaping
fn inline_code(
    _: &dyn htmd::element_handler::Handlers,
    element: &htmd::Element<'_>,
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
fn code_language(node: &Node<'_>) -> Option<String> {
    node.attr("class")?
        .split_ascii_whitespace()
        .find_map(|class| class.strip_prefix("language-"))
        .map(str::to_owned)
}

// proa-perf-ok(scope): opt-in links and images use the core destination escaper
fn link_or_image(
    handlers: &dyn htmd::element_handler::Handlers,
    element: &htmd::Element<'_>,
) -> Option<htmd::element_handler::HandlerResult> {
    let image = element.tag == "img";
    let attribute = |name: &str| element.node.attr(name);
    let destination = attribute(if image { "src" } else { "href" });
    let Some(destination) = destination.as_deref() else {
        return if image {
            let mut output = Vec::new();
            crate::md::escape::escape_md(&mut output, attribute("alt").as_deref().unwrap_or(""))
                .expect("growable Markdown buffer");
            Some(
                String::from_utf8(output)
                    .expect("UTF-8 Markdown escaping")
                    .into(),
            )
        } else {
            Some(handlers.walk_children(element.node, Context::INLINE))
        };
    };
    let mut output = Vec::new();
    let content = if image {
        crate::md::escape::escape_md(&mut output, attribute("alt").as_deref().unwrap_or(""))
            .expect("growable Markdown buffer");
        String::from_utf8(std::mem::take(&mut output)).expect("UTF-8 Markdown escaping")
    } else {
        handlers
            .walk_children(element.node, Context::INLINE)
            .content
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
    let escaped_destination = escape_attribute_entities(destination);
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
    if let Some(title) = attribute("title").as_deref() {
        output.extend_from_slice(b" \"");
        let title = escape_attribute_entities(title).unwrap_or_else(|| title.to_owned());
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
