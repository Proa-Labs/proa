// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use bytes::Bytes;

use crate::render::DataLoader;
use crate::shared::write_buf::{WriteBuf, WriteError};
use crate::web::WebContext;

/// Opaque snapshot of Markdown formatting and logical output suffix state.
/// Fragment scopes and streamed boundaries use it to restore the caller's
/// Markdown state without replacing the buffer or request environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MdState {
    heading_offset: u8,
    list_depth: u8,
    inline: bool,
    inline_scope: bool,
    trailing_line: TrailingLineState,
    pending_cr: bool,
}

impl Default for MdState {
    fn default() -> Self {
        Self {
            heading_offset: 0,
            list_depth: 0,
            inline: false,
            inline_scope: false,
            trailing_line: TrailingLineState::Start,
            pending_cr: false,
        }
    }
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

/// Write-buffer adapter used by [`WebContext::render_html_in_md`] to keep Markdown's
/// logical suffix state coherent while an HTML renderer writes to the same
/// physical buffer.
///
/// The observer tracks successful writes independently of fragment formatting
/// state. Its scope merges the suffix even if rendering errors or unwinds. Static,
/// shared, and in-place writes delegate to the matching inner fast path.
#[doc(hidden)]
pub struct ObservedMdWriteBuf<'a, B: WriteBuf> {
    inner: &'a mut B,
    trailing_line: TrailingLineState,
    pending_cr: bool,
}

impl<'a, B: WriteBuf> ObservedMdWriteBuf<'a, B> {
    pub(crate) fn new(inner: &'a mut B, state: MdState) -> Self {
        Self {
            inner,
            trailing_line: state.trailing_line,
            pending_cr: state.pending_cr,
        }
    }

    pub(crate) fn merge_suffix(&self, state: &mut MdState) {
        state.trailing_line = self.trailing_line;
        state.pending_cr = self.pending_cr;
    }

    #[inline(always)]
    fn observe(&mut self, bytes: &[u8]) {
        (self.trailing_line, self.pending_cr) =
            state_after_write(self.trailing_line, self.pending_cr, bytes);
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
        let previous = self.trailing_line;
        let previous_pending_cr = self.pending_cr;
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
            self.trailing_line = trailing_line;
            self.pending_cr = pending_cr;
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
        self.trailing_line = TrailingLineState::Start;
        self.pending_cr = false;
    }
}

/// Merge accepted escape-writer output even if a later write fails or panics.
struct MdOutputScope<'a, B: WriteBuf> {
    out: ObservedMdWriteBuf<'a, B>,
    state: &'a mut MdState,
}

impl<B: WriteBuf> Drop for MdOutputScope<'_, B> {
    #[inline(always)]
    fn drop(&mut self) {
        self.out.merge_suffix(self.state);
    }
}

impl<L: DataLoader, B: WriteBuf> WebContext<L, B> {
    /// Snapshot Markdown formatting and logical suffix state, independently of
    /// the physical buffer. Direct `out` writes bypass this bookkeeping; use
    /// Markdown helpers or [`Self::render_html_in_md`] for tracked composition.
    pub const fn md_state(&self) -> MdState {
        self.md_state
    }

    /// Restore Markdown state without rolling back output or request metadata.
    ///
    /// This also replaces suffix tracking. Use a snapshot only with its matching
    /// output position or intentional fragment isolation; use template/heading
    /// scopes for formatting cleanup when partial output must remain visible.
    pub fn set_md_state(&mut self, state: MdState) {
        self.md_state = state;
    }

    /// Set the initial Markdown state for a detached render.
    pub fn with_md_state(mut self, state: MdState) -> Self {
        self.set_md_state(state);
        self
    }

    /// Write a heading at the given logical level (1-based).
    ///
    /// The actual `#` count is `level + md_heading_offset`, clamped to 6.
    pub fn md_heading(&mut self, level: u8, text: &str) -> Result<(), WriteError> {
        self.md_heading_with(level, |cx| cx.write_md_heading_label(text))
    }

    /// Write a heading whose already-typed inline content renders directly.
    ///
    /// Unlike [`WebContext::md_heading`], this accepts composed Markdown children.
    /// Runtime strings rendered by the closure still take the literal-text
    /// path, while trusted inline components retain their authored semantics.
    pub fn md_heading_with<F>(&mut self, level: u8, render: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut Self) -> Result<(), WriteError>,
    {
        let actual = level.saturating_add(self.md_state.heading_offset).min(6);
        for _ in 0..actual {
            self.write_md_prepared_static(b"#")?;
        }
        self.write_md_prepared_static(b" ")?;
        {
            let mut scope = MdTemplateScope::<_, _, MD_TEMPLATE_INLINE>::enter(self)?;
            render(scope.context())?;
        }
        self.write_md_prepared_static(b"\n\n")
    }

    /// Write a blank line, deduplicating consecutive blank lines.
    pub fn md_blank_line(&mut self) -> Result<(), WriteError> {
        self.md_block_boundary()
    }

