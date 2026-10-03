// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Developer tooling for Proa templates.
//!
//! The crate is organized in four layers:
//!
//! - [`commands`]: one module per `proa` subcommand (clap argument types and
//!   the subcommand entry points), dispatched from [`cli`].
//! - Domain engines consumed by the commands: [`fmt`], [`lint`], [`chunks`],
//!   [`config`], [`font`], [`lifecycle`], [`scaffold`], [`source`], [`ui`],
//!   and the linked RSJS build pipeline under [`rsjs`].
//! - [`output`]: diagnostic presentation (terminal, JSON, GitHub annotations).
//! - [`util`]: small helpers shared across the command layer.

pub mod capabilities;
pub mod chunks;
mod cli;
mod commands;
pub mod config;
pub mod diagnostic;
pub mod fmt;
pub mod font;
mod hot_static;
pub mod lifecycle;
pub mod lint;
mod output;
pub mod rsjs;
pub mod scaffold;
pub mod site_scaffold;
pub mod source;
pub mod ui;
mod util;

pub use cli::run;
pub use cli::Cli;
