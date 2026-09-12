// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use tailwind_merge::compile_time::{
    tw_merge_const, tw_merge_const_str, tw_merge_const_str_array, ConstMergeResult,
};

const CONST_CONFLICT_RESOLUTION: [&str; 3] = tw_merge_const::<3, 3>(["px-2", "px-4", "px-2"]);

const CLASS_BG_RED: &str = "bg-red-500";
const CLASS_BG_BLUE: &str = "bg-blue-500";
const CONST_WITH_CONSTS: [&str; 2] = tw_merge_const::<2, 2>([CLASS_BG_RED, CLASS_BG_BLUE]);

const CONST_FROM_STR: ConstMergeResult = tw_merge_const_str("px-2 px-4 px-2");

#[test]
fn const_merging_is_available() {
    assert_eq!(CONST_CONFLICT_RESOLUTION, ["px-2", "", ""]);
}

#[test]
fn const_merging_accepts_const_bindings() {
    assert_eq!(CONST_WITH_CONSTS, ["bg-blue-500", ""]);
}

#[test]
fn const_merging_from_single_string() {
    assert_eq!(CONST_FROM_STR.as_slice(), ["px-2"]);
}

#[test]
fn const_merging_from_multiple_fragments() {
    let merged = tw_merge_const_str_array(&["px-2 px-4", "px-2"]);
    assert_eq!(merged.as_slice(), ["px-2"]);
}

#[test]
fn keeps_latest_conflicting_classes() {
    let out = tw_merge_const::<3, 3>(["appearance-auto", "appearance-none", "appearance-auto"]);
    assert_eq!(out, ["appearance-auto", "", ""]);
}

#[test]
fn prefix_validator_arbitrary_value() {
    let out = tw_merge_const::<3, 3>(["px-2", "px-4", "px-[3.5rem]"]);
    assert_eq!(out, ["px-[3.5rem]", "", ""]);
}

#[test]
fn bare_validator_arbitrary_value() {
    let out = tw_merge_const::<3, 2>(["opacity-50", "opacity-[0.1]"]);
    assert_eq!(out, ["opacity-[0.1]", "", ""]);
}

#[test]
fn tshirt_size_validator_support() {
    let out = tw_merge_const::<3, 3>(["blur-sm", "blur-lg", "blur-[12px]"]);
    assert_eq!(out, ["blur-[12px]", "", ""]);
}

#[test]
fn arbitrary_image_value() {
    let out = tw_merge_const::<3, 3>(["bg-blue-500", "bg-red-500", "bg-[url('/img/pattern.svg')]"]);
    assert_eq!(out, ["bg-[url('/img/pattern.svg')]", "", ""]);
}

#[test]
fn conflicts_with_position_keeps_latest() {
    let out = tw_merge_const::<4, 4>(["absolute", "relative", "fixed", "sticky"]);
    assert_eq!(out, ["sticky", "", "", ""]);
}

#[test]
fn preserves_unknown_classes() {
    let out = tw_merge_const::<3, 3>(["shadow", "absolute", "my-custom"]);
    assert_eq!(out, ["shadow", "absolute", "my-custom"]);
}
