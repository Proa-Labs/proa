// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod bootstrap;
mod builder;
mod children;
mod island;
mod island_def;
mod js;

pub use bootstrap::{
    react_bootstrap, render_import_map_entries, render_react_bootstrap, render_react_import_map,
    HydrationStrategy, ReactBootstrap, ReactBootstrapConfig, ReactModuleDescriptor,
};
pub use builder::{ErrorBoundaryConfig, Island, IslandStrategy};
pub use children::Children;
pub use island::{react_island, react_island_with_props, ReactIsland};
pub use island_def::IslandDef;

// Versioned CDN URLs keep the default runtime independent of build time and
// of any JavaScript workspace manifest outside the published crate.

/// Default React CDN URL.
pub const DEFAULT_REACT_URL: &str = "https://esm.sh/react@19.0.0";

/// Default ReactDOM CDN URL.
pub const DEFAULT_REACT_DOM_URL: &str = "https://esm.sh/react-dom@19.0.0/client";
