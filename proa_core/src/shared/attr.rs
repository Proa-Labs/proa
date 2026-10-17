// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Supported attribute keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttrKey {
    // Global attributes
    Class,
    Id,
    Style,
    Title,
    Lang,
    Dir,
    Hidden,
    Tabindex,
    AccessKey,
    ContentEditable,
    Draggable,
    Spellcheck,
    Translate,
    Autofocus,
    Inputmode,
    Enterkeyhint,
    Inert,
    Popover,
    Is,
    Autocapitalize,
    Autocorrect,
    Writingsuggestions,

    // Microdata attributes
    Itemscope,
    Itemtype,
    Itemprop,
    Itemid,
    Itemref,

    // ARIA attributes
    Role,
    AriaLabel,
    AriaLabelledby,
    AriaDescribedby,
    AriaHidden,
    AriaLive,
    AriaAtomic,
    AriaBusy,
    AriaControls,
    AriaCurrent,
    AriaDisabled,
    AriaExpanded,
    AriaHaspopup,
    AriaInvalid,
    AriaModal,
    AriaMultiselectable,
    AriaOrientation,
    AriaPressed,
    AriaReadonly,
    AriaRequired,
    AriaSelected,
    AriaSort,
    AriaValuemin,
    AriaValuemax,
    AriaValuenow,
    AriaValuetext,

    // Popover API attributes
    Popovertarget,
    Popovertargetaction,

    // Command attributes
    Command,
    Commandfor,

    // Link/Navigation attributes
    Href,
    Hreflang,
    Target,
    Rel,
    Download,
    Ping,

    // Resource loading
    Fetchpriority,
    Color,

    // Media attributes
    Src,
    Alt,
    Srcset,
    Sizes,
    Width,
    Height,
    Loading,
    Decoding,
    Crossorigin,
    Referrerpolicy,
    Ismap,
    Usemap,
    Poster,
    Preload,
    Autoplay,
    Loop,
    Muted,
    Controls,
    Playsinline,

    // Form attributes
    Action,
    Method,
    Enctype,
    Autocomplete,
    Novalidate,
    Accept,
    AcceptCharset,

    // Form override attributes
    Formaction,
    Formmethod,
    Formenctype,
    Formnovalidate,
    Formtarget,

    // Input attributes
    Type,
    Name,
    Value,
    Placeholder,
    Required,
    Readonly,
    Disabled,
    Checked,
    Selected,
    Multiple,
    Min,
    Max,
    Step,
    Pattern,
    Minlength,
    Maxlength,
    Size,
    Cols,
    Rows,
    Wrap,
    For,
    Form,
    List,

    // Table attributes
    Colspan,
    Rowspan,
    Headers,
    Scope,
    Abbr,

    // Meta attributes
    Charset,
    Content,
    HttpEquiv,
    Property,

    // Script/Style attributes
    Async,
    Defer,
    Integrity,
    Nonce,
    Nomodule,
    Blocking,

    // Interactive attributes
    Open,
    Closedby,

    // Media query
    Media,

    // Link preload attributes
    As,
    Imagesrcset,
    Imagesizes,

    // Web Components
    SlotAttr,
    Shadowrootmode,
    Shadowrootdelegatesfocus,
    Shadowrootclonable,
    Shadowrootserializable,
    Shadowrootcustomelementregistry,

    // Section heading attributes
    Headingoffset,
    Headingreset,

    // List attributes
    Reversed,
    Start,
    SpanAttr,

    // Input attributes (additional)
    Dirname,
    Alpha,
    Colorspace,

    // Area/Map attributes
    Coords,
    Shape,

    // Object attributes
    ObjectData,

    // Iframe attributes
    Allow,
    Allowfullscreen,
    Sandbox,
    Srcdoc,

    // Canvas
    CanvasWidth,
    CanvasHeight,

    // Legacy / URL-bearing attributes kept for security-layer coverage
    // (matched by `is_url_attr()` — see CVE review Gap A).
    Manifest,
    Background,
    Longdesc,

    // Other common attributes
    Datetime,
    Cite,
    High,
    Low,
    Optimum,
    Kind,
    Label,
    Default,
    Srclang,

    // SVG attributes
    ViewBox,
    Fill,
    Stroke,
    StrokeWidth,
    StrokeLinecap,
    StrokeLinejoin,
    D,
    Xmlns,
    X,
    Y,
    Cx,
    Cy,
    R,
    Rx,
    Ry,
    X1,
    Y1,
    X2,
    Y2,
    Points,
    Transform,
    Opacity,
    FillOpacity,
    StrokeOpacity,
    PathLength,
}

