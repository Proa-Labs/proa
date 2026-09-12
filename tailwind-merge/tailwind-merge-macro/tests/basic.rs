// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use tailwind_merge_macro::tw_merge;

#[test]
fn merges_literals_at_compile_time() {
    const MERGED: &str = tw_merge!("px-2", "py-3", "px-4", "text-sm", "text-lg");
    assert_eq!(MERGED, "py-3 px-4 text-lg");
}
