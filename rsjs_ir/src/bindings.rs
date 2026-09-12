// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Per-NodeId side-tables connecting the lifted parse tree to the
//! rsjs reactive IR.
//!
//! v0.1 (M1-M9) tables:
//!
//! - `text_bindings` (M1): which text nodes display the value of
//!   which reactive expression.
//! - `event_bindings` (M1): which DOM events on which nodes invoke
//!   which handler.
//! - `conditional_sections` (M4): for each parent node that hosts one
//!   or more conditional view sections, the predicate [`ExprId`] plus
//!   the lists of child [`NodeId`]s that make up each `then` and
//!   (optional) `else` branch. Branch children are statically present
//!   in the view tree as ordinary children of the parent; the
//!   side-table tells the runtime "evaluate this expr; show one
//!   branch's nodes and hide the other's." Conditional rendering is a
//!   binding concern, not a structural one.
//! - `input_bindings` (M5): two-way bindings declared via
//!   `bind_value` / `bind_checked` / `bind_selected` on form inputs.
//!   Each entry pairs a node with an [`InputKind`] and the
//!   [`SignalId`] on the other end of the binding.
//! - `attr_bindings` (M10): one-way bindings declared via ordinary
//!   dynamic attributes such as `class={state.get()}` or
//!   `aria-expanded={open.get()}`. Each entry pairs a node + attribute
//!   name with an [`ExprId`].
//!
//! Loops and match arms (additional structural side-tables) are post-v0.1.

use std::collections::HashMap;

pub use rsjs_web::EventInterface;

use crate::types::{EventHandlerId, ExprId, NodeId, NodeRefId, SignalId};

