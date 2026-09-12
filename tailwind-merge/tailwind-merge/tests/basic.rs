// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use tailwind_merge::{tw_merge, Config, TailwindMerge};

#[test]
fn removes_conflicting_spacing_classes() {
    assert_eq!(tw_merge("px-2 py-3 px-4"), "py-3 px-4");
}

#[test]
fn respects_variant_ordering() {
    let mut merger = TailwindMerge::new(Config::clone_default());
    let merged = merger.merge("hover:text-red-500 hover:text-blue-500 text-sm");
    assert_eq!(merged, "hover:text-blue-500 text-sm");
}

#[test]
fn iter_merge_flattens_arguments() {
    let merged = tailwind_merge::tw_merge_iter(["px-2 px-4", "text-sm", "px-1"]);
    assert_eq!(merged, "text-sm px-1");
}
