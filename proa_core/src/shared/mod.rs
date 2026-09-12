// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared utilities for multiple rendering targets
//!
//! This module contains code that is shared between different rendering targets,
//! particularly web and email which both output HTML.

// Core HTML building blocks
pub mod attr;
pub mod attr_value;
pub mod elem;
pub mod escape;
pub mod html_node;
pub mod mode;
pub mod numeric;
pub mod output_meter;
pub mod tag;
pub mod text;
pub mod write_buf;

// Component styling
pub mod class_list;
pub mod variant;

// Additional shared utilities
pub mod html;
