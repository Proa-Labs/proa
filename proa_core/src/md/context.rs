// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::rc::Rc;

use bytes::Bytes;

use crate::render::WebRenderSync;
use crate::render::{RenderMode, StreamChunkFlusher};
use crate::shared::output_meter::{
    MeteredWriteBuf, OutputMeterHandle, OutputReservation, ReservedWriteBuf,
};
use crate::shared::write_buf::{RenderBuffer, WriteBuf, WriteError};
use crate::web::WebContext;

/// Opaque snapshot of Markdown render state used when a streamed boundary
/// renders in a child context and then returns control to the parent context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MdContextState {
    heading_offset: u8,
    list_depth: u8,
    inline: bool,
    trailing_line: TrailingLineState,
    pending_cr: bool,
}

/// Compact description of the Markdown output suffix.
///
/// The state is independent of the concrete output buffer, so it remains
/// accurate when a scatter-gather buffer cannot expose a contiguous suffix or
/// when a streaming buffer has already flushed its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum TrailingLineState {
    /// No output has been written yet.
    Start,
    /// Output ends in non-line-ending text.
    Text,
    /// Output ends in exactly one logical line break.
    OneBreak,
    /// Output ends in at least two logical line breaks.
    Blank,
}

impl TrailingLineState {
    #[inline(always)]
    const fn after_break(self) -> Self {
        match self {
            Self::Start | Self::Text => Self::OneBreak,
            Self::OneBreak | Self::Blank => Self::Blank,
        }
    }
}

#[inline(always)]
const fn is_line_ending_byte(byte: u8) -> bool {
    matches!(byte, b'\r' | b'\n')
}

/// Derive the new output suffix from at most the final three bytes of a run.
///
/// A trailing run of three CR/LF bytes necessarily contains at least two
/// logical line breaks, even when the first LF completes a CR from the prior
/// write. Shorter suffixes need only distinguish CRLF from two independent
/// breaks. This keeps state maintenance O(1) regardless of static-run size.
#[inline(always)]
fn state_after_write(
    previous: TrailingLineState,
    previous_pending_cr: bool,
    bytes: &[u8],
) -> (TrailingLineState, bool) {
    let len = bytes.len();
    if len == 0 {
        return (previous, previous_pending_cr);
    }

    let last = bytes[len - 1];
    let pending_cr = last == b'\r';
    if !is_line_ending_byte(last) {
        return (TrailingLineState::Text, false);
    }

    if len >= 3 && is_line_ending_byte(bytes[len - 2]) && is_line_ending_byte(bytes[len - 3]) {
        return (TrailingLineState::Blank, pending_cr);
    }

    if len >= 2 && is_line_ending_byte(bytes[len - 2]) {
        let penultimate = bytes[len - 2];
        if penultimate != b'\r' || last != b'\n' {
            return (TrailingLineState::Blank, pending_cr);
        }

        // The run ends in one CRLF. Text earlier in this write resets the
        // suffix before that single break; otherwise advance the prior state.
        let state = if len > 2 {
            TrailingLineState::OneBreak
        } else {
            previous.after_break()
        };
        return (state, pending_cr);
    }

    // The run ends in one CR or LF. If it follows text in the same write, that
    // is unconditionally one break. A lone LF may instead complete a CR that
    // ended the previous write and therefore must not count twice.
    let state = if len > 1 {
        TrailingLineState::OneBreak
    } else if previous_pending_cr && last == b'\n' {
        previous
    } else {
        previous.after_break()
    };
    (state, pending_cr)
}

/// Write-buffer adapter used by [`MdContext::render_web`] to keep Markdown's
/// logical suffix state coherent while an HTML renderer writes to the same
/// physical buffer.
///
/// The state references are updated after every successful write, so they are
/// already correct if the renderer later returns an error or unwinds. Static,
/// shared, and in-place writes delegate to the matching inner fast path.
#[doc(hidden)]
pub struct ObservedMdWriteBuf<'a, B: WriteBuf> {
    inner: &'a mut B,
    trailing_line: &'a mut TrailingLineState,
    pending_cr: &'a mut bool,
}

impl<B: WriteBuf> ObservedMdWriteBuf<'_, B> {
    #[inline(always)]
    fn observe(&mut self, bytes: &[u8]) {
        (*self.trailing_line, *self.pending_cr) =
            state_after_write(*self.trailing_line, *self.pending_cr, bytes);
    }
}

impl<B: WriteBuf> WriteBuf for ObservedMdWriteBuf<'_, B> {
    #[inline(always)]
    fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.inner.len()
    }

    #[inline(always)]
    fn remaining(&self) -> usize {
        self.inner.remaining()
    }

    #[inline(always)]
    fn reserve(&mut self, additional: usize) -> Result<(), WriteError> {
        self.inner.reserve(additional)
    }

    #[inline(always)]
    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        self.inner.extend(bytes)?;
        self.observe(bytes);
        Ok(())
    }

    #[inline(always)]
    fn push(&mut self, byte: u8) -> Result<(), WriteError> {
        self.inner.push(byte)?;
        self.observe(core::slice::from_ref(&byte));
        Ok(())
    }

    #[inline(always)]
    fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
        self.inner.extend_static(bytes)?;
        self.observe(bytes);
        Ok(())
    }

    #[inline(always)]
    fn extend_shared(&mut self, bytes: &Bytes) -> Result<(), WriteError> {
        self.inner.extend_shared(bytes)?;
        self.observe(bytes);
        Ok(())
    }

    #[inline(always)]
    fn extend_with<F>(&mut self, max_len: usize, write: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        let previous = *self.trailing_line;
        let previous_pending_cr = *self.pending_cr;
        let mut next = None;
        self.inner.extend_with(max_len, |buffer| {
            let len = write(buffer);
            next = Some(state_after_write(
                previous,
                previous_pending_cr,
                &buffer[..len],
            ));
            len
        })?;
        if let Some((trailing_line, pending_cr)) = next {
            *self.trailing_line = trailing_line;
            *self.pending_cr = pending_cr;
        }
        Ok(())
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        self.inner.as_slice()
    }

    #[inline(always)]
    fn to_vec(&self) -> Vec<u8> {
        self.inner.to_vec()
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.inner.clear();
        *self.trailing_line = TrailingLineState::Start;
        *self.pending_cr = false;
    }
}

