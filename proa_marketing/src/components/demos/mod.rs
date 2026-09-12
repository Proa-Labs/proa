// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Live demos framed by docs pages.
//!
//! Docs articles are inert HTML, so anything that must actually run is served
//! as its own small document under `/demos/*` and embedded with `<Demo />`.

pub(crate) mod quantity_stepper;
