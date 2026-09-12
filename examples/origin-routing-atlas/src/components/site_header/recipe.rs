// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stable hooks for the sticky site header.

pub mod base {
    // No backdrop-filter here: a filtered ancestor becomes the containing
    // block for fixed descendants, which would pin the markdown dialog's
    // fixed portal to this 56px header instead of the viewport. The 94%-
    // alpha background carries the glass look on its own.
    pub const HEADER: &str = "sticky top-0 z-40 flex items-center gap-[20px] py-[10px] px-[22px] bg-blur border-b border-line";
    pub const WORDMARK: &str = "font-display font-bold text-[15px] tracking-[0.34em] text-ink";
    pub const MD_CHIP: &str = "display: flex; align-items: center; gap: 8px; cursor: pointer; border: 0; font-family: inherit; background: var(--brand, #DD7627); color: var(--bg, #0B0B0C); font-weight: 600; font-size: 10px; letter-spacing: 0.12em; text-transform: uppercase; padding: 4px 10px;";
    pub const THEME_CHIP: &str = "display: flex; align-items: center; gap: 7px; flex-shrink: 0; white-space: nowrap; cursor: pointer; border: 1px solid var(--line-soft, rgba(229,225,214,0.18)); background: transparent; font-family: inherit; color: var(--ink3, #8C887E); font-size: 10px; letter-spacing: 0.12em; text-transform: uppercase; padding: 3px 9px;";
}
