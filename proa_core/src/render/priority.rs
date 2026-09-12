// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Scheduling priority for an out-of-order streamable boundary.
///
/// Priority does not delay or serialize work: all boundaries start
/// concurrently. When multiple boundaries complete in the same driver turn,
/// `Critical` is emitted before `Optional`, then `Defer`; equal-priority
/// boundaries use their monotonically assigned boundary id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    #[default]
    Critical,
    Optional,
    Defer,
}
