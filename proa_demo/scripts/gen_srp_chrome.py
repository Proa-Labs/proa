#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Generate the pixel-faithful eBay SRP chrome for all engines.

Input: ../proa_showcase/src/ebay/data/srp_chrome_reference.html, a
visibility-pruned serialization of the captured eBay SRP (see docs in
bench/RESULTS.md). Before generation, the capture fixtures are rewritten
through the deterministic sanitizer below so scripts, inline handlers, and
other executable hooks can never reach a rendered showcase document.

Outputs:
  - ../proa_showcase/src/ebay/data/srp_real_style.css  page inline stylesheet
  - ../proa_showcase/src/ebay/srp_chrome_real.rs       Proa component
  - react-ssr/chrome.generated.tsx                     React component
  - marko-server/page_srp_real.marko                   full Marko template

Dynamic slots: page title, search box value, result count, query echo in the
heading, the river region (chips + cards, wrapped in #__app_root__), and a
tail slot before </body> for island scripts.

Run from proa_demo/: python3 scripts/gen_srp_chrome.py
"""

import json
import re
import sys
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SHOWCASE_ROOT = ROOT.parent / "proa_showcase"
EBAY_DATA = SHOWCASE_ROOT / "src/ebay/data"
REF = EBAY_DATA / "srp_chrome_reference.html"
CAPTURE_FIXTURES = (
    REF,
    EBAY_DATA / "shell_pre.html",
    EBAY_DATA / "shell_post.html",
)
_CAPTURE_IDENTIFIER_PREFIXES = {
    "parentrq": "fixture-parentrq-do-not-use-",
    "pageci": "fixture-pageci-do-not-use-",
}

VOID = {
    "area", "base", "br", "col", "embed", "hr", "img", "input",
    "link", "meta", "source", "track", "wbr",
}
SVG_VOID = {
    "path", "circle", "ellipse", "line", "polygon", "polyline", "rect",
    "stop", "use", "image", "animate", "animateMotion", "animateTransform", "set",
}
DROP_ATTRS = {"data-marko-key", "data-marko", "data-ebayui"}
MAX_ATTR_LEN = 240


# Capture safety is enforced on the source fixtures, not independently in the
# Rust/React/Marko emitters. That keeps every engine on exactly the same inert
# DOM and prevents a future emitter from accidentally reviving captured code.
_EXECUTABLE_CONTAINER_RE = re.compile(
    r"<(?P<tag>script|iframe|object|noscript|foreignObject)\b[^>]*>.*?</(?P=tag)\s*>",
    re.IGNORECASE | re.DOTALL,
)
_EXECUTABLE_VOID_RE = re.compile(r"<(?:embed)\b[^>]*>", re.IGNORECASE | re.DOTALL)
_EVENT_HANDLER_ATTR_RE = re.compile(
    r"\s+on[a-z][a-z0-9_.:-]*\s*=\s*(?:\"[^\"]*\"|'[^']*'|[^\s>]+)",
    re.IGNORECASE | re.DOTALL,
)
_EXECUTABLE_ATTR_RE = re.compile(
    r"\s+(?:srcdoc|ping|nonce)\s*=\s*(?:\"[^\"]*\"|'[^']*'|[^\s>]+)",
    re.IGNORECASE | re.DOTALL,
)
_DANGEROUS_URL_ATTR_RE = re.compile(
    r"""
    \s+(?:href|src|action|formaction|xlink:href)\s*=\s*
    (?:
        \"\s*(?:(?:javascript|vbscript)\s*:|data\s*:\s*text/html(?:\s*;[^,\"]*)?\s*,)[^\"]*\"
      | '\s*(?:(?:javascript|vbscript)\s*:|data\s*:\s*text/html(?:\s*;[^,']*)?\s*,)[^']*'
      | (?:(?:javascript|vbscript)\s*:|data\s*:\s*text/html(?:\s*;[^,\s>]*)?\s*,)[^\s>]*
    )
    """,
    re.IGNORECASE | re.VERBOSE,
)
_TAG_RE = re.compile(r"<(?:link|meta|img)\b[^>]*>", re.IGNORECASE | re.DOTALL)
_CAPTURE_IDENTIFIER_VALUE_RE = re.compile(
    r"""
    (?P<key_quote>&quot;|&\#34;|[\"'])
    (?P<key>parentrq|pageci)
    (?P=key_quote)
    (?P<separator>\s*:\s*)
    (?P<value_quote>&quot;|&\#34;|[\"'])
    (?P<value>[^&\"'<>\s]+)
    (?P=value_quote)
    """,
    re.IGNORECASE | re.VERBOSE,
)
_ESCAPED_CAPTURE_IDENTIFIER_VALUE_RE = re.compile(
    r"""
    (?P<key_quote>\\[\"'])
    (?P<key>parentrq|pageci)
    (?P=key_quote)
    (?P<separator>\s*:\s*)
    (?P<value_quote>\\[\"'])
    (?P<value>[^\\\"'<>\s]+)
    (?P=value_quote)
    """,
    re.IGNORECASE | re.VERBOSE,
)
_CAPTURE_IDENTIFIER_ATTR_RE = re.compile(
    r"""
    (?P<prefix>
        \bdata-(?P<key>parentrq|pageci)\s*=\s*
        (?P<value_quote>[\"'])
    )
    (?P<value>[^\"'<>\s]+)
    (?P=value_quote)
    """,
    re.IGNORECASE | re.VERBOSE,
)


def _capture_identifier_placeholder(key, value_length):
    key = key.lower()
    prefix = _CAPTURE_IDENTIFIER_PREFIXES[key]
    if value_length < len(prefix):
        raise RuntimeError(
            f"captured {key} value is too short for a labeled fixture placeholder"
        )
    return prefix + ("0" * (value_length - len(prefix)))


def _replace_capture_identifier_value(match):
    placeholder = _capture_identifier_placeholder(
        match.group("key"), len(match.group("value"))
    )
    return (
        match.group("key_quote")
        + match.group("key")
        + match.group("key_quote")
        + match.group("separator")
        + match.group("value_quote")
        + placeholder
        + match.group("value_quote")
    )


def _replace_capture_identifier_attr(match):
    placeholder = _capture_identifier_placeholder(
        match.group("key"), len(match.group("value"))
    )
    return match.group("prefix") + placeholder + match.group("value_quote")


def capture_identifier_values(source):
    """Return captured request-correlation keys and values from known encodings."""

    return [
        (match.group("key").lower(), match.group("value"))
        for pattern in (
            _CAPTURE_IDENTIFIER_VALUE_RE,
            _ESCAPED_CAPTURE_IDENTIFIER_VALUE_RE,
            _CAPTURE_IDENTIFIER_ATTR_RE,
        )
        for match in pattern.finditer(source)
    ]


def unplaceholdered_capture_identifiers(source):
    return [
        key
        for key, value in capture_identifier_values(source)
        if not value.startswith(_CAPTURE_IDENTIFIER_PREFIXES[key])
    ]


def _quoted_attr(tag, name):
    match = re.search(
        rf"\b{re.escape(name)}\s*=\s*(?:\"([^\"]*)\"|'([^']*)'|([^\s>]+))",
        tag,
        re.IGNORECASE | re.DOTALL,
    )
    if match is None:
        return ""
    return next((value for value in match.groups() if value is not None), "")


def _drop_network_or_navigation_hook(match):
    tag = match.group(0)
    lowered = tag.lower()
    if lowered.startswith("<link"):
        rel = _quoted_attr(tag, "rel").lower().split()
        as_kind = _quoted_attr(tag, "as").lower()
        network_hints = {"dns-prefetch", "preconnect", "compression-dictionary"}
        if (
            network_hints.intersection(rel)
            or "modulepreload" in rel
            or ("preload" in rel and as_kind == "script")
        ):
            return ""
    elif lowered.startswith("<meta"):
        if _quoted_attr(tag, "http-equiv").lower() == "refresh":
            return ""
        if _quoted_attr(tag, "name").lower() == "referrer":
            return '<meta name="referrer" content="no-referrer">'
    elif lowered.startswith("<img"):
        width = _quoted_attr(tag, "width")
        height = _quoted_attr(tag, "height")
        src = _quoted_attr(tag, "src").lower()
        known_beacon = any(
            marker in src
            for marker in (
                "1x1.gif",
                "/roverimp/",
                "marketingtracking/",
                "tr/p.gif",
                "/pixel",
            )
        )
        if known_beacon or (width == "1" and height == "1"):
            return ""
    return tag


def sanitize_capture_html(source):
    """Return an inert, deterministic capture serialization.

    This is intentionally a narrow textual sanitizer for trusted fixture
    captures, not a general-purpose HTML sanitizer. It removes every browser
    execution primitive present in (or plausibly introduced into) the source
    snapshots while preserving the visual DOM byte-for-byte otherwise.
    """

    previous = None
    sanitized = _CAPTURE_IDENTIFIER_VALUE_RE.sub(
        _replace_capture_identifier_value, source
    )
    sanitized = _ESCAPED_CAPTURE_IDENTIFIER_VALUE_RE.sub(
        _replace_capture_identifier_value, sanitized
    )
    sanitized = _CAPTURE_IDENTIFIER_ATTR_RE.sub(
        _replace_capture_identifier_attr, sanitized
    )
    # Repeat container removal to handle malformed/nested capture markup.
    while previous != sanitized:
        previous = sanitized
        sanitized = _EXECUTABLE_CONTAINER_RE.sub("", sanitized)
    sanitized = _EXECUTABLE_VOID_RE.sub("", sanitized)
    sanitized = _EVENT_HANDLER_ATTR_RE.sub("", sanitized)
    sanitized = _EXECUTABLE_ATTR_RE.sub("", sanitized)
    sanitized = _DANGEROUS_URL_ATTR_RE.sub("", sanitized)
    sanitized = _TAG_RE.sub(_drop_network_or_navigation_hook, sanitized)
    return sanitized


def executable_capture_hooks(source):
    """Describe executable hooks left after sanitization (empty is safe)."""

    checks = {
        "executable element": re.compile(
            r"<(?:script|iframe|object|embed|noscript|foreignObject)\b",
            re.IGNORECASE,
        ),
        "inline event handler": re.compile(
            r"\s+on[a-z][a-z0-9_.:-]*\s*=", re.IGNORECASE
        ),
        "executable attribute": re.compile(
            r"\s+(?:srcdoc|ping|nonce)\s*=", re.IGNORECASE
        ),
        "dangerous URL scheme": re.compile(
            r"(?:javascript|vbscript)\s*:|data\s*:\s*text/html(?:\s*;[^,\s>]*)?\s*,",
            re.IGNORECASE,
        ),
    }
    hooks = [name for name, pattern in checks.items() if pattern.search(source)]
    if any(
        _drop_network_or_navigation_hook(match) == ""
        for match in _TAG_RE.finditer(source)
    ):
        hooks.append("network or navigation hook")
    return hooks


def sanitize_capture_fixtures():
    for path in CAPTURE_FIXTURES:
        source = path.read_text()
        sanitized = sanitize_capture_html(source)
        hooks = executable_capture_hooks(sanitized)
        if hooks:
            raise RuntimeError(f"{path}: executable capture hooks remain: {', '.join(hooks)}")
        raw_identifiers = unplaceholdered_capture_identifiers(sanitized)
        if raw_identifiers:
            raise RuntimeError(
                f"{path}: raw capture identifiers remain: {', '.join(raw_identifiers)}"
            )
        if sanitized != source:
            path.write_text(sanitized)
            print(f"sanitized: {path.relative_to(ROOT.parent)}")


# HTML5 spec foreign-content case adjustments (12.2.6.5): Python's HTMLParser
# lowercases all names, so restore the SVG camelCase forms it destroyed.
SVG_TAG_CASE = {t.lower(): t for t in [
    "altGlyph", "altGlyphDef", "altGlyphItem", "animateColor", "animateMotion",
    "animateTransform", "clipPath", "feBlend", "feColorMatrix",
    "feComponentTransfer", "feComposite", "feConvolveMatrix",
    "feDiffuseLighting", "feDisplacementMap", "feDistantLight", "feDropShadow",
    "feFlood", "feFuncA", "feFuncB", "feFuncG", "feFuncR", "feGaussianBlur",
    "feImage", "feMerge", "feMergeNode", "feMorphology", "feOffset",
    "fePointLight", "feSpecularLighting", "feSpotLight", "feTile",
    "feTurbulence", "foreignObject", "glyphRef", "linearGradient",
    "radialGradient", "textPath",
]}
SVG_ATTR_CASE = {a.lower(): a for a in [
    "attributeName", "attributeType", "baseFrequency", "baseProfile",
    "calcMode", "clipPathUnits", "diffuseConstant", "edgeMode", "filterUnits",
    "glyphRef", "gradientTransform", "gradientUnits", "kernelMatrix",
    "kernelUnitLength", "keyPoints", "keySplines", "keyTimes", "lengthAdjust",
    "limitingConeAngle", "markerHeight", "markerUnits", "markerWidth",
    "maskContentUnits", "maskUnits", "numOctaves", "pathLength",
    "patternContentUnits", "patternTransform", "patternUnits", "pointsAtX",
    "pointsAtY", "pointsAtZ", "preserveAlpha", "preserveAspectRatio",
    "primitiveUnits", "refX", "refY", "repeatCount", "repeatDur",
    "requiredExtensions", "requiredFeatures", "specularConstant",
    "specularExponent", "spreadMethod", "startOffset", "stdDeviation",
    "stitchTiles", "surfaceScale", "systemLanguage", "tableValues", "targetX",
    "targetY", "textLength", "viewBox", "viewTarget", "xChannelSelector",
    "yChannelSelector", "zoomAndPan",
]}


class Node:
    __slots__ = ("tag", "attrs", "children")

    def __init__(self, tag, attrs):
        self.tag = SVG_TAG_CASE.get(tag, tag)
        self.attrs = [
            (SVG_ATTR_CASE.get(k, k), v if v is not None else "") for k, v in attrs
        ]
        self.children = []  # Node | str (text) | Slot


class Slot:
    __slots__ = ("name",)

    def __init__(self, name):
        self.name = name


class TreeParser(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.root = Node("#root", [])
        self.stack = [self.root]

    def _keep_attr(self, k, v):
        if k in DROP_ATTRS:
            return False
        # Length-filter only tracking/config payload attrs. Geometry and
        # styling attrs (path `d`, `points`, class lists, srcset, ...) are
        # legitimately long and must survive.
        if v and len(v) > MAX_ATTR_LEN and (k.startswith("data-") or k == "_sp"):
            return False
        return True

    def handle_starttag(self, tag, attrs):
        attrs = [(k, v) for k, v in attrs if self._keep_attr(k, v or "")]
        node = Node(tag, attrs)
        self.stack[-1].children.append(node)
        if tag not in VOID:
            self.stack.append(node)

    def handle_startendtag(self, tag, attrs):
        attrs = [(k, v) for k, v in attrs if self._keep_attr(k, v or "")]
        self.stack[-1].children.append(Node(tag, attrs))

    def handle_endtag(self, tag):
        for i in range(len(self.stack) - 1, 0, -1):
            if self.stack[i].tag == tag:
                del self.stack[i:]
                return

    def handle_data(self, data):
        if data:
            self.stack[-1].children.append(data)


def cls(node):
    for k, v in node.attrs:
        if k == "class":
            return v
    return ""


def find_first(node, pred):
    if isinstance(node, Node):
        if pred(node):
            return node
        for c in node.children:
            r = find_first(c, pred)
            if r is not None:
                return r
    return None


def find_parent_of(node, target):
    if isinstance(node, Node):
        for c in node.children:
            if c is target:
                return node
            r = find_parent_of(c, target)
            if r is not None:
                return r
    return None


def parse():
    src = REF.read_text()
    p = TreeParser()
    p.feed(src)
    html = find_first(p.root, lambda n: n.tag == "html")
    assert html is not None
    return html


def apply_slots(html):
    head = find_first(html, lambda n: n.tag == "head")
    body = find_first(html, lambda n: n.tag == "body")
    assert head and body

    # Title slot.
    title = find_first(head, lambda n: n.tag == "title")
    title.children = [Slot("title")]

    # Main stylesheet: extract to its own file, leave a slot.
    style = find_first(head, lambda n: n.tag == "style")
    css_text = "".join(c for c in style.children if isinstance(c, str))
    (EBAY_DATA / "srp_real_style.css").write_text(css_text)
    style.children = [Slot("style_main")]
    # Chips toolbar styling (our island) appended as a second style element.
    head.children.append(make_style_slot("style_chips"))

    # Search box: the global header search input.
    box = find_first(
        body, lambda n: n.tag == "input" and any(k == "type" and v == "text" for k, v in n.attrs)
    )
    if box is not None:
        box.attrs = [(k, v) for k, v in box.attrs if k != "value"]
        box.attrs.append(("value", Slot("query_attr")))

    # Results-count heading: first BOLD span = count, second BOLD span = query.
    heading = find_first(body, lambda n: "srp-controls__count-heading" in cls(n))
    bolds = []

    def collect_bolds(n):
        if isinstance(n, Node):
            if n.tag == "span" and cls(n) == "BOLD":
                bolds.append(n)
            for c in n.children:
                collect_bolds(c)

    collect_bolds(heading)
    if len(bolds) >= 2:
        bolds[0].children = [Slot("total")]
        bolds[1].children = [Slot("query")]

    # River: replace the empty results <ul> with the app-root + river slot.
    river_ul = find_first(body, lambda n: n.tag == "ul" and "srp-results" in cls(n))
    parent = find_parent_of(body, river_ul)
    app_root = Node("div", [("id", "__app_root__")])
    app_root.children = [Slot("river")]
    parent.children[parent.children.index(river_ul)] = app_root

    # Tail slot just before </body>.
    body.children.append(Slot("tail"))
    return html


def make_style_slot(name):
    n = Node("style", [])
    n.children = [Slot(name)]
    return n


def esc_text(t):
    return t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def esc_attr(t):
    return t.replace("&", "&amp;").replace('"', "&quot;")


def serialize(node, out, slot_token):
    """Serialize to HTML with ⟦slot⟧ placeholders."""
    if isinstance(node, str):
        out.append(esc_text(node))
        return
    if isinstance(node, Slot):
        out.append(slot_token(node.name))
        return
    if node.tag == "style" and node.children and all(isinstance(c, str) for c in node.children):
        out.append("<style>")
        out.append("".join(node.children))
        out.append("</style>")
        return
    out.append(f"<{node.tag}")
    for k, v in node.attrs:
        if isinstance(v, Slot):
            out.append(f' {k}="{slot_token(v.name)}"')
        elif v == "":
            out.append(f' {k}=""')
        else:
            out.append(f' {k}="{esc_attr(v)}"')
    if node.tag in VOID:
        out.append(">")
        return
    if node.tag in SVG_VOID and not node.children:
        out.append("/>")
        return
    out.append(">")
    for c in node.children:
        serialize(c, out, slot_token)
    out.append(f"</{node.tag}>")


SLOT_ORDER = []


def html_with_placeholders(html):
    out = []
    serialize(html, out, lambda name: f"⟦{name}⟧")
    return "<!DOCTYPE html>" + "".join(out)


# ---------------------------------------------------------------------------
# Rust emitter: static chunks + typed slots (the consolidated form html_sync!
# compiles to, generated directly so the 2,900-element template doesn't have
# to round-trip through the proc macro).
# ---------------------------------------------------------------------------

def rust_str(s):
    hashes = 1
    while ('"' + "#" * hashes) in s:
        hashes += 1
    h = "#" * hashes
    return f'r{h}"{s}"{h}'


def emit_rust(doc_html):
    parts = re.split(r"⟦([a-z_]+)⟧", doc_html)
    chunks, slots = [], []
    for i, part in enumerate(parts):
        if i % 2 == 0:
            chunks.append(part)
        else:
            slots.append(part)

    consts = "\n".join(
        f"const CHUNK_{i}: &str = {rust_str(c)};" for i, c in enumerate(chunks)
    )

    slot_code = {
        "title": 'text(self.query).render(cx)?;\n        StaticRaw(" for sale | eBay").render(cx)?;',
        "style_main": "StaticRaw(REAL_STYLE).render(cx)?;",
        "style_chips": "StaticRaw(CHIPS_STYLE).render(cx)?;",
        "query_attr": "escape_attr_value(cx.out, self.query)?;",
        "total": "Commas(self.total).render(cx)?;",
        "query": "text(self.query).render(cx)?;",
        "river": "river.render(cx)?;",
        "tail": "tail.render(cx)?;",
    }

    body = []
    river_done = tail_done = False
    for i, c in enumerate(chunks):
        body.append(f"        StaticRaw(CHUNK_{i}).render(cx)?;")
        if i < len(slots):
            name = slots[i]
            code = slot_code[name]
            if name == "river":
                code = "self.river.render(cx)?;"
            if name == "tail":
                code = "self.tail.render(cx)?;"
            body.append(f"        // slot: {name}")
            body.append(f"        {code}")
    body = "\n".join(body)

    out = f'''//! GENERATED by proa_demo/scripts/gen_srp_chrome.py from proa_showcase/src/ebay/data/srp_chrome_reference.html.
//! Do not edit by hand; re-run the generator instead.
//!
//! Pixel-faithful eBay SRP chrome: the captured page's real markup (scripts
//! and invisible overlays stripped) serialized into consolidated static
//! chunks with typed dynamic slots, exactly the form `html_sync!` compiles
//! to. Every byte is written per request through the same `WebRenderSync`
//! path as the rest of the page.

use super::srp::{{Commas, Listing}};
use proa_core::shared::escape::escape_attr_value;
use proa_core::shared::write_buf::WriteError as CoreWriteError;
use proa_core::{{text, StaticRaw, WebContext, WebRenderSync}};

pub const DEFAULT_QUERY: &str = "iphone 14 pro max";

const REAL_STYLE: &str = include_str!("data/srp_real_style.css");
const CHIPS_STYLE: &str = include_str!("data/srp_chips_style.css");

{consts}

/// Full SRP document with the captured eBay chrome. `river` renders inside
/// `#__app_root__` and `tail` renders just before `</body>` in the same pass.
/// The response owner finalizes the runtime artifacts recorded during render.
pub struct RealSrpDocument<'a, R, T> {{
    pub query: &'a str,
    pub total: u64,
    pub river: R,
    pub tail: T,
}}

impl<'a, R, T, L: ::proa_core::DataLoader> WebRenderSync<L>
    for RealSrpDocument<'a, R, T>
where
    R: WebRenderSync<L>,
    T: WebRenderSync<L>,
{{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), CoreWriteError> {{
{body}
        Ok(())
    }}
}}

/// Render the complete pixel-faithful SRP document in a single pass.
pub fn render_document<R, T>(query: &str, listings: &[Listing], river: R, tail: T) -> String
where
    R: WebRenderSync,
    T: WebRenderSync,
{{
    let total = 117_406 + (listings.len() as u64) * 7;
    let mut cx = WebContext::new();
    proa_core::WebRenderSync::render(
        RealSrpDocument {{
            query,
            total,
            river,
            tail,
        }},
        &mut cx,
    )
    .unwrap();
    cx.into_output().into_string()
}}
'''
    (SHOWCASE_ROOT / "src/ebay/srp_chrome_real.rs").write_text(out)
    print(f"rust: {len(chunks)} chunks, slots: {slots}")


# ---------------------------------------------------------------------------
# React emitter: createElement tree.
# ---------------------------------------------------------------------------

REACT_ATTR_MAP = {"class": "className", "for": "htmlFor", "xlink:href": "xlinkHref"}
# Browser serialization writes boolean attributes as `attr=""`; React and
# Marko both treat empty-string values as falsy and drop the attribute, so
# emit them as explicit booleans.
BOOL_ATTRS = {
    "hidden", "disabled", "checked", "selected", "readonly", "multiple",
    "autofocus", "required", "open", "controls", "loop", "muted", "async",
    "defer", "novalidate", "allowfullscreen", "itemscope", "inert",
}


def react_style_obj(css):
    props = {}
    for decl in css.split(";"):
        if ":" not in decl:
            continue
        k, v = decl.split(":", 1)
        k = k.strip()
        if k.startswith("--"):
            props[k] = v.strip()
        else:
            parts = k.split("-")
            props[parts[0] + "".join(p.title() for p in parts[1:])] = v.strip()
    return "{" + ", ".join(f"{json.dumps(k)}: {json.dumps(v)}" for k, v in props.items()) + "}"


def emit_react_node(node, buf, indent):
    pad = "  " * indent
    if isinstance(node, str):
        buf.append(pad + json.dumps(node) + ",")
        return
    if isinstance(node, Slot):
        slot_jsx = {
            "river": "p.river",
            "tail": "p.tail ?? null",
            "total": "p.totalFmt",
            "query": "p.query",
            "title": 'p.query + " for sale | eBay"',
        }
        buf.append(pad + slot_jsx[node.name] + ",")
        return
    props = []
    dangerous = None
    for k, v in node.attrs:
        if isinstance(v, Slot):
            props.append(f"{json.dumps(REACT_ATTR_MAP.get(k, k))}: p.query")
            continue
        if k == "style":
            props.append(f'"style": {react_style_obj(v)}')
            continue
        if v == "" and k in BOOL_ATTRS:
            props.append(f"{json.dumps(REACT_ATTR_MAP.get(k, k))}: true")
            continue
        props.append(f"{json.dumps(REACT_ATTR_MAP.get(k, k))}: {json.dumps(v)}")
    if node.tag == "style":
        inner = node.children[0]
        src = {"style_main": "p.styleMain", "style_chips": "p.styleChips"}[inner.name]
        buf.append(
            pad + f'h("style", {{dangerouslySetInnerHTML: {{__html: {src}}}}}),'
        )
        return
    if node.tag == "input":
        props = [pr.replace('"value":', '"defaultValue":').replace('"checked":', '"defaultChecked":') for pr in props]
    props_s = "{" + ", ".join(props) + "}" if props else "null"
    if not node.children:
        buf.append(pad + f"h({json.dumps(node.tag)}, {props_s}),")
        return
    buf.append(pad + f"h({json.dumps(node.tag)}, {props_s},")
    for c in node.children:
        emit_react_node(c, buf, indent + 1)
    buf.append(pad + "),")


def emit_react(html):
    body = find_first(html, lambda n: n.tag == "body")
    head = find_first(html, lambda n: n.tag == "head")
    # Body-level chrome only: html/head handled per app (react-ssr full doc,
    # next via its layout). Export the head assets (stylesheet links and
    # style blocks) in cascade order.
    parts = []
    for n in head.children:
        if not isinstance(n, Node):
            continue
        if n.tag == "link":
            attrs = {k: v for k, v in n.attrs if not isinstance(v, Slot)}
            parts.append({"link": attrs})
        elif n.tag == "style":
            inner = n.children[0] if n.children else ""
            if isinstance(inner, Slot):
                parts.append({"slot": inner.name})
            else:
                parts.append({"css": "".join(c for c in n.children if isinstance(c, str))})
    buf = []
    for c in body.children:
        emit_react_node(c, buf, 2)
    children = "\n".join(buf)
    links_src = json.dumps(parts, indent=2)
    body_class = next((v for k, v in body.attrs if k == "class"), "")

    body_class_json = json.dumps(body_class)
    out = f'''// GENERATED by scripts/gen_srp_chrome.py from proa_showcase/src/ebay/data/srp_chrome_reference.html.
// Do not edit by hand; re-run the generator instead.
//
// Pixel-faithful eBay SRP chrome as a React element tree: the captured
// page's real markup (scripts and invisible overlays stripped), rendered
// element-by-element per request like any React component.

import {{ createElement as h, Fragment }} from "react";
import type {{ ReactNode }} from "react";

export const DEFAULT_QUERY = "iphone 14 pro max";

// The captured page's <body> class list; eBay's stylesheets key layout on
// these (s-page, skin-large, ...), so every engine must carry them.
export const BODY_CLASS = {body_class_json};

type HeadPart = {{ link?: Record<string, string>; css?: string; slot?: string }};
export const HEAD_PARTS: HeadPart[] = {links_src};

/// Render the captured page's head assets (CSS links + style blocks) in
/// cascade order. `slot` parts are the main stylesheet and the chips
/// stylesheet, passed in by the caller.
export function RealHeadAssets(p: {{ styleMain: string; styleChips: string }}): ReactNode {{
  return h(Fragment, null,
    ...HEAD_PARTS.map((part, i) => {{
      if (part.link) return h("link", {{ key: i, ...part.link }});
      const css = part.slot === "style_main" ? p.styleMain : part.slot === "style_chips" ? p.styleChips : (part.css ?? "");
      return h("style", {{ key: i, dangerouslySetInnerHTML: {{ __html: css }} }});
    }}),
  );
}}

export interface RealChromeProps {{
  query: string;
  totalFmt: string;
  styleMain: string;
  styleChips: string;
  river: ReactNode;
  tail?: ReactNode;
}}

export function RealSrpChrome(p: RealChromeProps): ReactNode {{
  return h(Fragment, null,
{children}
  );
}}
'''
    (ROOT / "react-ssr/chrome.generated.tsx").write_text(out)
    print(f"react: emitted {out.count('h(')} createElement calls")


# ---------------------------------------------------------------------------
# Marko emitter: full template file with the existing river block spliced in.
# ---------------------------------------------------------------------------

def marko_text(t):
    return t.replace("$", "\\$")


def emit_marko_node(node, buf):
    if isinstance(node, str):
        buf.append(marko_text(node))
        return
    if isinstance(node, Slot):
        buf.append({
            "title": "${input.query} for sale | eBay",
            "total": "${input.totalFmt}",
            "query": "${input.query}",
            "river": "RIVER_PLACEHOLDER",
            "tail": '<html-script type="module" src="/marko-hydrate.js"></html-script>',
        }[node.name])
        return
    if node.tag == "style":
        inner = node.children[0] if node.children else ""
        if isinstance(inner, Slot):
            src = {"style_main": "input.styleMain", "style_chips": "input.styleChips"}[inner.name]
            buf.append(f"<html-style>$!{{{src}}}</html-style>")
        else:
            css = "".join(c for c in node.children if isinstance(c, str))
            buf.append("<html-style>" + marko_text(css) + "</html-style>")
        return
    is_script = node.tag == "script"
    tag = "html-script" if is_script else node.tag
    buf.append(f"<{tag}")
    for k, v in node.attrs:
        if isinstance(v, Slot):
            buf.append(f" {k}=input.query")
        elif v == "" and k in BOOL_ATTRS:
            buf.append(f" {k}")
        elif v == "":
            buf.append(f' {k}=""')
        else:
            buf.append(f" {k}={json.dumps(v)}")
    if node.tag in VOID:
        buf.append(">")
        return
    if not node.children and node.tag in SVG_VOID:
        buf.append("/>")
        return
    buf.append(">")
    for c in node.children:
        emit_marko_node(c, buf)
    buf.append(f"</{tag}>")


def emit_marko(html):
    current = (ROOT / "marko-server/page_srp_real.marko").read_text()
    m = re.search(r"(<let/buyItNow[\s\S]*?</ul>)", current)
    assert m, "river block not found in current page_srp_real.marko"
    river_block = m.group(1)
    cut = None
    for marker in ("// GENERATED chrome below", "<const/meta"):
        if marker in current:
            cut = current.index(marker)
            break
    assert cut is not None, "cannot locate imports/template boundary in page_srp_real.marko"
    imports = current[:cut]

    buf = []
    emit_marko_node(html, buf)
    doc = "".join(buf)
    doc = doc.replace("RIVER_PLACEHOLDER", "\n" + river_block + "\n")

    out = (
        imports.rstrip()
        + "\n\n// GENERATED chrome below (scripts/gen_srp_chrome.py); river block is hand-maintained.\n"
        + "<const/pages = 1/>\n"
        + doc
        + "\n"
    )
    (ROOT / "marko-server/page_srp_real.marko").write_text(out)
    print(f"marko: emitted {len(doc) // 1024}KB template")


def main():
    sanitize_capture_fixtures()
    html = parse()
    html = apply_slots(html)
    doc = html_with_placeholders(html)
    emit_rust(doc)
    emit_react(html)
    emit_marko(html)


if __name__ == "__main__":
    main()
