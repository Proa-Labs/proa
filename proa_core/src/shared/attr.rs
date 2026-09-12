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

/// Check whether a URL value starts with a dangerous scheme that could lead to
/// script execution (XSS).
///
/// Returns `true` for `javascript:` and `vbscript:` schemes (case-insensitive),
/// plus executable `data:` MIME types. These values are never legitimate in
/// ordinary server-rendered URL attributes.
///
/// ## Why this exists
///
/// The `javascript:` pseudo-scheme (RFC 4266 informational, but implemented by
/// all browsers) allows arbitrary script execution when navigated to. An
/// attacker who controls an `href` value can inject `javascript:alert(...)` to
/// achieve Cross-Site Scripting (XSS). `vbscript:` is the legacy IE equivalent.
///
/// OWASP XSS Prevention Cheat Sheet, Rule #7 — "Avoid JavaScript URLs":
/// <https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html>
///
/// React adopted the same block in v16.9 (facebook/react#16382).
///
/// ## Normalization
///
/// The URL spec (WHATWG §4.3) directs browsers to strip ASCII tab, LF, and CR
/// from URL inputs before parsing. An attacker can exploit this by embedding
/// control characters inside the scheme prefix — e.g. `java\tscript:alert(1)`
/// or `\njavascript:alert(1)` — to defeat naive prefix matching. We therefore
/// skip ASCII whitespace (space, tab, LF, CR) AND NUL throughout the scheme
/// prefix during comparison, not just at the leading edge.
///
/// The HTML5 parser ALSO decodes numeric character references
/// (`&#NNN;` and `&#xHH;`) inside attribute values before URL parsing. An
/// attacker can therefore smuggle `&#106;avascript:alert(1)` or
/// `&#0000000106;avascript:alert(1)` past a naive string-prefix check. This
/// is exactly how CVE-2026-39315 bypassed the Unhead `hasDangerousProtocol()`
/// function. We perform one pass of numeric-entity decoding with no digit
/// cap and treat a non-ASCII decoded code point as a non-match (it cannot
/// spell an ASCII scheme or MIME type). The same normalization is applied to
/// the `data:` scheme, its MIME type, and the MIME boundary.
///
/// References:
/// - WHATWG URL §4.3 — ASCII tab/newline stripping
/// - HTML5 §13.2.2.5 — attribute value processing
/// - HTML5 §13.2.5.72 — numeric character reference decoding
/// - CVE-2026-39315 — Unhead HTML-entity padded scheme bypass
/// - OWASP XSS Filter Evasion Cheat Sheet — control-char bypass techniques
///
/// Marked `#[inline]` so the release build can fold the function into
/// `elem::write_attr`, which calls it on every URL-bearing attribute in
/// the hot render path.
#[inline]
pub fn has_dangerous_url_scheme(url: &str) -> bool {
    let bytes = url.as_bytes();
    // Cheap early-out: the vast majority of URL values start with `h`
    // (http/https), `/`, `#`, `.`, `?`, `m` (mailto), `t` (tel), etc. The
    // slow path only needs to run when the first non-control byte could
    // plausibly be the start of `javascript:`, `vbscript:`, or
    // `data:text/html` — i.e. the ASCII letters j/J, v/V, d/D, OR an
    // HTML entity reference `&#`. This skips the full scheme walk for
    // ~99% of real-world attributes without changing the guarantee for
    // adversarial inputs.
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b <= 0x20 {
            i += 1;
            continue;
        }
        if !matches!(b, b'j' | b'J' | b'v' | b'V' | b'd' | b'D' | b'&') {
            return false;
        }
        break;
    }
    prefix_matches_ignoring_controls(bytes, b"javascript:")
        || prefix_matches_ignoring_controls(bytes, b"vbscript:")
        || is_dangerous_data_url(bytes)
}

