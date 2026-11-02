// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_macros::md_sync;

fn main() {
    // Arguments follow Rust ownership. Borrow an owned value to render it twice.
    let _ = proa_core::render_to_string(md_sync! {
        "{value} {value}", value = String::from("once")
    });
}