impl AttrKey {
    #[inline(always)]
    pub fn as_str(self) -> &'static str {
        match self {
            // Global attributes
            AttrKey::Class => "class",
            AttrKey::Id => "id",
            AttrKey::Style => "style",
            AttrKey::Title => "title",
            AttrKey::Lang => "lang",
            AttrKey::Dir => "dir",
            AttrKey::Hidden => "hidden",
            AttrKey::Tabindex => "tabindex",
            AttrKey::AccessKey => "accesskey",
            AttrKey::ContentEditable => "contenteditable",
            AttrKey::Draggable => "draggable",
            AttrKey::Spellcheck => "spellcheck",
            AttrKey::Translate => "translate",
            AttrKey::Autofocus => "autofocus",
            AttrKey::Inputmode => "inputmode",
            AttrKey::Enterkeyhint => "enterkeyhint",
            AttrKey::Inert => "inert",
            AttrKey::Popover => "popover",
            AttrKey::Is => "is",
            AttrKey::Autocapitalize => "autocapitalize",
            AttrKey::Autocorrect => "autocorrect",
            AttrKey::Writingsuggestions => "writingsuggestions",

            // Microdata attributes
            AttrKey::Itemscope => "itemscope",
            AttrKey::Itemtype => "itemtype",
            AttrKey::Itemprop => "itemprop",
            AttrKey::Itemid => "itemid",
            AttrKey::Itemref => "itemref",

            // ARIA attributes
            AttrKey::Role => "role",
            AttrKey::AriaLabel => "aria-label",
            AttrKey::AriaLabelledby => "aria-labelledby",
            AttrKey::AriaDescribedby => "aria-describedby",
            AttrKey::AriaHidden => "aria-hidden",
            AttrKey::AriaLive => "aria-live",
            AttrKey::AriaAtomic => "aria-atomic",
            AttrKey::AriaBusy => "aria-busy",
            AttrKey::AriaControls => "aria-controls",
            AttrKey::AriaCurrent => "aria-current",
            AttrKey::AriaDisabled => "aria-disabled",
            AttrKey::AriaExpanded => "aria-expanded",
            AttrKey::AriaHaspopup => "aria-haspopup",
            AttrKey::AriaInvalid => "aria-invalid",
            AttrKey::AriaModal => "aria-modal",
            AttrKey::AriaMultiselectable => "aria-multiselectable",
            AttrKey::AriaOrientation => "aria-orientation",
            AttrKey::AriaPressed => "aria-pressed",
            AttrKey::AriaReadonly => "aria-readonly",
            AttrKey::AriaRequired => "aria-required",
            AttrKey::AriaSelected => "aria-selected",
            AttrKey::AriaSort => "aria-sort",
            AttrKey::AriaValuemin => "aria-valuemin",
            AttrKey::AriaValuemax => "aria-valuemax",
            AttrKey::AriaValuenow => "aria-valuenow",
            AttrKey::AriaValuetext => "aria-valuetext",

            // Popover API attributes
            AttrKey::Popovertarget => "popovertarget",
            AttrKey::Popovertargetaction => "popovertargetaction",

            // Command attributes
            AttrKey::Command => "command",
            AttrKey::Commandfor => "commandfor",

            // Link/Navigation attributes
            AttrKey::Href => "href",
            AttrKey::Hreflang => "hreflang",
            AttrKey::Target => "target",
            AttrKey::Rel => "rel",
            AttrKey::Download => "download",
            AttrKey::Ping => "ping",

            // Resource loading
            AttrKey::Fetchpriority => "fetchpriority",
            AttrKey::Color => "color",

            // Media attributes
            AttrKey::Src => "src",
            AttrKey::Alt => "alt",
            AttrKey::Srcset => "srcset",
            AttrKey::Sizes => "sizes",
            AttrKey::Width => "width",
            AttrKey::Height => "height",
            AttrKey::Loading => "loading",
            AttrKey::Decoding => "decoding",
            AttrKey::Crossorigin => "crossorigin",
            AttrKey::Referrerpolicy => "referrerpolicy",
            AttrKey::Ismap => "ismap",
            AttrKey::Usemap => "usemap",
            AttrKey::Poster => "poster",
            AttrKey::Preload => "preload",
            AttrKey::Autoplay => "autoplay",
            AttrKey::Loop => "loop",
            AttrKey::Muted => "muted",
            AttrKey::Controls => "controls",
            AttrKey::Playsinline => "playsinline",

            // Form attributes
            AttrKey::Action => "action",
            AttrKey::Method => "method",
            AttrKey::Enctype => "enctype",
            AttrKey::Autocomplete => "autocomplete",
            AttrKey::Novalidate => "novalidate",
            AttrKey::Accept => "accept",
            AttrKey::AcceptCharset => "accept-charset",

            // Form override attributes
            AttrKey::Formaction => "formaction",
            AttrKey::Formmethod => "formmethod",
            AttrKey::Formenctype => "formenctype",
            AttrKey::Formnovalidate => "formnovalidate",
            AttrKey::Formtarget => "formtarget",

            // Input attributes
            AttrKey::Type => "type",
            AttrKey::Name => "name",
            AttrKey::Value => "value",
            AttrKey::Placeholder => "placeholder",
            AttrKey::Required => "required",
            AttrKey::Readonly => "readonly",
            AttrKey::Disabled => "disabled",
            AttrKey::Checked => "checked",
            AttrKey::Selected => "selected",
            AttrKey::Multiple => "multiple",
            AttrKey::Min => "min",
            AttrKey::Max => "max",
            AttrKey::Step => "step",
            AttrKey::Pattern => "pattern",
            AttrKey::Minlength => "minlength",
            AttrKey::Maxlength => "maxlength",
            AttrKey::Size => "size",
            AttrKey::Cols => "cols",
            AttrKey::Rows => "rows",
            AttrKey::Wrap => "wrap",
            AttrKey::For => "for",
            AttrKey::Form => "form",
            AttrKey::List => "list",

            // Table attributes
            AttrKey::Colspan => "colspan",
            AttrKey::Rowspan => "rowspan",
            AttrKey::Headers => "headers",
            AttrKey::Scope => "scope",
            AttrKey::Abbr => "abbr",

            // Meta attributes
            AttrKey::Charset => "charset",
            AttrKey::Content => "content",
            AttrKey::HttpEquiv => "http-equiv",
            AttrKey::Property => "property",

            // Script/Style attributes
            AttrKey::Async => "async",
            AttrKey::Defer => "defer",
            AttrKey::Integrity => "integrity",
            AttrKey::Nonce => "nonce",
            AttrKey::Nomodule => "nomodule",
            AttrKey::Blocking => "blocking",

            // Interactive attributes
            AttrKey::Open => "open",
            AttrKey::Closedby => "closedby",

            // Media query
            AttrKey::Media => "media",

            // Link preload attributes
            AttrKey::As => "as",
            AttrKey::Imagesrcset => "imagesrcset",
            AttrKey::Imagesizes => "imagesizes",

            // Web Components
            AttrKey::SlotAttr => "slot",
            AttrKey::Shadowrootmode => "shadowrootmode",
            AttrKey::Shadowrootdelegatesfocus => "shadowrootdelegatesfocus",
            AttrKey::Shadowrootclonable => "shadowrootclonable",
            AttrKey::Shadowrootserializable => "shadowrootserializable",
            AttrKey::Shadowrootcustomelementregistry => "shadowrootcustomelementregistry",

            // Section heading attributes
            AttrKey::Headingoffset => "headingoffset",
            AttrKey::Headingreset => "headingreset",

            // List attributes
            AttrKey::Reversed => "reversed",
            AttrKey::Start => "start",
            AttrKey::SpanAttr => "span",

            // Input attributes (additional)
            AttrKey::Dirname => "dirname",
            AttrKey::Alpha => "alpha",
            AttrKey::Colorspace => "colorspace",

            // Area/Map attributes
            AttrKey::Coords => "coords",
            AttrKey::Shape => "shape",

            // Object attributes
            AttrKey::ObjectData => "data",

            // Iframe attributes
            AttrKey::Allow => "allow",
            AttrKey::Allowfullscreen => "allowfullscreen",
            AttrKey::Sandbox => "sandbox",
            AttrKey::Srcdoc => "srcdoc",

            // Canvas
            AttrKey::CanvasWidth => "width",
            AttrKey::CanvasHeight => "height",

            // Legacy / URL-bearing attributes
            AttrKey::Manifest => "manifest",
            AttrKey::Background => "background",
            AttrKey::Longdesc => "longdesc",

            // Other common attributes
            AttrKey::Datetime => "datetime",
            AttrKey::Cite => "cite",
            AttrKey::High => "high",
            AttrKey::Low => "low",
            AttrKey::Optimum => "optimum",
            AttrKey::Kind => "kind",
            AttrKey::Label => "label",
            AttrKey::Default => "default",
            AttrKey::Srclang => "srclang",

            // SVG attributes
            AttrKey::ViewBox => "viewBox",
            AttrKey::Fill => "fill",
            AttrKey::Stroke => "stroke",
            AttrKey::StrokeWidth => "stroke-width",
            AttrKey::StrokeLinecap => "stroke-linecap",
            AttrKey::StrokeLinejoin => "stroke-linejoin",
            AttrKey::D => "d",
            AttrKey::Xmlns => "xmlns",
            AttrKey::X => "x",
            AttrKey::Y => "y",
            AttrKey::Cx => "cx",
            AttrKey::Cy => "cy",
            AttrKey::R => "r",
            AttrKey::Rx => "rx",
            AttrKey::Ry => "ry",
            AttrKey::X1 => "x1",
            AttrKey::Y1 => "y1",
            AttrKey::X2 => "x2",
            AttrKey::Y2 => "y2",
            AttrKey::Points => "points",
            AttrKey::Transform => "transform",
            AttrKey::Opacity => "opacity",
            AttrKey::FillOpacity => "fill-opacity",
            AttrKey::StrokeOpacity => "stroke-opacity",
            AttrKey::PathLength => "pathLength",
        }
    }

    /// Returns `true` for attribute keys that accept URL values. Used by the
    /// security layer to validate URL schemes.
    ///
    /// Covers the common URL-bearing attributes (`href`, `src`, `action`,
    /// `formaction`) plus a broader set that can execute script or leak data
    /// on navigation: `poster`, `ping`, `cite`, `data` (on `<object>`),
    /// `background` (legacy `<body>`/`<table>`), `longdesc`, `manifest`.
    ///
    /// ## Why each one matters
    /// - `poster` (`<video>`): older browsers fetch `javascript:` URLs.
    /// - `ping` (`<a>`): exfiltration beacon after link activation.
    /// - `cite` (`<blockquote>`/`<q>`/`<del>`/`<ins>`): legitimate URL context; rarely user-controlled but worth guarding.
    /// - `data` (`<object>`): loads an embedded resource; `javascript:` executes.
    /// - `manifest` (legacy `<html>`): AppCache vector.
    /// - `background` / `longdesc`: obsolete but still parsed by legacy clients.
    ///
    /// Referenced by CVE-2026-32635 (Angular `i18n-href` sanitizer bypass),
    /// which demonstrated that missing one URL attribute in the allowlist is a
    /// complete XSS bypass.
    #[inline(always)]
    pub fn is_url_attr(self) -> bool {
        matches!(
            self,
            AttrKey::Href
                | AttrKey::Src
                | AttrKey::Action
                | AttrKey::Formaction
                | AttrKey::Poster
                | AttrKey::Ping
                | AttrKey::Cite
                | AttrKey::ObjectData
                | AttrKey::Background
                | AttrKey::Longdesc
                | AttrKey::Manifest
        )
    }
}