/// Detect `data:` URLs whose MIME type is HTML-executable. These can
/// host phishing pages on top-level navigation and execute scripts in
/// older browsers / specific contexts (`<iframe srcdoc>`-adjacent
/// renderings). Raster image / audio / video / font data URLs are not blocked
/// because they have legitimate inline-content uses. SVG is blocked as an
/// executable XML container even when the particular payload looks inert.
///
/// Specifically blocks:
///   - `data:text/html` (and variants with charset / base64 params)
///   - `data:application/xhtml+xml`
///   - `data:application/xml` (rare but XML can host scripts via
///     SVG / XSL)
///   - `data:image/svg+xml`
///
/// The prior allowlist only blocked `javascript:` and `vbscript:`, leaving
/// `data:text/html,
/// <script>...</script>` as a viable XSS path.
fn is_dangerous_data_url(bytes: &[u8]) -> bool {
    let mut cursor = NormalizedAsciiCursor::new(bytes);
    if !cursor.consume_ascii_case_insensitive(b"data:") {
        return false;
    }

    let dangerous_mimes: &[&[u8]] = &[
        b"text/html",
        b"application/xhtml+xml",
        b"application/xhtml",
        b"application/xml",
        b"image/svg+xml", // svg can carry <script>; ban inline for v1
    ];
    for mime in dangerous_mimes {
        let mut candidate = cursor;
        let matches_mime = candidate.consume_ascii_case_insensitive(mime);
        let boundary = candidate.next();
        if matches_mime
            && (boundary.is_none()
                || boundary == Some(u32::from(b';'))
                || boundary == Some(u32::from(b',')))
        {
            return true;
        }
    }
    false
}

/// Allocation-free cursor over the browser-relevant normalized ASCII stream.
///
/// Raw and numeric-entity-decoded C0 controls are skipped. Numeric entities
/// are decoded once, including the HTML-permitted missing semicolon form. A
/// decoded non-ASCII scalar is returned as-is so an ASCII policy match fails
/// closed. Keeping the source index in the cursor lets `data:` MIME probes fork
/// after the scheme without reparsing it or maintaining a second, divergent
/// normalization loop.
#[derive(Clone, Copy)]
struct NormalizedAsciiCursor<'a> {
    input: &'a [u8],
    index: usize,
}

impl<'a> NormalizedAsciiCursor<'a> {
    #[inline(always)]
    const fn new(input: &'a [u8]) -> Self {
        Self { input, index: 0 }
    }

    #[inline(always)]
    fn next(&mut self) -> Option<u32> {
        while self.index < self.input.len() {
            let b = self.input[self.index];
            if b <= 0x20 {
                self.index += 1;
                continue;
            }

            if b == b'&' && self.input.get(self.index + 1) == Some(&b'#') {
                if let Some((code_point, consumed)) =
                    decode_numeric_entity(&self.input[self.index..])
                {
                    self.index += consumed;
                    if code_point <= 0x20 {
                        continue;
                    }
                    return Some(code_point);
                }
            }

            self.index += 1;
            return Some(u32::from(b));
        }
        None
    }

    #[inline(always)]
    fn consume_ascii_case_insensitive(&mut self, expected: &[u8]) -> bool {
        expected.iter().all(|expected| {
            self.next().is_some_and(|actual| {
                actual <= u32::from(u8::MAX) && (actual as u8).eq_ignore_ascii_case(expected)
            })
        })
    }
}

/// Case-insensitive prefix match that skips ASCII control characters (space,
/// tab, LF, CR, NUL) both at the leading edge and interleaved within the
/// scheme prefix, AND decodes numeric HTML entities (`&#NNN;`, `&#xHH;`)
/// before comparison. Matches the browser's URL-parse behaviour, which
/// strips these bytes and decodes these entities before scheme recognition.
#[inline(always)]
fn prefix_matches_ignoring_controls(input: &[u8], expected: &[u8]) -> bool {
    NormalizedAsciiCursor::new(input).consume_ascii_case_insensitive(expected)
}

