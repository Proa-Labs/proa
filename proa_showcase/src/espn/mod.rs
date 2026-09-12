// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ESPN MLB Gamecast / box-score showcase fixture.
//!
//! The sample is based on ESPN event `401695777` (Yankees at Dodgers,
//! May 31, 2025). The untouched summary response and downloaded media live in
//! `data/`; `fixture.rs` keeps the hot render path typed and allocation-free.

mod fixture;
mod page;

pub use fixture::{GameData, GAME};
pub use page::{
    render, render_into, render_into_with_overrides, EspnGamePage, EspnPageOverrides,
    SportsGamePage, SportsPageOverrides,
};

pub const CSS: &str = include_str!("espn.css");
