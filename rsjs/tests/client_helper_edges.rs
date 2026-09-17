// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Browser-reachable helper calls use ordinary Rust path resolution.

use proa_core::{render_to_string, DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

#[rsjs::client]
fn same_module(value: i64) -> i64 {
    value + 1
}

mod definitions {
    #[rsjs::client]
    pub fn ordinary_import(value: i64) -> i64 {
        value + 2
    }

    #[rsjs::client]
    pub fn renamed_import(value: i64) -> i64 {
        value + 3
    }

    #[rsjs::client]
    pub fn reexported(value: i64) -> i64 {
        value + 4
    }
}

use definitions::ordinary_import;
use definitions::renamed_import as RenamedHelper;

mod public_api {
    pub use super::definitions::reexported as PublicHelper;
}

struct HelperPathProbe;

#[rsjs::rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for HelperPathProbe {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = rsjs::signal(1_i64);

        proa_macros::html_sync! {
            <section data-helper-path-probe>
                <output>{same_module(value.get())}</output>
                <output>{ordinary_import(value.get())}</output>
                <output>{RenamedHelper(value.get())}</output>
                <output>{public_api::PublicHelper(value.get())}</output>
            </section>
        }
        .render(cx)
    }
}

#[test]
fn helper_paths_compile_and_render_without_clients_allowlists() {
    let html = render_to_string(HelperPathProbe);

    assert!(html.contains("data-helper-path-probe"));
    for expected in [2, 3, 4, 5] {
        assert!(
            html.contains(&format!(">{expected}</output>")),
            "missing server result for helper path {expected}: {html}"
        );
    }
}
