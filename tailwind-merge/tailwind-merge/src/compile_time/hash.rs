// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub const fn hash_signature(modifiers: &[&'static str], has_important: bool, group_id: u16) -> u64 {
    let mut hash = FNV_OFFSET;
    let mut idx = 0;
    while idx < modifiers.len() {
        hash = fnv_update_str(hash, modifiers[idx]);
        idx += 1;
    }

    hash = fnv_update_u16(hash, group_id);
    if has_important {
        hash = fnv_update_u8(hash, b'!');
    }
    hash
}

const fn fnv_update_str(mut hash: u64, value: &str) -> u64 {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

const fn fnv_update_u16(mut hash: u64, value: u16) -> u64 {
    hash ^= value as u64;
    hash = hash.wrapping_mul(FNV_PRIME);
    hash
}

const fn fnv_update_u8(mut hash: u64, value: u8) -> u64 {
    hash ^= value as u64;
    hash = hash.wrapping_mul(FNV_PRIME);
    hash
}
