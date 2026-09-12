// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[derive(Clone)]
pub struct SeenSet<const N: usize> {
    entries: [u64; N],
    len: usize,
}

impl<const N: usize> SeenSet<N> {
    pub const fn new() -> Self {
        Self {
            entries: [0; N],
            len: 0,
        }
    }

    pub const fn contains(&self, value: u64) -> bool {
        let mut idx = 0;
        while idx < self.len {
            if self.entries[idx] == value {
                return true;
            }
            idx += 1;
        }
        false
    }

    pub const fn insert(&mut self, value: u64) {
        if self.contains(value) || self.len >= N {
            return;
        }

        self.entries[self.len] = value;
        self.len += 1;
    }
}
