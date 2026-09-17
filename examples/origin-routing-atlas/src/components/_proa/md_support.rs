// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared helpers for component markdown (`md.rs`) implementations.
//!
//! # The block-boundary convention
//!
//! Markdown is composed: a `Card` can contain another block component, and a
//! `Dialog` can contain a `Table`. Block-level markdown (headings, tables,
//! lists, blockquotes, fenced code) is only valid when it starts at the
//! beginning of a line, and most renderers additionally require a blank line
//! before it. A component cannot know what its previous sibling wrote, so every
//! block-emitting `render_md` impl must:
//!
//! 1. call [`block_boundary`] BEFORE emitting its first block byte, and
//! 2. call [`block_boundary`] AFTER its last block byte,
//!
//! instead of assuming the cursor sits on a fresh line.
//!
//! The boundary implementation lives in `WebContext`, whose compact suffix
//! state survives static/shared zero-copy writes and streaming flushes. These
//! helpers remain as crate-local shims so component call sites share one
//! convention without depending on buffer contiguity.

use proa_core::WebContext;
use proa_core::{WriteBuf, WriteError};

/// Ensure the output sits at a block boundary (start of output or after a
/// blank line), writing the missing newlines if not.
///
/// No-op in inline context — inline consumers (table cells, link labels)
/// must never receive embedded newlines, so block components degrade
/// gracefully there.
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