    /// Write a horizontal rule.
    pub fn md_hr(&mut self) -> Result<(), WriteError> {
        self.ensure_newline()?;
        self.write_md_prepared_static(b"---\n\n")
    }

    /// Write an explicitly trusted, already-authored Markdown document.
    ///
    /// Component delimiters belong in `md_sync!`; this semantic entry point
    /// exists for APIs whose payload is Markdown source rather than literal
    /// runtime text. Prefer [`crate::md::TrustedMarkdown`] when composing.
    #[inline(always)]
    pub fn write_md_trusted(&mut self, source: &str) -> Result<(), WriteError> {
        self.write_md_prepared_str(source)
    }

    /// Write trusted, target-prepared Markdown bytes.
    ///
    /// This is the borrowed-byte path for syntax that has already been
    /// prepared by a compiler or is a trusted component delimiter. Runtime
    /// data must use a semantic escaping helper instead.
    #[inline(always)]
    pub fn write_md_prepared(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.out.extend(bytes)?;
        self.observe_written(bytes);
        Ok(())
    }

    /// Write a trusted, target-prepared Markdown string.
    #[inline(always)]
    pub fn write_md_prepared_str(&mut self, value: &str) -> Result<(), WriteError> {
        self.write_md_prepared(value.as_bytes())
    }

