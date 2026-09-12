// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Hydration marker conventions shared by the JS and Rust emitters.
//!
//! Both `rsjs_ir::emit_js` (the client-side JS that wires `bind_text` /
//! `bind_event` against rendered DOM) and `rsjs_macros::emit_rust` (the
//! server-side `WebRenderSync` impl that writes the marker attributes
//! into the HTML byte stream) need to agree on the exact attribute names
//! they read and write. Promoting them to a single source of truth
//! prevents the kind of drift that would silently break hydration: a typo
//! in one emitter produces unmatched markers, which the runtime can only
//! report as a missing-node warning at runtime.
//!
//! ## What each marker means
//!
//! - [`MARKER_BIND`] (`data-rsjs-bind="<NodeId>"`) — written by the Rust
//!   emitter onto an element whose `textContent` is bound to a reactive
//!   expression. The JS emitter reads it via `root.querySelector` to
//!   resolve the DOM node for the matching `bind_text(...)` call.
//! - [`MARKER_HANDLER`] (`data-rsjs-handler="<NodeId>"`) — written onto
//!   an element with at least one event handler bound to it. Same lookup
//!   pattern as `MARKER_BIND`, on the JS side.
//! - [`ISLAND_WRAPPER_TAG`] (`rsjs-island`) — the HTML5 custom-element
//!   wrapper the server emits around an island's rendered tree. The JS
//!   runtime's `hydrate(name, init)` resolves roots via
//!   `document.querySelectorAll('rsjs-island[data-rsjs-island-root="<asset_id>"]')`.
//!   The data attribute is used instead of `id` so several instances of
//!   the same island can appear in valid HTML.
//! - [`ISLAND_MARKER_ATTR`] (`data-rsjs-island`) — the attribute that
//!   carries the asset id on the dehydrated state `<script>` tag (and,
//!   pre-Task-3, on the wrapper element when the parser still couldn't
//!   accept hyphenated tags). Kept for the script-tag case; the wrapper
//!   element now uses [`ISLAND_ROOT_ATTR`].
//!
//! ## Why these are `&'static str`, not enums
//!
//! The two emitters embed these strings into byte streams (HTML, JS).
//! `&'static str` is the cheapest representation and lets callers
//! interpolate without a `.to_string()` allocation.

/// `data-rsjs-bind="<NodeId>"` — text-binding marker on an element.
pub const MARKER_BIND: &str = "data-rsjs-bind";

/// `data-rsjs-attr="<NodeId>"` — attribute-binding marker on an element.
pub const MARKER_ATTR: &str = "data-rsjs-attr";

/// `data-rsjs-handler="<NodeId>"` — event-handler marker on an element.
pub const MARKER_HANDLER: &str = "data-rsjs-handler";

/// `data-rsjs-ref="<NodeId>"` — node-ref marker on an element.
pub const MARKER_REF: &str = "data-rsjs-ref";

/// `data-rsjs-component="<asset_id>"` — reusable component-fragment root marker.
pub const MARKER_COMPONENT: &str = "data-rsjs-component";

/// `data-rsjs-component-mount="<NodeId>"` — per-parent mount marker for a
/// reusable component fragment. This is unique inside the parent island and
/// keeps selector fallback correct when loops disable static index walking.
pub const MARKER_COMPONENT_MOUNT: &str = "data-rsjs-component-mount";

/// `data-rsjs-input-value="<index>"` — input value-binding marker on an
/// `<input>` / `<textarea>` element. The JS emitter resolves the DOM
/// node by querying for this attribute and wires a two-way
/// `bind_input(node, "value", signal)` against it.
pub const MARKER_INPUT_VALUE: &str = "data-rsjs-input-value";

/// `data-rsjs-input-checked="<index>"` — input checked-binding marker
/// on a checkbox / radio `<input>`. Two-way: writes to the input
/// update the bound signal, signal updates re-render `node.checked`.
pub const MARKER_INPUT_CHECKED: &str = "data-rsjs-input-checked";

/// `data-rsjs-input-selected="<index>"` — selected-binding marker on a
/// `<select>` (or `<option>`) node. Two-way against `node.value` /
/// `node.selectedIndex` depending on the element kind.
pub const MARKER_INPUT_SELECTED: &str = "data-rsjs-input-selected";

/// `data-rsjs-list="<NodeId>"` — keyed-list container marker. Written
/// by the analyser as a static attribute on the element whose children
/// a `bind_list(...)` call owns; the JS emitter resolves the container
/// through the usual `walk_path` lookup against this marker.
pub const MARKER_LIST: &str = "data-rsjs-list";

/// `data-rsjs-key` — per-item key attribute on a keyed-list item root.
/// The Rust emitter renders the server-side key value here so the
/// runtime's `bind_list` adoption pass can pair each server-rendered
/// child with its initial item instead of re-creating the DOM.
pub const MARKER_LIST_KEY: &str = "data-rsjs-key";

/// `rsjs-island` — HTML5 custom-element tag the server emits as the
/// island root.
pub const ISLAND_WRAPPER_TAG: &str = "rsjs-island";

/// `data-rsjs-island-root="<asset_id>"` — attribute name that carries the
/// asset id on island wrapper roots. This intentionally avoids duplicate
/// HTML `id` attributes when a page renders the same island more than once.
pub const ISLAND_ROOT_ATTR: &str = "data-rsjs-island-root";

/// `data-rsjs-island` — attribute name that carries the asset id on the
/// dehydrated state `<script type="application/json">` blob.
pub const ISLAND_MARKER_ATTR: &str = "data-rsjs-island";
