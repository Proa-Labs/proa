// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod md;
mod receipt;
pub mod recipe;

pub use md::ReceiptMd;
pub use receipt::{record_page_render, Receipt, ReceiptFallback};