/// The DOM event names recognised by the rsjs analyser.
///
/// This is still a closed event surface. Its source, DOM, and descriptor
/// spellings live on this type, while its [`EventInterface`] family comes from
/// the generated `rsjs_web` vocabulary. Component fragments, islands, and
/// macro frontends therefore consume one mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EventName {
    /// `on_click` — pointer click on the bound element.
    Click,
    /// `on_input` — input value changed (typed character, paste,
    /// etc.). Fires on every keystroke.
    Input,
    /// `on_change` — input value committed. Fires on blur for text
    /// inputs, immediately for checkboxes/selects.
    Change,
    /// `on_paste` — clipboard paste into the bound element.
    Paste,
    /// `on_focus` — focus entered the bound element.
    Focus,
    /// `on_blur` — focus left the bound element.
    Blur,
    /// `on_keydown` — keyboard key pressed.
    KeyDown,
    /// `on_keyup` — keyboard key released.
    KeyUp,
    /// `on_mousedown` — pointing-device button pressed.
    MouseDown,
    /// `on_mouseup` — pointing-device button released.
    MouseUp,
    /// `on_mousemove` — mouse pointer moved.
    MouseMove,
    /// `on_mouseover` — mouse pointer entered the element or one of
    /// its descendants. Bubbles, unlike `mouseenter`.
    MouseOver,
    /// `on_mouseout` — mouse pointer left the element or one of its
    /// descendants. Bubbles, unlike `mouseleave`.
    MouseOut,
    /// `on_scroll` — element scroll position changed.
    Scroll,
    /// `on_wheel` — wheel or trackpad scroll gesture.
    Wheel,
    /// `on_beforeinput` — edit intent before the DOM value mutates.
    BeforeInput,
    /// `on_compositionstart` — IME text composition started.
    CompositionStart,
    /// `on_compositionupdate` — IME text composition updated.
    CompositionUpdate,
    /// `on_compositionend` — IME text composition committed/cancelled.
    CompositionEnd,
    /// `on_dragstart` — drag operation started.
    DragStart,
    /// `on_dragenter` — dragged item entered the bound element.
    DragEnter,
    /// `on_dragover` — dragged item is over the bound element.
    DragOver,
    /// `on_dragleave` — dragged item left the bound element.
    DragLeave,
    /// `on_drop` — dragged item dropped on the bound element.
    Drop,
    /// `on_dragend` — drag operation ended.
    DragEnd,
    /// `on_toggle` — an element such as `<details>` or a popover changed open state.
    Toggle,
    /// `on_beforetoggle` — an element such as a popover is about to change open state.
    BeforeToggle,
    /// `on_animationstart` — a CSS animation started.
    AnimationStart,
    /// `on_animationiteration` — a CSS animation completed an iteration.
    AnimationIteration,
    /// `on_animationend` — a CSS animation completed.
    AnimationEnd,
    /// `on_animationcancel` — a CSS animation was cancelled before completion.
    AnimationCancel,
    /// `on_transitionrun` — a CSS transition was created.
    TransitionRun,
    /// `on_transitionstart` — a CSS transition started.
    TransitionStart,
    /// `on_transitionend` — a CSS transition completed.
    TransitionEnd,
    /// `on_transitioncancel` — a CSS transition was cancelled before completion.
    TransitionCancel,
    /// `on_pointerdown` — pointer button/contact began.
    PointerDown,
    /// `on_pointerup` — pointer button/contact ended.
    PointerUp,
    /// `on_pointermove` — pointer moved.
    PointerMove,
    /// `on_pointerover` — pointer entered the element or one of its
    /// descendants. Bubbles, unlike `pointerenter`.
    PointerOver,
    /// `on_pointerout` — pointer left the element or one of its
    /// descendants. Bubbles, unlike `pointerleave`.
    PointerOut,
    /// `on_pointerenter` — pointer entered the bound element. Does not bubble.
    PointerEnter,
    /// `on_pointerleave` — pointer left the bound element. Does not bubble.
    PointerLeave,
    /// `on_gotpointercapture` — element received pointer capture.
    GotPointerCapture,
    /// `on_lostpointercapture` — element lost pointer capture.
    LostPointerCapture,
    /// `on_touchstart` — one or more touch points began.
    TouchStart,
    /// `on_touchmove` — one or more touch points moved.
    TouchMove,
    /// `on_touchend` — one or more touch points ended.
    TouchEnd,
    /// `on_touchcancel` — one or more touch points were interrupted.
    TouchCancel,
    /// `on_document_click` — document-level click observed for this
    /// island instance.
    DocumentClick,
    /// `on_document_pointerdown` — document-level pointerdown observed
    /// for this island instance.
    DocumentPointerDown,
    /// `on_document_keydown` — document-level keydown observed for this
    /// island instance.
    DocumentKeyDown,
    /// `on_window_popstate` — browser history entry changed.
    WindowPopState,
    /// `on_window_hashchange` — location hash changed.
    WindowHashChange,
    /// `on_window_storage` — another document changed local/session storage.
    WindowStorage,
    /// `on_window_resize` — viewport resized.
    WindowResize,
    /// `on_window_scroll` — document viewport scrolled.
    WindowScroll,
    /// `on_submit` — form submission (typically on a `<form>` node).
    Submit,
}

