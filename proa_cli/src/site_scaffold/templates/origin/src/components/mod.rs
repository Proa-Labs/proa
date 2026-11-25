// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// The source-installed catalog intentionally exposes components beyond the
// subset used by this example and follows the conventional name/name.rs layout.
#![allow(dead_code, clippy::module_inception)]

pub mod agent_surface;
mod atlas_page;
pub mod dialog;
pub mod dossier;
pub mod flood_lab;
pub mod hero;
pub mod live_wire;
pub mod receipt;
pub mod site_header;
pub mod theme_toggle;

pub use atlas_page::{markdown_string, markdown_string_with_base_path, AtlasMd, AtlasPage};
pub use dialog::*;
pub use flood_lab::LabRows;
pub use theme_toggle::*;

// The block below must list exactly the modules `proa.lock.json` records, or
// `proa ui add` refuses to rewrite it. `dialog` and `theme_toggle` are not
// lock-managed, so they live above.
// proa-ui:modules:start
pub mod _proa;
pub use _proa::*;
pub mod badge;
pub use badge::*;
pub mod button;
pub use button::*;
pub mod card;
pub use card::*;
pub mod input;
pub use input::*;
pub mod separator;
pub use separator::*;
pub mod spinner;
pub use spinner::*;
pub mod tooltip;
pub use tooltip::*;
// proa-ui:modules:end
