// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Typed, zero-cost browser authoring handles for RSJS.
//!
//! Values in this crate are compiler handles. They carry Rust types while an
//! RSJS frontend checks and lowers browser-only code into Client IR; they do
//! not import a DOM runtime and do not require WebAssembly.
//!
//! A typed compiler handle can describe its matched element directly:
//!
//! ```
//! use rsjs_web::{dom, CurrentTarget};
//!
//! fn read(target: CurrentTarget<dom::HtmlInputElement>) -> String {
//!     target.value()
//! }
//! ```
//!
//! Capabilities are closed and statically checked. For example, a button does
//! not have the input-only `checked` capability:
//!
//! ```compile_fail
//! use rsjs_web::{dom, CurrentTarget};
//!
//! fn read(target: CurrentTarget<dom::HtmlButtonElement>) -> bool {
//!     target.checked()
//! }
//! ```
//!
//! Global event targets are not elements and therefore expose no element-only
//! operations:
//!
//! ```compile_fail
//! use rsjs_web::{dom, CurrentTarget};
//!
//! fn read(target: CurrentTarget<dom::Document>) -> String {
//!     target.id()
//! }
//! ```
//!
//! The broad `dom::Any` compatibility marker is element-only. Global event
//! bindings must name their exact `Document` or `Window` current target:
//!
//! ```compile_fail
//! use rsjs_web::{dom, EventCurrentTargetFor};
//!
//! fn require_document<T: EventCurrentTargetFor<dom::Document>>() {}
//!
//! fn main() {
//!     require_document::<dom::Any>();
//! }
//! ```
//!
//! HTML-only layout properties are not exposed on the generic `Element`
//! marker, which is also used for SVG elements:
//!
//! ```compile_fail
//! use rsjs_web::{dom, ElementRef};
//!
//! fn width(element: ElementRef<dom::Element>) -> i32 {
//!     element.offset_width()
//! }
//! ```
//!
//! Event families and template target types can be checked independently of a
//! proc macro's own name tables. Canonical rich event handles remain in `rsjs`
//! and implement this crate's [`EventParam`] and [`EventFor`] bridge traits.

#![forbid(unsafe_code)]

mod generated;
mod handles;

pub use generated::*;
pub use handles::{CurrentTarget, ElementRef};

#[cfg(test)]
mod tests {
    use super::*;
    use core::marker::PhantomData;
    use core::mem::{needs_drop, size_of};

    struct CanonicalInputEvent<E: dom::DomTargetMarker>(PhantomData<fn() -> E>);
    struct CanonicalMouseEvent<E: dom::DomTargetMarker>(PhantomData<fn() -> E>);
    struct CanonicalBaseEvent<E: dom::DomTargetMarker>(PhantomData<fn() -> E>);

    impl<E: dom::DomTargetMarker> EventParam for CanonicalInputEvent<E> {
        type CurrentTarget = E;
    }

    impl<E: dom::DomTargetMarker> EventFor<event_interface::InputEvent> for CanonicalInputEvent<E> {}

    impl<E: dom::DomTargetMarker> EventParam for CanonicalMouseEvent<E> {
        type CurrentTarget = E;
    }

    impl<E: dom::DomTargetMarker> EventFor<event_interface::MouseEvent> for CanonicalMouseEvent<E> {}

    impl<E: dom::DomTargetMarker> EventParam for CanonicalBaseEvent<E> {
        type CurrentTarget = E;
    }

    impl<E, Interface> EventFor<Interface> for CanonicalBaseEvent<E>
    where
        E: dom::DomTargetMarker,
        Interface: event_interface::Interface,
    {
    }

    fn assert_dom_target<E: dom::DomTargetMarker>() {}
    fn assert_element<E: dom::ElementMarker>() {}
    fn assert_html_element<E: dom::HtmlElementMarker>() {}
    fn assert_value<E: dom::HasValue>() {}
    fn assert_checked<E: dom::HasChecked>() {}
    fn assert_target_assignable<Current, Target>()
    where
        Current: ElementTargetFor<Target>,
        Target: dom::ElementMarker,
    {
    }

    fn assert_event_target_assignable<Current, Target>()
    where
        Current: EventCurrentTargetFor<Target>,
        Target: dom::DomTargetMarker,
    {
    }

    #[test]
    fn generated_markers_expose_the_reviewed_hierarchy_and_capabilities() {
        assert_eq!(BINDING_SCHEMA_VERSION, 5);
        assert_dom_target::<dom::Document>();
        assert_dom_target::<dom::Window>();
        assert_element::<dom::Element>();
        assert_html_element::<dom::HtmlInputElement>();
        assert_value::<dom::HtmlButtonElement>();
        assert_value::<dom::HtmlTextAreaElement>();
        assert_checked::<dom::HtmlInputElement>();
        assert_target_assignable::<dom::HtmlElement, dom::HtmlDialogElement>();
        assert_target_assignable::<dom::HtmlInputElement, dom::HtmlInputElement>();
        assert_event_target_assignable::<dom::Document, dom::Document>();
        assert_event_target_assignable::<dom::Window, dom::Window>();
        assert_event_target_assignable::<dom::Any, dom::HtmlButtonElement>();
        assert_event_target_assignable::<dom::Element, dom::HtmlInputElement>();
        assert_event_target_assignable::<dom::HtmlElement, dom::HtmlDialogElement>();

        assert_eq!(
            <dom::HtmlInputElement as dom::DomTargetMarker>::INTERFACE,
            "HTMLInputElement"
        );
    }

