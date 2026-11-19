// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Small helpers shared across the command layer.

mod executable;
mod project;

pub(crate) use executable::current_executable;
pub(crate) use project::{cargo_package_name, resolve_config_root};
