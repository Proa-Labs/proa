// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Regression: a prelude that calls a server helper function whose name is
//! rebound to a signal further down must keep every server `let` it declared.
//!
//! The reactive name tables are built over the whole prelude, so before this
//! was scoped the call to `selected_label(..)` was read as a use of the
//! `selected_label` signal declared three lines later. That marked `label` (and
//! then `display_label`, derived from it) as client-derived, dropped both from
//! the server replay, and made the generated render fail to compile with
//! `cannot find value `display_label` in this scope`.
//!
//! Failing here means failing to compile, which is exactly the shape the bug
//! took in a downstream crate.

use proa_core::{Ctx, NoLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

/// Ordinary server helper. Its name is deliberately the one the island later
/// binds a signal to.
fn selected_label(options: &[&'static str], selected: Option<usize>) -> Option<&'static str> {
    selected.and_then(|index| options.get(index).copied())
}

struct LabelPicker {
    options: &'static [&'static str],
    selected: Option<usize>,
    placeholder: &'static str,
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for LabelPicker {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let label = selected_label(self.options, self.selected);
        let display_label = label.unwrap_or(self.placeholder);
        let selected_label = rsjs::signal(display_label.to_string());

        proa_macros::html_sync! {
            <output>{selected_label.get()}</output>
        }
        .render(cx)
    }
}

const OPTIONS: &[&str] = &["Slate", "Red", "Blue"];

#[test]
fn a_server_helper_call_survives_a_later_signal_of_the_same_name() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    LabelPicker {
        options: OPTIONS,
        selected: Some(2),
        placeholder: "Pick one",
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(
        html.contains(">Blue</output>"),
        "the server helper call must still run and seed the signal: {html}"
    );
}

#[test]
fn the_placeholder_path_still_runs_on_the_server() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    LabelPicker {
        options: OPTIONS,
        selected: None,
        placeholder: "Pick one",
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(
        html.contains(">Pick one</output>"),
        "the server local derived from the helper call must still render: {html}"
    );
}