impl EventName {
    /// Every source-level event supported by the RSJS descriptor format.
    pub const ALL: &'static [Self] = &[
        Self::Click,
        Self::Input,
        Self::Change,
        Self::Paste,
        Self::Focus,
        Self::Blur,
        Self::KeyDown,
        Self::KeyUp,
        Self::MouseDown,
        Self::MouseUp,
        Self::MouseMove,
        Self::MouseOver,
        Self::MouseOut,
        Self::Scroll,
        Self::Wheel,
        Self::BeforeInput,
        Self::CompositionStart,
        Self::CompositionUpdate,
        Self::CompositionEnd,
        Self::DragStart,
        Self::DragEnter,
        Self::DragOver,
        Self::DragLeave,
        Self::Drop,
        Self::DragEnd,
        Self::Toggle,
        Self::BeforeToggle,
        Self::AnimationStart,
        Self::AnimationIteration,
        Self::AnimationEnd,
        Self::AnimationCancel,
        Self::TransitionRun,
        Self::TransitionStart,
        Self::TransitionEnd,
        Self::TransitionCancel,
        Self::PointerDown,
        Self::PointerUp,
        Self::PointerMove,
        Self::PointerOver,
        Self::PointerOut,
        Self::PointerEnter,
        Self::PointerLeave,
        Self::GotPointerCapture,
        Self::LostPointerCapture,
        Self::TouchStart,
        Self::TouchMove,
        Self::TouchEnd,
        Self::TouchCancel,
        Self::DocumentClick,
        Self::DocumentPointerDown,
        Self::DocumentKeyDown,
        Self::WindowPopState,
        Self::WindowHashChange,
        Self::WindowStorage,
        Self::WindowResize,
        Self::WindowScroll,
        Self::Submit,
    ];

    /// Parse the canonical `on_*` source attribute name.
    pub fn from_attr_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|event| event.as_attr_name() == name)
    }

    /// Parse the stable spelling used in component descriptors.
    pub fn from_descriptor_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|event| event.as_descriptor_name() == name)
    }

    /// The most-specific browser event interface guaranteed at runtime.
    pub const fn interface(self) -> EventInterface {
        match self {
            Self::Click
            | Self::MouseDown
            | Self::MouseUp
            | Self::MouseMove
            | Self::MouseOver
            | Self::MouseOut
            | Self::DocumentClick => EventInterface::MouseEvent,
            Self::Input | Self::BeforeInput => EventInterface::InputEvent,
            Self::Change => EventInterface::ChangeEvent,
            Self::Paste => EventInterface::ClipboardEvent,
            Self::KeyDown | Self::KeyUp | Self::DocumentKeyDown => EventInterface::KeyboardEvent,
            Self::Wheel => EventInterface::WheelEvent,
            Self::CompositionStart | Self::CompositionUpdate | Self::CompositionEnd => {
                EventInterface::CompositionEvent
            }
            Self::DragStart
            | Self::DragEnter
            | Self::DragOver
            | Self::DragLeave
            | Self::Drop
            | Self::DragEnd => EventInterface::DragEvent,
            Self::Toggle | Self::BeforeToggle => EventInterface::ToggleEvent,
            Self::AnimationStart
            | Self::AnimationIteration
            | Self::AnimationEnd
            | Self::AnimationCancel => EventInterface::AnimationEvent,
            Self::TransitionRun
            | Self::TransitionStart
            | Self::TransitionEnd
            | Self::TransitionCancel => EventInterface::TransitionEvent,
            Self::PointerDown
            | Self::PointerUp
            | Self::PointerMove
            | Self::PointerOver
            | Self::PointerOut
            | Self::PointerEnter
            | Self::PointerLeave
            | Self::GotPointerCapture
            | Self::LostPointerCapture
            | Self::DocumentPointerDown => EventInterface::PointerEvent,
            Self::TouchStart | Self::TouchMove | Self::TouchEnd | Self::TouchCancel => {
                EventInterface::TouchEvent
            }
            Self::WindowPopState => EventInterface::PopStateEvent,
            Self::WindowHashChange => EventInterface::HashChangeEvent,
            Self::WindowStorage => EventInterface::StorageEvent,
            Self::Submit => EventInterface::SubmitEvent,
            Self::Focus | Self::Blur | Self::Scroll | Self::WindowResize | Self::WindowScroll => {
                EventInterface::Event
            }
        }
    }

    /// The DOM event name as it appears in `addEventListener`,
    /// e.g. `EventName::Click` -> `"click"`.
    pub const fn as_dom_name(self) -> &'static str {
        match self {
            EventName::Click => "click",
            EventName::Input => "input",
            EventName::Change => "change",
            EventName::Paste => "paste",
            EventName::Focus => "focus",
            EventName::Blur => "blur",
            EventName::KeyDown => "keydown",
            EventName::KeyUp => "keyup",
            EventName::MouseDown => "mousedown",
            EventName::MouseUp => "mouseup",
            EventName::MouseMove => "mousemove",
            EventName::MouseOver => "mouseover",
            EventName::MouseOut => "mouseout",
            EventName::Scroll => "scroll",
            EventName::Wheel => "wheel",
            EventName::BeforeInput => "beforeinput",
            EventName::CompositionStart => "compositionstart",
            EventName::CompositionUpdate => "compositionupdate",
            EventName::CompositionEnd => "compositionend",
            EventName::DragStart => "dragstart",
            EventName::DragEnter => "dragenter",
            EventName::DragOver => "dragover",
            EventName::DragLeave => "dragleave",
            EventName::Drop => "drop",
            EventName::DragEnd => "dragend",
            EventName::Toggle => "toggle",
            EventName::BeforeToggle => "beforetoggle",
            EventName::AnimationStart => "animationstart",
            EventName::AnimationIteration => "animationiteration",
            EventName::AnimationEnd => "animationend",
            EventName::AnimationCancel => "animationcancel",
            EventName::TransitionRun => "transitionrun",
            EventName::TransitionStart => "transitionstart",
            EventName::TransitionEnd => "transitionend",
            EventName::TransitionCancel => "transitioncancel",
            EventName::PointerDown => "pointerdown",
            EventName::PointerUp => "pointerup",
            EventName::PointerMove => "pointermove",
            EventName::PointerOver => "pointerover",
            EventName::PointerOut => "pointerout",
            EventName::PointerEnter => "pointerenter",
            EventName::PointerLeave => "pointerleave",
            EventName::GotPointerCapture => "gotpointercapture",
            EventName::LostPointerCapture => "lostpointercapture",
            EventName::TouchStart => "touchstart",
            EventName::TouchMove => "touchmove",
            EventName::TouchEnd => "touchend",
            EventName::TouchCancel => "touchcancel",
            EventName::DocumentClick => "click",
            EventName::DocumentPointerDown => "pointerdown",
            EventName::DocumentKeyDown => "keydown",
            EventName::WindowPopState => "popstate",
            EventName::WindowHashChange => "hashchange",
            EventName::WindowStorage => "storage",
            EventName::WindowResize => "resize",
            EventName::WindowScroll => "scroll",
            EventName::Submit => "submit",
        }
    }

    /// Stable event spelling used in serialized component descriptors.
    ///
    /// Local events use their DOM spelling. Document/window bindings keep
    /// their target prefix because several of them share the same DOM event
    /// name as local bindings.
    pub const fn as_descriptor_name(self) -> &'static str {
        match self {
            Self::DocumentClick => "document_click",
            Self::DocumentPointerDown => "document_pointerdown",
            Self::DocumentKeyDown => "document_keydown",
            Self::WindowPopState => "window_popstate",
            Self::WindowHashChange => "window_hashchange",
            Self::WindowStorage => "window_storage",
            Self::WindowResize => "window_resize",
            Self::WindowScroll => "window_scroll",
            event => event.as_dom_name(),
        }
    }

    /// The `on_*` source-level attribute name, e.g.
    /// `EventName::Click` -> `"on_click"`.
    pub const fn as_attr_name(self) -> &'static str {
        match self {
            EventName::Click => "on_click",
            EventName::Input => "on_input",
            EventName::Change => "on_change",
            EventName::Paste => "on_paste",
            EventName::Focus => "on_focus",
            EventName::Blur => "on_blur",
            EventName::KeyDown => "on_keydown",
            EventName::KeyUp => "on_keyup",
            EventName::MouseDown => "on_mousedown",
            EventName::MouseUp => "on_mouseup",
            EventName::MouseMove => "on_mousemove",
            EventName::MouseOver => "on_mouseover",
            EventName::MouseOut => "on_mouseout",
            EventName::Scroll => "on_scroll",
            EventName::Wheel => "on_wheel",
            EventName::BeforeInput => "on_beforeinput",
            EventName::CompositionStart => "on_compositionstart",
            EventName::CompositionUpdate => "on_compositionupdate",
            EventName::CompositionEnd => "on_compositionend",
            EventName::DragStart => "on_dragstart",
            EventName::DragEnter => "on_dragenter",
            EventName::DragOver => "on_dragover",
            EventName::DragLeave => "on_dragleave",
            EventName::Drop => "on_drop",
            EventName::DragEnd => "on_dragend",
            EventName::Toggle => "on_toggle",
            EventName::BeforeToggle => "on_beforetoggle",
            EventName::AnimationStart => "on_animationstart",
            EventName::AnimationIteration => "on_animationiteration",
            EventName::AnimationEnd => "on_animationend",
            EventName::AnimationCancel => "on_animationcancel",
            EventName::TransitionRun => "on_transitionrun",
            EventName::TransitionStart => "on_transitionstart",
            EventName::TransitionEnd => "on_transitionend",
            EventName::TransitionCancel => "on_transitioncancel",
            EventName::PointerDown => "on_pointerdown",
            EventName::PointerUp => "on_pointerup",
            EventName::PointerMove => "on_pointermove",
            EventName::PointerOver => "on_pointerover",
            EventName::PointerOut => "on_pointerout",
            EventName::PointerEnter => "on_pointerenter",
            EventName::PointerLeave => "on_pointerleave",
            EventName::GotPointerCapture => "on_gotpointercapture",
            EventName::LostPointerCapture => "on_lostpointercapture",
            EventName::TouchStart => "on_touchstart",
            EventName::TouchMove => "on_touchmove",
            EventName::TouchEnd => "on_touchend",
            EventName::TouchCancel => "on_touchcancel",
            EventName::DocumentClick => "on_document_click",
            EventName::DocumentPointerDown => "on_document_pointerdown",
            EventName::DocumentKeyDown => "on_document_keydown",
            EventName::WindowPopState => "on_window_popstate",
            EventName::WindowHashChange => "on_window_hashchange",
            EventName::WindowStorage => "on_window_storage",
            EventName::WindowResize => "on_window_resize",
            EventName::WindowScroll => "on_window_scroll",
            EventName::Submit => "on_submit",
        }
    }

    /// Whether this event binds to `document` instead of being delegated
    /// from the island root or attached directly to the source node.
    pub const fn is_document_event(self) -> bool {
        matches!(
            self,
            EventName::DocumentClick | EventName::DocumentPointerDown | EventName::DocumentKeyDown
        )
    }

    /// Whether this event binds to `window` instead of being delegated
    /// from the island root or attached directly to the source node.
    pub const fn is_window_event(self) -> bool {
        matches!(
            self,
            EventName::WindowPopState
                | EventName::WindowHashChange
                | EventName::WindowStorage
                | EventName::WindowResize
                | EventName::WindowScroll
        )
    }
}

