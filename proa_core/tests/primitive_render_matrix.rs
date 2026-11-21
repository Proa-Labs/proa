// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Every Rust primitive scalar is a first-class template child.
//!
//! The first test is a compile-time matrix: all 16 primitives against all six
//! render traits and `AttrValue`, for every loader and buffer. The remaining
//! tests pin behavior at the edges of each type across every template dialect
//! (`html_sync!`, `html!`, `html_send!`, `xml_sync!`, `xml!`, `md_sync!`,
//! `md!`): integer extremes, non-finite and extreme floats, and escaping of
//! `char` values, which must match a one-character string exactly.

use proa_core::shared::attr_value::AttrValue;
use proa_core::{
    render_in_send_context, render_md_to_string_async, render_to_string, render_to_string_async,
    DataLoader, MdRender, MdRenderSync, SendWebContext, WebContext, WebRender, WebRenderSend,
    WebRenderSendSync, WebRenderSync, WriteBuf,
};
use proa_macros::{html, html_send, html_sync, md, md_sync, xml, xml_sync};

fn implements_every_render_trait<T, L, B>()
where
    L: DataLoader,
    B: WriteBuf + Send,
    T: WebRenderSync<L, B>
        + WebRender<L, B>
        + WebRenderSendSync<L, B>
        + WebRenderSend<L, B>
        + MdRenderSync<L, B>
        + MdRender<L, B>
        + AttrValue,
{
}

/// `Option<T>` composes through every trait that supports optional children.
/// Async Markdown intentionally has no `Option` impl for any `T`.
fn optional_child_renders<T, L, B>()
where
    L: DataLoader,
    B: WriteBuf + Send,
    Option<T>: WebRenderSync<L, B>
        + WebRender<L, B>
        + WebRenderSendSync<L, B>
        + WebRenderSend<L, B>
        + MdRenderSync<L, B>,
{
}

macro_rules! for_every_primitive {
    ($check:ident::<L, B>) => {
        for_every_primitive!(
            $check::<L, B>:
            u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64, bool, char
        )
    };
    ($check:ident::<L, B>: $($ty:ty),+) => {{
        $( $check::<$ty, L, B>(); )+
    }};
}

/// Generic over the loader and buffer, so the proof covers custom writers and
/// application loaders rather than one default instantiation.
fn primitive_matrix<L: DataLoader, B: WriteBuf + Send>() {
    for_every_primitive!(implements_every_render_trait::<L, B>);
    for_every_primitive!(optional_child_renders::<L, B>);
}

#[test]
fn every_primitive_implements_every_render_trait_for_any_loader_and_buffer() {
    primitive_matrix::<proa_core::NoLoader, proa_core::RenderBuffer>();
    primitive_matrix::<proa_core::NoLoader, Vec<u8>>();
}

/// One value rendered as a text child by every dialect and execution mode.
struct TextRenders {
    html_sync: String,
    html_async: String,
    html_send: String,
    xml_sync: String,
    xml_async: String,
    md_sync: String,
    md_async: String,
}

impl TextRenders {
    /// The XML renders unwrapped from the `<v>` element the fixture adds.
    fn xml(&self) -> [&str; 2] {
        [&self.xml_sync, &self.xml_async].map(|xml| {
            xml.strip_prefix("<v>")
                .and_then(|xml| xml.strip_suffix("</v>"))
                .unwrap_or_else(|| panic!("unexpected XML fixture output: {xml}"))
        })
    }

    fn html(&self) -> [&str; 3] {
        [&self.html_sync, &self.html_async, &self.html_send]
    }

    fn md(&self) -> [&str; 2] {
        [&self.md_sync, &self.md_async]
    }

    /// Asserts a value whose spelling needs no escaping in any dialect.
    fn assert_all(&self, expected: &str, what: &str) {
        for rendered in self.html().into_iter().chain(self.xml()).chain(self.md()) {
            assert_eq!(rendered, expected, "{what}");
        }
    }

    fn assert_same_as(&self, other: &Self, what: &str) {
        assert_eq!(self.html(), other.html(), "html: {what}");
        assert_eq!(self.xml(), other.xml(), "xml: {what}");
        assert_eq!(self.md(), other.md(), "markdown: {what}");
    }
}

