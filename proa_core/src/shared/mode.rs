// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Rendering mode influences escaping behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentMode {
    Html,
    Script,
    Style,
}