/// Rendering context for Markdown output.
///
/// Tracks nesting state so components can produce properly indented
/// and leveled markdown without knowing their position in the tree.
///
/// Generic over `B: WriteBuf`, mirroring [`WebContext`]'s design.
pub struct MdContext<'a, B: WriteBuf = Vec<u8>> {
    /// Output buffer.
    pub out: &'a mut B,
    /// Heading level offset — incremented when nesting (e.g. Card inside Card).
    heading_offset: u8,
    /// Current list indentation depth.
    list_depth: u8,
    /// When true, suppress block-level newlines (inline context).
    inline: bool,
    /// Compact state for the logical trailing line-break suffix.
    trailing_line: TrailingLineState,
    /// Whether the final byte was CR, allowing the next write's LF to complete
    /// a CRLF without counting as a second logical break.
    pending_cr: bool,
    /// Request render mode used by streaming-aware Markdown boundaries.
    render_mode: RenderMode,
    /// Optional sink for pure Markdown chunk flushing in in-order streaming mode.
    stream_chunk_flusher: Option<Rc<dyn StreamChunkFlusher>>,
}

/// Mutable slots for Markdown state that stay behind on the parent context.
///
/// Keeping these as disjoint field borrows lets a scoped child simultaneously
/// reborrow the parent's output through a different [`WriteBuf`] adapter.
struct ParentMdState<'scope> {
    heading_offset: &'scope mut u8,
    list_depth: &'scope mut u8,
    inline: &'scope mut bool,
    trailing_line: &'scope mut TrailingLineState,
    pending_cr: &'scope mut bool,
    render_mode: &'scope mut RenderMode,
    stream_chunk_flusher: &'scope mut Option<Rc<dyn StreamChunkFlusher>>,
}

/// Owns Markdown render state moved into a scoped output-type reborrow.
///
/// Restoring from `Drop` keeps the parent context coherent when a renderer
/// returns an error or unwinds. Only the temporary context points at the child
/// output; the parent's output reference is never moved.
struct ScopedMdOutput<'scope, 'child, C>
where
    C: WriteBuf,
{
    parent: ParentMdState<'scope>,
    child: MdContext<'child, C>,
}

impl<'scope, 'child, C> ScopedMdOutput<'scope, 'child, C>
where
    C: WriteBuf,
{
    fn new(parent: ParentMdState<'scope>, out: &'child mut C) -> Self {
        let child = MdContext {
            out,
            heading_offset: *parent.heading_offset,
            list_depth: *parent.list_depth,
            inline: *parent.inline,
            trailing_line: *parent.trailing_line,
            pending_cr: *parent.pending_cr,
            render_mode: *parent.render_mode,
            stream_chunk_flusher: parent.stream_chunk_flusher.take(),
        };
        Self { parent, child }
    }
}

impl<C> Drop for ScopedMdOutput<'_, '_, C>
where
    C: WriteBuf,
{
    fn drop(&mut self) {
        *self.parent.heading_offset = self.child.heading_offset;
        *self.parent.list_depth = self.child.list_depth;
        *self.parent.inline = self.child.inline;
        *self.parent.trailing_line = self.child.trailing_line;
        *self.parent.pending_cr = self.child.pending_cr;
        *self.parent.render_mode = self.child.render_mode;
        *self.parent.stream_chunk_flusher = self.child.stream_chunk_flusher.take();
    }
}