/// A conditional view section: the predicate plus the lists of child
/// [`NodeId`]s that make up the two branches.
///
/// Stored in [`Bindings::conditional_sections`] grouped by the
/// *parent* NodeId (the node whose direct children are partitioned
/// into branches). The parent is otherwise an ordinary
/// [`crate::island::ViewNode::Element`]; its `children` list contains
/// every node from every branch in source order, plus whatever
/// non-conditional siblings the parent also owns. The side-table
/// records which of those children belong to each branch; the
/// runtime swaps their visibility as each `condition` changes.
///
/// `else_children` is `Vec<NodeId>::new()` for the `if`-only shape
/// (no `else` branch). An empty vector and an absent table entry are
/// distinguishable to callers, but emit_js / emit_rust treat them
/// the same: nothing to render in the `else` slot.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalSection {
    /// Predicate expression. Lowers to a boolean-valued JS expression.
    pub condition: ExprId,
    /// NodeIds of children rendered when `condition` evaluates true.
    /// Source order matches their order under the parent.
    pub then_children: Vec<NodeId>,
    /// NodeIds of children rendered when `condition` evaluates false.
    /// Empty for the `if`-only shape (no `else` branch).
    pub else_children: Vec<NodeId>,
}

/// Which property of a form control a two-way input binding wires to.
///
/// The kinds correspond to the three authoring shapes M5 ships:
///
/// - [`InputKind::Value`] — `<input>` / `<textarea>` `value` property,
///   bound via `bind_value={signal}`. The signal is typically a
///   `String`. Updates fire on `input` and `change`.
/// - [`InputKind::Checked`] — `<input type="checkbox" | "radio">`
///   `checked` property, bound via `bind_checked={signal}`. The signal
///   is typically a `bool`. Updates fire on `change`.
/// - [`InputKind::Selected`] — `<select>` `value` property, bound via
///   `bind_selected={signal}`. The signal carries the selected
///   option's value. Updates fire on `change`.
///
/// The IR does not distinguish text inputs from textareas; both lower
/// the same way. M7 (the parser milestone) is responsible for picking
/// the right `InputKind` based on the source-level attribute name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputKind {
    /// `bind_value={signal}` — wire the `value` property.
    Value,
    /// `bind_checked={signal}` — wire the `checked` property.
    Checked,
    /// `bind_selected={signal}` — wire the `value` property of a
    /// `<select>`.
    Selected,
}