macro_rules! text_renders {
    ($value:expr) => {{
        let value = $value;
        let mut send_cx = SendWebContext::new();
        render_in_send_context(&mut send_cx, html_send! { {value} })
            .await
            .unwrap();
        let mut md_cx = WebContext::with_buffer(Vec::new());
        MdRenderSync::render_md(md_sync! { "{value}" }, &mut md_cx).unwrap();
        TextRenders {
            html_sync: render_to_string(html_sync! { {value} }),
            html_async: render_to_string_async(html! { {value} }).await,
            html_send: send_cx.into_output().into_string(),
            xml_sync: render_to_string(xml_sync! { <v>{value}</v> }),
            xml_async: render_to_string_async(xml! { <v>{value}</v> }).await,
            md_sync: String::from_utf8(md_cx.into_output()).unwrap(),
            md_async: render_md_to_string_async(md! { "{value}" }).await,
        }
    }};
}

/// One value rendered as an attribute value in HTML and XML, sync and async.
macro_rules! attr_renders {
    ($value:expr) => {{
        let value = $value;
        [
            render_to_string(html_sync! { <i data-v={value}></i> }),
            render_to_string_async(html! { <i data-v={value}></i> }).await,
            render_to_string(xml_sync! { <i v={value}></i> }),
            render_to_string_async(xml! { <i v={value}></i> }).await,
        ]
    }};
}

macro_rules! assert_integer_extremes {
    ($($ty:ty),+) => {$(
        for value in [<$ty>::MIN, <$ty>::MAX, 0 as $ty, 1 as $ty] {
            let expected = value.to_string();
            let what = concat!(stringify!($ty), " text");
            text_renders!(value).assert_all(&expected, what);
            let [html_sync, html_async, xml_sync, xml_async] = attr_renders!(value);
            let html = format!("<i data-v=\"{expected}\"></i>");
            let xml = format!("<i v=\"{expected}\"></i>");
            assert_eq!([html_sync, html_async], [html.clone(), html], "{} attr", stringify!($ty));
            assert_eq!([xml_sync, xml_async], [xml.clone(), xml], "{} attr", stringify!($ty));
        }
    )+};
}

#[tokio::test]
async fn integer_extremes_match_rust_display_in_every_dialect() {
    assert_integer_extremes!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);
}

macro_rules! assert_float_edges {
    ($ty:ty: $($value:expr),+ $(,)?) => {$(
        let value: $ty = $value;
        let renders = text_renders!(value);
        let spelling = renders.html_sync.clone();
        let what = format!("{} {value:?}", stringify!($ty));
        // One spelling everywhere: no dialect reformats or escapes a float.
        renders.assert_all(&spelling, &what);
        let [html_sync, html_async, xml_sync, xml_async] = attr_renders!(value);
        let html = format!("<i data-v=\"{spelling}\"></i>");
        let xml = format!("<i v=\"{spelling}\"></i>");
        assert_eq!([html_sync, html_async], [html.clone(), html], "{what} attr");
        assert_eq!([xml_sync, xml_async], [xml.clone(), xml], "{what} attr");
        if value.is_finite() {
            // Shortest round-trip: parsing the output recovers the exact bits,
            // including the sign of zero and subnormals.
            let parsed: $ty = spelling.parse().unwrap_or_else(|_| panic!("{what}: {spelling}"));
            assert_eq!(parsed.to_bits(), value.to_bits(), "{what}: {spelling}");
            // Exponents never carry `+`, which Markdown and URLs would escape.
            assert!(!spelling.contains('+'), "{what}: {spelling}");
        } else {
            // Non-finite values use Rust's own spelling rather than panicking
            // or emitting digits.
            assert_eq!(spelling, value.to_string(), "{what}");
        }
    )+};
}

#[tokio::test]
async fn float_edges_round_trip_and_agree_in_every_dialect() {
    assert_float_edges!(f64:
        0.0, -0.0, 1.5, -2.25, 0.1 + 0.2, 1e21, 1e-7, 1e300, -1e300,
        f64::MAX, f64::MIN, f64::MIN_POSITIVE, 5e-324, f64::EPSILON,
        f64::NAN, f64::INFINITY, f64::NEG_INFINITY,
    );
    assert_float_edges!(f32:
        0.0, -0.0, 1.5, -2.25, 0.1, 16_777_216.0, 1e-7, 1e30,
        f32::MAX, f32::MIN, f32::MIN_POSITIVE, 1e-45, f32::EPSILON,
        f32::NAN, f32::INFINITY, f32::NEG_INFINITY,
    );
}