impl<'a, I, H> MdContext<'a, MeteredWriteBuf<I, H>>
where
    I: WriteBuf,
    H: OutputMeterHandle,
{
    /// Render one exactly measured semantic operation through pre-debited
    /// reservation credit.
    ///
    /// The child bypasses the outer meter and writes directly to its inner
    /// buffer, so the same bytes are not charged twice. A successful closure
    /// must consume the reservation exactly. When the closure returns an
    /// error, that original error wins and completion is deliberately not
    /// validated. Markdown state is merged back in every exit mode, including
    /// unwinding.
    pub fn with_reserved_output<R>(
        &mut self,
        reservation: OutputReservation,
        render: impl FnOnce(&mut MdContext<'_, ReservedWriteBuf<&mut I>>) -> Result<R, WriteError>,
    ) -> Result<R, WriteError> {
        let (metered, parent) = self.output_and_state();
        let mut reserved = ReservedWriteBuf::new(metered.inner_mut(), reservation);
        let mut scoped = ScopedMdOutput::new(parent, &mut reserved);
        let result = render(&mut scoped.child);
        drop(scoped);

        match result {
            Err(error) => Err(error),
            Ok(value) => {
                let _ = reserved.finish()?;
                Ok(value)
            }
        }
    }
}

impl<'a> MdContext<'a, Vec<u8>> {
    /// Create a new markdown rendering context backed by a `Vec<u8>`.
    pub fn new(out: &'a mut Vec<u8>) -> Self {
        Self::with_buffer(out)
    }
}

impl<'a, B: WriteBuf> MdContext<'a, B> {
    /// Create a new markdown rendering context with a generic buffer.
    pub fn with_buffer(out: &'a mut B) -> Self {
        Self {
            out,
            heading_offset: 0,
            list_depth: 0,
            inline: false,
            trailing_line: TrailingLineState::Start,
            pending_cr: false,
            render_mode: RenderMode::Static,
            stream_chunk_flusher: None,
        }
    }

    fn state_slots(&mut self) -> ParentMdState<'_> {
        let Self {
            out: _,
            heading_offset,
            list_depth,
            inline,
            trailing_line,
            pending_cr,
            render_mode,
            stream_chunk_flusher,
        } = self;
        ParentMdState {
            heading_offset,
            list_depth,
            inline,
            trailing_line,
            pending_cr,
            render_mode,
            stream_chunk_flusher,
        }
    }

    fn output_and_state(&mut self) -> (&mut B, ParentMdState<'_>) {
        let Self {
            out,
            heading_offset,
            list_depth,
            inline,
            trailing_line,
            pending_cr,
            render_mode,
            stream_chunk_flusher,
        } = self;
        (
            &mut **out,
            ParentMdState {
                heading_offset,
                list_depth,
                inline,
                trailing_line,
                pending_cr,
                render_mode,
                stream_chunk_flusher,
            },
        )
    }

    /// Temporarily render through another output type while preserving all
    /// Markdown-local and streaming state.
    ///
    /// State mutations made through the child context are moved back into the
    /// parent on success, error, or panic. The optional flusher handle is moved
    /// rather than cloned, and this ordinary path allocates nothing.
    /// This is intended for metered and reservation-scoped buffer wrappers.
    pub fn with_output<C, R>(
        &mut self,
        out: &mut C,
        render: impl FnOnce(&mut MdContext<'_, C>) -> R,
    ) -> R
    where
        C: WriteBuf,
    {
        let parent = self.state_slots();
        let mut scoped = ScopedMdOutput::new(parent, out);
        render(&mut scoped.child)
    }

    /// Render through a meter installed over this context's own output.
    ///
    /// The wrapper is constructed only after the parent output and Markdown
    /// state have been split into disjoint reborrows, so callers never need an
    /// unrepresentable simultaneous borrow of `self` and `self.out`. State is
    /// merged back on success, error, or panic.
    pub fn with_metered_output<H, R>(
        &mut self,
        meter: H,
        render: impl FnOnce(&mut MdContext<'_, MeteredWriteBuf<&mut B, H>>) -> R,
    ) -> R
    where
        H: OutputMeterHandle,
    {
        let (out, parent) = self.output_and_state();
        let mut metered = MeteredWriteBuf::new(out, meter);
        let mut scoped = ScopedMdOutput::new(parent, &mut metered);
        render(&mut scoped.child)
    }

    /// Current request render mode.
    pub const fn render_mode(&self) -> RenderMode {
        self.render_mode
    }

    /// Set the current request render mode.
    pub fn set_render_mode(&mut self, render_mode: RenderMode) {
        self.render_mode = render_mode;
    }

    /// Builder-style render mode setter.
    pub fn with_render_mode(mut self, render_mode: RenderMode) -> Self {
        self.render_mode = render_mode;
        self
    }

    /// Returns the configured in-order stream chunk flusher, if any.
    pub fn stream_chunk_flusher(&self) -> Option<&Rc<dyn StreamChunkFlusher>> {
        self.stream_chunk_flusher.as_ref()
    }

    /// Install the in-order stream chunk flusher on this context.
    pub fn set_stream_chunk_flusher(&mut self, stream_chunk_flusher: Rc<dyn StreamChunkFlusher>) {
        self.stream_chunk_flusher = Some(stream_chunk_flusher);
    }

    /// Builder-style in-order stream chunk flusher setter.
    pub fn with_stream_chunk_flusher(
        mut self,
        stream_chunk_flusher: Rc<dyn StreamChunkFlusher>,
    ) -> Self {
        self.stream_chunk_flusher = Some(stream_chunk_flusher);
        self
    }

    /// Snapshot Markdown-local render state for a child streaming context.
    pub const fn render_state(&self) -> MdContextState {
        MdContextState {
            heading_offset: self.heading_offset,
            list_depth: self.list_depth,
            inline: self.inline,
            trailing_line: self.trailing_line,
            pending_cr: self.pending_cr,
        }
    }

    /// Restore Markdown-local render state.
    pub fn set_render_state(&mut self, state: MdContextState) {
        self.heading_offset = state.heading_offset;
        self.list_depth = state.list_depth;
        self.inline = state.inline;
        self.trailing_line = state.trailing_line;
        self.pending_cr = state.pending_cr;
    }

    /// Builder-style render-state setter.
    pub fn with_render_state(mut self, state: MdContextState) -> Self {
        self.set_render_state(state);
        self
    }

    /// Write a heading at the given logical level (1-based).
    ///
    /// The actual `#` count is `level + heading_offset`, clamped to 6.
    pub fn heading(&mut self, level: u8, text: &str) -> Result<(), WriteError> {
        self.heading_with(level, |cx| cx.write_heading_label(text))
    }

    /// Write a heading whose already-typed inline content renders directly.
    ///
    /// Unlike [`MdContext::heading`], this accepts composed Markdown children.
    /// Runtime strings rendered by the closure still take the literal-text
    /// path, while trusted inline components retain their authored semantics.
    pub fn heading_with<F>(&mut self, level: u8, render: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut Self) -> Result<(), WriteError>,
    {
        let actual = (level + self.heading_offset).min(6);
        for _ in 0..actual {
            self.write_prepared_static(b"#")?;
        }
        self.write_prepared_static(b" ")?;
        let previous_inline = self.set_inline(true);
        let result = render(self);
        self.set_inline(previous_inline);
        result?;
        self.write_prepared_static(b"\n\n")
    }

    /// Write a blank line, deduplicating consecutive blank lines.
    pub fn blank_line(&mut self) -> Result<(), WriteError> {
        self.block_boundary()
    }

    /// Write a horizontal rule.
    pub fn hr(&mut self) -> Result<(), WriteError> {
        self.ensure_newline()?;
        self.write_prepared_static(b"---\n\n")
    }

    /// Write an explicitly trusted, already-authored Markdown document.
    ///
    /// Component delimiters belong in `md_sync!`; this semantic entry point
    /// exists for APIs whose payload is Markdown source rather than literal
    /// runtime text. Prefer [`crate::md::TrustedMarkdown`] when composing.
    #[inline(always)]
    pub fn write_trusted_markdown(&mut self, source: &str) -> Result<(), WriteError> {
        self.write_prepared_str(source)
    }

    /// Write trusted, target-prepared Markdown bytes.
    ///
    /// This is the borrowed-byte path for syntax that has already been
    /// prepared by a compiler or is a trusted component delimiter. Runtime
    /// data must use a semantic escaping helper instead.
    #[inline(always)]
    pub fn write_prepared(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.out.extend(bytes)?;
        self.observe_written(bytes);
        Ok(())
    }

    /// Write a trusted, target-prepared Markdown string.
    #[inline(always)]
    pub fn write_prepared_str(&mut self, value: &str) -> Result<(), WriteError> {
        self.write_prepared(value.as_bytes())
    }

    /// Write trusted static Markdown while preserving the buffer's static
    /// zero-copy fast path.
    #[inline(always)]
    pub fn write_prepared_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.out.extend_static(bytes)?;
        self.observe_written(bytes);
        Ok(())
    }

    /// Write trusted shared Markdown while preserving the buffer's shared-run
    /// fast path.
    #[inline(always)]
    pub fn write_prepared_shared(&mut self, bytes: &Bytes) -> Result<(), WriteError> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.out.extend_shared(bytes)?;
        self.observe_written(bytes);
        Ok(())
    }

    /// Write trusted, target-prepared Markdown bytes.
    ///
    /// Deprecated compatibility alias for [`MdContext::write_prepared`]. Data
    /// from runtime values must never reach this API.
    #[deprecated(note = "use write_prepared for trusted Markdown syntax")]
    #[inline(always)]
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        self.write_prepared(bytes)
    }

    /// Write a trusted, target-prepared Markdown string.
    ///
    /// Deprecated compatibility alias for [`MdContext::write_prepared_str`].
    /// Data from runtime values must never reach this API.
    #[deprecated(note = "use write_prepared_str for trusted Markdown syntax")]
    #[inline(always)]
    pub fn write_str(&mut self, s: &str) -> Result<(), WriteError> {
        self.write_prepared_str(s)
    }

    /// Write literal text with the current Markdown escaping contract.
    ///
    /// The escaping contract is intentionally centralized here so it can be
    /// hardened without changing scalar render call sites.
    pub fn write_text(&mut self, s: &str) -> Result<(), WriteError> {
        if s.is_empty() {
            return Ok(());
        }
        super::escape::escape_md(self.out, s)?;
        // Literal text never contains a source-level Markdown line ending:
        // CRLF/CR/LF/tab are normalized to spaces by the escaper.
        self.trailing_line = TrailingLineState::Text;
        self.pending_cr = false;
        Ok(())
    }

    /// Write a runtime heading label as literal, single-line Markdown text.
    pub fn write_heading_label(&mut self, value: &str) -> Result<(), WriteError> {
        if value.is_empty() {
            return Ok(());
        }
        super::escape::write_heading_label(self.out, value)?;
        self.trailing_line = TrailingLineState::Text;
        self.pending_cr = false;
        Ok(())
    }

    /// Write runtime blockquote content as literal, single-line Markdown text.
    pub fn write_blockquote_text(&mut self, value: &str) -> Result<(), WriteError> {
        if value.is_empty() {
            return Ok(());
        }
        super::escape::write_blockquote_text(self.out, value)?;
        self.trailing_line = TrailingLineState::Text;
        self.pending_cr = false;
        Ok(())
    }

    /// Write arbitrary runtime text as a delimiter-safe inline-code span.
    pub fn write_inline_code(&mut self, value: &str) -> Result<(), WriteError> {
        super::escape::write_inline_code(self.out, value)?;
        self.trailing_line = TrailingLineState::Text;
        self.pending_cr = false;
        Ok(())
    }

    /// Write a sanitized CommonMark link destination without delimiters.
    ///
    /// Dangerous schemes emit no bytes. Whitespace, controls, backslashes,
    /// and parentheses are neutralized for an unbracketed destination.
    pub fn write_url(&mut self, value: &str) -> Result<(), WriteError> {
        if super::escape::write_link_destination(self.out, value)? {
            self.trailing_line = TrailingLineState::Text;
            self.pending_cr = false;
        }
        Ok(())
    }

    /// Write runtime code in an adaptive fenced block.
    ///
    /// The content cannot close its generated fence. The method establishes a
    /// block boundary before the fence and leaves one after it.
    pub fn fenced_code(&mut self, info: &str, value: &str) -> Result<(), WriteError> {
        self.block_boundary()?;
        super::escape::write_fenced_code(self.out, info, value)?;
        self.trailing_line = TrailingLineState::Blank;
        self.pending_cr = false;
        Ok(())
    }

    /// Write literal text with Markdown escaping.
    ///
    /// Deprecated compatibility alias for [`MdContext::write_text`].
    #[deprecated(note = "use write_text")]
    pub fn write_escaped(&mut self, s: &str) -> Result<(), WriteError> {
        self.write_text(s)
    }

    /// Ensure output is at a Markdown block boundary.
    ///
    /// Emits the missing bytes in one write, which keeps bounded/metered
    /// buffers from observing a half-written boundary. Generated boundary
    /// bytes use LF even when preceding source used CR or CRLF.
    pub fn block_boundary(&mut self) -> Result<(), WriteError> {
        if self.inline {
            return Ok(());
        }
        let missing: &'static [u8] = match (self.trailing_line, self.pending_cr) {
            (TrailingLineState::Start | TrailingLineState::Blank, _) => return Ok(()),
            (TrailingLineState::OneBreak, false) => b"\n",
            (TrailingLineState::OneBreak, true) | (TrailingLineState::Text, _) => b"\n\n",
        };
        self.write_prepared_static(missing)
    }

    /// Ensure output is at the start of a line.
    ///
    /// Unlike [`MdContext::block_boundary`], this never inserts a blank line,
    /// so adjacent list items remain part of the same list.
    pub fn line_boundary(&mut self) -> Result<(), WriteError> {
        if self.inline || self.trailing_line != TrailingLineState::Text {
            return Ok(());
        }
        self.write_prepared_static(b"\n")
    }

    /// Ensure the buffer ends with at least one newline.
    fn ensure_newline(&mut self) -> Result<(), WriteError> {
        if self.trailing_line == TrailingLineState::Text {
            self.write_prepared_static(b"\n")?;
        }
        Ok(())
    }

    #[inline(always)]
    fn observe_written(&mut self, bytes: &[u8]) {
        (self.trailing_line, self.pending_cr) =
            state_after_write(self.trailing_line, self.pending_cr, bytes);
    }

    /// Write generated numeric ASCII directly into spare output capacity while
    /// keeping Markdown suffix state accurate.
    #[inline(always)]
    pub(crate) fn write_numeric_with<F>(
        &mut self,
        max_len: usize,
        write: F,
    ) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        let previous = self.trailing_line;
        let previous_pending_cr = self.pending_cr;
        let mut next = None;
        self.out.extend_with(max_len, |buf| {
            let len = write(buf);
            next = Some(state_after_write(
                previous,
                previous_pending_cr,
                &buf[..len],
            ));
            len
        })?;
        if let Some((trailing_line, pending_cr)) = next {
            self.trailing_line = trailing_line;
            self.pending_cr = pending_cr;
        }
        Ok(())
    }

    /// Write already-formatted numeric ASCII without reformatting or
    /// allocating.
    #[inline(always)]
    pub(crate) fn write_numeric_str(&mut self, value: &str) -> Result<(), WriteError> {
        self.write_prepared_str(value)
    }

    /// Push heading offset for nesting. Returns the previous offset to restore later.
    pub fn push_heading_offset(&mut self, n: u8) -> u8 {
        let prev = self.heading_offset;
        self.heading_offset = self.heading_offset.saturating_add(n);
        prev
    }

    /// Restore heading offset from a previous `push_heading_offset` call.
    pub fn pop_heading_offset(&mut self, prev: u8) {
        self.heading_offset = prev;
    }

    /// Current heading offset.
    pub fn heading_offset(&self) -> u8 {
        self.heading_offset
    }

    /// Current list depth.
    pub fn list_depth(&self) -> u8 {
        self.list_depth
    }

    /// Push list depth. Returns previous depth.
    pub fn push_list_depth(&mut self) -> u8 {
        let prev = self.list_depth;
        self.list_depth = self.list_depth.saturating_add(1);
        prev
    }

    /// Restore list depth.
    pub fn pop_list_depth(&mut self, prev: u8) {
        self.list_depth = prev;
    }

    /// Write indentation for the current list depth.
    pub fn write_list_indent(&mut self) -> Result<(), WriteError> {
        for _ in 0..self.list_depth {
            self.write_prepared_static(b"  ")?;
        }
        Ok(())
    }

    /// Whether we're in inline mode.
    pub fn inline(&self) -> bool {
        self.inline
    }

    /// Set inline mode. Returns previous value.
    pub fn set_inline(&mut self, inline: bool) -> bool {
        let prev = self.inline;
        self.inline = inline;
        prev
    }

    /// Write a blockquote-prefixed line.
    pub fn blockquote(&mut self, text: &str) -> Result<(), WriteError> {
        self.ensure_newline()?;
        self.write_prepared_static(b"> ")?;
        self.write_blockquote_text(text)?;
        self.write_prepared_static(b"\n")
    }

    /// Write bold text inline: `**text**`.
    pub fn bold(&mut self, text: &str) -> Result<(), WriteError> {
        self.write_prepared_static(b"**")?;
        self.write_text(text)?;
        self.write_prepared_static(b"**")
    }

    /// Write inline code: `` `text` ``.
    pub fn code(&mut self, text: &str) -> Result<(), WriteError> {
        self.write_inline_code(text)
    }

    /// Write a markdown link: `[text](url)`.
    pub fn link(&mut self, text: &str, url: &str) -> Result<(), WriteError> {
        self.link_with(url, |cx| cx.write_text(text))
    }

    /// Write a Markdown link with composed, already-typed inline label content.
    pub fn link_with<F>(&mut self, url: &str, render_label: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut Self) -> Result<(), WriteError>,
    {
        self.write_prepared_static(b"[")?;
        let previous_inline = self.set_inline(true);
        let result = render_label(self);
        self.set_inline(previous_inline);
        result?;
        self.write_prepared_static(b"](")?;
        self.write_url(url)?;
        self.write_prepared_static(b")")
    }

    /// Render an [`MdRenderSync`](crate::render::MdRenderSync) component
    /// (e.g. `md_sync!` output) into this context.
    ///
    /// This is the primary synchronous composition method — `md_sync!` output and other
    /// `MdRenderSync` components compose naturally via `cx.render(child)`.
    pub fn render<T: crate::render::MdRenderSync<B>>(&mut self, node: T) -> Result<(), WriteError> {
        node.render_md(self)
    }

    /// Render a [`WebRenderSync`] component into this context.
    ///
    /// Bridges `WebRenderSync` output (e.g. from `html_sync!`) into markdown context
    /// by creating a temporary `WebContext` wrapping the same buffer. Every
    /// delegated write updates the Markdown suffix tracker, including static,
    /// shared, and in-place fast paths.
    pub fn render_web<'scope, T>(&'scope mut self, node: T) -> Result<(), WriteError>
    where
        T: WebRenderSync<::proa_core::NoLoader, ObservedMdWriteBuf<'scope, B>>,
    {
        let observed = ObservedMdWriteBuf {
            inner: self.out,
            trailing_line: &mut self.trailing_line,
            pending_cr: &mut self.pending_cr,
        };
        let mut cx = WebContext::with_buffer(observed);
        node.render(&mut cx)
    }
}