impl InputKind {
    /// The DOM property name this kind targets, e.g.
    /// `InputKind::Value` -> `"value"`. The JS runtime uses this to
    /// pick the right read/write path.
    pub const fn as_dom_property(self) -> &'static str {
        match self {
            InputKind::Value => "value",
            InputKind::Checked => "checked",
            InputKind::Selected => "value",
        }
    }

    /// The `bind_*` source-level attribute name, e.g.
    /// `InputKind::Value` -> `"bind_value"`. Used by the M7 parser to
    /// recognise the authoring shape; the IR carries the kind directly.
    pub const fn as_attr_name(self) -> &'static str {
        match self {
            InputKind::Value => "bind_value",
            InputKind::Checked => "bind_checked",
            InputKind::Selected => "bind_selected",
        }
    }
}

/// A two-way binding between a form control's DOM property and a
/// reactive signal.
///
/// Stored in [`Bindings::input_bindings`] keyed by the `<input>` /
/// `<textarea>` / `<select>` NodeId. Two-way semantics: when the user
/// edits the input, the runtime writes the new value into the signal;
/// when the signal changes (typically because some other code wrote
/// it), the runtime updates the input's DOM property to match.
///
/// The signal's initial value also drives the SSR-time output: the
/// `emit_rust` path renders `value="<initial>"` (text inputs) or
/// `checked` (checkboxes) so the rendered HTML matches what the
/// hydrated runtime will produce on first render.
#[derive(Debug, Clone, PartialEq)]
pub struct InputBinding {
    /// The form-control node this binding lives on.
    pub node: NodeId,
    /// Which DOM property the binding wires.
    pub kind: InputKind,
    /// The signal whose value the input mirrors.
    pub signal: SignalId,
}