#[tokio::test]
async fn bool_renders_its_keyword_as_text_in_every_dialect() {
    text_renders!(true).assert_all("true", "true");
    text_renders!(false).assert_all("false", "false");
}

#[tokio::test]
async fn char_escapes_exactly_like_a_one_character_string() {
    // Each entry is one scalar; the string form is the reference rendering.
    for string in [
        // Markup and attribute delimiters.
        "<",
        ">",
        "&",
        "\"",
        "'",
        "=",
        "/",
        "`",
        // Markdown syntax, including table and line-start markers.
        "*",
        "_",
        "[",
        "]",
        "(",
        ")",
        "\\",
        "|",
        "#",
        "-",
        "+",
        "!",
        "~",
        ".",
        "1",
        // Whitespace and controls.
        " ",
        "\n",
        "\r",
        "\t",
        // Plain ASCII and 2, 3, and 4 byte UTF-8 scalars.
        "a",
        "é",
        "✓",
        "😀",
        "\u{00A0}",
        "\u{10FFFF}",
    ] {
        let mut scalars = string.chars();
        let value = scalars.next().unwrap();
        assert_eq!(
            scalars.next(),
            None,
            "fixture must be one scalar: {string:?}"
        );
        let what = format!("{value:?}");
        text_renders!(value).assert_same_as(&text_renders!(string), &what);
        assert_eq!(attr_renders!(value), attr_renders!(string), "attr {what}");
    }
}

#[tokio::test]
async fn char_markup_and_markdown_syntax_cannot_escape_its_context() {
    let lt = text_renders!('<');
    assert_eq!(lt.html(), ["&lt;"; 3]);
    assert_eq!(lt.xml(), ["&lt;"; 2]);
    let amp = text_renders!('&');
    assert_eq!(amp.html(), ["&amp;"; 3]);
    assert_eq!(amp.xml(), ["&amp;"; 2]);
    // A quote must not terminate the attribute it is rendered into.
    for rendered in attr_renders!('"') {
        assert_eq!(rendered.matches('"').count(), 2, "{rendered}");
    }
    // Emphasis, code, link, and table delimiters stay literal text.
    for (value, escaped) in [
        ('*', "\\*"),
        ('_', "\\_"),
        ('`', "\\`"),
        ('[', "\\["),
        ('|', "\\|"),
    ] {
        assert_eq!(text_renders!(value).md(), [escaped; 2], "{value:?}");
    }
    // Multi-byte scalars pass through every dialect unchanged.
    text_renders!('😀').assert_all("😀", "4-byte scalar");
    text_renders!('é').assert_all("é", "2-byte scalar");
}

#[tokio::test]
async fn optional_primitives_render_their_value_or_nothing() {
    let (some_int, none_int) = (Some(7_u8), None::<u8>);
    let (some_bool, some_char, some_float) = (Some(false), Some('<'), Some(-0.5_f32));
    let expected = "<p>7false&lt;-0.5</p>";
    assert_eq!(
        render_to_string(html_sync! {
            <p>{some_int}{none_int}{some_bool}{some_char}{some_float}</p>
        }),
        expected
    );
    assert_eq!(
        render_to_string_async(html! {
            <p>{some_int}{none_int}{some_bool}{some_char}{some_float}</p>
        })
        .await,
        expected
    );
    let mut send_cx = SendWebContext::new();
    render_in_send_context(
        &mut send_cx,
        html_send! { <p>{some_int}{none_int}{some_bool}{some_char}{some_float}</p> },
    )
    .await
    .unwrap();
    assert_eq!(send_cx.into_output().into_string(), expected);
    let mut md_cx = WebContext::with_buffer(Vec::new());
    MdRenderSync::render_md(
        md_sync! { "{some_int}{none_int}{some_bool}{some_char}{some_float}" },
        &mut md_cx,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(md_cx.into_output()).unwrap(),
        "7false\\<-0.5"
    );
}