impl<'a> MdContext<'a, Vec<u8>> {
    /// Flush the currently buffered Markdown bytes to the configured streamed
    /// response sink, if any.
    pub fn flush_stream_chunk(&mut self) -> Result<(), WriteError> {
        let Some(flusher) = self.stream_chunk_flusher.as_ref().cloned() else {
            return Ok(());
        };

        if self.out.is_empty() {
            return Ok(());
        }

        let capacity = self.out.capacity().max(128);
        let chunk = std::mem::replace(self.out, Vec::with_capacity(capacity));
        flusher.flush_chunk(chunk)
    }
}

impl<'a> MdContext<'a, RenderBuffer> {
    /// Flush the currently buffered Markdown bytes to the configured streamed
    /// response sink, then recycle the standard buffer's pages for reuse.
    pub fn flush_stream_chunk(&mut self) -> Result<(), WriteError> {
        let Some(flusher) = self.stream_chunk_flusher.as_ref().cloned() else {
            return Ok(());
        };

        if self.out.is_empty() {
            return Ok(());
        }

        let result = flusher.flush_chunk_view(self.out);
        self.out.clear();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MdRenderSync, OutputMeter, StreamingZeroCopyBuf, ZeroCopyBuf};

    struct NoopFlusher;

    impl StreamChunkFlusher for NoopFlusher {
        fn flush_chunk(&self, _chunk: Vec<u8>) -> Result<(), WriteError> {
            Ok(())
        }
    }