/// How a one-way attribute binding should write to the DOM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AttrBindingKind {
    /// String-valued attributes: `class`, `aria-expanded`, `data-state`,
    /// `id`, and other ordinary attributes. The runtime stringifies
    /// non-null values and removes the attribute only for `null` /
    /// `undefined`.
    Value,
    /// HTML boolean attributes: `disabled`, `hidden`, `required`, etc.
    /// The runtime writes by presence/absence and mirrors the DOM
    /// property when the property exists.
    Boolean,
    /// Single URL-bearing attributes (`href`, `src`, `action`, ...).
    /// The runtime sanitizes dangerous schemes before writing the
    /// attribute so client updates match Proa's SSR URL guard.
    Url,
}

impl AttrBindingKind {
    /// Runtime discriminator consumed by `bind_attr(...)`.
    pub const fn as_runtime_kind(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Boolean => "boolean",
            Self::Url => "url",
        }
    }
}

/// A one-way binding between a reactive expression and an HTML attribute.
#[derive(Debug, Clone, PartialEq)]
pub struct AttrBinding {
    /// The element node this binding lives on.
    pub node: NodeId,
    /// Source-level HTML attribute name, e.g. `"class"` or
    /// `"aria-expanded"`.
    pub name: String,
    /// Which DOM write semantics to use.
    pub kind: AttrBindingKind,
    /// Expression whose value is written to the attribute.
    pub expr: ExprId,
}

