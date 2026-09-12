// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stable hooks for the flood-the-table render lab.

pub mod base {
    pub const FLOOD_BUTTON: &str = "border: 1px solid var(--ink, #E5E1D6); background: transparent; color: var(--ink, #E5E1D6); padding: 12px 20px; font-family: inherit; font-size: 11px; font-weight: 600; letter-spacing: 0.14em; text-transform: uppercase; cursor: pointer;";
    pub const CLEAR_BUTTON: &str = "border: 1px solid var(--line-mid, rgba(229,225,214,0.2)); background: transparent; color: var(--ink3, #8C887E); padding: 12px 18px; font-family: inherit; font-size: 11px; letter-spacing: 0.14em; text-transform: uppercase; cursor: pointer;";
    pub const READOUT_VALUE: &str =
        "font-display text-[32px] font-semibold tracking-[-0.04em] mt-[8px] text-ink4";
}
