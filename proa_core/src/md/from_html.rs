// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{future::Future, rc::Rc, sync::OnceLock};

use htmd::{options::BulletListMarker, options::Options, HtmlToMarkdown};
use markup5ever_rcdom::NodeData;

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
/// preserved. Head metadata, scripts, styles, templates, SVG, and elements with
/// hidden or aria-hidden="true" attributes are omitted. CSS is not evaluated.
/// The converted fragment has no leading or trailing newline; author fragment
/// separators in the surrounding Markdown template.
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
        let converter = converter();
        let tree = converter
            .html_to_tree(&html)
            .map_err(|error| WriteError::render(error.to_string()))?;
        prepare_dom(&tree, false);
        let markdown = converter.tree_to_markdown(&tree);
        cx.write_md_trusted(&markdown)
    }
}

// proa-perf-ok(scope): DOM preparation preserves literal text and attributes during opt-in conversion
fn prepare_dom(node: &Rc<htmd::Node>, in_code: bool) {
    let in_code = match &node.data {
        NodeData::Element { name, attrs, .. } => {
            for attr in attrs.borrow_mut().iter_mut() {
                match attr.name.local.as_ref() {
                    "alt" if name.local.as_ref() == "img" => {
                        let mut escaped = Vec::new();
                        // A growable Vec cannot produce a WriteError. Use the
                        // core escaper because htmd emits alt text verbatim.
                        crate::md::escape::escape_md(&mut escaped, &attr.value)
                            .expect("growable Markdown buffer");
                        attr.value = String::from_utf8(escaped)
                            .expect("UTF-8 Markdown escaping")
                            .into();
                    }
                    "href" | "src" | "title" => {
                        if let Some(escaped) = escape_literal_entities(&attr.value, false) {
                            attr.value = escaped.into();
                        }
                    }
                    _ => {}
                }
            }
            in_code || matches!(name.local.as_ref(), "pre" | "code")
        }
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
                "head" | "script" | "style" | "template" | "svg"
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
    if !text.match_indices('&').any(|(index, _)| is_entity(index)) {
        return None;
    }
    let mut escaped = String::with_capacity(text.len());
    for (index, ch) in text.char_indices() {
        if ch == '&' && is_entity(index) {
            escaped.push_str("&amp;");
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

// proa-perf-ok(scope): converter configuration allocates once, only when conversion is first requested
fn converter() -> &'static HtmlToMarkdown {
    static CONVERTER: OnceLock<HtmlToMarkdown> = OnceLock::new();
    CONVERTER.get_or_init(|| {
        HtmlToMarkdown::builder()
            .options(Options {
                bullet_list_marker: BulletListMarker::Dash,
                ul_bullet_spacing: 1,
                ol_number_spacing: 1,
                ..Options::default()
            })
            .add_handler(vec!["pre"], plain_pre)
            .build()
    })
}

// proa-perf-ok(scope): plain preformatted HTML needs a buffered adaptive Markdown code fence
fn plain_pre(
    handlers: &dyn htmd::element_handler::Handlers,
    element: htmd::Element<'_>,
) -> Option<htmd::element_handler::HandlerResult> {
    if element.node.children.borrow().iter().any(|child| {
        matches!(&child.data, NodeData::Element { name, .. } if name.local.as_ref() == "code")
    }) {
        return handlers.fallback(element);
    }
    // htmd fences <pre><code> but otherwise emits bare preformatted text.
    // Preserve all whitespace in a plain <pre>, including styled spans.
    let mut content = String::new();
    append_pre_text(element.node, &mut content);
    let mut output = Vec::new();
    output.extend_from_slice(b"\n\n");
    crate::md::escape::write_fenced_code(&mut output, "", &content)
        .expect("growable Markdown buffer");
    output.extend_from_slice(b"\n\n");
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
