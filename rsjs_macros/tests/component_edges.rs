// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Direct component syntax must use normal Rust name resolution.

use proa_core::{render_to_string, DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

struct SameModuleBadge;

#[rsjs::rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for SameModuleBadge {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <span data-edge-kind="same-module"></span>
        }
        .render(cx)
    }
}

mod definitions {
    use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

    pub struct OrdinaryImportedBadge;
    pub struct RenamedSourceBadge;
    pub struct ReexportSourceBadge;

    #[rsjs::rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for OrdinaryImportedBadge {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <span data-edge-kind="ordinary-import"></span>
            }
            .render(cx)
        }
    }

    #[rsjs::rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for RenamedSourceBadge {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <span data-edge-kind="renamed-import"></span>
            }
            .render(cx)
        }
    }

    #[rsjs::rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ReexportSourceBadge {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <span data-edge-kind="re-export"></span>
            }
            .render(cx)
        }
    }
}

use definitions::OrdinaryImportedBadge;
use definitions::RenamedSourceBadge as RenamedBadge;

mod public_api {
    pub use super::definitions::ReexportSourceBadge as PublicBadge;
}

struct EdgeParent;

#[rsjs::rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for EdgeParent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section data-component-edge-parent>
                {SameModuleBadge {}}
                {OrdinaryImportedBadge {}}
                {RenamedBadge {}}
                {public_api::PublicBadge {}}
            </section>
        }
        .render(cx)
    }
}

struct InteractiveEdgeChild;

#[rsjs::rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for InteractiveEdgeChild {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = rsjs::signal(3_i64);
        proa_macros::html_sync! {
            <button
                type="button"
                data-interactive-edge-child
                on_click={|_| count.update(|value| value + 1_i64)}
            >
                {count.get()}
            </button>
        }
        .render(cx)
    }
}

struct OpaqueEdgeParent;

#[rsjs::rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for OpaqueEdgeParent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section data-opaque-edge-parent>{InteractiveEdgeChild {}}</section>
        }
        .render(cx)
    }
}

#[test]
fn direct_edges_compile_and_render_through_normal_rust_names() {
    let html = render_to_string(EdgeParent);

    assert!(html.contains("data-component-edge-parent"));
    assert_eq!(
        html.matches("data-edge-kind=").count(),
        4,
        "all four Rust-resolved component spellings should render: {html}"
    );
}

#[test]
fn descriptor_compilation_does_not_copy_a_child_client_graph_into_its_parent() {
    let html = render_to_string(OpaqueEdgeParent);

    assert!(html.contains("data-opaque-edge-parent"));
    assert!(html.contains("data-interactive-edge-child"));
    assert!(html.contains(">3</button>"), "{html}");

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest
            .islands
            .iter()
            .any(|entry| entry.asset_id.starts_with("interactiveedgechild_")),
        "the direct child must retain its own provisional client artifact"
    );
    assert!(
        manifest
            .islands
            .iter()
            .all(|entry| !entry.asset_id.starts_with("opaqueedgeparent_")),
        "descriptor/local compilation must not legacy-inline the discoverable child graph into \
         the parent"
    );
}
