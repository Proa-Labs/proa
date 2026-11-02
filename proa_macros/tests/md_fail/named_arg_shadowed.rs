// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_macros::md_sync;

fn main() {
    let _ = proa_core::render_to_string(md_sync! {
        "{if true { let value = 2; value }}", value = 1
    });
}