// Shared with template validation without making compilers depend on the renderer.
pub use proa_html_policy::has_dangerous_url_scheme;

// Pre-computed static byte slices for scatter-gather zero-copy rendering.
macro_rules! attr_static_bytes {
    ($($variant:ident => $name:literal),* $(,)?) => {
        impl AttrKey {
            /// Returns `b" attr=\""` — space + attr name + `="`.
            #[inline(always)]
            pub fn attr_open_bytes(self) -> &'static [u8] {
                match self {
                    $(AttrKey::$variant => concat!(" ", $name, "=\"").as_bytes(),)*
                }
            }

            /// Returns `b" attr"` — space + attr name (for boolean attrs).
            #[inline(always)]
            pub fn attr_bare_bytes(self) -> &'static [u8] {
                match self {
                    $(AttrKey::$variant => concat!(" ", $name).as_bytes(),)*
                }
            }
        }
    };
}

attr_static_bytes! {
    Class => "class",
    Id => "id",
    Style => "style",
    Title => "title",
    Lang => "lang",
    Dir => "dir",
    Hidden => "hidden",
    Tabindex => "tabindex",
    AccessKey => "accesskey",
    ContentEditable => "contenteditable",
    Draggable => "draggable",
    Spellcheck => "spellcheck",
    Translate => "translate",
    Autofocus => "autofocus",
    Inputmode => "inputmode",
    Enterkeyhint => "enterkeyhint",
    Inert => "inert",
    Popover => "popover",
    Is => "is",
    Autocapitalize => "autocapitalize",
    Autocorrect => "autocorrect",
    Writingsuggestions => "writingsuggestions",
    Itemscope => "itemscope",
    Itemtype => "itemtype",
    Itemprop => "itemprop",
    Itemid => "itemid",
    Itemref => "itemref",
    Role => "role",
    AriaLabel => "aria-label",
    AriaLabelledby => "aria-labelledby",
    AriaDescribedby => "aria-describedby",
    AriaHidden => "aria-hidden",
    AriaLive => "aria-live",
    AriaAtomic => "aria-atomic",
    AriaBusy => "aria-busy",
    AriaControls => "aria-controls",
    AriaCurrent => "aria-current",
    AriaDisabled => "aria-disabled",
    AriaExpanded => "aria-expanded",
    AriaHaspopup => "aria-haspopup",
    AriaInvalid => "aria-invalid",
    AriaModal => "aria-modal",
    AriaMultiselectable => "aria-multiselectable",
    AriaOrientation => "aria-orientation",
    AriaPressed => "aria-pressed",
    AriaReadonly => "aria-readonly",
    AriaRequired => "aria-required",
    AriaSelected => "aria-selected",
    AriaSort => "aria-sort",
    AriaValuemin => "aria-valuemin",
    AriaValuemax => "aria-valuemax",
    AriaValuenow => "aria-valuenow",
    AriaValuetext => "aria-valuetext",
    Popovertarget => "popovertarget",
    Popovertargetaction => "popovertargetaction",
    Command => "command",
    Commandfor => "commandfor",
    Href => "href",
    Hreflang => "hreflang",
    Target => "target",
    Rel => "rel",
    Download => "download",
    Ping => "ping",
    Fetchpriority => "fetchpriority",
    Color => "color",
    Src => "src",
    Alt => "alt",
    Srcset => "srcset",
    Sizes => "sizes",
    Width => "width",
    Height => "height",
    Loading => "loading",
    Decoding => "decoding",
    Crossorigin => "crossorigin",
    Referrerpolicy => "referrerpolicy",
    Ismap => "ismap",
    Usemap => "usemap",
    Poster => "poster",
    Preload => "preload",
    Autoplay => "autoplay",
    Loop => "loop",
    Muted => "muted",
    Controls => "controls",
    Playsinline => "playsinline",
    Action => "action",
    Method => "method",
    Enctype => "enctype",
    Autocomplete => "autocomplete",
    Novalidate => "novalidate",
    Accept => "accept",
    AcceptCharset => "accept-charset",
    Formaction => "formaction",
    Formmethod => "formmethod",
    Formenctype => "formenctype",
    Formnovalidate => "formnovalidate",
    Formtarget => "formtarget",
    Type => "type",
    Name => "name",
    Value => "value",
    Placeholder => "placeholder",
    Required => "required",
    Readonly => "readonly",
    Disabled => "disabled",
    Checked => "checked",
    Selected => "selected",
    Multiple => "multiple",
    Min => "min",
    Max => "max",
    Step => "step",
    Pattern => "pattern",
    Minlength => "minlength",
    Maxlength => "maxlength",
    Size => "size",
    Cols => "cols",
    Rows => "rows",
    Wrap => "wrap",
    For => "for",
    Form => "form",
    List => "list",
    Colspan => "colspan",
    Rowspan => "rowspan",
    Headers => "headers",
    Scope => "scope",
    Abbr => "abbr",
    Charset => "charset",
    Content => "content",
    HttpEquiv => "http-equiv",
    Property => "property",
    Async => "async",
    Defer => "defer",
    Integrity => "integrity",
    Nonce => "nonce",
    Nomodule => "nomodule",
    Blocking => "blocking",
    Open => "open",
    Closedby => "closedby",
    Media => "media",
    As => "as",
    Imagesrcset => "imagesrcset",
    Imagesizes => "imagesizes",
    SlotAttr => "slot",
    Shadowrootmode => "shadowrootmode",
    Shadowrootdelegatesfocus => "shadowrootdelegatesfocus",
    Shadowrootclonable => "shadowrootclonable",
    Shadowrootserializable => "shadowrootserializable",
    Shadowrootcustomelementregistry => "shadowrootcustomelementregistry",
    Headingoffset => "headingoffset",
    Headingreset => "headingreset",
    Reversed => "reversed",
    Start => "start",
    SpanAttr => "span",
    Dirname => "dirname",
    Alpha => "alpha",
    Colorspace => "colorspace",
    Coords => "coords",
    Shape => "shape",
    ObjectData => "data",
    Allow => "allow",
    Allowfullscreen => "allowfullscreen",
    Sandbox => "sandbox",
    Srcdoc => "srcdoc",
    CanvasWidth => "width",
    CanvasHeight => "height",
    Manifest => "manifest",
    Background => "background",
    Longdesc => "longdesc",
    Datetime => "datetime",
    Cite => "cite",
    High => "high",
    Low => "low",
    Optimum => "optimum",
    Kind => "kind",
    Label => "label",
    Default => "default",
    Srclang => "srclang",
    ViewBox => "viewBox",
    Fill => "fill",
    Stroke => "stroke",
    StrokeWidth => "stroke-width",
    StrokeLinecap => "stroke-linecap",
    StrokeLinejoin => "stroke-linejoin",
    D => "d",
    Xmlns => "xmlns",
    X => "x",
    Y => "y",
    Cx => "cx",
    Cy => "cy",
    R => "r",
    Rx => "rx",
    Ry => "ry",
    X1 => "x1",
    Y1 => "y1",
    X2 => "x2",
    Y2 => "y2",
    Points => "points",
    Transform => "transform",
    Opacity => "opacity",
    FillOpacity => "fill-opacity",
    StrokeOpacity => "stroke-opacity",
    PathLength => "pathLength",
}

