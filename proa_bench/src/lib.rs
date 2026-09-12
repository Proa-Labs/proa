// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Internal performance regression fixtures for Proa.
//!
//! Cross-framework and cross-language comparisons live in the dedicated
//! `Proa-Labs/proa-benchmarks` repository so the production workspace does not
//! compile competitor dependency graphs during ordinary CI.

pub mod button_bench;
pub mod complex_page;
pub mod proa_page;
pub mod streaming_marketplace;

/// Number of cards per section in the benchmark page
pub const CARDS_PER_SECTION: usize = 8;
/// Number of sections in the benchmark page
pub const NUM_SECTIONS: usize = 4;
/// Number of list items per card
pub const ITEMS_PER_CARD: usize = 5;

/// Reuse a `Vec<u8>` for benchmark cases that intentionally measure the
/// lower-level custom-writer path instead of Proa's standard `RenderBuffer`.
pub fn render_into_vec<R>(target: &mut Vec<u8>, root: R)
where
    R: proa_core::WebRenderSync<proa_core::NoLoader, Vec<u8>>,
{
    let mut buffer = std::mem::take(target);
    buffer.clear();
    let mut cx = proa_core::WebContext::with_buffer(buffer);
    root.render(&mut cx).unwrap();
    *target = cx.into_output();
}
