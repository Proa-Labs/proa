// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::const_utils::{cmp_str, starts_with_char, str_eq};
use super::data::ORDER_SENSITIVE_MODIFIERS;

pub const fn sort_modifiers(modifiers: &mut [&'static str]) {
    let len = modifiers.len();
    if len <= 1 {
        return;
    }

    let mut i = 1;
    while i < len {
        let mut j = i;
        while j > 0 {
            let left = modifiers[j - 1];
            let right = modifiers[j];

            let left_sensitive = is_order_sensitive(left);
            let right_sensitive = is_order_sensitive(right);

            let should_swap = if left_sensitive || right_sensitive {
                false
            } else {
                cmp_str(right, left) < 0
            };

            if should_swap {
                let tmp = modifiers[j - 1];
                modifiers[j - 1] = modifiers[j];
                modifiers[j] = tmp;
            } else {
                break;
            }
            j -= 1;
        }
        i += 1;
    }
}

const fn is_order_sensitive(modifier: &str) -> bool {
    if starts_with_char(modifier, '[') {
        return true;
    }

    let mut idx = 0;
    while idx < ORDER_SENSITIVE_MODIFIERS.len() {
        if str_eq(ORDER_SENSITIVE_MODIFIERS[idx], modifier) {
            return true;
        }
        idx += 1;
    }
    false
}