    /// Write trusted static Markdown while preserving the buffer's static
    /// zero-copy fast path.
    #[inline(always)]
    pub fn write_md_prepared_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
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
    pub fn write_md_prepared_shared(&mut self, bytes: &Bytes) -> Result<(), WriteError> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.out.extend_shared(bytes)?;
        self.observe_written(bytes);
        Ok(())
    }

    /// Write literal text with the current Markdown escaping contract.
    ///
    /// The escaping contract is intentionally centralized here so it can be
    /// hardened without changing scalar render call sites.
    pub fn write_md_text(&mut self, s: &str) -> Result<(), WriteError> {
        if s.is_empty() {
            return Ok(());
        }
        self.write_md_observed(|out| super::escape::escape_md(out, s))
    }

    /// Write a runtime heading label as literal, single-line Markdown text.
    pub fn write_md_heading_label(&mut self, value: &str) -> Result<(), WriteError> {
        if value.is_empty() {
            return Ok(());
        }
        self.write_md_observed(|out| super::escape::write_heading_label(out, value))
    }

    /// Write runtime blockquote content as literal, single-line Markdown text.
    pub fn write_md_blockquote_text(&mut self, value: &str) -> Result<(), WriteError> {
        if value.is_empty() {
            return Ok(());
        }
        self.write_md_observed(|out| super::escape::write_blockquote_text(out, value))
    }

    /// Write arbitrary runtime text as a delimiter-safe inline-code span.
    pub fn write_md_inline_code(&mut self, value: &str) -> Result<(), WriteError> {
        self.write_md_observed(|out| super::escape::write_inline_code(out, value))
    }

    /// Write a sanitized CommonMark link destination without delimiters.
    ///
    /// Dangerous schemes emit no bytes. Whitespace, controls, backslashes,
    /// and parentheses are neutralized for an unbracketed destination.
    pub fn write_md_url(&mut self, value: &str) -> Result<(), WriteError> {
        self.write_md_observed(|out| super::escape::write_link_destination(out, value).map(|_| ()))
    }

    /// Write runtime code in an adaptive fenced block.
    ///
    /// The content cannot close its generated fence. The method establishes a
    /// block boundary before the fence and leaves one after it.
    pub fn md_fenced_code(&mut self, info: &str, value: &str) -> Result<(), WriteError> {
        self.md_block_boundary()?;
        self.write_md_observed(|out| super::escape::write_fenced_code(out, info, value))
    }

    /// Ensure output is at a Markdown block boundary.
    ///
    /// Emits the missing bytes in one write, which keeps bounded/metered
    /// buffers from observing a half-written boundary. Generated boundary
    /// bytes use LF even when preceding source used CR or CRLF.
    pub fn md_block_boundary(&mut self) -> Result<(), WriteError> {
        if self.md_state.inline {
            return Ok(());
        }
        let missing: &'static [u8] = match (self.md_state.trailing_line, self.md_state.pending_cr) {
            (TrailingLineState::Start | TrailingLineState::Blank, _) => return Ok(()),
            (TrailingLineState::OneBreak, false) => b"\n",
            (TrailingLineState::OneBreak, true) | (TrailingLineState::Text, _) => b"\n\n",
        };
        self.write_md_prepared_static(missing)
    }

    /// Ensure output is at the start of a line.
    ///
    /// Unlike [`WebContext::md_block_boundary`], this never inserts a blank line,
    /// so adjacent list items remain part of the same list.
    pub fn md_line_boundary(&mut self) -> Result<(), WriteError> {
        if self.md_state.inline || self.md_state.trailing_line != TrailingLineState::Text {
            return Ok(());
        }
        self.write_md_prepared_static(b"\n")
    }

    /// Ensure the buffer ends with at least one newline.
    fn ensure_newline(&mut self) -> Result<(), WriteError> {
        if self.md_state.trailing_line == TrailingLineState::Text {
            self.write_md_prepared_static(b"\n")?;
        }
        Ok(())
    }

    #[inline(always)]
    fn observe_written(&mut self, bytes: &[u8]) {
        (self.md_state.trailing_line, self.md_state.pending_cr) =
            state_after_write(self.md_state.trailing_line, self.md_state.pending_cr, bytes);
    }

    #[inline(always)]
    fn write_md_observed(
        &mut self,
        write: impl FnOnce(&mut ObservedMdWriteBuf<'_, B>) -> Result<(), WriteError>,
    ) -> Result<(), WriteError> {
        let mut scope = MdOutputScope {
            out: ObservedMdWriteBuf::new(&mut self.out, self.md_state),
            state: &mut self.md_state,
        };
        write(&mut scope.out)
    }

    /// Write generated numeric ASCII directly into spare output capacity while
    /// keeping Markdown suffix state accurate.
    #[inline(always)]
    pub(crate) fn write_md_numeric_with<F>(
        &mut self,
        max_len: usize,
        write: F,
    ) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        let previous = self.md_state.trailing_line;
        let previous_pending_cr = self.md_state.pending_cr;
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
            self.md_state.trailing_line = trailing_line;
            self.md_state.pending_cr = pending_cr;
        }
        Ok(())
    }

    /// Write already-formatted numeric ASCII without reformatting or
    /// allocating.
    #[inline(always)]
    pub(crate) fn write_md_numeric_str(&mut self, value: &str) -> Result<(), WriteError> {
        self.write_md_prepared_str(value)
    }

    /// Temporarily increase heading offset for nested components.
    ///
    /// The borrowed scope restores only heading offset on drop, including when
    /// rendering errors, unwinds, or an awaiting future is cancelled. Emitted
    /// output, suffix tracking, and the other formatting settings remain live.
    #[inline(always)]
    pub fn md_heading_scope(&mut self, additional: u8) -> MdHeadingScope<'_, L, B> {
        let previous = self.md_push_heading_offset(additional);
        MdHeadingScope { cx: self, previous }
    }

    /// Push heading offset for nesting. Returns the previous offset to restore later.
    ///
    /// Prefer [`Self::md_heading_scope`] when rendering children: manual push/pop
    /// does not restore the setting on early returns or panic unwinding.
    pub fn md_push_heading_offset(&mut self, n: u8) -> u8 {
        let prev = self.md_state.heading_offset;
        self.md_state.heading_offset = self.md_state.heading_offset.saturating_add(n);
        prev
    }

    /// Restore heading offset from a previous `md_push_heading_offset` call.
    pub fn md_pop_heading_offset(&mut self, prev: u8) {
        self.md_state.heading_offset = prev;
    }

    /// Current heading offset.
    pub fn md_heading_offset(&self) -> u8 {
        self.md_state.heading_offset
    }

    /// Current list depth.
    pub fn md_list_depth(&self) -> u8 {
        self.md_state.list_depth
    }

    /// Push list depth. Returns previous depth.
    /// This low-level operation requires its own unwinding-safe cleanup;
    /// inline/block template scopes do not restore list depth.
    pub fn md_push_list_depth(&mut self) -> u8 {
        let prev = self.md_state.list_depth;
        self.md_state.list_depth = self.md_state.list_depth.saturating_add(1);
        prev
    }

    /// Restore list depth.
    pub fn md_pop_list_depth(&mut self, prev: u8) {
        self.md_state.list_depth = prev;
    }

    /// Write indentation for the current list depth.
    pub fn write_md_list_indent(&mut self) -> Result<(), WriteError> {
        for _ in 0..self.md_state.list_depth {
            self.write_md_prepared_static(b"  ")?;
        }
        Ok(())
    }

    /// Whether we're in inline mode.
    pub fn md_inline(&self) -> bool {
        self.md_state.inline
    }

    /// Set the low-level inline mode. Returns the previous value.
    ///
    /// Single-literal Markdown templates scope this setting automatically.
    /// Prefer an ordinary-string `md_sync!` or `md!` template around inline
    /// children; manual save/restore is not safe across all render exits.
    /// Setting it to false cannot permit a block inside an active inline scope.
    pub fn md_set_inline(&mut self, inline: bool) -> bool {
        let prev = self.md_state.inline;
        self.md_state.inline = inline;
        prev
    }

    /// Write a blockquote-prefixed line.
    pub fn md_blockquote(&mut self, text: &str) -> Result<(), WriteError> {
        self.ensure_newline()?;
        self.write_md_prepared_static(b"> ")?;
        self.write_md_blockquote_text(text)?;
        self.write_md_prepared_static(b"\n")
    }

    /// Write bold text inline: `**text**`.
    pub fn md_bold(&mut self, text: &str) -> Result<(), WriteError> {
        self.write_md_prepared_static(b"**")?;
        self.write_md_text(text)?;
        self.write_md_prepared_static(b"**")
    }

    /// Write inline code: `` `text` ``.
    pub fn md_code(&mut self, text: &str) -> Result<(), WriteError> {
        self.write_md_inline_code(text)
    }

    /// Write a markdown link: `[text](url)`.
    pub fn md_link(&mut self, text: &str, url: &str) -> Result<(), WriteError> {
        self.md_link_with(url, |cx| cx.write_md_text(text))
    }

    /// Write a Markdown link with composed, already-typed inline label content.
    pub fn md_link_with<F>(&mut self, url: &str, render_label: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut Self) -> Result<(), WriteError>,
    {
        self.write_md_prepared_static(b"[")?;
        {
            let mut scope = MdTemplateScope::<_, _, MD_TEMPLATE_INLINE>::enter(self)?;
            render_label(scope.context())?;
        }
        self.write_md_prepared_static(b"](")?;
        self.write_md_url(url)?;
        self.write_md_prepared_static(b")")
    }
}

