// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared helpers for component markdown (`md.rs`) implementations.
//!
//! # Automatic template scopes
//!
//! Prefer a physically multiline raw `md_sync!` template for block content.
//! It establishes boundaries before rendering and after success, and restores
//! the enclosing mode on errors and panic unwinding. Ordinary strings select
//! inline mode while preserving explicit newlines, including list-row endings.
//!
//! These shims support existing imperative renderers. Their boundary tracking
//! is independent of buffer contiguity and survives zero-copy writes and flushes.
//! Do not use a boundary shim to choose a template's mode or restore formatting.

use proa_core::WebContext;
use proa_core::{WriteBuf, WriteError};

/// Ensure the output sits at a block boundary (start of output or after a
/// blank line), writing the missing newlines if not.
///
/// This low-level helper is a no-op in inline mode; it does not validate or
/// flatten its caller's content. Block templates reject active inline scopes,
/// including link labels and table cells. Supply an inline representation there.
pub(crate) fn block_boundary<L: proa_core::DataLoader, B: WriteBuf>(
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    cx.md_block_boundary()
}

/// Ensure the output sits at the start of a line (start of output or right
/// after a `\n`), writing a single `\n` if the buffer ends mid-line.
///
/// This is the list-item variant of [`block_boundary`]: consecutive list
/// items must stay adjacent (a blank line between them would split the
/// list), so item emitters (checkbox, toggle, radio items) need a fresh
/// LINE, not a full blank-line boundary. Like [`block_boundary`], it is
/// buffer-accurate and a no-op in inline context.
pub(crate) fn line_boundary<L: proa_core::DataLoader, B: WriteBuf>(
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    cx.md_line_boundary()
}
