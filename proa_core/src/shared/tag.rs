// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::mode::ContentMode;

/// HTML tag representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tag {
    // Document structure
    Html,
    Head,
    Body,
    Title,
    Meta,
    Link,
    Base,

    // Content sectioning
    Header,
    Footer,
    Nav,
    Main,
    Section,
    Article,
    Aside,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    Hgroup,
    Address,

    // Text content
    Div,
    P,
    Hr,
    Pre,
    Blockquote,
    Ol,
    Ul,
    Li,
    Dl,
    Dt,
    Dd,
    Figure,
    Figcaption,

    // Inline text semantics
    A,
    Abbr,
    B,
    Bdi,
    Bdo,
    Br,
    Cite,
    Code,
    Data,
    Dfn,
    Em,
    I,
    Kbd,
    Mark,
    Q,
    Rp,
    Rt,
    Ruby,
    S,
    Samp,
    Small,
    Span,
    Strong,
    Sub,
    Sup,
    Time,
    U,
    Var,
    Wbr,

    // Image and multimedia
    Img,
    Audio,
    Video,
    Track,
    Source,
    Picture,
    Canvas,
    Map,
    Area,

    // Embedded content
    Iframe,
    Embed,
    Object,
    Param,

    // Scripting
    Script,
    Noscript,
    Template,
    Slot,

    // Demarcating edits
    Del,
    Ins,

    // Table content
    Table,
    Caption,
    Colgroup,
    Col,
    Tbody,
    Thead,
    Tfoot,
    Tr,
    Td,
    Th,

    // Forms
    Form,
    Label,
    Input,
    Button,
    Select,
    Datalist,
    Optgroup,
    Option,
    Textarea,
    Output,
    Progress,
    Meter,
    Fieldset,
    Legend,

    // Interactive elements
    Details,
    Summary,
    Dialog,
    Menu,
    Search,
    Selectedcontent,

    // Web Components
    // (Slot and Template already listed above)

    // SVG elements
    Svg,
    Path,
    Circle,
    Rect,
    Line,
    Polyline,
    Polygon,
    Ellipse,
    G,
    Defs,
    Use,
    Symbol,
    ClipPath,
    Mask,
    Pattern,
    LinearGradient,
    RadialGradient,
    Stop,
    Image,
    Animate,
    AnimateMotion,
    AnimateTransform,
    Set,
    Text,
    Tspan,
    TextPath,
    ForeignObject,

    // Other
    Style,
}

