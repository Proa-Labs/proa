// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Analyses that compute derived information from the IR.
//!
//! Analyses are intentionally separate from the side-tables they
//! consume: dependency information, for example, is computed on
//! demand from [`crate::client::ClientExpr`] trees rather than stored
//! on signals or handlers. Keeping it derived means
//! there is no second source of truth for passes to keep in sync as
//! they rewrite expressions.
//!
//! v0.1 surface:
//!
//! - [`dep_collector`] — collect the reactive ids read by a binding
//!   expression, a Client IR v3 expression, or a Client IR v3 block.
//!   Used by the JS emitter to drive subscription registration and by
//!   conformance tests to pin handler dependency semantics.
//! - [`typing`] — whole-island typing: assign a
//!   [`crate::client::ClientType`] to any [`crate::client::ClientExpr`]
//!   given island context. Consumed by the macro frontend's numeric
//!   lowering (truncating-division selection, BigInt literal width
//!   coercion, mixed-width rejection) and by `ClientStmt::Let` type
//!   annotation.
//! - [`helper_validation`] — fail-closed structural, lexical-scope, and
//!   origin-freedom validation for transported standalone helpers.
//! - [`helper_typing`] — closed-world verification of standalone-helper
//!   expressions against parameter, local, return, and resolved target
//!   signatures.
//! - [`strategy`] — usage-driven render-strategy classification:
//!   decide whether an analysed island is `ServerOnly` or
//!   `SSR + Hydrate` based on reachable client roots.

pub mod dep_collector;
pub mod helper_typing;
pub mod helper_validation;
pub mod strategy;
pub mod typing;

pub use dep_collector::{
    collect_client_block_deps, collect_client_block_external_dependencies,
    collect_client_block_helper_edges, collect_client_block_reactive_deps,
    collect_client_expr_external_dependencies, collect_client_expr_reactive_deps, collect_deps,
    walk_client_block, walk_client_expr, walk_client_stmt, ClientBlockDependencyError,
    ClientBlockExternalDependencies, ClientModuleCall,
};
pub use helper_typing::{
    validate_client_helper_block_types, validate_client_root_helper_block_types,
    validate_client_root_helper_expr_types, ClientHelperBodyTypeError, ClientHelperInferredType,
    ClientHelperTargetSignature, ClientHelperTypeSite,
};
pub use helper_validation::{
    validate_client_helper_block, validate_client_helper_expr, ClientHelperCallOccurrence,
    ClientHelperValidationError, ClientHelperValidationReport,
};
pub use strategy::{
    classify_render_strategy, client_crossings, client_reachability_roots,
    client_reachability_roots_with_linked_roots, ClientCrossing, ClientCrossingKind,
    ClientCrossingRequirement, ClientCrossingSurface, ClientReachabilityRoot, DeadClientPrimitive,
    DeadClientPrimitiveKind, RenderStrategy, StrategyAnalysis,
};
pub use typing::{
    backfill_derived_signal_types, block_types, numeric_width, NumericAnchors, NumericWidth,
    TypeEnv,
};