    struct WebBytes(&'static [u8]);

    impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for WebBytes {
        fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
            cx.out.extend_static(self.0)
        }
    }

    struct PanickingWebBytes(&'static [u8]);

    impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for PanickingWebBytes {
        fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
            cx.out.extend_static(self.0)?;
            panic!("test render_web unwind");
        }
    }

    #[derive(Default)]
    struct SpyBuf {
        bytes: Vec<u8>,
        borrowed_calls: usize,
        static_calls: usize,
        shared_calls: usize,
        extend_with_calls: usize,
    }

    impl WriteBuf for SpyBuf {
        fn capacity(&self) -> usize {
            usize::MAX
        }

        fn len(&self) -> usize {
            self.bytes.len()
        }

        fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
            self.borrowed_calls += 1;
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }

        fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
            self.static_calls += 1;
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }

        fn extend_shared(&mut self, bytes: &Bytes) -> Result<(), WriteError> {
            self.shared_calls += 1;
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }

        fn extend_with<F>(&mut self, max_len: usize, write: F) -> Result<(), WriteError>
        where
            F: FnOnce(&mut [u8]) -> usize,
        {
            self.extend_with_calls += 1;
            let mut scratch = [0_u8; 40];
            let len = write(&mut scratch[..max_len]);
            self.bytes.extend_from_slice(&scratch[..len]);
            Ok(())
        }

        fn as_slice(&self) -> &[u8] {
            &self.bytes
        }

        fn to_vec(&self) -> Vec<u8> {
            self.bytes.clone()
        }

        fn clear(&mut self) {
            self.bytes.clear();
        }
    }

    #[derive(Default)]
    struct RejectingBuf;

    impl WriteBuf for RejectingBuf {
        fn capacity(&self) -> usize {
            usize::MAX
        }

        fn len(&self) -> usize {
            0
        }

        fn extend(&mut self, _bytes: &[u8]) -> Result<(), WriteError> {
            Err(WriteError::OutOfCapacity)
        }

        fn as_slice(&self) -> &[u8] {
            &[]
        }

        fn to_vec(&self) -> Vec<u8> {
            Vec::new()
        }

        fn clear(&mut self) {}
    }

    fn reference_state(bytes: &[u8]) -> (TrailingLineState, bool) {
        let mut state = TrailingLineState::Start;
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'\r' => {
                    state = state.after_break();
                    index += 1;
                    if bytes.get(index) == Some(&b'\n') {
                        index += 1;
                    }
                }
                b'\n' => {
                    state = state.after_break();
                    index += 1;
                }
                _ => {
                    state = TrailingLineState::Text;
                    index += 1;
                }
            }
        }
        (state, bytes.last() == Some(&b'\r'))
    }

    #[test]
    fn constant_time_suffix_state_matches_reference_across_split_writes() {
        const ALPHABET: &[u8] = b"x\r\n";
        for len in 0..=7_u32 {
            let case_count = 3_u32.pow(len);
            for mut case in 0..case_count {
                let mut bytes = vec![0_u8; len as usize];
                for byte in &mut bytes {
                    *byte = ALPHABET[(case % 3) as usize];
                    case /= 3;
                }
                let expected = reference_state(&bytes);
                for first in 0..=bytes.len() {
                    for second in first..=bytes.len() {
                        let mut state = (TrailingLineState::Start, false);
                        for run in [&bytes[..first], &bytes[first..second], &bytes[second..]] {
                            state = state_after_write(state.0, state.1, run);
                        }
                        assert_eq!(
                            state, expected,
                            "bytes={bytes:?}, splits=({first}, {second})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn block_boundaries_handle_lf_cr_and_crlf_suffixes() {
        let cases: &[(&[u8], &[u8])] = &[
            (b"text", b"text\n\n"),
            (b"text\n", b"text\n\n"),
            (b"text\r", b"text\r\n\n"),
            (b"text\r\n", b"text\r\n\n"),
            (b"text\n\n", b"text\n\n"),
            (b"text\r\r", b"text\r\r"),
            (b"text\n\r", b"text\n\r"),
            (b"text\r\n\r\n", b"text\r\n\r\n"),
        ];

        for (input, expected) in cases {
            let mut out = Vec::new();
            let mut cx = MdContext::new(&mut out);
            cx.write_prepared(input).unwrap();
            cx.block_boundary().unwrap();
            assert_eq!(&out, expected, "input={input:?}");
        }
    }

    #[test]
    fn line_boundaries_handle_lf_cr_and_crlf_suffixes() {
        let cases: &[(&[u8], &[u8])] = &[
            (b"", b""),
            (b"text", b"text\n"),
            (b"text\n", b"text\n"),
            (b"text\r", b"text\r"),
            (b"text\r\n", b"text\r\n"),
        ];

        for (input, expected) in cases {
            let mut out = Vec::new();
            let mut cx = MdContext::new(&mut out);
            cx.write_prepared(input).unwrap();
            cx.line_boundary().unwrap();
            assert_eq!(&out, expected, "input={input:?}");
        }
    }

    #[test]
    fn split_crlf_survives_render_state_snapshot() {
        let mut prefix = Vec::new();
        let state = {
            let mut cx = MdContext::new(&mut prefix);
            cx.write_prepared(b"text\r").unwrap();
            cx.render_state()
        };

        let mut suffix = Vec::new();
        let mut cx = MdContext::new(&mut suffix).with_render_state(state);
        cx.write_prepared(b"\n").unwrap();
        cx.block_boundary().unwrap();

        prefix.extend_from_slice(&suffix);
        assert_eq!(prefix, b"text\r\n\n");
    }

    #[test]
    fn zero_copy_buffers_do_not_need_a_contiguous_suffix() {
        static LARGE_ZERO_COPY_RUN: [u8; 300] = [b'x'; 300];
        let mut out = ZeroCopyBuf::with_capacity(4, 8);
        {
            let mut cx = MdContext::with_buffer(&mut out);
            cx.write_prepared_static(&LARGE_ZERO_COPY_RUN).unwrap();
            assert!(cx.out.as_slice().is_empty());
            cx.block_boundary().unwrap();
        }
        let bytes = out.to_vec();
        assert!(bytes.ends_with(b"x\n\n"));

        static LARGE_STREAMING_RUN: [u8; 1100] = [b'y'; 1100];
        let mut out = StreamingZeroCopyBuf::with_capacity(4, 8);
        {
            let mut cx = MdContext::with_buffer(&mut out);
            cx.write_prepared_static(&LARGE_STREAMING_RUN).unwrap();
            assert!(cx.out.as_slice().is_empty());
            cx.block_boundary().unwrap();
        }
        let bytes = out.to_vec();
        assert!(bytes.ends_with(b"y\n\n"));
    }

    #[test]
    fn streaming_flush_keeps_suffix_state_in_snapshot() {
        let mut out = StreamingZeroCopyBuf::with_capacity(4, 8);
        let state = {
            let mut cx = MdContext::with_buffer(&mut out);
            cx.write_prepared_static(b"paragraph\r").unwrap();
            cx.render_state()
        };
        let flushed = out.flush_all().unwrap().to_vec();
        assert_eq!(flushed, b"paragraph\r");

        let mut cx = MdContext::with_buffer(&mut out).with_render_state(state);
        cx.write_prepared_static(b"\n").unwrap();
        cx.block_boundary().unwrap();
        assert_eq!(out.to_vec(), b"\n\n");
    }

    #[test]
    fn prepared_methods_preserve_write_buf_delegation() {
        let mut out = SpyBuf::default();
        {
            let mut cx = MdContext::with_buffer(&mut out);
            cx.write_prepared(b"borrowed").unwrap();
            cx.write_prepared_str(" string").unwrap();
            cx.write_prepared_static(b" static").unwrap();
            cx.write_prepared_shared(&Bytes::from_static(b" shared"))
                .unwrap();
        }
        assert_eq!(out.borrowed_calls, 2);
        assert_eq!(out.static_calls, 1);
        assert_eq!(out.shared_calls, 1);
        assert_eq!(out.bytes, b"borrowed string static shared");
    }

    #[test]
    fn scoped_output_reborrow_moves_and_restores_complete_state() {
        let original_flusher: Rc<dyn StreamChunkFlusher> = Rc::new(NoopFlusher);
        let replacement_flusher: Rc<dyn StreamChunkFlusher> = Rc::new(NoopFlusher);
        let mut parent_out = Vec::new();
        let mut child_out = Vec::new();
        let mut cx = MdContext::new(&mut parent_out);
        cx.write_prepared_static(b"parent\n\n").unwrap();
        cx.push_heading_offset(1);
        cx.push_list_depth();
        cx.set_render_mode(RenderMode::ChunkedInOrder);
        cx.set_stream_chunk_flusher(Rc::clone(&original_flusher));

        cx.with_output(&mut child_out, |child| {
            assert_eq!(child.heading_offset(), 1);
            assert_eq!(child.list_depth(), 1);
            assert!(!child.inline());
            assert_eq!(child.trailing_line, TrailingLineState::Blank);
            assert!(!child.pending_cr);
            assert_eq!(child.render_mode(), RenderMode::ChunkedInOrder);
            assert!(Rc::ptr_eq(
                child.stream_chunk_flusher().unwrap(),
                &original_flusher
            ));
            child.push_heading_offset(2);
            child.push_list_depth();
            child.set_inline(true);
            child.set_render_mode(RenderMode::Agent);
            child.write_prepared_static(b"child\r")?;
            child.set_stream_chunk_flusher(Rc::clone(&replacement_flusher));
            Ok::<_, WriteError>(())
        })
        .unwrap();

        assert_eq!(child_out, b"child\r");
        assert_eq!(cx.heading_offset(), 3);
        assert_eq!(cx.list_depth(), 2);
        assert!(cx.inline());
        assert_eq!(cx.trailing_line, TrailingLineState::OneBreak);
        assert!(cx.pending_cr);
        assert_eq!(cx.render_mode(), RenderMode::Agent);
        assert!(Rc::ptr_eq(
            cx.stream_chunk_flusher().unwrap(),
            &replacement_flusher
        ));
        drop(cx);
        assert_eq!(parent_out, b"parent\n\n");
    }

    #[test]
    fn render_web_updates_suffix_state_and_preserves_static_delegation() {
        let mut out = SpyBuf::default();
        let mut cx = MdContext::with_buffer(&mut out);

        cx.render_web(WebBytes(b"bridged\r\n")).unwrap();
        cx.block_boundary().unwrap();

        assert_eq!(cx.out.bytes, b"bridged\r\n\n");
        assert_eq!(cx.out.static_calls, 2);
    }

    #[test]
    fn render_web_keeps_suffix_state_after_unwind() {
        let mut out = Vec::new();
        let mut cx = MdContext::new(&mut out);

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = cx.render_web(PanickingWebBytes(b"bridged\r"));
        }));
        assert!(panic.is_err());
        cx.block_boundary().unwrap();

        drop(cx);
        assert_eq!(out, b"bridged\r\n\n");
    }

    #[test]
    fn scoped_output_reborrow_restores_mutations_after_error() {
        let flusher: Rc<dyn StreamChunkFlusher> = Rc::new(NoopFlusher);
        let mut parent_out = Vec::new();
        let mut child_out = Vec::new();
        let mut cx = MdContext::new(&mut parent_out);

        let result = cx.with_output(&mut child_out, |child| {
            child.push_heading_offset(4);
            child.push_list_depth();
            child.set_inline(true);
            child.set_render_mode(RenderMode::ChunkedOutOfOrder);
            child.write_prepared_static(b"error\r")?;
            child.set_stream_chunk_flusher(Rc::clone(&flusher));
            Err::<(), _>(WriteError::OutOfCapacity)
        });

        assert_eq!(result, Err(WriteError::OutOfCapacity));
        assert_eq!(cx.heading_offset(), 4);
        assert_eq!(cx.list_depth(), 1);
        assert!(cx.inline());
        assert_eq!(cx.trailing_line, TrailingLineState::OneBreak);
        assert!(cx.pending_cr);
        assert_eq!(cx.render_mode(), RenderMode::ChunkedOutOfOrder);
        assert!(Rc::ptr_eq(cx.stream_chunk_flusher().unwrap(), &flusher));
    }

    #[test]
    fn scoped_output_reborrow_restores_state_during_unwind() {
        let flusher: Rc<dyn StreamChunkFlusher> = Rc::new(NoopFlusher);
        let mut parent_out = Vec::new();
        let mut child_out = Vec::new();
        let mut cx = MdContext::new(&mut parent_out);

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cx.with_output(&mut child_out, |child| {
                child.push_heading_offset(2);
                child.push_list_depth();
                child.set_inline(true);
                child.set_render_mode(RenderMode::Agent);
                child.write_prepared_static(b"panic\n").unwrap();
                child.set_stream_chunk_flusher(Rc::clone(&flusher));
                panic!("test unwind");
            });
        }));

        assert!(panic.is_err());
        assert_eq!(cx.heading_offset(), 2);
        assert_eq!(cx.list_depth(), 1);
        assert!(cx.inline());
        assert_eq!(cx.trailing_line, TrailingLineState::OneBreak);
        assert!(!cx.pending_cr);
        assert_eq!(cx.render_mode(), RenderMode::Agent);
        assert!(Rc::ptr_eq(cx.stream_chunk_flusher().unwrap(), &flusher));
    }

    #[test]
    fn self_metered_output_reborrow_is_compile_safe_and_restores_state() {
        let meter = OutputMeter::new(32);
        let mut out = Vec::new();
        let mut cx = MdContext::new(&mut out);
        cx.push_heading_offset(1);

        let value = cx
            .with_metered_output(meter.borrow(), |child| {
                child.push_heading_offset(2);
                child.push_list_depth();
                child.set_inline(true);
                child.set_render_mode(RenderMode::Agent);
                child.write_prepared_static(b"metered\r")?;
                Ok::<_, WriteError>(17)
            })
            .unwrap();

        assert_eq!(value, 17);
        assert_eq!(meter.consumed(), 8);
        assert_eq!(cx.heading_offset(), 3);
        assert_eq!(cx.list_depth(), 1);
        assert!(cx.inline());
        assert_eq!(cx.trailing_line, TrailingLineState::OneBreak);
        assert!(cx.pending_cr);
        assert_eq!(cx.render_mode(), RenderMode::Agent);
        drop(cx);
        assert_eq!(out, b"metered\r");
    }

    #[test]
    fn self_metered_output_reborrow_restores_state_during_unwind() {
        let meter = OutputMeter::new(8);
        let mut out = Vec::new();
        let mut cx = MdContext::new(&mut out);

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cx.with_metered_output(meter.borrow(), |child| {
                child.push_heading_offset(3);
                child.push_list_depth();
                child.set_inline(true);
                panic!("test metered unwind");
            });
        }));

        assert!(panic.is_err());
        assert_eq!(cx.heading_offset(), 3);
        assert_eq!(cx.list_depth(), 1);
        assert!(cx.inline());
    }

    #[test]
    fn reserved_output_bypasses_outer_meter_and_validates_exact_completion() {
        let meter = OutputMeter::new(32);
        let mut metered = MeteredWriteBuf::new(Vec::new(), meter.borrow());
        let mut cx = MdContext::with_buffer(&mut metered);
        let reservation = meter.reserve_exact(5).unwrap();

        let value = cx
            .with_reserved_output(reservation, |child| {
                child.push_heading_offset(2);
                child.set_inline(true);
                child.write_prepared_static(b"hello")?;
                Ok(23)
            })
            .unwrap();

        assert_eq!(value, 23);
        assert_eq!(
            meter.consumed(),
            5,
            "reserved bytes must not be double-debited"
        );
        assert_eq!(cx.heading_offset(), 2);
        assert!(cx.inline());
        drop(cx);
        assert_eq!(metered.as_slice(), b"hello");
    }

    #[test]
    fn reserved_output_reports_incomplete_success() {
        let meter = OutputMeter::new(8);
        let mut metered = MeteredWriteBuf::new(Vec::new(), meter.borrow());
        let mut cx = MdContext::with_buffer(&mut metered);
        let reservation = meter.reserve_exact(4).unwrap();

        let error = cx
            .with_reserved_output(reservation, |child| {
                child.write_prepared_static(b"ok")?;
                Ok(())
            })
            .unwrap_err();

        assert_eq!(
            error,
            WriteError::OutputReservationIncomplete {
                reserved: 4,
                consumed: 2,
                remaining: 2,
            }
        );
    }

    #[test]
    fn reserved_output_preserves_underlying_error_before_validation() {
        let meter = OutputMeter::new(8);
        let mut metered = MeteredWriteBuf::new(RejectingBuf, meter.borrow());
        let mut cx = MdContext::with_buffer(&mut metered);
        let reservation = meter.reserve_exact(2).unwrap();

        let error = cx
            .with_reserved_output(reservation, |child| {
                child.push_heading_offset(1);
                child.write_prepared_static(b"x")?;
                Ok(())
            })
            .unwrap_err();

        assert_eq!(error, WriteError::OutOfCapacity);
        assert_eq!(cx.heading_offset(), 1);
    }

    #[test]
    fn reserved_output_restores_state_during_unwind_without_validation() {
        let meter = OutputMeter::new(8);
        let mut metered = MeteredWriteBuf::new(Vec::new(), meter.borrow());
        let mut cx = MdContext::with_buffer(&mut metered);
        let reservation = meter.reserve_exact(3).unwrap();

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = cx.with_reserved_output(reservation, |child| -> Result<(), WriteError> {
                child.push_heading_offset(2);
                child.push_list_depth();
                child.set_render_mode(RenderMode::Agent);
                panic!("test reserved unwind");
            });
        }));

        assert!(panic.is_err());
        assert_eq!(meter.consumed(), 3);
        assert_eq!(cx.heading_offset(), 2);
        assert_eq!(cx.list_depth(), 1);
        assert_eq!(cx.render_mode(), RenderMode::Agent);
    }

    #[test]
    fn prepared_shared_writes_update_boundary_state() {
        let mut out = Vec::new();
        let mut cx = MdContext::new(&mut out);
        cx.write_prepared_shared(&Bytes::from_static(b"shared\n"))
            .unwrap();
        cx.block_boundary().unwrap();
        assert_eq!(out, b"shared\n\n");
    }

    #[test]
    fn boundary_emits_missing_bytes_in_one_write() {
        let mut out = SpyBuf::default();
        {
            let mut cx = MdContext::with_buffer(&mut out);
            cx.write_prepared_str("text").unwrap();
            cx.block_boundary().unwrap();
        }
        assert_eq!(out.static_calls, 1);
        assert_eq!(out.bytes, b"text\n\n");

        let mut out = SpyBuf::default();
        {
            let mut cx = MdContext::with_buffer(&mut out);
            cx.write_prepared(b"text\n").unwrap();
            cx.block_boundary().unwrap();
        }
        assert_eq!(out.static_calls, 1);
        assert_eq!(out.bytes, b"text\n\n");

        let mut out = SpyBuf::default();
        {
            let mut cx = MdContext::with_buffer(&mut out);
            cx.write_prepared(b"text\r").unwrap();
            cx.block_boundary().unwrap();
        }
        assert_eq!(out.static_calls, 1);
        assert_eq!(out.bytes, b"text\r\n\n");
    }

    #[test]
    fn numeric_render_uses_extend_with_and_updates_suffix_state() {
        let mut out = SpyBuf::default();
        {
            let mut cx = MdContext::with_buffer(&mut out);
            cx.write_prepared_static(b"\n\n").unwrap();
            42_u32.render_md(&mut cx).unwrap();
            cx.block_boundary().unwrap();
        }
        assert_eq!(out.extend_with_calls, 1);
        assert_eq!(out.bytes, b"\n\n42\n\n");
    }

    #[test]
    #[allow(deprecated)]
    fn compatibility_write_aliases_remain_state_aware() {
        let mut out = Vec::new();
        let mut cx = MdContext::new(&mut out);
        cx.write(b"text\r").unwrap();
        cx.write_str("\n").unwrap();
        cx.write_escaped("value").unwrap();
        cx.block_boundary().unwrap();
        assert_eq!(out, b"text\r\nvalue\n\n");
    }

    #[test]
    fn heading_basic() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.heading(2, "Hello").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "## Hello\n\n");
    }

    #[test]
    fn heading_with_offset() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        let prev = cx.push_heading_offset(1);
        cx.heading(2, "Nested").unwrap();
        cx.pop_heading_offset(prev);
        assert_eq!(String::from_utf8(buf).unwrap(), "### Nested\n\n");
    }

    #[test]
    fn heading_clamps_at_6() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.push_heading_offset(5);
        cx.heading(3, "Deep").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "###### Deep\n\n");
    }

    #[test]
    fn blank_line_dedup() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.write_prepared_str("text").unwrap();
        cx.blank_line().unwrap();
        cx.blank_line().unwrap(); // should be deduplicated
        cx.write_prepared_str("more").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "text\n\nmore");
    }

    #[test]
    fn hr() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.write_prepared_str("above").unwrap();
        cx.hr().unwrap();
        cx.write_prepared_str("below").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "above\n---\n\nbelow");
    }

    #[test]
    fn blockquote() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.blockquote("quoted text").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "> quoted text\n");
    }

    #[test]
    fn bold_and_code() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.bold("important").unwrap();
        cx.write_prepared_str(" and ").unwrap();
        cx.code("code").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "**important** and `code`");
    }

    #[test]
    fn link() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.link("click here", "https://example.com").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[click here](https://example.com)"
        );
    }

    #[test]
    fn literal_text_cannot_inject_markdown_blocks_links_or_html() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.write_text("safe\r\n# heading\r- item\n[x](javascript:bad)\t<div>")
            .unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "safe \\# heading \\- item \\[x\\]\\(javascript\\:bad\\) \\<div\\>"
        );
    }

    #[test]
    fn semantic_labels_are_single_line_and_unicode_preserving() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.heading(1, "世界\r\n# nested").unwrap();
        cx.blockquote("引用\n> nested").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "# 世界 \\# nested\n\n> 引用 \\> nested\n"
        );
    }

    #[test]
    fn links_sanitize_destinations_and_keep_labels_literal() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.link("[click](elsewhere)", "javascript:alert(1)")
            .unwrap();
        cx.write_prepared_static(b" ").unwrap();
        cx.link("safe", "https://example.test/a (b)\\c").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[\\[click\\]\\(elsewhere\\)]() [safe](https://example.test/a%20\\(b\\)\\\\c)"
        );
    }

    #[test]
    fn inline_code_payload_cannot_close_its_span() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.code("`a``b```c`\r\n# still code").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "```` `a``b```c` # still code ````"
        );
    }

    #[test]
    fn normalized_crlf_updates_suffix_state_as_one_space() {
        let mut buf = Vec::new();
        let mut cx = MdContext::new(&mut buf);
        cx.write_text("\r\n").unwrap();
        cx.block_boundary().unwrap();
        assert_eq!(buf, b" \n\n");
    }
}