/// A Markdown fragment entered through a web trait starts with a fresh heading
/// offset and list depth but keeps the caller's inline-scope flags and output
/// suffix tracking. A borrowed guard restores the formatting and scope state
/// even when an awaited fragment is cancelled or panics, while the suffix
/// tracking follows the bytes actually written. The output and request state
/// stay live, so this scope neither allocates nor rolls back their mutations.
pub(crate) struct MdFragmentScope<'cx, L: DataLoader, B: WriteBuf> {
    cx: &'cx mut WebContext<L, B>,
    saved: MdState,
}

/// A borrowed heading-offset scope returned by [`WebContext::md_heading_scope`].
/// Dropping the scope restores the immediate parent's offset without changing
/// output, boundary tracking, inline mode, or list depth.
#[must_use = "keep the heading scope alive while rendering its children"]
pub struct MdHeadingScope<'cx, L: DataLoader, B: WriteBuf> {
    cx: &'cx mut WebContext<L, B>,
    previous: u8,
}

impl<L: DataLoader, B: WriteBuf> MdHeadingScope<'_, L, B> {
    /// Reborrow the live context with this scope's heading offset.
    #[inline(always)]
    pub fn context(&mut self) -> &mut WebContext<L, B> {
        self.cx
    }
}

impl<L: DataLoader, B: WriteBuf> Drop for MdHeadingScope<'_, L, B> {
    #[inline(always)]
    fn drop(&mut self) {
        self.cx.md_pop_heading_offset(self.previous);
    }
}

/// Template modes: the behavior a compiled Markdown template wraps around its
/// body at render time. This constant is the inherit mode; the table below
/// covers all three.
///
/// `md_sync!` and `md!` pick one of these constants at expansion time from the
/// shape of the literal and stamp it on the template as the `MODE` const
/// parameter of `OptimizedMd` and `AsyncMdFn`, through the hidden
/// `__proa_template_mode` method. When the template renders,
/// `MdTemplateScope<MODE>` turns the value into a prologue that runs before the
/// render closure and an epilogue that runs after it succeeds:
///
/// | Mode | Literal form | Before the body | After the body |
/// |---|---|---|---|
/// | [`MD_TEMPLATE_BLOCK`] | multiline raw string | error inside an inline scope, otherwise complete a paragraph boundary | complete a paragraph boundary |
/// | [`MD_TEMPLATE_INLINE`] | ordinary string or single-line raw string | mark the inline scope so block children are rejected | restore the caller's flags |
/// | [`MD_TEMPLATE_INHERIT`] | token composition such as `md_sync! { {a} "x" }` | nothing | nothing |
///
/// The modes are `u8` constants rather than an enum because stable const
/// generics accept only integer, `bool`, and `char` parameters. Carrying the
/// mode in the type means each variant is monomorphized separately: a template
/// stores no mode byte and branches on nothing at render time. The constants
/// are hidden from the public documentation because only macro output names
/// them; application code selects a mode by choosing the literal form.
///
/// Inherit mode: no prologue or epilogue. The template renders in the caller's
/// mode, as every template did before literal modes existed.
#[doc(hidden)]
pub const MD_TEMPLATE_INHERIT: u8 = 0;
/// Inline mode, selected by an ordinary string or a single-line raw literal:
/// marks the inline scope so block children are rejected, then restores the
/// caller's flags. Explicit newlines in the body are written as authored.
/// See [`MD_TEMPLATE_INHERIT`] for the full mode table.
#[doc(hidden)]
pub const MD_TEMPLATE_INLINE: u8 = 1;
/// Block mode, selected by a multiline raw literal: errors inside an inline
/// scope, otherwise completes a paragraph boundary before the body and again
/// after it succeeds. See [`MD_TEMPLATE_INHERIT`] for the full mode table.
#[doc(hidden)]
pub const MD_TEMPLATE_BLOCK: u8 = 2;

/// Borrow the live context while restoring only the template's inline setting.
/// In particular, successful writes remain reflected in the suffix tracker on
/// errors, unwinding, and cancellation. Finishing a block is an explicit success
/// operation; dropping a scope never writes to the output.
#[doc(hidden)]
pub struct MdTemplateScope<'cx, L: DataLoader, B: WriteBuf, const MODE: u8> {
    cx: &'cx mut WebContext<L, B>,
    inline: bool,
    inline_scope: bool,
}

