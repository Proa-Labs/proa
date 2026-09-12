// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Experimental Tailwind merge engine optimised for `const fn` friendliness and
//! zero heap allocation.
//!
//! This module is the first step towards a compile-time capable Tailwind class
//! merger. It uses stack-based containers (`heapless`) and avoids dynamic
//! allocation. The current implementation supports a **small subset** of
//! Tailwind classes (padding, text-size, font-weight) to demonstrate the data
//! layout and merge pipeline.
//!
//! # Example
//!
//! ```
//! use tailwind_merge::compile_time::tw_merge_const;
//!
//! let merged = tw_merge_const::<3, 3>(["appearance-auto", "appearance-none", "appearance-auto"]);
//! assert_eq!(merged, ["appearance-auto", "", ""]);
//! ```
//!
//! The API is intentionally conservative while we iterate on the design.
//! Future revisions will expand the built-in dataset and move more pieces into
//! `const fn` territory.

mod const_utils;
mod data;
mod hash;
mod parser;
mod seen;
mod sorter;

use core::slice;

use self::data::{class_conflicts, lookup_class, MAX_CONFLICTS};
use self::hash::hash_signature;
use self::parser::ParsedClass;
use self::seen::SeenSet;
use self::sorter::sort_modifiers;
use crate::crabtime::split::split_whitespace;

/// Maximum number of modifiers supported per class name.
pub const MAX_MODIFIERS: usize = 6;

/// Merge a pre-split array of Tailwind class literals in a fully `const` context.
///
/// The caller is responsible for reserving enough output capacity. Empty
/// strings are used as padding for unused slots.
pub const fn tw_merge_const<const CAP: usize, const N: usize>(
    class_lists: [&'static str; N],
) -> [&'static str; CAP] {
    merge_array_impl::<CAP, N>(class_lists)
}

/// Split a class list on ASCII whitespace and merge it in a `const fn` context.
///
/// `MAX_CLASSES` controls the maximum number of tokens that will be captured
/// from `class_list`. Any additional tokens are silently ignored.
pub const fn tw_merge_const_str(class_list: &'static str) -> ConstMergeResult {
    tw_merge_const_str_array(&[class_list])
}

/// Merge multiple whitespace-delimited class fragments and collect the result.
///
/// This helper is useful when the inputs already come split into fragments
/// (e.g. from macros) but each fragment may contain multiple classes separated
/// by whitespace.
pub const fn tw_merge_const_str_array(class_lists: &[&'static str]) -> ConstMergeResult {
    const MAX_CLASSES: usize = DEFAULT_MAX_CLASSES;
    let mut buffer = [""; MAX_CLASSES];
    let mut total = 0usize;
    let mut idx = 0usize;
    while idx < class_lists.len() && total < MAX_CLASSES {
        let split = split_whitespace::<MAX_CLASSES>(class_lists[idx]).into_array();
        let mut part_idx = 0usize;
        while part_idx < MAX_CLASSES && total < MAX_CLASSES {
            let value = split[part_idx];
            if value.is_empty() {
                break;
            }
            buffer[total] = value;
            total += 1;
            part_idx += 1;
        }
        idx += 1;
    }

    let merged = tw_merge_const::<MAX_CLASSES, MAX_CLASSES>(buffer);

    let mut data = [""; DEFAULT_MAX_CLASSES];
    let mut len = 0usize;
    let mut merge_idx = 0usize;
    while merge_idx < MAX_CLASSES {
        let value = merged[merge_idx];
        if !value.is_empty() {
            if len < DEFAULT_MAX_CLASSES {
                data[len] = value;
                len += 1;
            } else {
                break;
            }
        }
        merge_idx += 1;
    }

    ConstMergeResult { data, len }
}

const fn merge_array_impl<const CAP: usize, const N: usize>(
    class_lists: [&'static str; N],
) -> [&'static str; CAP] {
    let mut result = MergeBuffer::<CAP>::new();
    let mut seen = SeenSet::<{ MAX_CONFLICTS }>::new();

    let mut idx = N;
    while idx > 0 {
        idx -= 1;
        let original = class_lists[idx];
        if original.is_empty() {
            continue;
        }

        let parsed = ParsedClass::<MAX_MODIFIERS>::parse(original);

        if parsed.is_external || parsed.base.is_empty() {
            result.push_front(original);
            continue;
        }

        let mut base = parsed.base;
        let mut has_postfix = parsed.has_postfix;

        // Try lookup without postfix first (if any).
        let mut group_id = lookup_class(base);

        if group_id.is_none() && has_postfix {
            base = parsed.base;
            has_postfix = false;
            group_id = lookup_class(base);
        }

        let Some(group_id) = group_id else {
            // Unknown class – pass through.
            result.push_front(original);
            continue;
        };

        let mut parsed_for_modifiers = parsed;
        sort_modifiers(parsed_for_modifiers.modifiers_mut());
        let modifiers = parsed_for_modifiers.modifiers();

        let signature = hash_signature(modifiers, parsed.has_important, group_id);
        if seen.contains(signature) {
            continue;
        }
        seen.insert(signature);

        let conflicts = class_conflicts(group_id, has_postfix);
        let mut conflict_idx = 0;
        while conflict_idx < conflicts.len() {
            let conflict_sig =
                hash_signature(modifiers, parsed.has_important, conflicts[conflict_idx]);
            seen.insert(conflict_sig);
            conflict_idx += 1;
        }

        result.push_front(original);
    }

    result.into_array()
}

/// Minimal fixed-capacity buffer used in const contexts.
#[derive(Clone, Copy)]
struct MergeBuffer<const N: usize> {
    data: [&'static str; N],
    len: usize,
}

impl<const N: usize> MergeBuffer<N> {
    pub const fn new() -> Self {
        Self {
            data: [""; N],
            len: 0,
        }
    }

    pub const fn push_front(&mut self, value: &'static str) {
        let target_len = if self.len < N { self.len + 1 } else { N };
        let mut i = target_len;
        while i > 1 {
            self.data[i - 1] = self.data[i - 2];
            i -= 1;
        }
        self.data[0] = value;
        self.len = target_len;
    }

    pub const fn into_array(self) -> [&'static str; N] {
        self.data
    }
}

/// Maximum number of classes we capture when splitting string inputs.
pub const DEFAULT_MAX_CLASSES: usize = 128;

/// Result of merging classes from a whitespace-delimited string.
#[derive(Clone, Copy)]
pub struct ConstMergeResult {
    data: [&'static str; DEFAULT_MAX_CLASSES],
    len: usize,
}

impl ConstMergeResult {
    /// Number of classes retained after the merge.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the merge produced any classes.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Access the retained classes as a slice trimmed to the correct length.
    pub const fn as_slice(&self) -> &[&'static str] {
        unsafe { slice::from_raw_parts(self.data.as_ptr(), self.len) }
    }

    /// Consume the result and return the backing array (padded with empty strings).
    pub const fn into_array(self) -> [&'static str; DEFAULT_MAX_CLASSES] {
        self.data
    }
}