impl Tag {
    #[inline(always)]
    pub fn as_str(self) -> &'static str {
        match self {
            // Document structure
            Tag::Html => "html",
            Tag::Head => "head",
            Tag::Body => "body",
            Tag::Title => "title",
            Tag::Meta => "meta",
            Tag::Link => "link",
            Tag::Base => "base",

            // Content sectioning
            Tag::Header => "header",
            Tag::Footer => "footer",
            Tag::Nav => "nav",
            Tag::Main => "main",
            Tag::Section => "section",
            Tag::Article => "article",
            Tag::Aside => "aside",
            Tag::H1 => "h1",
            Tag::H2 => "h2",
            Tag::H3 => "h3",
            Tag::H4 => "h4",
            Tag::H5 => "h5",
            Tag::H6 => "h6",
            Tag::Hgroup => "hgroup",
            Tag::Address => "address",

            // Text content
            Tag::Div => "div",
            Tag::P => "p",
            Tag::Hr => "hr",
            Tag::Pre => "pre",
            Tag::Blockquote => "blockquote",
            Tag::Ol => "ol",
            Tag::Ul => "ul",
            Tag::Li => "li",
            Tag::Dl => "dl",
            Tag::Dt => "dt",
            Tag::Dd => "dd",
            Tag::Figure => "figure",
            Tag::Figcaption => "figcaption",

            // Inline text semantics
            Tag::A => "a",
            Tag::Abbr => "abbr",
            Tag::B => "b",
            Tag::Bdi => "bdi",
            Tag::Bdo => "bdo",
            Tag::Br => "br",
            Tag::Cite => "cite",
            Tag::Code => "code",
            Tag::Data => "data",
            Tag::Dfn => "dfn",
            Tag::Em => "em",
            Tag::I => "i",
            Tag::Kbd => "kbd",
            Tag::Mark => "mark",
            Tag::Q => "q",
            Tag::Rp => "rp",
            Tag::Rt => "rt",
            Tag::Ruby => "ruby",
            Tag::S => "s",
            Tag::Samp => "samp",
            Tag::Small => "small",
            Tag::Span => "span",
            Tag::Strong => "strong",
            Tag::Sub => "sub",
            Tag::Sup => "sup",
            Tag::Time => "time",
            Tag::U => "u",
            Tag::Var => "var",
            Tag::Wbr => "wbr",

            // Image and multimedia
            Tag::Img => "img",
            Tag::Audio => "audio",
            Tag::Video => "video",
            Tag::Track => "track",
            Tag::Source => "source",
            Tag::Picture => "picture",
            Tag::Canvas => "canvas",
            Tag::Map => "map",
            Tag::Area => "area",

            // Embedded content
            Tag::Iframe => "iframe",
            Tag::Embed => "embed",
            Tag::Object => "object",
            Tag::Param => "param",

            // Scripting
            Tag::Script => "script",
            Tag::Noscript => "noscript",
            Tag::Template => "template",
            Tag::Slot => "slot",

            // Demarcating edits
            Tag::Del => "del",
            Tag::Ins => "ins",

            // Table content
            Tag::Table => "table",
            Tag::Caption => "caption",
            Tag::Colgroup => "colgroup",
            Tag::Col => "col",
            Tag::Tbody => "tbody",
            Tag::Thead => "thead",
            Tag::Tfoot => "tfoot",
            Tag::Tr => "tr",
            Tag::Td => "td",
            Tag::Th => "th",

            // Forms
            Tag::Form => "form",
            Tag::Label => "label",
            Tag::Input => "input",
            Tag::Button => "button",
            Tag::Select => "select",
            Tag::Datalist => "datalist",
            Tag::Optgroup => "optgroup",
            Tag::Option => "option",
            Tag::Textarea => "textarea",
            Tag::Output => "output",
            Tag::Progress => "progress",
            Tag::Meter => "meter",
            Tag::Fieldset => "fieldset",
            Tag::Legend => "legend",

            // Interactive elements
            Tag::Details => "details",
            Tag::Summary => "summary",
            Tag::Dialog => "dialog",
            Tag::Menu => "menu",
            Tag::Search => "search",
            Tag::Selectedcontent => "selectedcontent",

            // SVG elements
            Tag::Svg => "svg",
            Tag::Path => "path",
            Tag::Circle => "circle",
            Tag::Rect => "rect",
            Tag::Line => "line",
            Tag::Polyline => "polyline",
            Tag::Polygon => "polygon",
            Tag::Ellipse => "ellipse",
            Tag::G => "g",
            Tag::Defs => "defs",
            Tag::Use => "use",
            Tag::Symbol => "symbol",
            Tag::ClipPath => "clipPath",
            Tag::Mask => "mask",
            Tag::Pattern => "pattern",
            Tag::LinearGradient => "linearGradient",
            Tag::RadialGradient => "radialGradient",
            Tag::Stop => "stop",
            Tag::Image => "image",
            Tag::Animate => "animate",
            Tag::AnimateMotion => "animateMotion",
            Tag::AnimateTransform => "animateTransform",
            Tag::Set => "set",
            Tag::Text => "text",
            Tag::Tspan => "tspan",
            Tag::TextPath => "textPath",
            Tag::ForeignObject => "foreignObject",

            // Other
            Tag::Style => "style",
        }
    }

    #[inline(always)]
    pub fn is_void(self) -> bool {
        matches!(
            self,
            Tag::Area
                | Tag::Base
                | Tag::Br
                | Tag::Col
                | Tag::Embed
                | Tag::Hr
                | Tag::Img
                | Tag::Input
                | Tag::Link
                | Tag::Meta
                | Tag::Param
                | Tag::Source
                | Tag::Track
                | Tag::Wbr
                // SVG void elements
                | Tag::Path
                | Tag::Circle
                | Tag::Rect
                | Tag::Line
                | Tag::Ellipse
                | Tag::Polyline
                | Tag::Polygon
                | Tag::Stop
                | Tag::Use
                | Tag::Image
                | Tag::Animate
                | Tag::AnimateMotion
                | Tag::AnimateTransform
                | Tag::Set
        )
    }

    #[inline(always)]
    pub(crate) fn mode(self) -> ContentMode {
        match self {
            Tag::Script => ContentMode::Script,
            Tag::Style => ContentMode::Style,
            _ => ContentMode::Html,
        }
    }
}