/// A one-way binding between an author-declared node ref and a DOM node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRefBinding {
    /// Element node carrying `ref={...}`.
    pub node: NodeId,
    /// Node ref assigned to that DOM node during hydration.
    pub node_ref: NodeRefId,
}

/// All per-NodeId side-tables for an island.
///
/// Each table is keyed by [`NodeId`] (or, for events, by
/// `(NodeId, EventName)`) and references either an [`ExprId`] in
/// the island's [`crate::island::ExprArena`] or an
/// [`EventHandlerId`] in the island's handler list.
///
/// Tables are owned by [`crate::island::Island`]; the bindings
/// struct is a flat container, not its own arena owner. v0.1 ships
/// a HashMap-backed implementation; if iteration order or
/// allocation patterns become an issue, the implementation can swap
/// in `BTreeMap` or a denser `Vec`-of-pairs layout without changing
/// the public surface.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bindings {
    /// Text bindings: each entry says "the text content of this
    /// node is the value of this expression."
    pub text_bindings: HashMap<NodeId, ExprId>,
    /// Event bindings: each entry says "this DOM event on this
    /// node invokes this handler."
    pub event_bindings: HashMap<(NodeId, EventName), EventHandlerId>,
    /// Conditional sections: each parent maps to one or more
    /// conditionals. Every section says "show this list of children
    /// when this predicate is true, this other list when false."
    pub conditional_sections: HashMap<NodeId, Vec<ConditionalSection>>,
    /// Input bindings: each entry says "this `<input>` / `<textarea>`
    /// / `<select>` element is wired to this signal via this DOM
    /// property kind." Keyed by the form-control's NodeId. Two-way:
    /// the runtime writes signal -> DOM property and vice versa.
    pub input_bindings: HashMap<NodeId, InputBinding>,
    /// One-way attribute bindings keyed by `(node, attr_name)`.
    /// Attribute names are part of the key so a single element can
    /// bind multiple attributes while still rejecting duplicate writes
    /// to the same attribute.
    pub attr_bindings: HashMap<(NodeId, String), AttrBinding>,
    /// Node-ref bindings keyed by the element node carrying `ref={...}`.
    pub node_ref_bindings: HashMap<NodeId, NodeRefBinding>,
}

impl Bindings {
    /// Construct an empty side-table set.
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn event_attribute_vocabulary_is_unique_and_round_trips() {
        let mut attributes = HashSet::new();
        let mut descriptor_names = HashSet::new();
        for event in EventName::ALL {
            assert!(
                attributes.insert(event.as_attr_name()),
                "duplicate event attribute {}",
                event.as_attr_name()
            );
            assert_eq!(
                EventName::from_attr_name(event.as_attr_name()),
                Some(*event)
            );
            assert!(
                descriptor_names.insert(event.as_descriptor_name()),
                "duplicate descriptor event name {}",
                event.as_descriptor_name()
            );
            assert_eq!(
                EventName::from_descriptor_name(event.as_descriptor_name()),
                Some(*event)
            );
        }
        assert_eq!(EventName::from_attr_name("onclick"), None);
        assert_eq!(EventName::from_attr_name("on_input_typo"), None);
    }

    #[test]
    fn event_names_map_to_the_shared_generated_interface_vocabulary() {
        assert_eq!(EventName::Click.interface(), EventInterface::MouseEvent);
        assert_eq!(EventName::Input.interface(), EventInterface::InputEvent);
        assert_eq!(
            EventName::DocumentKeyDown.interface(),
            EventInterface::KeyboardEvent
        );
        assert_eq!(
            EventName::WindowStorage.interface(),
            EventInterface::StorageEvent
        );
        assert_eq!(
            EventName::WindowStorage.interface().as_rust_marker_name(),
            "StorageEvent"
        );
    }
}
