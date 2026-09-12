// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stable hooks and shared style fragments for the dossier panes.
//!
//! The ORIGIN skin is transcribed from the v2 prototype as Tailwind utility
//! fragments; these constants are the single source for the pieces that vary
//! by state so the render code stays allocation-free.

pub mod base {
    pub const PANE_TITLE: &str = "shrink-0 whitespace-nowrap text-[10px] tracking-[0.18em] text-ink3 uppercase relative cursor-help underline decoration-dotted decoration-line-strong underline-offset-[3px]";
    pub const PANE_COUNT: &str = "shrink-0 whitespace-nowrap text-[10px] text-ink4";
    pub const METRIC_VALUE_BONE: &str =
        "font-display text-[28px] font-semibold tracking-[-0.035em] mt-[8px] text-ink";
    pub const METRIC_VALUE_CYAN: &str =
        "font-display text-[28px] font-semibold tracking-[-0.035em] mt-[8px] text-cyan";
}

pub mod state {
    pub const RANGE_VALID: &str = "text-cyan text-[10px]";
    pub const RANGE_CONTESTED: &str = "text-red text-[10px]";
    pub const REL_TRANSIT: &str = "text-brand text-[9px] tracking-[0.08em] text-right";
    pub const REL_PEER: &str = "text-cyan text-[9px] tracking-[0.08em] text-right";
    pub const HOP: &str = "text-ink3 py-[2px] px-0 font-light";
    pub const NET_NAME: &str = "font-display inline-block bg-bg pr-[12px] text-[clamp(18px,2.6vw,34px)] font-bold tracking-[-0.035em] leading-[1.05] mt-[12px] text-cyan";
    pub const NET_NAME_MUTED: &str = "font-display inline-block bg-bg pr-[12px] text-[clamp(18px,2.6vw,34px)] font-bold tracking-[-0.035em] leading-[1.05] mt-[12px] text-ink3";
    pub const HOP_ORIGIN: &str = "text-bg bg-cyan py-[2px] px-[6px] font-semibold";
}