/// Attribute representation with type-safe string and boolean variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attr<'a> {
    Str { k: AttrKey, v: &'a str },
    Bool { k: AttrKey, present: bool },
    Data { name: &'a str, v: &'a str },
}

impl<'a> Attr<'a> {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Attr::Data { name, .. } = self {
            if !is_valid_data_key(name) {
                return Err("data-* attribute names must start with `data-` followed by at least one character, and must not contain ASCII uppercase letters");
            }
        }
        Ok(())
    }
}

pub(crate) fn is_valid_data_key(name: &str) -> bool {
    if !name.starts_with("data-") || name.len() <= 5 {
        return false;
    }
    // Spec only prohibits ASCII uppercase (A-Z) in data-* attribute names.
    !name.as_bytes().iter().any(|c| c.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_attr_classification() {
        // Core set
        assert!(AttrKey::Href.is_url_attr());
        assert!(AttrKey::Src.is_url_attr());
        assert!(AttrKey::Action.is_url_attr());
        assert!(AttrKey::Formaction.is_url_attr());
        // Expanded set (CVE review Gap A).
        assert!(AttrKey::Poster.is_url_attr());
        assert!(AttrKey::Ping.is_url_attr());
        assert!(AttrKey::Cite.is_url_attr());
        assert!(AttrKey::ObjectData.is_url_attr());
        assert!(AttrKey::Background.is_url_attr());
        assert!(AttrKey::Longdesc.is_url_attr());
        assert!(AttrKey::Manifest.is_url_attr());

        // Non-URL attributes still excluded
        assert!(!AttrKey::Class.is_url_attr());
        assert!(!AttrKey::Id.is_url_attr());
        assert!(!AttrKey::Style.is_url_attr());
        assert!(!AttrKey::Value.is_url_attr());
    }
}