// Pre-computed static byte slices for scatter-gather zero-copy rendering.
// Each arm returns a &'static [u8] baked into the binary's read-only section.
macro_rules! tag_static_bytes {
    ($($variant:ident => $name:literal),* $(,)?) => {
        impl Tag {
            /// Returns `b"<tag"` — the opening `<` fused with the tag name.
            #[inline(always)]
            pub fn open_bytes(self) -> &'static [u8] {
                match self {
                    $(Tag::$variant => concat!("<", $name).as_bytes(),)*
                }
            }

            /// Returns `b"</tag>"` — the full closing tag.
            #[inline(always)]
            pub fn close_bytes(self) -> &'static [u8] {
                match self {
                    $(Tag::$variant => concat!("</", $name, ">").as_bytes(),)*
                }
            }
        }
    };
}

tag_static_bytes! {
    Html => "html",
    Head => "head",
    Body => "body",
    Title => "title",
    Meta => "meta",
    Link => "link",
    Base => "base",
    Header => "header",
    Footer => "footer",
    Nav => "nav",
    Main => "main",
    Section => "section",
    Article => "article",
    Aside => "aside",
    H1 => "h1",
    H2 => "h2",
    H3 => "h3",
    H4 => "h4",
    H5 => "h5",
    H6 => "h6",
    Hgroup => "hgroup",
    Address => "address",
    Div => "div",
    P => "p",
    Hr => "hr",
    Pre => "pre",
    Blockquote => "blockquote",
    Ol => "ol",
    Ul => "ul",
    Li => "li",
    Dl => "dl",
    Dt => "dt",
    Dd => "dd",
    Figure => "figure",
    Figcaption => "figcaption",
    A => "a",
    Abbr => "abbr",
    B => "b",
    Bdi => "bdi",
    Bdo => "bdo",
    Br => "br",
    Cite => "cite",
    Code => "code",
    Data => "data",
    Dfn => "dfn",
    Em => "em",
    I => "i",
    Kbd => "kbd",
    Mark => "mark",
    Q => "q",
    Rp => "rp",
    Rt => "rt",
    Ruby => "ruby",
    S => "s",
    Samp => "samp",
    Small => "small",
    Span => "span",
    Strong => "strong",
    Sub => "sub",
    Sup => "sup",
    Time => "time",
    U => "u",
    Var => "var",
    Wbr => "wbr",
    Img => "img",
    Audio => "audio",
    Video => "video",
    Track => "track",
    Source => "source",
    Picture => "picture",
    Canvas => "canvas",
    Map => "map",
    Area => "area",
    Iframe => "iframe",
    Embed => "embed",
    Object => "object",
    Param => "param",
    Script => "script",
    Noscript => "noscript",
    Template => "template",
    Slot => "slot",
    Del => "del",
    Ins => "ins",
    Table => "table",
    Caption => "caption",
    Colgroup => "colgroup",
    Col => "col",
    Tbody => "tbody",
    Thead => "thead",
    Tfoot => "tfoot",
    Tr => "tr",
    Td => "td",
    Th => "th",
    Form => "form",
    Label => "label",
    Input => "input",
    Button => "button",
    Select => "select",
    Datalist => "datalist",
    Optgroup => "optgroup",
    Option => "option",
    Textarea => "textarea",
    Output => "output",
    Progress => "progress",
    Meter => "meter",
    Fieldset => "fieldset",
    Legend => "legend",
    Details => "details",
    Summary => "summary",
    Dialog => "dialog",
    Menu => "menu",
    Search => "search",
    Selectedcontent => "selectedcontent",
    Svg => "svg",
    Path => "path",
    Circle => "circle",
    Rect => "rect",
    Line => "line",
    Polyline => "polyline",
    Polygon => "polygon",
    Ellipse => "ellipse",
    G => "g",
    Defs => "defs",
    Use => "use",
    Symbol => "symbol",
    ClipPath => "clipPath",
    Mask => "mask",
    Pattern => "pattern",
    LinearGradient => "linearGradient",
    RadialGradient => "radialGradient",
    Stop => "stop",
    Image => "image",
    Animate => "animate",
    AnimateMotion => "animateMotion",
    AnimateTransform => "animateTransform",
    Set => "set",
    Text => "text",
    Tspan => "tspan",
    TextPath => "textPath",
    ForeignObject => "foreignObject",
    Style => "style",
}
