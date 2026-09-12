// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Linked-pipeline acceptance coverage kept in one integration-test target.
//!
//! Cargo executes separate integration-test targets serially. Keeping these
//! independent fixtures as modules lets libtest run a bounded number of their
//! nested Cargo builds concurrently without changing their isolated target
//! directories or their end-to-end assertions.

#[path = "rsjs_linked_helper_acceptance/helper_codegen.rs"]
mod helper_codegen;
#[path = "rsjs_linked_helper_acceptance/helper_range_codegen.rs"]
mod helper_range_codegen;
#[path = "rsjs_linked_helper_acceptance/helper_wide_literals.rs"]
mod helper_wide_literals;
#[path = "rsjs_linked_helper_acceptance/node_ref_handle_alias.rs"]
mod node_ref_handle_alias;
