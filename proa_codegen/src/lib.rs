// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Product-neutral TypeScript generation from Proa component metadata.

pub mod config;
pub mod ts_generator;
pub mod types;

pub use config::{CodegenConfig, ReactVersion};
pub use ts_generator::TsGenerator;
pub use types::{ComponentDef, ComponentType, VariantDef, VariantOption};