impl<'cx, L: DataLoader, B: WriteBuf, const MODE: u8> MdTemplateScope<'cx, L, B, MODE> {
    #[inline(always)]
    pub fn enter(cx: &'cx mut WebContext<L, B>) -> Result<Self, WriteError> {
        let inline = cx.md_inline();
        let inline_scope = cx.md_state.inline_scope;
        if MODE == MD_TEMPLATE_BLOCK && (inline || inline_scope) {
            return Err(WriteError::render(
                "cannot render a block Markdown template inside an inline scope; use an ordinary-string inline template",
            ));
        }
        let scope = Self {
            cx,
            inline,
            inline_scope,
        };
        if MODE == MD_TEMPLATE_INLINE {
            scope.cx.md_set_inline(true);
            scope.cx.md_state.inline_scope = true;
        } else if MODE == MD_TEMPLATE_BLOCK {
            scope.cx.md_block_boundary()?;
        }
        Ok(scope)
    }

    #[inline(always)]
    pub fn context(&mut self) -> &mut WebContext<L, B> {
        self.cx
    }

    #[inline(always)]
    pub(crate) fn finish(&mut self) -> Result<(), WriteError> {
        if MODE == MD_TEMPLATE_BLOCK {
            self.cx.md_set_inline(false);
            self.cx.md_block_boundary()?;
        }
        Ok(())
    }
}

impl<L: DataLoader, B: WriteBuf, const MODE: u8> Drop for MdTemplateScope<'_, L, B, MODE> {
    #[inline(always)]
    fn drop(&mut self) {
        if MODE != MD_TEMPLATE_INHERIT {
            self.cx.md_set_inline(self.inline);
            self.cx.md_state.inline_scope = self.inline_scope;
        }
    }
}

impl<'cx, L: DataLoader, B: WriteBuf> MdFragmentScope<'cx, L, B> {
    pub(crate) fn new(cx: &'cx mut WebContext<L, B>) -> Self {
        let saved = cx.md_state;
        // Formatting state starts fresh. The inline-scope flags carry in so a
        // block fragment inside an inline scope is still rejected, and the
        // suffix tracking carries in so the fragment's leading boundary sees
        // the bytes already written to the shared buffer.
        cx.md_state = MdState {
            heading_offset: 0,
            list_depth: 0,
            ..saved
        };
        Self { cx, saved }
    }

    pub(crate) fn context(&mut self) -> &mut WebContext<L, B> {
        self.cx
    }
}

