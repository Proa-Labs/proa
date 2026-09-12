// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The dossier component family: every pane awaits the same
//! `ripestat:dossier:<query>` key through the request-scoped loader, so one
//! page render fans out to RIPEstat exactly once no matter how many panes
//! consume the result.

mod dossier;
mod md;
pub mod recipe;

pub use dossier::{
    CarriersPane, DossierMetrics, NeighbourhoodGraph, PathsPane, RangesPane, SummaryBar, TraceStrip,
};
pub use md::DossierMd;
