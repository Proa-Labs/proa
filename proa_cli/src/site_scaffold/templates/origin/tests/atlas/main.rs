// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The atlas's linked integration tests, in one binary.
//!
//! `cargo proa test` builds a linked descriptor target and a final target per
//! test binary, and that build (about three minutes in CI) dwarfs the tests
//! themselves, so every integration test lives here instead of in its own
//! `tests/*.rs` target. Run with:
//!
//! ```text
//! cargo proa test -p __CRATE_IDENT__ --test atlas
//! ```

mod dataloader;
mod golden;
mod origin;
