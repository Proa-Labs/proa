// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Proa source discovery, formatting, linting, and compiler-artifact inspection.
//!
//! These mutually dependent tools share parsing and diagnostics without
//! depending on the linked build engine or CLI command layer.

pub mod chunks;
pub mod diagnostic;
pub mod fmt;
pub mod lint;
pub mod rsjs;
pub mod source;