impl<L: DataLoader, B: WriteBuf> Drop for MdFragmentScope<'_, L, B> {
    fn drop(&mut self) {
        // Restore formatting and scope flags; keep the suffix tracking that
        // describes the bytes the fragment actually wrote, so the caller's
        // next boundary is not emitted twice.
        let live = self.cx.md_state;
        self.cx.md_state = MdState {
            trailing_line: live.trailing_line,
            pending_cr: live.pending_cr,
            ..self.saved
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MdRenderSync, MeteredWriteBuf, OutputMeter, RenderMode, StreamChunkFlusher,
        StreamingZeroCopyBuf, WebRenderSync, ZeroCopyBuf,
    };
    use std::rc::Rc;

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
            let mut cx = WebContext::with_buffer(&mut out);
            cx.write_md_prepared(input).unwrap();
            cx.md_block_boundary().unwrap();
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
            let mut cx = WebContext::with_buffer(&mut out);
            cx.write_md_prepared(input).unwrap();
            cx.md_line_boundary().unwrap();
            assert_eq!(&out, expected, "input={input:?}");
        }
    }

    #[test]
    fn split_crlf_survives_render_state_snapshot() {
        let mut prefix = Vec::new();
        let state = {
            let mut cx = WebContext::with_buffer(&mut prefix);
            cx.write_md_prepared(b"text\r").unwrap();
            cx.md_state()
        };

        let mut suffix = Vec::new();
        let mut cx = WebContext::with_buffer(&mut suffix).with_md_state(state);
        cx.write_md_prepared(b"\n").unwrap();
        cx.md_block_boundary().unwrap();

        prefix.extend_from_slice(&suffix);
        assert_eq!(prefix, b"text\r\n\n");
    }

    #[test]
    fn zero_copy_buffers_do_not_need_a_contiguous_suffix() {
        static LARGE_ZERO_COPY_RUN: [u8; 300] = [b'x'; 300];
        let mut out = ZeroCopyBuf::with_capacity(4, 8);
        {
            let mut cx = WebContext::with_buffer(&mut out);
            cx.write_md_prepared_static(&LARGE_ZERO_COPY_RUN).unwrap();
            assert!(cx.out.as_slice().is_empty());
            cx.md_block_boundary().unwrap();
        }
        let bytes = out.to_vec();
        assert!(bytes.ends_with(b"x\n\n"));

        static LARGE_STREAMING_RUN: [u8; 1100] = [b'y'; 1100];
        let mut out = StreamingZeroCopyBuf::with_capacity(4, 8);
        {
            let mut cx = WebContext::with_buffer(&mut out);
            cx.write_md_prepared_static(&LARGE_STREAMING_RUN).unwrap();
            assert!(cx.out.as_slice().is_empty());
            cx.md_block_boundary().unwrap();
        }
        let bytes = out.to_vec();
        assert!(bytes.ends_with(b"y\n\n"));
    }

    #[test]
    fn streaming_flush_keeps_suffix_state_in_snapshot() {
        let mut out = StreamingZeroCopyBuf::with_capacity(4, 8);
        let state = {
            let mut cx = WebContext::with_buffer(&mut out);
            cx.write_md_prepared_static(b"paragraph\r").unwrap();
            cx.md_state()
        };
        let flushed = out.flush_all().unwrap().to_vec();
        assert_eq!(flushed, b"paragraph\r");

        let mut cx = WebContext::with_buffer(&mut out).with_md_state(state);
        cx.write_md_prepared_static(b"\n").unwrap();
        cx.md_block_boundary().unwrap();
        assert_eq!(out.to_vec(), b"\n\n");
    }

    #[test]
    fn prepared_methods_preserve_write_buf_delegation() {
        let mut out = SpyBuf::default();
        {
            let mut cx = WebContext::with_buffer(&mut out);
            cx.write_md_prepared(b"borrowed").unwrap();
            cx.write_md_prepared_str(" string").unwrap();
            cx.write_md_prepared_static(b" static").unwrap();
            cx.write_md_prepared_shared(&Bytes::from_static(b" shared"))
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
        let mut cx = WebContext::with_buffer(&mut parent_out);
        cx.write_md_prepared_static(b"parent\n\n").unwrap();
        cx.md_push_heading_offset(1);
        cx.md_push_list_depth();
        cx.set_render_mode(RenderMode::ChunkedInOrder);
        cx.set_stream_chunk_flusher(Rc::clone(&original_flusher));

        cx.with_output(&mut child_out, |child| {
            assert_eq!(child.md_heading_offset(), 1);
            assert_eq!(child.md_list_depth(), 1);
            assert!(!child.md_inline());
            assert_eq!(child.md_state.trailing_line, TrailingLineState::Blank);
            assert!(!child.md_state.pending_cr);
            assert_eq!(child.render_mode(), RenderMode::ChunkedInOrder);
            assert!(Rc::ptr_eq(
                child.stream_chunk_flusher().unwrap(),
                &original_flusher
            ));
            child.md_push_heading_offset(2);
            child.md_push_list_depth();
            child.md_set_inline(true);
            child.set_render_mode(RenderMode::Agent);
            child.write_md_prepared_static(b"child\r")?;
            child.set_stream_chunk_flusher(Rc::clone(&replacement_flusher));
            Ok::<_, WriteError>(())
        })
        .unwrap();

        assert_eq!(child_out, b"child\r");
        assert_eq!(cx.md_heading_offset(), 3);
        assert_eq!(cx.md_list_depth(), 2);
        assert!(cx.md_inline());
        assert_eq!(cx.md_state.trailing_line, TrailingLineState::OneBreak);
        assert!(cx.md_state.pending_cr);
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
        let mut cx = WebContext::with_buffer(&mut out);

        cx.render_html_in_md(WebBytes(b"bridged\r\n")).unwrap();
        cx.md_block_boundary().unwrap();

        assert_eq!(cx.out.bytes, b"bridged\r\n\n");
        assert_eq!(cx.out.static_calls, 2);
    }

    #[test]
    fn render_web_keeps_suffix_state_after_unwind() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = cx.render_html_in_md(PanickingWebBytes(b"bridged\r"));
        }));
        assert!(panic.is_err());
        cx.md_block_boundary().unwrap();

        drop(cx);
        assert_eq!(out, b"bridged\r\n\n");
    }

    #[test]
    fn scoped_output_reborrow_restores_mutations_after_error() {
        let flusher: Rc<dyn StreamChunkFlusher> = Rc::new(NoopFlusher);
        let mut parent_out = Vec::new();
        let mut child_out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut parent_out);

        let result = cx.with_output(&mut child_out, |child| {
            child.md_push_heading_offset(4);
            child.md_push_list_depth();
            child.md_set_inline(true);
            child.set_render_mode(RenderMode::ChunkedOutOfOrder);
            child.write_md_prepared_static(b"error\r")?;
            child.set_stream_chunk_flusher(Rc::clone(&flusher));
            Err::<(), _>(WriteError::OutOfCapacity)
        });

        assert_eq!(result, Err(WriteError::OutOfCapacity));
        assert_eq!(cx.md_heading_offset(), 4);
        assert_eq!(cx.md_list_depth(), 1);
        assert!(cx.md_inline());
        assert_eq!(cx.md_state.trailing_line, TrailingLineState::OneBreak);
        assert!(cx.md_state.pending_cr);
        assert_eq!(cx.render_mode(), RenderMode::ChunkedOutOfOrder);
        assert!(Rc::ptr_eq(cx.stream_chunk_flusher().unwrap(), &flusher));
    }

    #[test]
    fn scoped_output_reborrow_restores_state_during_unwind() {
        let flusher: Rc<dyn StreamChunkFlusher> = Rc::new(NoopFlusher);
        let mut parent_out = Vec::new();
        let mut child_out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut parent_out);

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cx.with_output(&mut child_out, |child| {
                child.md_push_heading_offset(2);
                child.md_push_list_depth();
                child.md_set_inline(true);
                child.set_render_mode(RenderMode::Agent);
                child.write_md_prepared_static(b"panic\n").unwrap();
                child.set_stream_chunk_flusher(Rc::clone(&flusher));
                panic!("test unwind");
            });
        }));

        assert!(panic.is_err());
        assert_eq!(cx.md_heading_offset(), 2);
        assert_eq!(cx.md_list_depth(), 1);
        assert!(cx.md_inline());
        assert_eq!(cx.md_state.trailing_line, TrailingLineState::OneBreak);
        assert!(!cx.md_state.pending_cr);
        assert_eq!(cx.render_mode(), RenderMode::Agent);
        assert!(Rc::ptr_eq(cx.stream_chunk_flusher().unwrap(), &flusher));
    }

    #[test]
    fn self_metered_output_reborrow_is_compile_safe_and_restores_state() {
        let meter = OutputMeter::new(32);
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        cx.md_push_heading_offset(1);

        let value = cx
            .with_metered_output(meter.borrow(), |child| {
                child.md_push_heading_offset(2);
                child.md_push_list_depth();
                child.md_set_inline(true);
                child.set_render_mode(RenderMode::Agent);
                child.write_md_prepared_static(b"metered\r")?;
                Ok::<_, WriteError>(17)
            })
            .unwrap();

        assert_eq!(value, 17);
        assert_eq!(meter.consumed(), 8);
        assert_eq!(cx.md_heading_offset(), 3);
        assert_eq!(cx.md_list_depth(), 1);
        assert!(cx.md_inline());
        assert_eq!(cx.md_state.trailing_line, TrailingLineState::OneBreak);
        assert!(cx.md_state.pending_cr);
        assert_eq!(cx.render_mode(), RenderMode::Agent);
        drop(cx);
        assert_eq!(out, b"metered\r");
    }

    #[test]
    fn self_metered_output_reborrow_restores_state_during_unwind() {
        let meter = OutputMeter::new(8);
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cx.with_metered_output(meter.borrow(), |child| {
                child.md_push_heading_offset(3);
                child.md_push_list_depth();
                child.md_set_inline(true);
                panic!("test metered unwind");
            });
        }));

        assert!(panic.is_err());
        assert_eq!(cx.md_heading_offset(), 3);
        assert_eq!(cx.md_list_depth(), 1);
        assert!(cx.md_inline());
    }

    #[test]
    fn reserved_output_bypasses_outer_meter_and_validates_exact_completion() {
        let meter = OutputMeter::new(32);
        let metered = MeteredWriteBuf::new(Vec::new(), meter.borrow());
        let mut cx = WebContext::with_buffer(metered);
        let reservation = meter.reserve_exact(5).unwrap();

        let value = cx
            .with_reserved_output(reservation, |child| {
                child.md_push_heading_offset(2);
                child.md_set_inline(true);
                child.write_md_prepared_static(b"hello")?;
                Ok(23)
            })
            .unwrap();

        assert_eq!(value, 23);
        assert_eq!(
            meter.consumed(),
            5,
            "reserved bytes must not be double-debited"
        );
        assert_eq!(cx.md_heading_offset(), 2);
        assert!(cx.md_inline());
        assert_eq!(cx.into_output().as_slice(), b"hello");
    }

    #[test]
    fn reserved_output_reports_incomplete_success() {
        let meter = OutputMeter::new(8);
        let metered = MeteredWriteBuf::new(Vec::new(), meter.borrow());
        let mut cx = WebContext::with_buffer(metered);
        let reservation = meter.reserve_exact(4).unwrap();

        let error = cx
            .with_reserved_output(reservation, |child| {
                child.write_md_prepared_static(b"ok")?;
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
        let metered = MeteredWriteBuf::new(RejectingBuf, meter.borrow());
        let mut cx = WebContext::with_buffer(metered);
        let reservation = meter.reserve_exact(2).unwrap();

        let error = cx
            .with_reserved_output(reservation, |child| {
                child.md_push_heading_offset(1);
                child.write_md_prepared_static(b"x")?;
                Ok(())
            })
            .unwrap_err();

        assert_eq!(error, WriteError::OutOfCapacity);
        assert_eq!(cx.md_heading_offset(), 1);
    }

    #[test]
    fn reserved_output_restores_state_during_unwind_without_validation() {
        let meter = OutputMeter::new(8);
        let metered = MeteredWriteBuf::new(Vec::new(), meter.borrow());
        let mut cx = WebContext::with_buffer(metered);
        let reservation = meter.reserve_exact(3).unwrap();

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = cx.with_reserved_output(reservation, |child| -> Result<(), WriteError> {
                child.md_push_heading_offset(2);
                child.md_push_list_depth();
                child.set_render_mode(RenderMode::Agent);
                panic!("test reserved unwind");
            });
        }));

        assert!(panic.is_err());
        assert_eq!(meter.consumed(), 3);
        assert_eq!(cx.md_heading_offset(), 2);
        assert_eq!(cx.md_list_depth(), 1);
        assert_eq!(cx.render_mode(), RenderMode::Agent);
    }

    #[test]
    fn prepared_shared_writes_update_boundary_state() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        cx.write_md_prepared_shared(&Bytes::from_static(b"shared\n"))
            .unwrap();
        cx.md_block_boundary().unwrap();
        assert_eq!(out, b"shared\n\n");
    }

    #[test]
    fn boundary_emits_missing_bytes_in_one_write() {
        let mut out = SpyBuf::default();
        {
            let mut cx = WebContext::with_buffer(&mut out);
            cx.write_md_prepared_str("text").unwrap();
            cx.md_block_boundary().unwrap();
        }
        assert_eq!(out.static_calls, 1);
        assert_eq!(out.bytes, b"text\n\n");

        let mut out = SpyBuf::default();
        {
            let mut cx = WebContext::with_buffer(&mut out);
            cx.write_md_prepared(b"text\n").unwrap();
            cx.md_block_boundary().unwrap();
        }
        assert_eq!(out.static_calls, 1);
        assert_eq!(out.bytes, b"text\n\n");

        let mut out = SpyBuf::default();
        {
            let mut cx = WebContext::with_buffer(&mut out);
            cx.write_md_prepared(b"text\r").unwrap();
            cx.md_block_boundary().unwrap();
        }
        assert_eq!(out.static_calls, 1);
        assert_eq!(out.bytes, b"text\r\n\n");
    }

    #[test]
    fn numeric_render_uses_extend_with_and_updates_suffix_state() {
        let mut out = SpyBuf::default();
        {
            let mut cx = WebContext::with_buffer(&mut out);
            cx.write_md_prepared_static(b"\n\n").unwrap();
            42_u32.render_md(&mut cx).unwrap();
            cx.md_block_boundary().unwrap();
        }
        assert_eq!(out.extend_with_calls, 1);
        assert_eq!(out.bytes, b"\n\n42\n\n");
    }

    #[test]
    fn heading_basic() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.md_heading(2, "Hello").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "## Hello\n\n");
    }

    #[test]
    fn heading_with_offset() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        let prev = cx.md_push_heading_offset(1);
        cx.md_heading(2, "Nested").unwrap();
        cx.md_pop_heading_offset(prev);
        assert_eq!(String::from_utf8(buf).unwrap(), "### Nested\n\n");
    }

    #[test]
    fn heading_clamps_at_6() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.md_push_heading_offset(5);
        cx.md_heading(3, "Deep").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "###### Deep\n\n");
    }

    #[test]
    fn blank_line_dedup() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.write_md_prepared_str("text").unwrap();
        cx.md_blank_line().unwrap();
        cx.md_blank_line().unwrap(); // should be deduplicated
        cx.write_md_prepared_str("more").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "text\n\nmore");
    }

    #[test]
    fn hr() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.write_md_prepared_str("above").unwrap();
        cx.md_hr().unwrap();
        cx.write_md_prepared_str("below").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "above\n---\n\nbelow");
    }

    #[test]
    fn blockquote() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.md_blockquote("quoted text").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "> quoted text\n");
    }

    #[test]
    fn bold_and_code() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.md_bold("important").unwrap();
        cx.write_md_prepared_str(" and ").unwrap();
        cx.md_code("code").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "**important** and `code`");
    }

    #[test]
    fn link() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.md_link("click here", "https://example.com").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[click here](https://example.com)"
        );
    }

    #[test]
    fn literal_text_cannot_inject_markdown_blocks_links_or_html() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.write_md_text("safe\r\n# heading\r- item\n[x](javascript:bad)\t<div>")
            .unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "safe \\# heading \\- item \\[x\\]\\(javascript\\:bad\\) \\<div\\>"
        );
    }

    #[test]
    fn semantic_labels_are_single_line_and_unicode_preserving() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.md_heading(1, "世界\r\n# nested").unwrap();
        cx.md_blockquote("引用\n> nested").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "# 世界 \\# nested\n\n> 引用 \\> nested\n"
        );
    }

    #[test]
    fn links_sanitize_destinations_and_keep_labels_literal() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.md_link("[click](elsewhere)", "javascript:alert(1)")
            .unwrap();
        cx.write_md_prepared_static(b" ").unwrap();
        cx.md_link("safe", "https://example.test/a (b)\\c").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[\\[click\\]\\(elsewhere\\)]() [safe](https://example.test/a%20\\(b\\)\\\\c)"
        );
    }

    #[test]
    fn inline_code_payload_cannot_close_its_span() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.md_code("`a``b```c`\r\n# still code").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "```` `a``b```c` # still code ````"
        );
    }

    #[test]
    fn normalized_crlf_updates_suffix_state_as_one_space() {
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        cx.write_md_text("\r\n").unwrap();
        cx.md_block_boundary().unwrap();
        assert_eq!(buf, b" \n\n");
    }
}