/// Decode a numeric HTML entity from the start of `input`. Input must begin
/// with `&#`. Returns `(code_point, bytes_consumed)` on success.
///
/// HTML5 accepts both `&#NNN;` (decimal) and `&#xHH;` / `&#XHH;` (hex) with
/// an arbitrary number of digits and an optional trailing semicolon. Per
/// HTML5 §13.2.5.80 (numeric character reference end state):
///
/// - Value `0x00` becomes `U+FFFD` (null-character-reference parse error).
/// - Value `> 0x10FFFF` becomes `U+FFFD`.
/// - Surrogates (`0xD800..=0xDFFF`) become `U+FFFD`.
/// - Non-whitespace C0 controls (`0x01..=0x08`, `0x0B`, `0x0E..=0x1F`) are
///   kept as-is (parse error only).
/// - ASCII whitespace controls (`0x09`, `0x0A`, `0x0D`, `0x20`) are kept
///   as-is — these are later stripped by the URL parser (WHATWG URL §4.3)
///   which is why the caller must re-check for `<= 0x20` after decoding.
/// - C1 range `0x80..=0x9F` would normally be remapped via a Windows-1252
///   table, but we do not apply that remap: all values in this range are
///   `> 0x7F` and the caller's non-ASCII gate rejects them fail-closed,
///   matching real-browser behavior because no Windows-1252 remap target
///   is an ASCII letter anyway.
///
/// Saturating arithmetic prevents adversarial padding from overflowing
/// `u32` into a collision on a small ASCII target.
#[inline]
fn decode_numeric_entity(input: &[u8]) -> Option<(u32, usize)> {
    // Caller guarantees input[0..2] == b"&#".
    if input.len() < 3 {
        return None;
    }
    let (radix, start) = if input[2] == b'x' || input[2] == b'X' {
        (16u32, 3usize)
    } else {
        (10u32, 2usize)
    };
    let mut value: u32 = 0;
    let mut i = start;
    let mut digit_count = 0usize;
    while i < input.len() {
        let b = input[i];
        let digit = match (radix, b) {
            (10, b'0'..=b'9') => (b - b'0') as u32,
            (16, b'0'..=b'9') => (b - b'0') as u32,
            (16, b'a'..=b'f') => (b - b'a' + 10) as u32,
            (16, b'A'..=b'F') => (b - b'A' + 10) as u32,
            _ => break,
        };
        value = value.saturating_mul(radix).saturating_add(digit);
        if value > 0x10FFFF {
            value = 0xFFFD;
        }
        i += 1;
        digit_count += 1;
    }
    if digit_count == 0 {
        return None;
    }
    // Trailing `;` is optional per HTML5 (a parse error but still decoded).
    if i < input.len() && input[i] == b';' {
        i += 1;
    }
    // HTML5 §13.2.5.80 replacements.
    if value == 0 || (0xD800..=0xDFFF).contains(&value) {
        value = 0xFFFD;
    }
    Some((value, i))
}

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
    fn dangerous_url_scheme_detection() {
        // javascript: variants
        assert!(has_dangerous_url_scheme("javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("JavaScript:alert(1)"));
        assert!(has_dangerous_url_scheme("JAVASCRIPT:alert(1)"));
        assert!(has_dangerous_url_scheme("jAvAsCrIpT:void(0)"));

        // vbscript: variants
        assert!(has_dangerous_url_scheme("vbscript:MsgBox"));
        assert!(has_dangerous_url_scheme("VBScript:foo"));

        // leading whitespace bypass attempts
        assert!(has_dangerous_url_scheme("  javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("\tjavascript:alert(1)"));
        assert!(has_dangerous_url_scheme("\njavascript:alert(1)"));
        assert!(has_dangerous_url_scheme(" \t\n\rjavascript:alert(1)"));

        // Embedded control-character bypass attempts (browsers strip these
        // per WHATWG URL §4.3 before scheme parsing).
        assert!(has_dangerous_url_scheme("java\tscript:alert(1)"));
        assert!(has_dangerous_url_scheme("java\nscript:alert(1)"));
        assert!(has_dangerous_url_scheme("java\rscript:alert(1)"));
        assert!(has_dangerous_url_scheme("j a v a s c r i p t:alert(1)"));
        assert!(has_dangerous_url_scheme("\0javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("java\0script:alert(1)"));
        assert!(has_dangerous_url_scheme("vb\tscript:MsgBox"));

        // HTML numeric entity padding — CVE-2026-39315 class. HTML5 decodes
        // numeric entities with no digit cap before URL parsing.
        assert!(has_dangerous_url_scheme("&#106;avascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#0000000106;avascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#x6a;avascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#X6A;avascript:alert(1)"));
        // Missing trailing semicolon — still decoded per HTML5 §13.2.5.72.
        assert!(has_dangerous_url_scheme("&#106avascript:alert(1)"));
        // Every scheme char encoded.
        assert!(has_dangerous_url_scheme(
            "&#106;&#97;&#118;&#97;&#115;&#99;&#114;&#105;&#112;&#116;:alert(1)"
        ));
        // Mixed entities + control chars.
        assert!(has_dangerous_url_scheme("  &#106;\tavascript:\n\talert(1)"));
        // Overflow-pad attempt — saturating arithmetic clamps to U+FFFD
        // which is non-ASCII, so the check fails-closed on bogus padding.
        assert!(!has_dangerous_url_scheme(
            "&#99999999999999999999999999;avascript:alert(1)"
        ));
        // Non-ASCII code point cannot match ASCII expected char.
        assert!(!has_dangerous_url_scheme("&#x1F600;avascript:alert(1)"));
        // Malformed entity (no digits) — treated as a literal `&`.
        assert!(!has_dangerous_url_scheme("&#;javascript:alert(1)"));
        // `&amp;#106;avascript:` — because `&` comes first and doesn't match
        // `j`, this fails. Confirms that escaping `&` to `&amp;` in attribute
        // values is still the primary defense.
        assert!(!has_dangerous_url_scheme("&amp;#106;avascript:alert(1)"));

        // Entity-decoded ASCII controls (tab/LF/CR/space/NUL) — browsers
        // strip these AFTER entity decoding per WHATWG URL §4.3. Found in
        // code review; must be caught identically to raw control bytes.
        assert!(has_dangerous_url_scheme("&#9;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#10;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#13;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#013;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#32;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#x9;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#x0A;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#x0D;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("java&#9;script:alert(1)"));
        assert!(has_dangerous_url_scheme("java&#10;script:alert(1)"));
        assert!(has_dangerous_url_scheme("java&#13;script:alert(1)"));
        assert!(has_dangerous_url_scheme("&#106;&#9;&#97;vascript:alert(1)"));
        assert!(has_dangerous_url_scheme("vb&#9;script:MsgBox"));
        // Raw control + entity combo.
        assert!(has_dangerous_url_scheme("\t&#106;avascript:alert(1)"));
        // Decoded NUL — HTML5 replaces &#0; with U+FFFD which is non-ASCII,
        // so the check fails-closed. Browser behavior matches (URL starts
        // with replacement char, not `javascript:`).
        assert!(!has_dangerous_url_scheme("&#0;javascript:alert(1)"));

        // safe URLs
        assert!(!has_dangerous_url_scheme("https://example.com"));
        assert!(!has_dangerous_url_scheme("http://example.com"));
        assert!(!has_dangerous_url_scheme("/path/to/page"));
        assert!(!has_dangerous_url_scheme("#anchor"));
        assert!(!has_dangerous_url_scheme("mailto:user@example.com"));
        assert!(!has_dangerous_url_scheme("tel:+1234567890"));
        assert!(!has_dangerous_url_scheme(""));
        assert!(!has_dangerous_url_scheme("   "));

        // Substrings that should NOT match
        assert!(!has_dangerous_url_scheme("not-javascript:foo"));
        assert!(!has_dangerous_url_scheme(
            "https://example.com/javascript:foo"
        ));
    }

    /// Cover executable `data:` MIME types in the dangerous-scheme allowlist.
    /// Image / audio / video /
    /// font data URLs remain accepted (legitimate inline-content use).
    #[test]
    fn dangerous_data_url_detection() {
        const BLOCK: &[&str] = &[
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "java\tscript:alert(1)",
            "vbscript:msgbox(1)",
            "data:text/html,<script>alert(1)</script>",
            "data:text/html;base64,PHNjcmlwdD4=",
            "data:text/html;charset=utf-8,<b>x</b>",
            "data:application/xhtml+xml,<html/>",
            "data:application/xhtml,<html/>",
            "data:application/xml,<root/>",
            "data:image/svg+xml,<svg/>",
            " Data:\tText/HTML;base64,WA==",
            "d\0a\tt\ra:\ntext/html,x",
            "&#100;ata:text/html,x",
            "&#100ata:text/html,x",
            "data&#58;text/html,x",
            "d&#9;ata:text/html,x",
            "data:&#116;ext/html,x",
            "data:&#9;text/html,x",
            "data:t&#9;ext/html,x",
        ];
        const ALLOW: &[&str] = &[
            "https://example.com/x",
            "/relative",
            "#anchor",
            "mailto:a@b.c",
            "tel:+123",
            "data:text/plain,hello",
            "data:image/png;base64,iVBORw0K",
            "data:image/jpeg;base64,/9j/",
            "data:image/webp;base64,UklGR",
            "data:image/gif;base64,R0lGOD",
            "data:audio/mp3;base64,SUQzAw",
            "data:video/mp4;base64,AAAAGGZ",
            "data:font/woff;base64,d09GMg",
            "data:,",
            "data:base64,",
            "data:text/htmlx,x",
            "data:application/xml-dtd,x",
            "data:image/svg+xmlx,x",
            "&amp;#100;ata:text/html,x",
        ];

        for url in BLOCK {
            assert!(
                has_dangerous_url_scheme(url),
                "dangerous URL was allowed: {url:?}"
            );
        }
        for url in ALLOW {
            assert!(
                !has_dangerous_url_scheme(url),
                "safe URL was blocked: {url:?}"
            );
        }
    }

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