    #[test]
    fn references_and_current_targets_are_zero_sized() {
        assert_eq!(size_of::<ElementRef<dom::HtmlInputElement>>(), 0);
        assert_eq!(size_of::<CurrentTarget<dom::HtmlInputElement>>(), 0);
        assert_eq!(size_of::<CurrentTarget<dom::Document>>(), 0);
        assert_eq!(size_of::<CurrentTarget<dom::Window>>(), 0);
        assert!(!needs_drop::<ElementRef<dom::HtmlInputElement>>());
        assert!(!needs_drop::<CurrentTarget<dom::HtmlInputElement>>());
        assert!(!needs_drop::<CurrentTarget<dom::Document>>());
        assert!(!needs_drop::<CurrentTarget<dom::Window>>());
    }

    #[test]
    fn both_handle_origins_share_capability_gating() {
        let element = ElementRef::<dom::HtmlInputElement>::__compiler_stub();
        let current = CurrentTarget::<dom::HtmlInputElement>::__compiler_stub();

        assert_eq!(element.id(), "");
        assert_eq!(element.value(), "");
        assert!(!element.checked());
        assert_eq!(current.id(), "");
        assert_eq!(current.value(), "");
        assert!(!current.checked());
        assert_eq!(current.get_attribute("data-id"), "");
        assert_eq!(current.client_width(), 0);
    }

    #[test]
    fn generated_operation_registry_is_unique_and_origin_aware() {
        let mut operations = std::collections::HashSet::new();
        let mut methods = std::collections::HashSet::new();
        for spec in DOM_OPERATIONS {
            assert!(operations.insert(spec.operation));
            assert!(methods.insert(spec.method));
            for alias in spec.aliases {
                assert!(methods.insert(alias));
            }
            assert_eq!(find_dom_operation(spec.method), Some(spec));
        }

        let value = find_dom_operation("value").expect("value operation");
        assert_eq!(value.kind, DomOperationKind::Read);
        assert!(value.supports(DomHandleOrigin::NodeRef));
        assert!(value.supports(DomHandleOrigin::CurrentTarget));

        let click = find_dom_operation("click").expect("click operation");
        assert_eq!(click.kind, DomOperationKind::Effect);
        assert!(click.supports(DomHandleOrigin::NodeRef));
        assert!(!click.supports(DomHandleOrigin::CurrentTarget));
    }

    #[test]
    fn generated_tag_registry_uses_the_canonical_marker_identity() {
        let input = find_dom_tag("INPUT").expect("input tag");
        assert_eq!(input.element, DomElementKind::HtmlInputElement);
        assert_eq!(input.element.as_rust_marker_name(), "HtmlInputElement");
        assert_eq!(input.element.interface(), "HTMLInputElement");
        assert!(find_dom_tag("custom-widget").is_none());
    }

    #[test]
    fn event_witnesses_match_interface_and_concrete_target() {
        assert_eq!(
            EventInterface::InputEvent.as_rust_marker_name(),
            "InputEvent"
        );
        assert_event_type::<
            dom::HtmlInputElement,
            event_interface::InputEvent,
            CanonicalInputEvent<dom::HtmlInputElement>,
        >();
        assert_event_type::<
            dom::HtmlButtonElement,
            event_interface::MouseEvent,
            CanonicalMouseEvent<dom::HtmlButtonElement>,
        >();
        assert_event_type::<
            dom::HtmlInputElement,
            event_interface::InputEvent,
            CanonicalBaseEvent<dom::Element>,
        >();
        assert_event_type::<
            dom::Document,
            event_interface::MouseEvent,
            CanonicalMouseEvent<dom::Document>,
        >();
        assert_event_type::<dom::Window, event_interface::Event, CanonicalBaseEvent<dom::Window>>();
    }

    #[test]
    fn current_target_uses_the_shared_dom_marker() {
        let target = CurrentTarget::<dom::HtmlInputElement>::__compiler_stub();

        assert_eq!(target.value(), "");
    }

    #[test]
    fn short_element_names_are_compatibility_aliases() {
        assert_eq!(size_of::<dom::Input>(), size_of::<dom::HtmlInputElement>());
        assert_eq!(
            size_of::<dom::HTMLInputElement>(),
            size_of::<dom::HtmlInputElement>()
        );
        assert_eq!(
            <dom::Input as dom::DomTargetMarker>::INTERFACE,
            <dom::HtmlInputElement as dom::DomTargetMarker>::INTERFACE
        );
    }
}
