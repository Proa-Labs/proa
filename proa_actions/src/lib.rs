// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub use proa_macros::Action;

// Module organization:
// - `dynamic`: Action data types with heap-allocated strings (ActionData, ActionHandler, metadata)
// - `static_data`: Static action data for compile-time optimization (ActionDataStatic)
// - `codegen`: TypeScript code generation
// - `collision_check`: Build-time collision detection
pub mod codegen;
pub mod collision_check;
pub mod dynamic;
pub mod static_data;

// Re-export commonly used types at the crate root
pub use collision_check::{check_action_collisions, clear_action_registry};
pub use dynamic::{
    ActionData, ActionHandler, ActionHandlerMetadata, ActionMetadata, ActionParam, ActionVariant,
};
pub use static_data::ActionDataStatic;
