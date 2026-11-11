// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Write buffer traits and implementations for zero-allocation rendering.
//!
//! This module provides a `WriteBuf` trait that abstracts over different buffer types,
//! enabling both heap-allocated (`Vec<u8>`) and stack-allocated (fixed slices via `WriteCursor`)
//! rendering paths.

use bytes::{BufMut, Bytes, BytesMut};

use crate::render::LoadError;

/// Error type for write buffer operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// The buffer ran out of capacity.
    OutOfCapacity,
    /// A render activation attempted to emit more bytes than its output
    /// budget permits.
    ///
    /// The debit is checked before the corresponding write reaches the
    /// underlying buffer. This error is deliberately not recoverable by a UI
    /// error boundary: rendering fallback output would otherwise bypass the
    /// activation-wide cap.
    OutputLimitExceeded {
        /// Number of bytes the rejected write attempted to reserve.
        requested: u64,
        /// Number of bytes still available when the reservation failed.
        remaining: u64,
    },
    /// A reservation-scoped writer attempted to consume more than the exact
    /// credit granted to it.
    ///
    /// This is a target contract violation rather than an activation output
    /// limit failure. Interpreters should map it to their target-contract
    /// error instead of reporting [`WriteError::OutputLimitExceeded`].
    OutputReservationExceeded {
        /// Number of bytes requested by the write that exceeded the credit.
        requested: u64,
        /// Total byte count granted to this reservation.
        reserved: u64,
        /// Bytes already consumed before the rejected write.
        consumed: u64,
        /// Credit still available when the write was rejected.
        remaining: u64,
    },
    /// A reservation-scoped writer finished before consuming its exact byte
    /// credit.
    ///
    /// Like [`WriteError::OutputReservationExceeded`], this is a target
    /// contract violation and is not recoverable by a UI error boundary.
    OutputReservationIncomplete {
        /// Total byte count granted to this reservation.
        reserved: u64,
        /// Bytes consumed before completion was checked.
        consumed: u64,
        /// Unused credit at completion.
        remaining: u64,
    },
    /// An invalid attribute was passed to the writer.
    InvalidAttribute(&'static str),
    /// The input contained a character forbidden by the active output dialect.
    InvalidCharForDialect,
    /// Raw-text escaping was requested for a dialect without raw-text states.
    RawTextUnsupportedForDialect,
    /// Render-time data loading failed.
    Load(LoadError),
    /// The streaming response sink closed before the render finished.
    StreamClosed,
    /// A raw byte write was not valid UTF-8 on its own. Text writes (`&str`)
    /// are never checked; raw byte writes are validated where they are written.
    InvalidUtf8,
    /// A runtime renderer failed outside the write path itself.
    ///
    /// Compiled templates normally fail through the write/load variants above.
    /// This variant also supports conversion adapters, template interpreters,
    /// and similar hosts whose render-time failures (budget exhaustion,
    /// malformed programs, type errors in evaluated expressions) are not
    /// buffer or loader conditions but must still surface through
    /// `WebRender`'s `Result<(), WriteError>`.
    Render(std::borrow::Cow<'static, str>),
    /// A rejected managed-head lifecycle operation.
    HeadLifecycle(crate::managed_head::HeadError),
    /// A managed-head failure retaining its original error and component.
    HeadFailure(std::sync::Arc<crate::managed_head::HeadFailure>),
}

impl WriteError {
    /// Build a [`WriteError::Render`] from an interpreter/renderer failure
    /// message.
    pub fn render(message: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        Self::Render(message.into())
    }

    /// Whether this error represents render-local failure that an
    /// `ErrorBoundary` may replace with fallback UI.
    ///
    /// Transport and capacity failures are not catchable: once the sink is
    /// closed or the caller-provided buffer is full, rendering fallback HTML is
    /// unlikely to succeed and may hide the actual system failure.
    pub const fn is_boundary_catchable(&self) -> bool {
        matches!(
            self,
            Self::InvalidAttribute(_)
                | Self::InvalidCharForDialect
                | Self::RawTextUnsupportedForDialect
                | Self::Load(_)
                | Self::Render(_)
        )
    }
}

impl core::fmt::Display for WriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WriteError::HeadFailure(e) => core::fmt::Display::fmt(e, f),
            WriteError::HeadLifecycle(e) => write!(f, "head lifecycle ({e:?}): {e}"),
            WriteError::OutOfCapacity => write!(f, "buffer out of capacity"),
            WriteError::OutputLimitExceeded {
                requested,
                remaining,
            } => write!(
                f,
                "output limit exceeded: requested {requested} bytes with {remaining} remaining"
            ),
            WriteError::OutputReservationExceeded {
                requested,
                reserved,
                consumed,
                remaining,
            } => write!(
                f,
                "output reservation exceeded: requested {requested} bytes with {remaining} of {reserved} remaining after consuming {consumed}"
            ),
            WriteError::OutputReservationIncomplete {
                reserved,
                consumed,
                remaining,
            } => write!(
                f,
                "output reservation incomplete: consumed {consumed} of {reserved} bytes with {remaining} remaining"
            ),
            WriteError::InvalidAttribute(msg) => write!(f, "invalid attribute: {}", msg),
            WriteError::InvalidCharForDialect => {
                write!(f, "invalid character for output dialect")
            }
            WriteError::RawTextUnsupportedForDialect => {
                write!(f, "raw-text escaping is unsupported for output dialect")
            }
            WriteError::Load(err) => write!(f, "load failed: {err}"),
            WriteError::StreamClosed => write!(f, "stream closed"),
            WriteError::InvalidUtf8 => write!(f, "raw byte write was not valid UTF-8"),
            WriteError::Render(msg) => write!(f, "render failed: {msg}"),
        }
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::HeadFailure(e) => Some(e.as_ref()),
            Self::Load(e) => Some(e),
            _ => None,
        }
    }
}

impl From<LoadError> for WriteError {
    fn from(value: LoadError) -> Self {
        Self::Load(value)
    }
}

/// Trait for types that can be used as write buffers for HTML rendering.
///
/// This trait abstracts over different buffer types (`Vec<u8>`, fixed slices, etc.)
/// to allow zero-allocation rendering when the caller provides their own buffer.
///
/// # Implementors
///
/// - `Vec<u8>` - Heap-allocated, growable buffer (never fails)
/// - `WriteCursor<'a>` - Stack-allocated, fixed-capacity buffer (may fail)
/// - `BumpWriter<N>` - Fixed-capacity bump writer with compile-time known size (may fail)
/// - `ChainedBumpWriter<N>` - Inline buffer that spills to heap on overflow (never fails)
/// - `RenderBuffer` - Standard 4 KiB chained bump writer for reusable render buffers
/// - `StreamingZeroCopyBuf` - Streaming-oriented segmented buffer with zero-copy checkpoints
#[allow(unsafe_code)]
pub trait WriteBuf {
    fn __head_supports_pages() -> bool {
        false
    }
    fn __head_take_pages(&mut self) -> Result<Vec<Bytes>, WriteError> {
        Err(WriteError::HeadLifecycle(
            crate::managed_head::HeadError::UnsupportedWriter,
        ))
    }

    /// Returns the total capacity of the buffer.
    fn capacity(&self) -> usize;

    /// Returns the current length (bytes written).
    fn len(&self) -> usize;

    /// Returns true if no bytes have been written.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the remaining capacity.
    fn remaining(&self) -> usize {
        self.capacity().saturating_sub(self.len())
    }

    /// Reserve additional capacity. No-op for fixed-size buffers.
    fn reserve(&mut self, _additional: usize) -> Result<(), WriteError> {
        Ok(())
    }

    /// Append text.
    ///
    /// This is the storage primitive every writer implements. `&str` carries
    /// the UTF-8 guarantee in its type, so nothing is validated here.
    fn extend_str(&mut self, text: &str) -> Result<(), WriteError>;

    /// Append static text. Implementations that support scatter-gather may
    /// store the pointer directly (zero-copy) instead of copying bytes.
    /// Default delegates to `extend_str()`.
    ///
    /// Always inlined. Every template literal reaches the writer through this
    /// method, and its length is a compile-time constant only while the call
    /// is inlined. Left to the optimizer, this wrapper was emitted as a real
    /// function once `extend_str` grew, the literal's length became a runtime
    /// argument, and the standard benchmark page rendered 28% slower (12.9 to
    /// 16.6 µs for ten fresh renders).
    #[inline(always)]
    fn extend_static_str(&mut self, text: &'static str) -> Result<(), WriteError> {
        self.extend_str(text)
    }

    /// Append raw bytes, validating at the write that they are UTF-8.
    ///
    /// The bytes must be valid UTF-8 on their own: a multi-byte character
    /// split across two calls is rejected with [`WriteError::InvalidUtf8`].
    /// Prefer [`WriteBuf::extend_str`] when the source is already text.
    #[inline]
    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        self.extend_str(checked_utf8(bytes)?)
    }

    /// Append one ASCII byte. Non-ASCII bytes are rejected because a lone
    /// byte above `0x7F` is never valid UTF-8.
    #[inline]
    fn push(&mut self, byte: u8) -> Result<(), WriteError> {
        if !byte.is_ascii() {
            return Err(WriteError::InvalidUtf8);
        }
        // SAFETY: checked ASCII above.
        unsafe { self.push_unchecked(byte) }
    }

    /// Append one byte without checking it.
    ///
    /// # Safety
    ///
    /// `byte` must be ASCII.
    #[inline]
    unsafe fn push_unchecked(&mut self, byte: u8) -> Result<(), WriteError> {
        let buf = [byte];
        // SAFETY: the caller guarantees one ASCII byte.
        self.extend_str(unsafe { core::str::from_utf8_unchecked(&buf) })
    }

    /// Append static raw bytes, validating at the write that they are UTF-8.
    /// Prefer [`WriteBuf::extend_static_str`] for literals.
    #[inline]
    fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
        self.extend_static_str(checked_utf8(bytes)?)
    }

    /// Extend with runtime-owned shared bytes, validating at the write that
    /// they are UTF-8.
    ///
    /// This is the runtime-data counterpart to [`WriteBuf::extend_static_str`]:
    /// callers that hold pre-rendered byte runs in refcounted
    /// [`bytes::Bytes`] storage (interpreted page programs, cached
    /// fragments) pass the handle here instead of a plain slice. The default
    /// copies through `extend()`. Scatter-gather implementations may override
    /// this to keep a clone of the handle instead; an override must validate
    /// the bytes itself.
    #[inline]
    fn extend_shared(&mut self, bytes: &Bytes) -> Result<(), WriteError> {
        self.extend(bytes)
    }

    /// Write generated text directly into spare capacity via a closure. The
    /// closure receives a `&mut [u8]` of `max_len` bytes and returns the number
    /// of bytes it wrote. The window's prior contents are unspecified and
    /// differ between writers, so the closure must not read before it writes.
    ///
    /// Implementations with contiguous spare capacity (Vec, WriteCursor, etc.)
    /// override this to write in-place with zero intermediate copy. The default
    /// uses a stack buffer of up to 40 bytes.
    ///
    /// `max_len` must be ≤ 40.
    ///
    /// # Safety
    ///
    /// The bytes the closure reports as written must be valid UTF-8 on their
    /// own. Nothing validates them.
    #[inline]
    unsafe fn extend_with<F>(&mut self, max_len: usize, f: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        debug_assert!(max_len <= 40);
        let mut tmp = [0u8; 40];
        let len = f(&mut tmp[..max_len]);
        // SAFETY: the caller guarantees the written bytes are valid UTF-8.
        self.extend_str(unsafe { core::str::from_utf8_unchecked(&tmp[..len]) })
    }

    /// View of the written portion.
    fn as_slice(&self) -> &[u8];

    /// Convert to a `Vec<u8>` (may allocate).
    fn to_vec(&self) -> Vec<u8>;

    /// Clear the buffer for reuse.
    fn clear(&mut self);
}

/// Whether truncating `written` at `at` leaves the stream on a character
/// boundary. `following` is the first byte after `written` when the cut lands
/// exactly at its end.
///
/// Checkpoints are captured between writes and every write is whole UTF-8, so
/// a live checkpoint always passes. A stale checkpoint (captured before an
/// earlier rewind and rewrite) can land inside a character; rejecting it keeps
/// the buffer valid UTF-8.
#[inline]
fn cut_is_char_boundary(written: &[u8], at: usize, following: Option<u8>) -> bool {
    match written.get(at).copied().or(following) {
        // UTF-8 continuation bytes are 0b10xx_xxxx.
        Some(byte) => (byte as i8) >= -0x40,
        None => true,
    }
}

/// Validate a raw byte write. Every raw-byte entry point funnels through here.
///
/// Most raw writes are ASCII markup. The ASCII check is inlined so it folds
/// away for byte-string literals and stays a short word-at-a-time loop for
/// runtime slices; only non-ASCII input pays for full UTF-8 validation.
#[inline(always)]
#[allow(unsafe_code)]
fn checked_utf8(bytes: &[u8]) -> Result<&str, WriteError> {
    if bytes.is_ascii() {
        // SAFETY: ASCII is valid UTF-8.
        return Ok(unsafe { core::str::from_utf8_unchecked(bytes) });
    }
    checked_utf8_slow(bytes)
}

#[inline(never)]
fn checked_utf8_slow(bytes: &[u8]) -> Result<&str, WriteError> {
    core::str::from_utf8(bytes).map_err(|_| WriteError::InvalidUtf8)
}

/// Copy `len` bytes, without calling `memcpy` when `len` is at most 64.
///
/// # Why
///
/// A copy whose length is only known at run time compiles to a call into the
/// C library's `memcpy`. For a short string that call costs more than the
/// copy itself: the jump through the dynamic linker's stub, `memcpy` working
/// out which size class it was handed, and the registers the caller loses
/// across the call add up to about 1.3 ns on an Apple M4. Moving 25 bytes
/// takes a fraction of that.
///
/// Fixed text never paid this. A template literal reaches the writer with a
/// constant length, and the compiler already turns that into a few direct
/// loads and stores. Dynamic text always paid it: a name, a price, a label or
/// an escaped run has a run-time length, and most of them are short.
///
/// # How
///
/// A length from 2 to 64 is copied with two unaligned loads and two unaligned
/// stores of one fixed width, overlapping in the middle. A 25-byte string
/// moves as bytes 0..16 and bytes 9..25. The width is 32, 16, 8, 4 or 2 bytes,
/// chosen by `len`. Anything longer goes to `memcpy`, where the call is a
/// small part of the work and the library can pick wider vector registers
/// than this build is allowed to assume.
///
/// When `len` is a compile-time constant the branches fold away, so fixed
/// text compiles to the same code as a plain copy.
///
/// # What was measured
///
/// The workload is the `large_page` fixture from the framework benchmarks:
/// 16 sections of 16 cards, each card with 8 list items. One render writes
/// 269,751 bytes of HTML in about 7,800 appends, and 2,048 of those appends
/// are item labels of 20 to 35 bytes.
///
/// Rendering it into a reused `RenderBuffer` on an Apple M4 Max:
///
/// | Label lengths | Plain copy | This function |
/// |---|---|---|
/// | Known only at run time | 10.5 µs | 7.8 µs |
/// | Hidden from the optimizer, as real data is | 13.0 µs | 9.2 µs |
/// | Folded to constants by the optimizer | 7.8 µs | 7.8 µs |
///
/// The first row is the 2,048 `memcpy` calls going away, about 1.3 ns each.
/// The last row is the benchmark's own best case, where the optimizer unrolls
/// the item loop and works out every label's length ahead of time. Real data
/// cannot be folded like that, so the first two rows are the ones that
/// describe real pages.
///
/// The 64-byte limit comes from reasoning, not from a sweep: at about that
/// size the call and the copy cost the same, and most dynamic HTML values are
/// far shorter.
///
/// # Safety
///
/// `src` must be valid for reads of `len` bytes, `dst` must be valid for
/// writes of `len` bytes, and the two ranges must not overlap.
#[inline(always)]
#[allow(unsafe_code)]
unsafe fn copy_short(src: *const u8, dst: *mut u8, len: usize) {
    // SAFETY: each branch runs only when `len` is at least the width it moves,
    // so both the read at the start and the read ending at `len` stay inside
    // `[src, src + len)`, and the matching writes stay inside
    // `[dst, dst + len)`. The caller guarantees those ranges are valid and
    // disjoint.
    unsafe {
        if len > 64 {
            core::ptr::copy_nonoverlapping(src, dst, len);
        } else if len >= 32 {
            let head = src.cast::<[u8; 32]>().read_unaligned();
            let tail = src.add(len - 32).cast::<[u8; 32]>().read_unaligned();
            dst.cast::<[u8; 32]>().write_unaligned(head);
            dst.add(len - 32).cast::<[u8; 32]>().write_unaligned(tail);
        } else if len >= 16 {
            let head = src.cast::<u128>().read_unaligned();
            let tail = src.add(len - 16).cast::<u128>().read_unaligned();
            dst.cast::<u128>().write_unaligned(head);
            dst.add(len - 16).cast::<u128>().write_unaligned(tail);
        } else if len >= 8 {
            let head = src.cast::<u64>().read_unaligned();
            let tail = src.add(len - 8).cast::<u64>().read_unaligned();
            dst.cast::<u64>().write_unaligned(head);
            dst.add(len - 8).cast::<u64>().write_unaligned(tail);
        } else if len >= 4 {
            let head = src.cast::<u32>().read_unaligned();
            let tail = src.add(len - 4).cast::<u32>().read_unaligned();
            dst.cast::<u32>().write_unaligned(head);
            dst.add(len - 4).cast::<u32>().write_unaligned(tail);
        } else if len >= 2 {
            let head = src.cast::<u16>().read_unaligned();
            let tail = src.add(len - 2).cast::<u16>().read_unaligned();
            dst.cast::<u16>().write_unaligned(head);
            dst.add(len - 2).cast::<u16>().write_unaligned(tail);
        } else if len == 1 {
            *dst = *src;
        }
    }
}

/// Buffer types that can rewind bytes written since a local checkpoint.
///
/// This is a low-level primitive for pre-flush error recovery. The buffer does
/// not know about error boundaries; it only restores the output cursor to a
/// caller-owned checkpoint when that checkpoint is still valid.
pub trait CheckpointWriteBuf: WriteBuf {
    type Checkpoint: Copy;

    /// Capture the current write position.
    fn checkpoint(&self) -> Self::Checkpoint;

    /// Rewind the buffer to a previously captured write position.
    ///
    /// Returns `false` when the checkpoint no longer belongs to the live
    /// buffer suffix, for example after an incremental flush consumed the
    /// bytes needed to restore the position.
    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool;
}

// A metered writer frequently wraps a scoped `&mut B` so it can be installed
// in an existing render context without moving the caller's buffer. Keep the
// forwarding implementations here, beside the traits, so borrowed buffers
// preserve every optimized write hook and do not acquire a `B: 'static` bound.
#[allow(unsafe_code)]
impl<B: WriteBuf + ?Sized> WriteBuf for &mut B {
    fn __head_supports_pages() -> bool {
        B::__head_supports_pages()
    }
    fn __head_take_pages(&mut self) -> Result<Vec<Bytes>, WriteError> {
        (**self).__head_take_pages()
    }

    #[inline(always)]
    fn capacity(&self) -> usize {
        (**self).capacity()
    }

    #[inline(always)]
    fn len(&self) -> usize {
        (**self).len()
    }

    #[inline(always)]
    fn remaining(&self) -> usize {
        (**self).remaining()
    }

    #[inline(always)]
    fn reserve(&mut self, additional: usize) -> Result<(), WriteError> {
        (**self).reserve(additional)
    }

    #[inline(always)]
    fn extend_str(&mut self, text: &str) -> Result<(), WriteError> {
        (**self).extend_str(text)
    }

    #[inline(always)]
    unsafe fn push_unchecked(&mut self, byte: u8) -> Result<(), WriteError> {
        // SAFETY: forwarded contract.
        unsafe { (**self).push_unchecked(byte) }
    }

    #[inline(always)]
    fn extend_static_str(&mut self, text: &'static str) -> Result<(), WriteError> {
        (**self).extend_static_str(text)
    }

    #[inline(always)]
    fn extend_shared(&mut self, bytes: &Bytes) -> Result<(), WriteError> {
        (**self).extend_shared(bytes)
    }

    #[inline(always)]
    unsafe fn extend_with<F>(&mut self, max_len: usize, f: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        // SAFETY: forwarded contract.
        unsafe { (**self).extend_with(max_len, f) }
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        (**self).as_slice()
    }

    #[inline(always)]
    fn to_vec(&self) -> Vec<u8> {
        (**self).to_vec()
    }

    #[inline(always)]
    fn clear(&mut self) {
        (**self).clear();
    }
}

impl<B: CheckpointWriteBuf + ?Sized> CheckpointWriteBuf for &mut B {
    type Checkpoint = B::Checkpoint;

    #[inline(always)]
    fn checkpoint(&self) -> Self::Checkpoint {
        (**self).checkpoint()
    }

    #[inline(always)]
    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
        (**self).rewind_to(checkpoint)
    }
}

// ============================================================================
// Vec<u8> implementation - heap allocated, growable, never fails
// ============================================================================

#[allow(unsafe_code)]
impl WriteBuf for Vec<u8> {
    #[inline(always)]
    fn capacity(&self) -> usize {
        Vec::capacity(self)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        Vec::len(self)
    }

    #[inline(always)]
    fn reserve(&mut self, additional: usize) -> Result<(), WriteError> {
        Vec::reserve(self, additional);
        Ok(())
    }

    #[inline(always)]
    fn extend_str(&mut self, text: &str) -> Result<(), WriteError> {
        self.extend_from_slice(text.as_bytes());
        Ok(())
    }

    #[inline(always)]
    unsafe fn push_unchecked(&mut self, byte: u8) -> Result<(), WriteError> {
        Vec::push(self, byte);
        Ok(())
    }

    #[inline(always)]
    unsafe fn extend_with<F>(&mut self, max_len: usize, f: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        self.reserve(max_len);
        let start = self.len();
        // Grow the Vec to make spare capacity addressable, write into it,
        // then shrink to the actual length.
        self.resize(start + max_len, 0);
        let len = f(&mut self[start..start + max_len]);
        self.truncate(start + len);
        Ok(())
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        self
    }

    #[inline(always)]
    fn to_vec(&self) -> Vec<u8> {
        self.clone()
    }

    #[inline(always)]
    fn clear(&mut self) {
        Vec::clear(self);
    }
}

impl CheckpointWriteBuf for Vec<u8> {
    type Checkpoint = usize;

    #[inline(always)]
    fn checkpoint(&self) -> Self::Checkpoint {
        self.len()
    }

    #[inline(always)]
    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
        if checkpoint > self.len() {
            return false;
        }
        self.truncate(checkpoint);
        true
    }
}

// ============================================================================
// WriteCursor - stack/bump-allocator style writes to a fixed slice
// ============================================================================

/// Cursor-backed writer over a fixed-size mutable slice.
///
/// This provides zero-allocation rendering by writing directly to a caller-provided
/// buffer. Use this for maximum performance when the buffer size is known ahead of time.
///
/// Works like a bump allocator - writes advance a cursor through a pre-allocated region,
/// with no per-write allocation overhead.
///
/// # Example
///
/// ```rust
/// use proa_core::shared::write_buf::{WriteBuf, WriteCursor};
///
/// // Stack-allocated 4KB buffer
/// let mut backing = [0u8; 4096];
/// let mut cursor = WriteCursor::new(&mut backing);
///
/// cursor.extend_str("<div>").unwrap();
/// cursor.extend_str("Hello").unwrap();
/// cursor.extend_str("</div>").unwrap();
///
/// assert_eq!(cursor.as_slice(), b"<div>Hello</div>");
/// ```
///
/// # Performance
///
/// `WriteCursor` uses unchecked indexing after bounds validation, making it
/// nearly as fast as raw pointer writes while remaining safe. The single
/// bounds check per operation is unavoidable but the actual memory access
/// has zero overhead.
pub struct WriteCursor<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> WriteCursor<'a> {
    /// Create a new cursor over the given buffer.
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    /// View of the written portion of the buffer.
    #[inline]
    pub fn as_written(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Returns the backing buffer's total size.
    #[inline]
    pub fn backing_len(&self) -> usize {
        self.buf.len()
    }
}

#[allow(unsafe_code)]
impl<'a> WriteBuf for WriteCursor<'a> {
    #[inline(always)]
    fn capacity(&self) -> usize {
        self.buf.len()
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    fn reserve(&mut self, additional: usize) -> Result<(), WriteError> {
        if additional > self.remaining() {
            return Err(WriteError::OutOfCapacity);
        }
        Ok(())
    }

    #[inline(always)]
    fn extend_str(&mut self, text: &str) -> Result<(), WriteError> {
        let bytes = text.as_bytes();
        let new_len = self.len + bytes.len();
        if new_len > self.buf.len() {
            return Err(WriteError::OutOfCapacity);
        }
        self.buf[self.len..new_len].copy_from_slice(bytes);
        self.len = new_len;
        Ok(())
    }

    #[inline(always)]
    unsafe fn push_unchecked(&mut self, byte: u8) -> Result<(), WriteError> {
        if self.len == self.buf.len() {
            return Err(WriteError::OutOfCapacity);
        }
        self.buf[self.len] = byte;
        self.len += 1;
        Ok(())
    }

    #[inline(always)]
    unsafe fn extend_with<F>(&mut self, max_len: usize, f: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        if self.len + max_len > self.buf.len() {
            return Err(WriteError::OutOfCapacity);
        }
        let len = f(&mut self.buf[self.len..self.len + max_len]);
        self.len += len;
        Ok(())
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    #[inline(always)]
    fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.len = 0;
    }
}

impl<'a> CheckpointWriteBuf for WriteCursor<'a> {
    type Checkpoint = usize;

    #[inline(always)]
    fn checkpoint(&self) -> Self::Checkpoint {
        self.len
    }

    #[inline(always)]
    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
        if checkpoint > self.len {
            return false;
        }
        self.len = checkpoint;
        true
    }
}

// ============================================================================
// BumpWriter<N> - const-generic fixed buffer with thin pointer
// ============================================================================

/// Fixed-size heap-allocated buffer with compile-time-known capacity.
///
/// Uses `Box<[u8; N]>` — a thin pointer whose capacity is a compile-time
/// constant. This lets LLVM treat the capacity check as a comparison against
/// an immediate value rather than a memory load.
///
/// # Example
///
/// ```rust
/// use proa_core::shared::write_buf::{BumpWriter, WriteBuf};
///
/// let mut buf = BumpWriter::<4096>::new();
/// buf.extend_str("<div>Hello</div>").unwrap();
/// assert_eq!(buf.as_slice(), b"<div>Hello</div>");
/// ```
pub struct BumpWriter<const N: usize> {
    // `N` bytes that start out uninitialized: like `Vec::with_capacity(N)`,
    // the page is allocated but not zero-filled. Only `buf[..len]` has been
    // written, and only that prefix is ever read.
    //
    // A boxed array is a thin pointer, which keeps this struct at two words.
    // A boxed slice here (a fat pointer) measurably slows the write path.
    buf: Box<[core::mem::MaybeUninit<u8>; N]>,
    len: usize,
}

impl<const N: usize> BumpWriter<N> {
    /// Create an empty `BumpWriter`.
    ///
    /// The page is allocated without being zero-filled.
    #[inline]
    pub fn new() -> Self {
        Self {
            buf: Box::new_uninit_slice(N).try_into().unwrap(),
            len: 0,
        }
    }

    /// Hand the page to a transport as `Bytes` without copying it.
    ///
    /// `Bytes` owns the whole allocation as initialized bytes, so the unwritten
    /// tail is zeroed here, at hand-off, instead of the whole page at
    /// allocation.
    #[inline]
    #[allow(unsafe_code)]
    fn into_bytes(mut self) -> Bytes {
        let len = self.len;
        // SAFETY: `len <= N`, so the tail lies inside the page; after zeroing
        // it every byte of the allocation is initialized.
        let owned: Box<[u8]> = unsafe {
            core::ptr::write_bytes(self.ptr().add(len), 0, N - len);
            let page: Box<[core::mem::MaybeUninit<u8>]> = self.buf;
            page.assume_init()
        };
        let mut bytes = Bytes::from(owned);
        bytes.truncate(len);
        bytes
    }

    /// Start of the page.
    #[inline(always)]
    fn ptr(&mut self) -> *mut u8 {
        self.buf.as_mut_ptr().cast()
    }

    /// A zeroed window of `max_len` spare bytes for the in-place writer.
    ///
    /// Only the window is zeroed, so the closure it is handed to never sees
    /// uninitialized bytes, or bytes a reused buffer wrote earlier, and the
    /// rest of the page stays untouched.
    ///
    /// # Safety
    ///
    /// `self.len + max_len` must not exceed `N`.
    #[inline(always)]
    #[allow(unsafe_code)]
    unsafe fn spare_window(&mut self, max_len: usize) -> &mut [u8] {
        // SAFETY: the caller guarantees the window lies inside the page, and
        // zeroing it makes every byte of the returned slice initialized.
        unsafe {
            let start = self.ptr().add(self.len);
            core::ptr::write_bytes(start, 0, max_len);
            core::slice::from_raw_parts_mut(start, max_len)
        }
    }

    /// View of the written portion.
    #[inline]
    #[allow(unsafe_code)]
    pub fn as_written(&self) -> &[u8] {
        // SAFETY: every write path initializes the bytes below `len` before
        // advancing it.
        unsafe { core::slice::from_raw_parts(self.buf.as_ptr().cast(), self.len) }
    }
}

impl<const N: usize> Default for BumpWriter<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(unsafe_code)]
impl<const N: usize> WriteBuf for BumpWriter<N> {
    #[inline(always)]
    fn capacity(&self) -> usize {
        N
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    fn reserve(&mut self, _additional: usize) -> Result<(), WriteError> {
        Ok(())
    }

    #[inline(always)]
    #[allow(unsafe_code)]
    fn extend_str(&mut self, text: &str) -> Result<(), WriteError> {
        let bytes = text.as_bytes();
        let new_len = self.len + bytes.len();
        if new_len > N {
            return Err(WriteError::OutOfCapacity);
        }
        // SAFETY: bounds check above guarantees self.len + bytes.len() <= N,
        // and N is the exact size of the allocation.
        //
        // This is the same pattern used by Vec::extend_from_slice in the standard
        // library: capacity check → copy_nonoverlapping → set_len.
        // See: https://doc.rust-lang.org/src/alloc/vec/spec_extend.rs.html
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr().add(self.len), bytes.len());
        }
        self.len = new_len;
        Ok(())
    }

    #[inline(always)]
    #[allow(unsafe_code)]
    unsafe fn push_unchecked(&mut self, byte: u8) -> Result<(), WriteError> {
        if self.len >= N {
            return Err(WriteError::OutOfCapacity);
        }
        // SAFETY: bounds check above guarantees self.len < N.
        unsafe {
            *self.ptr().add(self.len) = byte;
        }
        self.len += 1;
        Ok(())
    }

    #[inline(always)]
    unsafe fn extend_with<F>(&mut self, max_len: usize, f: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        if self.len + max_len > N {
            return Err(WriteError::OutOfCapacity);
        }
        // SAFETY: the bounds check above keeps the window inside the page.
        let written = f(unsafe { self.spare_window(max_len) });
        assert!(written <= max_len, "in-place writer overran its window");
        self.len += written;
        Ok(())
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        self.as_written()
    }

    #[inline(always)]
    fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.len = 0;
    }
}

impl<const N: usize> CheckpointWriteBuf for BumpWriter<N> {
    type Checkpoint = usize;

    #[inline(always)]
    fn checkpoint(&self) -> Self::Checkpoint {
        self.len
    }

    #[inline(always)]
    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
        if checkpoint > self.len {
            return false;
        }
        self.len = checkpoint;
        true
    }
}

// ============================================================================
// ChainedBumpWriter - linked chain of BumpWriter<N> pages
// ============================================================================

/// Standard page size for reusable chained bump render buffers.
pub const DEFAULT_CHAINED_BUMP_PAGE_SIZE: usize = 4096;

/// The standard reusable buffer for buffered rendering.
///
/// A plain `Vec<u8>` also implements [`WriteBuf`], which makes it an easy
/// wrong answer: it reallocates and copies as the document grows, and forces a
/// UTF-8 re-validation pass over the whole output at the end. This one bump
/// allocates into fixed pages, never copies what it has already written, and
/// hands back a `String` without revalidating.
///
/// The name is deliberately the obvious one, so reaching for `Vec::new()` is a
/// deliberate choice rather than the path of least resistance.
pub type RenderBuffer = ChainedBumpWriter<DEFAULT_CHAINED_BUMP_PAGE_SIZE>;

/// Chained fixed-size bump writers for unbounded output.
///
/// Uses [`BumpWriter<N>`] pages internally. When the current page fills up,
/// it is parked in a `Vec` and a new page is allocated. Each page gets the
/// same fast bump-pointer writes with compile-time-known capacity.
/// Pages are recycled across `clear()` calls, so steady-state rendering
/// avoids reallocating the chain on every request.
///
/// At the end, the filled pages can be sent via scatter-gather (`writev`) or
/// flattened into a single contiguous buffer.
///
/// # Example
///
/// ```rust
/// use proa_core::shared::write_buf::{RenderBuffer, WriteBuf};
///
/// let mut buf = RenderBuffer::new();
/// buf.extend_str("<div>Hello</div>").unwrap();
/// let output = buf.to_vec(); // flatten for HTTP response
/// ```
pub struct ChainedBumpWriter<const N: usize> {
    full: Vec<BumpWriter<N>>,
    spare: Vec<BumpWriter<N>>,
    current: BumpWriter<N>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainedBumpCheckpoint {
    full_len: usize,
    current_len: usize,
}

impl<const N: usize> ChainedBumpWriter<N> {
    /// Create a new `ChainedBumpWriter` with `N`-byte pages.
    #[inline]
    pub fn new() -> Self {
        Self {
            full: Vec::new(),
            spare: Vec::new(),
            current: BumpWriter::new(),
        }
    }

    /// Returns an iterator over all written slices (for scatter-gather I/O).
    pub fn slices(&self) -> impl Iterator<Item = &[u8]> {
        self.full
            .iter()
            .map(|page| page.as_slice())
            .chain(std::iter::once(self.current.as_slice()))
    }

    /// Number of pages used (full + current).
    pub fn page_count(&self) -> usize {
        self.full.len() + 1
    }

    /// Consume the written pages without copying their bytes.
    ///
    /// Each chunk owns its original page allocation and exposes only written
    /// bytes. Empty and spare pages are dropped. Dropping the iterator releases
    /// pages that the transport has not consumed yet.
    pub fn into_pages(self) -> impl Iterator<Item = Bytes> + Send + 'static {
        self.full
            .into_iter()
            .chain(std::iter::once(self.current))
            .filter_map(|page| {
                if page.len == 0 {
                    return None;
                }
                Some(page.into_bytes())
            })
    }

    #[inline]
    fn take_spare_page(&mut self) -> BumpWriter<N> {
        self.spare.pop().unwrap_or_default()
    }

    #[inline]
    fn recycle_full_pages(&mut self) {
        while let Some(mut page) = self.full.pop() {
            page.clear();
            self.spare.push(page);
        }
    }

    /// Rotate: move current page to `full` and reuse a cleared spare page if available.
    #[cold]
    #[inline(never)]
    fn rotate(&mut self) {
        let next_page = self.take_spare_page();
        let full_page = std::mem::replace(&mut self.current, next_page);
        self.full.push(full_page);
    }

    #[cold]
    #[inline(never)]
    fn extend_slow(&mut self, bytes: &[u8]) {
        self.rotate();
        if bytes.len() <= N {
            // SAFETY: we just rotated to a fresh page with capacity N,
            // and bytes.len() <= N.
            #[allow(unsafe_code)]
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.current.ptr(), bytes.len());
            }
            self.current.len = bytes.len();
        } else {
            // Data larger than a page — split across pages, copying each chunk
            // directly into a fresh page. No intermediate allocation.
            for chunk in bytes.chunks(N) {
                if self.current.len + chunk.len() > N {
                    self.rotate();
                }
                #[allow(unsafe_code)]
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        chunk.as_ptr(),
                        self.current.ptr().add(self.current.len),
                        chunk.len(),
                    );
                }
                self.current.len += chunk.len();
            }
        }
    }

    #[cold]
    #[inline(never)]
    fn push_slow(&mut self, byte: u8) {
        self.rotate();
        // Checked on purpose: a zero-sized page panics here. For any real page
        // size the bound is a constant and the check compiles away.
        self.current.buf[0].write(byte);
        self.current.len = 1;
    }

    /// Render a component into this growable buffer.
    ///
    /// Clears the buffer and renders the component. ChainedBumpWriter
    /// chains pages on overflow, so the render always succeeds.
    pub fn render_into<R>(&mut self, root: R)
    where
        R: crate::render::WebRenderSync<::proa_core::NoLoader, ChainedBumpWriter<N>>,
    {
        let mut reusable = crate::reusable_web_context(self);
        // ChainedBumpWriter::extend never returns Err — it chains pages.
        crate::head_dispatch::sync(root, reusable.context()).unwrap();
    }

    /// Consume the buffer into contiguous bytes.
    ///
    /// The common single-page case transfers the page allocation directly
    /// into the returned `Vec` without copying. Multi-page output is flattened
    /// once at this ownership boundary.
    #[allow(unsafe_code)]
    pub fn into_vec(self) -> Vec<u8> {
        if self.full.is_empty() {
            let len = self.current.len;
            let ptr = Box::into_raw(self.current.buf) as *mut u8;
            // SAFETY: a page is one allocation of `N` bytes with byte
            // alignment, which is exactly the layout of a `Vec<u8>` with
            // capacity `N`. The first `len` bytes are initialized and
            // `len <= N`; the uninitialized tail becomes spare capacity.
            unsafe { Vec::from_raw_parts(ptr, len, N) }
        } else {
            self.to_vec()
        }
    }

    /// Convert the buffer contents to a `String`, reusing the underlying
    /// allocation when all data fits in a single page (the common case).
    ///
    /// When data spans multiple pages, the pages are copied once into a
    /// contiguous `String`. Nothing is scanned: text writes are UTF-8 by type,
    /// raw byte writes were validated when they were written, and rewinds only
    /// land on character boundaries.
    #[allow(unsafe_code)]
    pub fn into_string(self) -> String {
        let bytes = self.into_vec();
        debug_assert!(
            core::str::from_utf8(&bytes).is_ok(),
            "ChainedBumpWriter held non-UTF-8 bytes; a writer broke the UTF-8 contract"
        );
        // SAFETY: every write path keeps the buffer valid UTF-8 (see above).
        unsafe { String::from_utf8_unchecked(bytes) }
    }
}

impl<const N: usize> Default for ChainedBumpWriter<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(unsafe_code)]
impl<const N: usize> WriteBuf for ChainedBumpWriter<N> {
    fn __head_supports_pages() -> bool {
        true
    }
    fn __head_take_pages(&mut self) -> Result<Vec<Bytes>, WriteError> {
        Ok(std::mem::take(self).into_pages().collect())
    }

    #[inline(always)]
    fn capacity(&self) -> usize {
        usize::MAX
    }

    #[inline(always)]
    fn len(&self) -> usize {
        let mut total = self.current.len;
        for page in &self.full {
            total += page.len;
        }
        total
    }

    #[inline(always)]
    fn reserve(&mut self, _additional: usize) -> Result<(), WriteError> {
        Ok(())
    }

    #[inline(always)]
    #[allow(unsafe_code)]
    fn extend_str(&mut self, text: &str) -> Result<(), WriteError> {
        let bytes = text.as_bytes();
        let new_len = self.current.len + bytes.len();
        if new_len <= N {
            // SAFETY: the bounds check above keeps the destination inside the
            // page, and `text` cannot alias a page this writer borrows mutably.
            // Short run-time lengths are copied inline; see `copy_short`.
            unsafe {
                copy_short(
                    bytes.as_ptr(),
                    self.current.ptr().add(self.current.len),
                    bytes.len(),
                );
            }
            self.current.len = new_len;
            return Ok(());
        }
        self.extend_slow(bytes);
        Ok(())
    }

    #[inline(always)]
    #[allow(unsafe_code)]
    unsafe fn push_unchecked(&mut self, byte: u8) -> Result<(), WriteError> {
        if self.current.len < N {
            // SAFETY: bounds check above guarantees self.current.len < N.
            unsafe {
                *self.current.ptr().add(self.current.len) = byte;
            }
            self.current.len += 1;
            return Ok(());
        }
        self.push_slow(byte);
        Ok(())
    }

    #[inline(always)]
    unsafe fn extend_with<F>(&mut self, max_len: usize, f: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        let new_end = self.current.len + max_len;
        if new_end <= N {
            // SAFETY: the bounds check above keeps the window inside the page.
            let written = f(unsafe { self.current.spare_window(max_len) });
            assert!(written <= max_len, "in-place writer overran its window");
            self.current.len += written;
            return Ok(());
        }
        // Slow path: fall back to stack buffer + extend
        let mut tmp = [0u8; 40];
        let len = f(&mut tmp[..max_len]);
        // SAFETY: the caller guarantees the written bytes are valid UTF-8.
        self.extend_str(unsafe { core::str::from_utf8_unchecked(&tmp[..len]) })
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        if self.full.is_empty() {
            self.current.as_slice()
        } else {
            &[]
        }
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len());
        for page in &self.full {
            out.extend_from_slice(page.as_slice());
        }
        out.extend_from_slice(self.current.as_slice());
        out
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.recycle_full_pages();
        self.current.clear();
    }
}

impl<const N: usize> CheckpointWriteBuf for ChainedBumpWriter<N> {
    type Checkpoint = ChainedBumpCheckpoint;

    #[inline(always)]
    fn checkpoint(&self) -> Self::Checkpoint {
        ChainedBumpCheckpoint {
            full_len: self.full.len(),
            current_len: self.current.len,
        }
    }

    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
        if checkpoint.full_len > self.full.len() {
            return false;
        }

        if checkpoint.full_len == self.full.len() {
            if checkpoint.current_len > self.current.len {
                return false;
            }
            if !cut_is_char_boundary(self.current.as_slice(), checkpoint.current_len, None) {
                return false;
            }
            self.current.len = checkpoint.current_len;
            return true;
        }

        let target = &self.full[checkpoint.full_len];
        if checkpoint.current_len > target.len {
            return false;
        }
        // A write larger than one page is split across pages, so the page
        // after the target can begin with a continuation byte.
        let following = self
            .full
            .get(checkpoint.full_len + 1)
            .unwrap_or(&self.current)
            .as_slice()
            .first()
            .copied();
        if !cut_is_char_boundary(target.as_slice(), checkpoint.current_len, following) {
            return false;
        }
        let mut restored_current = self.full.remove(checkpoint.full_len);
        restored_current.len = checkpoint.current_len;

        while self.full.len() > checkpoint.full_len {
            let mut page = self.full.pop().expect("full len checked above");
            page.clear();
            self.spare.push(page);
        }

        let mut old_current = std::mem::replace(&mut self.current, restored_current);
        old_current.clear();
        self.spare.push(old_current);
        true
    }
}

// ============================================================================
// SizeHint - adaptive pre-allocation via exponential moving average
// ============================================================================

/// Adaptive size hint that learns the output size across renders.
///
/// Tracks the rendered byte count using an exponential moving average and
/// returns a slightly over-sized estimate on the next call. When paired with
/// [`ChainedBumpWriter`], this lets you start with a single page large enough
/// to hold the entire output — eliminating page-chain transitions after the
/// first render.
///
/// The hint is stored in an [`AtomicUsize`](core::sync::atomic::AtomicUsize) so
/// it is `Send + Sync` and can
/// live in a `static`.
///
/// # Example
///
/// ```rust
/// use proa_core::shared::write_buf::{SizeHint, WriteBuf};
///
/// static HINT: SizeHint = SizeHint::new();
///
/// let mut buf: Vec<u8> = Vec::with_capacity(HINT.get());
/// // ... render into buf ...
/// HINT.update(buf.len());
/// ```
pub struct SizeHint {
    value: core::sync::atomic::AtomicUsize,
}

impl SizeHint {
    /// Create a new size hint with no prior data.
    pub const fn new() -> Self {
        Self {
            value: core::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Return the recommended buffer capacity.
    ///
    /// Adds 12.5% headroom plus a small constant to the moving average,
    /// ensuring that normal render-size jitter doesn't trigger a page
    /// transition.
    #[inline]
    pub fn get(&self) -> usize {
        let v = self.value.load(core::sync::atomic::Ordering::Acquire);
        v + v / 8 + 128
    }

    /// Feed back the actual rendered size.
    ///
    /// Uses an exponential moving average (weight = 1/4) so the hint
    /// converges quickly without over-reacting to one-off outliers.
    #[inline]
    pub fn update(&self, actual: usize) {
        let old = self.value.load(core::sync::atomic::Ordering::Acquire);
        let next = if old == 0 {
            actual
        } else {
            // EMA: 75% old + 25% new
            old - old / 4 + actual / 4
        };
        self.value
            .store(next, core::sync::atomic::Ordering::Release);
    }
}

impl Default for SizeHint {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SmallBuf - inline stack buffer with transparent heap spill
// ============================================================================

/// Small-buffer-optimized write buffer.
///
/// Stores the first `N` bytes inline (on the stack). If the output exceeds `N`,
/// the contents are moved to a heap-allocated `Vec<u8>` and all subsequent
/// writes go to the vec. This gives stack-buffer speed for typical pages while
/// gracefully handling outliers without failure.
///
/// Analogous to `SmallVec` / `compact_str` but specialised for `WriteBuf`.
///
/// # Choosing N
///
/// - `4096`  (4 KB) — small components, fragments, email blocks
/// - `8192`  (8 KB) — typical pages
/// - `16384` (16 KB) — larger pages with lists
/// - `32768` (32 KB) — recommended default; above this, spill cost is negligible
///
/// # Example
///
/// ```rust
/// use proa_core::shared::write_buf::{SmallBuf, WriteBuf};
///
/// let mut buf = SmallBuf::<4096>::new();
/// buf.extend_str("<div>Hello</div>").unwrap();
/// assert_eq!(buf.as_slice(), b"<div>Hello</div>");
/// assert!(!buf.is_spilled()); // still inline
/// ```
pub struct SmallBuf<const N: usize> {
    inline: [u8; N],
    len: usize,
    spill: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmallBufCheckpoint {
    spilled: bool,
    len: usize,
}

impl<const N: usize> SmallBuf<N> {
    /// Create a new `SmallBuf` with `N` bytes of inline capacity.
    #[inline]
    pub fn new() -> Self {
        Self {
            inline: [0u8; N],
            len: 0,
            spill: None,
        }
    }

    /// Returns `true` if the buffer has spilled to the heap.
    #[inline]
    pub fn is_spilled(&self) -> bool {
        self.spill.is_some()
    }

    /// Move inline data to heap. Called once when we first exceed N.
    #[cold]
    #[inline(never)]
    fn spill(&mut self, extra: usize) {
        let needed = self.len + extra;
        // Grow to at least 2x current or needed, whichever is larger
        let cap = needed.max(N * 2);
        let mut v = Vec::with_capacity(cap);
        v.extend_from_slice(&self.inline[..self.len]);
        self.spill = Some(v);
    }
}

impl<const N: usize> Default for SmallBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(unsafe_code)]
impl<const N: usize> WriteBuf for SmallBuf<N> {
    #[inline(always)]
    fn capacity(&self) -> usize {
        match &self.spill {
            Some(v) => v.capacity(),
            None => N,
        }
    }

    #[inline(always)]
    fn len(&self) -> usize {
        match &self.spill {
            Some(v) => v.len(),
            None => self.len,
        }
    }

    #[inline(always)]
    fn reserve(&mut self, additional: usize) -> Result<(), WriteError> {
        if let Some(v) = &mut self.spill {
            v.reserve(additional);
        } else if self.len + additional > N {
            self.spill(additional);
        }
        Ok(())
    }

    #[inline(always)]
    fn extend_str(&mut self, text: &str) -> Result<(), WriteError> {
        let bytes = text.as_bytes();
        if let Some(v) = &mut self.spill {
            v.extend_from_slice(bytes);
            return Ok(());
        }
        let new_len = self.len + bytes.len();
        if new_len <= N {
            self.inline[self.len..new_len].copy_from_slice(bytes);
            self.len = new_len;
        } else {
            self.spill(bytes.len());
            // spill moved inline data to vec; now append the new bytes
            self.spill.as_mut().unwrap().extend_from_slice(bytes);
        }
        Ok(())
    }

    #[inline(always)]
    unsafe fn push_unchecked(&mut self, byte: u8) -> Result<(), WriteError> {
        if let Some(v) = &mut self.spill {
            v.push(byte);
            return Ok(());
        }
        if self.len < N {
            self.inline[self.len] = byte;
            self.len += 1;
        } else {
            self.spill(1);
            self.spill.as_mut().unwrap().push(byte);
        }
        Ok(())
    }

    #[inline(always)]
    unsafe fn extend_with<F>(&mut self, max_len: usize, f: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        if let Some(v) = &mut self.spill {
            // SAFETY: forwarded contract.
            return unsafe { v.extend_with(max_len, f) };
        }
        if self.len + max_len <= N {
            let len = f(&mut self.inline[self.len..self.len + max_len]);
            self.len += len;
            Ok(())
        } else {
            // Fall back to stack buffer + extend (will spill if needed)
            let mut tmp = [0u8; 40];
            let len = f(&mut tmp[..max_len]);
            // SAFETY: the caller guarantees the written bytes are valid UTF-8.
            self.extend_str(unsafe { core::str::from_utf8_unchecked(&tmp[..len]) })
        }
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        match &self.spill {
            Some(v) => v.as_slice(),
            None => &self.inline[..self.len],
        }
    }

    #[inline(always)]
    fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.spill = None;
        self.len = 0;
    }
}

impl<const N: usize> CheckpointWriteBuf for SmallBuf<N> {
    type Checkpoint = SmallBufCheckpoint;

    #[inline(always)]
    fn checkpoint(&self) -> Self::Checkpoint {
        SmallBufCheckpoint {
            spilled: self.spill.is_some(),
            len: self.len(),
        }
    }

    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
        match (checkpoint.spilled, self.spill.as_mut()) {
            (false, None) => {
                if checkpoint.len > self.len {
                    return false;
                }
                self.len = checkpoint.len;
                true
            }
            (false, Some(_)) => {
                if checkpoint.len > N {
                    return false;
                }
                self.spill = None;
                self.len = checkpoint.len;
                true
            }
            (true, Some(spill)) => {
                if checkpoint.len > spill.len() {
                    return false;
                }
                spill.truncate(checkpoint.len);
                true
            }
            (true, None) => false,
        }
    }
}

// ============================================================================
// StagedBuf - stack-staged writes flushed in bulk to Vec
// ============================================================================

/// Write buffer that stages small writes in a stack-allocated buffer and
/// flushes them to a `Vec<u8>` in bulk when the stage fills up.
///
/// This exploits the fact that [`WriteCursor`]-style writes (bounds check +
/// `copy_from_slice` + bump) are cheaper per-call than `Vec::extend_from_slice`
/// (capacity check + potential realloc + copy + update len+capacity). By batching
/// many small writes into one large `Vec::extend_from_slice`, the per-call
/// overhead of Vec is amortised away.
///
/// # How it works
///
/// 1. Small `extend()` calls write into `inline: [u8; N]` (stack, like WriteCursor)
/// 2. When the stage is full, its contents are flushed to `out: Vec<u8>` as one bulk copy
/// 3. The stage is reset and the cycle repeats
///
/// # Choosing N
///
/// - `4096` (4 KB) — good default; fits in L1 cache
/// - `8192` (8 KB) — still fits in L1 on most CPUs
///
/// Larger values reduce flush frequency but may push the stage out of L1.
///
/// # Example
///
/// ```rust
/// use proa_core::shared::write_buf::{StagedBuf, WriteBuf};
///
/// let mut buf = StagedBuf::<4096>::new();
/// buf.extend_str("<div>Hello</div>").unwrap();
/// assert_eq!(buf.len(), 16);
/// let output = buf.finish();
/// assert_eq!(output, b"<div>Hello</div>");
/// ```
pub struct StagedBuf<const N: usize> {
    inline: [u8; N],
    pos: usize,
    out: Vec<u8>,
}

impl<const N: usize> StagedBuf<N> {
    /// Create a new `StagedBuf` with no pre-allocated output capacity.
    #[inline]
    pub fn new() -> Self {
        Self {
            inline: [0u8; N],
            pos: 0,
            out: Vec::new(),
        }
    }

    /// Create a new `StagedBuf` with pre-allocated output capacity.
    #[inline]
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            inline: [0u8; N],
            pos: 0,
            out: Vec::with_capacity(cap),
        }
    }

    /// Flush the stage buffer into the output Vec.
    #[inline(always)]
    fn flush(&mut self) {
        if self.pos > 0 {
            self.out.extend_from_slice(&self.inline[..self.pos]);
            self.pos = 0;
        }
    }

    /// Flush any remaining staged data and return the final output.
    #[inline]
    pub fn finish(&mut self) -> &[u8] {
        self.flush();
        &self.out
    }
}

impl<const N: usize> Default for StagedBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(unsafe_code)]
impl<const N: usize> WriteBuf for StagedBuf<N> {
    #[inline(always)]
    fn capacity(&self) -> usize {
        usize::MAX
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.out.len() + self.pos
    }

    #[inline(always)]
    fn reserve(&mut self, additional: usize) -> Result<(), WriteError> {
        // Reserve on the output vec, accounting for staged data
        let total_needed = self.pos + additional;
        if total_needed > N {
            self.flush();
            self.out.reserve(additional);
        }
        Ok(())
    }

    #[inline(always)]
    fn extend_str(&mut self, text: &str) -> Result<(), WriteError> {
        let bytes = text.as_bytes();
        let new_pos = self.pos + bytes.len();
        if new_pos <= N {
            // Fast path: fits in stage
            self.inline[self.pos..new_pos].copy_from_slice(bytes);
            self.pos = new_pos;
        } else {
            // Flush stage, then either stage or write directly
            self.flush();
            if bytes.len() <= N {
                self.inline[..bytes.len()].copy_from_slice(bytes);
                self.pos = bytes.len();
            } else {
                // Larger than stage — write directly to Vec
                self.out.extend_from_slice(bytes);
            }
        }
        Ok(())
    }

    #[inline(always)]
    unsafe fn push_unchecked(&mut self, byte: u8) -> Result<(), WriteError> {
        if self.pos >= N {
            self.flush();
        }
        self.inline[self.pos] = byte;
        self.pos += 1;
        Ok(())
    }

    #[inline(always)]
    unsafe fn extend_with<F>(&mut self, max_len: usize, f: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        if self.pos + max_len <= N {
            let len = f(&mut self.inline[self.pos..self.pos + max_len]);
            self.pos += len;
            Ok(())
        } else {
            let mut tmp = [0u8; 40];
            let len = f(&mut tmp[..max_len]);
            // SAFETY: the caller guarantees the written bytes are valid UTF-8.
            self.extend_str(unsafe { core::str::from_utf8_unchecked(&tmp[..len]) })
        }
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        // Only valid if stage is empty (after finish/flush/clear)
        if self.pos == 0 {
            &self.out
        } else {
            &[]
        }
    }

    #[inline(always)]
    fn to_vec(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.out.len() + self.pos);
        v.extend_from_slice(&self.out);
        v.extend_from_slice(&self.inline[..self.pos]);
        v
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.pos = 0;
        self.out.clear();
    }
}

// ============================================================================
// ZeroCopyBuf - scatter-gather buffer with zero-copy static slices
// ============================================================================

/// A part in a [`ZeroCopyBuf`] — either a zero-copy static reference or an
/// offset into the shared owned buffer.
pub enum Part {
    /// Zero-copy reference to a `&'static [u8]` (e.g. HTML tag bytes from macro codegen).
    Static(&'static [u8]),
    /// Range into the [`ZeroCopyBuf::owned`] buffer (dynamic content like escaped text).
    Owned { start: u32, len: u32 },
}

/// Scatter-gather write buffer that avoids copying `&'static [u8]` slices.
///
/// Static byte slices (from [`WriteBuf::extend_static`]) are stored as direct
/// pointer references ([`Part::Static`]), while dynamic content (from
/// [`WriteBuf::extend`]) is appended to a single contiguous `owned` buffer and
/// tracked via [`Part::Owned`] offsets.
///
/// When combined with the `html!` macro (which consolidates static HTML into
/// `static BYTES: &[u8]` declarations and emits `extend_static` calls), roughly
/// 78% of output bytes become zero-copy pointer stores — only dynamic attribute
/// values and escaped text require copying.
pub struct ZeroCopyBuf {
    /// Ordered list of parts that make up the output.
    pub parts: Vec<Part>,
    /// Shared buffer for all dynamically-written (non-static) content.
    pub owned: Vec<u8>,
    total_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroCopyCheckpoint {
    parts_len: usize,
    owned_len: usize,
    total_len: usize,
    last_owned_len: Option<u32>,
}

const STREAMING_INLINE_STATIC_THRESHOLD: usize = 1024;

impl ZeroCopyBuf {
    /// Create a new `ZeroCopyBuf` with pre-allocated capacity.
    ///
    /// - `est_parts`: estimated number of `Part` entries
    /// - `est_owned`: estimated bytes of dynamic (non-static) content
    pub fn with_capacity(est_parts: usize, est_owned: usize) -> Self {
        Self {
            parts: Vec::with_capacity(est_parts),
            owned: Vec::with_capacity(est_owned),
            total_len: 0,
        }
    }

    /// Number of parts collected so far.
    #[inline]
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// Flatten all parts into an existing buffer, clearing it first.
    ///
    /// This avoids allocation when the destination buffer is reused across
    /// requests (e.g. a per-connection response buffer).
    #[inline]
    pub fn flatten_into(&self, dst: &mut Vec<u8>) {
        dst.clear();
        dst.reserve(self.total_len);
        for part in &self.parts {
            match part {
                Part::Static(s) => dst.extend_from_slice(s),
                Part::Owned { start, len } => {
                    let s = *start as usize;
                    let l = *len as usize;
                    dst.extend_from_slice(&self.owned[s..s + l]);
                }
            }
        }
    }
}

#[allow(unsafe_code)]
impl WriteBuf for ZeroCopyBuf {
    #[inline(always)]
    fn capacity(&self) -> usize {
        usize::MAX
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.total_len
    }

    fn reserve(&mut self, _additional: usize) -> Result<(), WriteError> {
        Ok(())
    }

    #[inline(always)]
    fn extend_str(&mut self, text: &str) -> Result<(), WriteError> {
        let bytes = text.as_bytes();
        let start = self.owned.len() as u32;
        self.owned.extend_from_slice(bytes);
        // Coalesce with previous Owned part (they're always contiguous in the owned buffer)
        if let Some(Part::Owned { len: prev_len, .. }) = self.parts.last_mut() {
            *prev_len += bytes.len() as u32;
        } else {
            self.parts.push(Part::Owned {
                start,
                len: bytes.len() as u32,
            });
        }
        self.total_len += bytes.len();
        Ok(())
    }

    #[inline(always)]
    fn extend_static_str(&mut self, text: &'static str) -> Result<(), WriteError> {
        let bytes = text.as_bytes();
        // Inline small static slices into the owned buffer rather than
        // creating separate zero-copy parts. This collapses tiny fragments
        // (quotes, closing tags, attribute prefixes) into adjacent Owned
        // runs, dramatically reducing part count.
        if bytes.len() <= 256 {
            return self.extend_str(text);
        }
        self.parts.push(Part::Static(bytes));
        self.total_len += bytes.len();
        Ok(())
    }

    fn as_slice(&self) -> &[u8] {
        &[]
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.total_len);
        for part in &self.parts {
            match part {
                Part::Static(s) => out.extend_from_slice(s),
                Part::Owned { start, len } => {
                    let s = *start as usize;
                    let l = *len as usize;
                    out.extend_from_slice(&self.owned[s..s + l]);
                }
            }
        }
        out
    }

    fn clear(&mut self) {
        self.parts.clear();
        self.owned.clear();
        self.total_len = 0;
    }
}

impl CheckpointWriteBuf for ZeroCopyBuf {
    type Checkpoint = ZeroCopyCheckpoint;

    fn checkpoint(&self) -> Self::Checkpoint {
        let last_owned_len = match self.parts.last() {
            Some(Part::Owned { len, .. }) => Some(*len),
            _ => None,
        };
        ZeroCopyCheckpoint {
            parts_len: self.parts.len(),
            owned_len: self.owned.len(),
            total_len: self.total_len,
            last_owned_len,
        }
    }

    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
        if checkpoint.parts_len > self.parts.len()
            || checkpoint.owned_len > self.owned.len()
            || checkpoint.total_len > self.total_len
        {
            return false;
        }

        self.parts.truncate(checkpoint.parts_len);
        if let Some(expected_len) = checkpoint.last_owned_len {
            match self.parts.last_mut() {
                Some(Part::Owned { len, .. }) => *len = expected_len,
                _ => return false,
            }
        }
        self.owned.truncate(checkpoint.owned_len);
        self.total_len = checkpoint.total_len;
        true
    }
}

// ============================================================================
// StreamingZeroCopyBuf - checkpointable zero-copy builder for streaming SSR
// ============================================================================

/// A frozen streaming checkpoint captured from [`StreamingZeroCopyBuf`].
///
/// Each part is transport-ready and self-contained:
/// - [`StreamingChunkPart::Static`] references `'static` bytes directly
/// - [`StreamingChunkPart::Owned`] owns immutable `Bytes`
#[derive(Debug)]
pub struct StreamingChunk {
    parts: Vec<StreamingChunkPart>,
    total_len: usize,
}

impl StreamingChunk {
    /// Total bytes across all parts.
    #[inline]
    pub fn len(&self) -> usize {
        self.total_len
    }

    /// Returns `true` if the chunk contains no bytes.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// Number of parts in this chunk.
    #[inline]
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// Borrowed transport view for vectored I/O paths.
    #[inline]
    pub fn slices(&self) -> impl ExactSizeIterator<Item = &[u8]> + '_ {
        self.parts.iter().map(|part| match part {
            StreamingChunkPart::Static(bytes) => *bytes,
            StreamingChunkPart::Owned(bytes) => bytes.as_ref(),
        })
    }

    /// Owned frame view for transports that want `Bytes`.
    #[inline]
    pub fn into_frames(self) -> impl ExactSizeIterator<Item = Bytes> {
        self.parts.into_iter().map(|part| match part {
            StreamingChunkPart::Static(bytes) => Bytes::from_static(bytes),
            StreamingChunkPart::Owned(bytes) => bytes,
        })
    }

    /// Flatten all parts into `dst`, clearing it first.
    #[inline]
    pub fn flatten_into(&self, dst: &mut Vec<u8>) {
        dst.clear();
        dst.reserve(self.total_len);
        for part in &self.parts {
            match part {
                StreamingChunkPart::Static(bytes) => dst.extend_from_slice(bytes),
                StreamingChunkPart::Owned(bytes) => dst.extend_from_slice(bytes),
            }
        }
    }

    /// Materialize this chunk into a contiguous `Vec<u8>`.
    #[inline]
    pub fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.total_len);
        self.flatten_into(&mut out);
        out
    }
}

/// A detached checkpoint token returned by [`StreamingZeroCopyBuf::mark`].
///
/// Marks are absolute within the lifetime of a builder, so multiple marks can
/// be captured before earlier prefixes are flushed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlushMark {
    part_index: usize,
    total_len: usize,
}

impl FlushMark {
    /// Absolute part index captured by this mark.
    #[inline]
    pub fn part_index(self) -> usize {
        self.part_index
    }

    /// Absolute byte count captured by this mark.
    #[inline]
    pub fn total_len(self) -> usize {
        self.total_len
    }
}

/// A transport-ready part inside a [`StreamingChunk`].
#[derive(Debug)]
pub enum StreamingChunkPart {
    /// Zero-copy reference to `'static` bytes.
    Static(&'static [u8]),
    /// Immutable owned bytes captured at a checkpoint.
    Owned(Bytes),
}

/// Streaming-oriented zero-copy buffer with explicit checkpointing.
///
/// Unlike [`ZeroCopyBuf`], this builder is designed for incremental prefix
/// flushes. The current mutable segment is sealed only at explicit checkpoint
/// boundaries, so flushes always happen **between parts**, never mid-part.
pub struct StreamingZeroCopyBuf {
    parts: Vec<StreamingChunkPart>,
    current: BytesMut,
    total_len: usize,
    part_offset: usize,
    byte_offset: usize,
    current_capacity_hint: usize,
}

impl StreamingZeroCopyBuf {
    /// Create a new streaming buffer with conservative defaults.
    #[inline]
    pub fn new() -> Self {
        Self::with_capacity(32, 4096)
    }

    /// Create a new streaming buffer with pre-allocated part metadata and
    /// mutable segment capacity.
    #[inline]
    pub fn with_capacity(est_parts: usize, current_segment_capacity: usize) -> Self {
        Self {
            parts: Vec::with_capacity(est_parts),
            current: BytesMut::with_capacity(current_segment_capacity),
            total_len: 0,
            part_offset: 0,
            byte_offset: 0,
            current_capacity_hint: current_segment_capacity,
        }
    }

    #[inline]
    fn ensure_current_capacity(&mut self, additional: usize) {
        if self.current.capacity() == 0 {
            self.current
                .reserve(self.current_capacity_hint.max(additional.max(1)));
        } else {
            self.current.reserve(additional);
        }
    }

    #[inline]
    fn seal_current(&mut self) {
        if self.current.is_empty() {
            return;
        }
        let bytes = std::mem::replace(
            &mut self.current,
            BytesMut::with_capacity(self.current_capacity_hint),
        )
        .freeze();
        self.parts.push(StreamingChunkPart::Owned(bytes));
    }

    #[inline]
    fn drain_prefix(&mut self, drain_count: usize, drained_len: usize) -> Option<StreamingChunk> {
        if drain_count == 0 {
            return None;
        }
        let parts = if drain_count == self.parts.len() {
            std::mem::take(&mut self.parts)
        } else {
            self.parts.drain(..drain_count).collect::<Vec<_>>()
        };
        self.part_offset += drain_count;
        self.byte_offset += drained_len;
        self.total_len -= drained_len;
        Some(StreamingChunk {
            parts,
            total_len: drained_len,
        })
    }

    /// Seal the current mutable segment and return a checkpoint token.
    ///
    /// The returned mark always lands between parts, never inside one.
    #[inline]
    pub fn mark(&mut self) -> FlushMark {
        self.seal_current();
        FlushMark {
            part_index: self.part_offset + self.parts.len(),
            total_len: self.byte_offset + self.total_len,
        }
    }

    /// Flush all bytes up to `mark`, keeping the suffix live in the builder.
    ///
    /// Returns `None` when `mark` points at the current head of the buffer or
    /// when the mark no longer belongs to the builder's live suffix.
    pub fn flush_through(&mut self, mark: FlushMark) -> Option<StreamingChunk> {
        if mark.part_index < self.part_offset {
            return None;
        }
        let drain_count = mark.part_index.saturating_sub(self.part_offset);
        if drain_count > self.parts.len() || mark.total_len < self.byte_offset {
            return None;
        }
        let drained_len = mark.total_len - self.byte_offset;
        self.drain_prefix(drain_count, drained_len)
    }

    /// Flush all currently written bytes, keeping the builder reusable.
    #[inline]
    pub fn flush_all(&mut self) -> Option<StreamingChunk> {
        self.seal_current();
        let drained_len = self.total_len;
        let drain_count = self.parts.len();
        self.drain_prefix(drain_count, drained_len)
    }

    /// Consume the builder and return the remaining bytes as one final chunk.
    #[inline]
    pub fn freeze(mut self) -> StreamingChunk {
        self.seal_current();
        StreamingChunk {
            parts: self.parts,
            total_len: self.total_len,
        }
    }
}

impl Default for StreamingZeroCopyBuf {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[allow(unsafe_code)]
impl WriteBuf for StreamingZeroCopyBuf {
    #[inline(always)]
    fn capacity(&self) -> usize {
        usize::MAX
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.total_len
    }

    #[inline(always)]
    fn reserve(&mut self, additional: usize) -> Result<(), WriteError> {
        self.ensure_current_capacity(additional);
        Ok(())
    }

    #[inline(always)]
    fn extend_str(&mut self, text: &str) -> Result<(), WriteError> {
        let bytes = text.as_bytes();
        self.ensure_current_capacity(bytes.len());
        self.current.extend_from_slice(bytes);
        self.total_len += bytes.len();
        Ok(())
    }

    #[inline(always)]
    fn extend_static_str(&mut self, text: &'static str) -> Result<(), WriteError> {
        let bytes = text.as_bytes();
        // Keep tiny static fragments coalesced with adjacent dynamic bytes to
        // avoid exploding the part count on normal templates.
        if bytes.len() <= STREAMING_INLINE_STATIC_THRESHOLD {
            return self.extend_str(text);
        }
        self.seal_current();
        self.parts.push(StreamingChunkPart::Static(bytes));
        self.total_len += bytes.len();
        Ok(())
    }

    #[inline(always)]
    unsafe fn push_unchecked(&mut self, byte: u8) -> Result<(), WriteError> {
        self.ensure_current_capacity(1);
        self.current.put_u8(byte);
        self.total_len += 1;
        Ok(())
    }

    #[inline(always)]
    unsafe fn extend_with<F>(&mut self, max_len: usize, f: F) -> Result<(), WriteError>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        self.ensure_current_capacity(max_len);
        let start = self.current.len();
        self.current.resize(start + max_len, 0);
        let written = f(&mut self.current[start..start + max_len]);
        self.current.truncate(start + written);
        self.total_len += written;
        Ok(())
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        if self.parts.is_empty() {
            self.current.as_ref()
        } else {
            &[]
        }
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.total_len);
        for part in &self.parts {
            match part {
                StreamingChunkPart::Static(bytes) => out.extend_from_slice(bytes),
                StreamingChunkPart::Owned(bytes) => out.extend_from_slice(bytes),
            }
        }
        out.extend_from_slice(self.current.as_ref());
        out
    }

    fn clear(&mut self) {
        self.parts.clear();
        self.current.clear();
        self.total_len = 0;
        self.part_offset = 0;
        self.byte_offset = 0;
    }
}

// ============================================================================
// render_growable - Page-speed rendering with automatic growth
// ============================================================================

/// Render a component into a buffer that automatically grows on overflow.
///
/// Starts with `initial_capacity` bytes and doubles until the render fits.
/// Returns the rendered output as a `Vec<u8>`.
///
/// # Example
///
/// ```rust
/// use proa_core::shared::write_buf::render_growable;
///
/// # use proa_core::{WebRenderSync, WebContext, WriteBuf, WriteError, text};
/// # #[derive(Clone)]
/// # struct MyPage;
/// # impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for MyPage {
/// #     fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
/// #         text("hello").render(cx)
/// #     }
/// # }
/// let buf = render_growable(MyPage, 4096);
/// assert_eq!(&buf, b"hello");
/// ```
///
/// # Panics
///
/// Panics if the buffer exceeds 1 GiB.
pub fn render_growable<R>(root: R, initial_capacity: usize) -> Vec<u8>
where
    R: crate::render::WebRenderSync<::proa_core::NoLoader, Vec<u8>> + Clone,
{
    // proa-perf-ok: this root helper intentionally owns the returned Vec
    let buf = Vec::with_capacity(initial_capacity.max(4096));
    let mut cx = crate::web::WebContext::with_buffer(buf);
    // Vec<u8> never returns Err — it grows as needed.
    root.render(&mut cx).unwrap();
    cx.into_output()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_basic_operations() {
        let mut buf: Vec<u8> = Vec::new();
        WriteBuf::extend_str(&mut buf, "hello").unwrap();
        WriteBuf::push(&mut buf, b' ').unwrap();
        WriteBuf::extend_str(&mut buf, "world").unwrap();
        assert_eq!(WriteBuf::as_slice(&buf), b"hello world");
        assert_eq!(WriteBuf::len(&buf), 11);
    }

    #[test]
    fn extend_shared_writes_runtime_owned_bytes() {
        let run = Bytes::from(String::from("<p>shared run</p>"));
        let mut buf: Vec<u8> = Vec::new();
        WriteBuf::extend_shared(&mut buf, &run).unwrap();
        WriteBuf::extend_shared(&mut buf, &run.slice(3..9)).unwrap();
        assert_eq!(WriteBuf::as_slice(&buf), b"<p>shared run</p>shared");

        let mut chained = ChainedBumpWriter::<16>::new();
        chained.extend_shared(&run).unwrap();
        assert_eq!(chained.to_vec(), b"<p>shared run</p>");

        let mut backing = [0u8; 4];
        let mut cur = WriteCursor::new(&mut backing);
        assert!(matches!(
            cur.extend_shared(&run),
            Err(WriteError::OutOfCapacity)
        ));
    }

    #[test]
    fn render_error_displays_message() {
        let err = WriteError::render("op budget exhausted");
        assert_eq!(err.to_string(), "render failed: op budget exhausted");
        let owned = WriteError::render(format!("bad op {}", 7));
        assert_eq!(owned.to_string(), "render failed: bad op 7");
    }

    #[test]
    fn cursor_writes_within_capacity() {
        let mut backing = [0u8; 32];
        let mut cur = WriteCursor::new(&mut backing);
        cur.extend_str("<div>").unwrap();
        cur.extend_str("test").unwrap();
        cur.extend_str("</div>").unwrap();
        assert_eq!(cur.as_slice(), b"<div>test</div>");
        assert_eq!(cur.len(), 15);
        assert_eq!(cur.remaining(), 17);
    }

    #[test]
    fn cursor_overflow_errors() {
        let mut backing = [0u8; 4];
        let mut cur = WriteCursor::new(&mut backing);
        assert!(matches!(
            cur.extend_str("12345"),
            Err(WriteError::OutOfCapacity)
        ));
        // len should remain unchanged on error
        assert_eq!(cur.len(), 0);
    }

    #[test]
    fn cursor_push_single_byte() {
        let mut backing = [0u8; 4];
        let mut cur = WriteCursor::new(&mut backing);
        cur.push(b'a').unwrap();
        cur.push(b'b').unwrap();
        cur.push(b'c').unwrap();
        cur.push(b'd').unwrap();
        assert_eq!(cur.as_slice(), b"abcd");
        assert!(matches!(cur.push(b'e'), Err(WriteError::OutOfCapacity)));
    }

    #[test]
    fn cursor_clear_and_reuse() {
        let mut backing = [0u8; 16];
        let mut cur = WriteCursor::new(&mut backing);
        cur.extend_str("first").unwrap();
        assert_eq!(cur.len(), 5);
        cur.clear();
        assert_eq!(cur.len(), 0);
        cur.extend_str("second").unwrap();
        assert_eq!(cur.as_slice(), b"second");
    }

    #[test]
    fn cursor_reserve_check() {
        let mut backing = [0u8; 8];
        let mut cur = WriteCursor::new(&mut backing);
        assert!(cur.reserve(8).is_ok());
        assert!(cur.reserve(9).is_err());
        cur.extend_str("1234").unwrap();
        assert!(cur.reserve(4).is_ok());
        assert!(cur.reserve(5).is_err());
    }

    // ChainedBumpWriter tests

    #[test]
    fn chained_single_page() {
        let mut buf = ChainedBumpWriter::<64>::new();
        buf.extend_str("hello world").unwrap();
        assert_eq!(buf.len(), 11);
        assert_eq!(buf.page_count(), 1);
        assert_eq!(buf.to_vec(), b"hello world");
    }

    #[test]
    fn chained_into_vec_transfers_single_page_allocation() {
        let mut buf = ChainedBumpWriter::<64>::new();
        buf.extend_str("hello world").unwrap();
        let page_ptr = buf.current.buf.as_ptr().cast::<u8>();

        let bytes = buf.into_vec();

        assert_eq!(bytes, b"hello world");
        assert_eq!(bytes.as_ptr(), page_ptr);
        assert_eq!(bytes.capacity(), 64);
    }

    #[test]
    fn chained_into_vec_flattens_multiple_pages_once() {
        let mut buf = ChainedBumpWriter::<4>::new();
        buf.extend_str("123456789").unwrap();

        assert_eq!(buf.into_vec(), b"123456789");
    }

    #[test]
    fn chained_into_pages_transfers_allocations_and_written_lengths() {
        let mut buf = ChainedBumpWriter::<4>::new();
        buf.extend_str("123456789").unwrap();
        let pointers: Vec<_> = buf
            .slices()
            .filter(|page| !page.is_empty())
            .map(|page| page.as_ptr())
            .collect();
        let pages: Vec<_> = buf.into_pages().collect();
        assert_eq!(
            pages.iter().map(|page| page.as_ptr()).collect::<Vec<_>>(),
            pointers
        );
        assert_eq!(pages.iter().map(Bytes::len).collect::<Vec<_>>(), [4, 4, 1]);
        assert_eq!(pages.concat(), b"123456789");
    }

    #[test]
    fn chained_into_pages_omits_empty_and_recycled_pages() {
        let mut buf = ChainedBumpWriter::<4>::new();
        buf.extend_str("old pages to recycle").unwrap();
        buf.clear();
        buf.extend_str("new").unwrap();
        let pages: Vec<_> = buf.into_pages().collect();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0], b"new"[..]);
        assert_eq!(ChainedBumpWriter::<4>::new().into_pages().count(), 0);
    }

    #[test]
    fn chained_rotates_on_overflow() {
        let mut buf = ChainedBumpWriter::<8>::new();
        buf.extend_str("1234").unwrap();
        buf.extend_str("5678").unwrap(); // fills page 1
        assert_eq!(buf.page_count(), 1);
        buf.extend_str("9abc").unwrap(); // triggers rotation
        assert_eq!(buf.page_count(), 2);
        assert_eq!(buf.to_vec(), b"123456789abc");
    }

    #[test]
    fn chained_short_copies_match_a_plain_copy_at_every_length() {
        // 0..=130 crosses every width class and its edges (1, 2, 4, 8, 16, 32,
        // 64, 65). `black_box` keeps each length a run-time value so the
        // branches in `copy_short` run instead of folding away.
        let source: String = (0..130u8).map(|i| char::from(b'!' + i % 90)).collect();
        for len in 0..=source.len() {
            let mut buffer = RenderBuffer::new();
            buffer.extend_str("xyz").unwrap();
            buffer
                .extend_str(std::hint::black_box(&source[..len]))
                .unwrap();
            buffer.extend_str("|").unwrap();
            assert_eq!(buffer.into_string(), format!("xyz{}|", &source[..len]));
        }
    }

    #[test]
    #[should_panic]
    fn chained_zero_sized_page_panics_on_push() {
        let _ = ChainedBumpWriter::<0>::new().push(b'x');
    }

    #[test]
    fn chained_oversized_write() {
        let mut buf = ChainedBumpWriter::<4>::new();
        buf.extend_str("12345678").unwrap(); // larger than page size
        assert_eq!(buf.to_vec(), b"12345678");
    }

    #[test]
    fn chained_clear_reuse() {
        let mut buf = ChainedBumpWriter::<8>::new();
        buf.extend_str("1234567890").unwrap();
        assert!(buf.page_count() >= 2);
        buf.clear();
        assert_eq!(buf.len(), 0);
        buf.extend_str("abc").unwrap();
        assert_eq!(buf.to_vec(), b"abc");
    }

    #[test]
    fn chained_clear_recycles_pages() {
        let mut buf = ChainedBumpWriter::<8>::new();
        // Write 10 bytes — overflows first 8-byte page, rotates to second
        buf.extend_str("1234567890").unwrap();
        assert!(!buf.full.is_empty());
        assert_eq!(buf.spare.len(), 0);

        buf.clear();
        assert_eq!(buf.full.len(), 0);
        // All pages are the same size N, so all get recycled
        assert!(!buf.spare.is_empty());
    }

    #[test]
    fn chained_oversized_write_spans_pages() {
        let mut buf = ChainedBumpWriter::<4>::new();
        buf.extend_str("12345678").unwrap();
        // 8 bytes across 4-byte pages = at least 2 full pages
        assert!(!buf.full.is_empty());
        assert_eq!(buf.len(), 8);

        buf.clear();
        assert_eq!(buf.full.len(), 0);
    }

    #[test]
    fn chained_slices_iterator() {
        let mut buf = ChainedBumpWriter::<4>::new();
        buf.extend_str("aaaa").unwrap(); // fills page 1
        buf.extend_str("bb").unwrap(); // starts page 2
        let slices: Vec<&[u8]> = buf.slices().collect();
        assert_eq!(slices, vec![b"aaaa".as_slice(), b"bb".as_slice()]);
    }

    // StreamingZeroCopyBuf tests

    #[test]
    fn streaming_zero_copy_flush_through_preserves_suffix() {
        static LARGE: &[u8] = &[b'X'; 300];

        let mut buf = StreamingZeroCopyBuf::with_capacity(8, 16);
        buf.extend_str("hello").unwrap();
        let hello = buf.mark();

        buf.extend_static(LARGE).unwrap();
        buf.extend_str("world").unwrap();
        let tail = buf.mark();

        let first = buf.flush_through(hello).unwrap();
        assert_eq!(first.to_vec(), b"hello");
        assert_eq!(buf.to_vec(), [LARGE, b"world".as_slice()].concat());

        let second = buf.flush_through(tail).unwrap();
        assert_eq!(second.to_vec(), [LARGE, b"world".as_slice()].concat());
        assert!(buf.is_empty());
    }

    #[test]
    fn streaming_zero_copy_supports_multiple_absolute_marks() {
        static LARGE_A: &[u8] = &[b'A'; 300];
        static LARGE_B: &[u8] = &[b'B'; 320];

        let mut buf = StreamingZeroCopyBuf::with_capacity(8, 16);
        buf.extend_static(LARGE_A).unwrap();
        let m1 = buf.mark();

        buf.extend_static(LARGE_B).unwrap();
        let m2 = buf.mark();

        let c1 = buf.flush_through(m1).unwrap();
        assert_eq!(c1.to_vec(), LARGE_A);

        let c2 = buf.flush_through(m2).unwrap();
        assert_eq!(c2.to_vec(), LARGE_B);
        assert!(buf.is_empty());
    }

    #[test]
    fn streaming_zero_copy_flush_all_keeps_builder_reusable() {
        let mut buf = StreamingZeroCopyBuf::with_capacity(4, 8);
        buf.extend_str("abc").unwrap();

        let first = buf.flush_all().unwrap();
        assert_eq!(first.to_vec(), b"abc");
        assert!(buf.is_empty());

        buf.extend_str("def").unwrap();
        let second = buf.freeze();
        assert_eq!(second.to_vec(), b"def");
    }

    #[test]
    fn streaming_chunk_into_frames_keeps_static_zero_copy() {
        static LARGE: &[u8] = &[b'Z'; STREAMING_INLINE_STATIC_THRESHOLD + 1];

        let mut buf = StreamingZeroCopyBuf::with_capacity(4, 8);
        buf.extend_static(LARGE).unwrap();
        let chunk = buf.freeze();

        let frames: Vec<Bytes> = chunk.into_frames().collect();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].as_ref(), LARGE);
        assert_eq!(frames[0].as_ptr(), LARGE.as_ptr());
    }

    // SmallBuf tests

    #[test]
    fn smallbuf_stays_inline() {
        let mut buf = SmallBuf::<64>::new();
        buf.extend_str("hello world").unwrap();
        assert_eq!(buf.as_slice(), b"hello world");
        assert_eq!(buf.len(), 11);
        assert!(!buf.is_spilled());
    }

    #[test]
    fn smallbuf_spills_to_heap() {
        let mut buf = SmallBuf::<8>::new();
        buf.extend_str("1234").unwrap();
        assert!(!buf.is_spilled());
        buf.extend_str("56789ABC").unwrap(); // exceeds 8 bytes
        assert!(buf.is_spilled());
        assert_eq!(buf.as_slice(), b"123456789ABC");
        assert_eq!(buf.len(), 12);
    }

    #[test]
    fn smallbuf_push_spills() {
        let mut buf = SmallBuf::<4>::new();
        for b in b"abcd" {
            buf.push(*b).unwrap();
        }
        assert!(!buf.is_spilled());
        buf.push(b'e').unwrap();
        assert!(buf.is_spilled());
        assert_eq!(buf.as_slice(), b"abcde");
    }

    #[test]
    fn smallbuf_clear_reuse() {
        let mut buf = SmallBuf::<16>::new();
        buf.extend_str("first").unwrap();
        buf.clear();
        assert_eq!(buf.len(), 0);
        buf.extend_str("second").unwrap();
        assert_eq!(buf.as_slice(), b"second");
    }

    #[test]
    fn smallbuf_clear_after_spill_reuses_heap() {
        let mut buf = SmallBuf::<4>::new();
        buf.extend_str("12345678").unwrap(); // spill
        assert!(buf.is_spilled());
        buf.clear();
        assert_eq!(buf.len(), 0);
        // still spilled (vec is cleared, not dropped) — avoids re-spill
        buf.extend_str("abc").unwrap();
        assert_eq!(buf.as_slice(), b"abc");
    }

    #[test]
    fn smallbuf_exact_capacity_no_spill() {
        let mut buf = SmallBuf::<5>::new();
        buf.extend_str("exact").unwrap();
        assert!(!buf.is_spilled());
        assert_eq!(buf.as_slice(), b"exact");
    }

    #[test]
    fn smallbuf_to_vec() {
        let mut buf = SmallBuf::<32>::new();
        buf.extend_str("<div>test</div>").unwrap();
        assert_eq!(buf.to_vec(), b"<div>test</div>");
    }

    // StagedBuf tests

    #[test]
    fn stagedbuf_small_writes() {
        let mut buf = StagedBuf::<16>::new();
        buf.extend_str("hello").unwrap();
        buf.extend_str(" ").unwrap();
        buf.extend_str("world").unwrap();
        assert_eq!(buf.len(), 11);
        assert_eq!(buf.finish(), b"hello world");
    }

    #[test]
    fn stagedbuf_flush_on_overflow() {
        let mut buf = StagedBuf::<8>::new();
        buf.extend_str("1234").unwrap();
        buf.extend_str("5678").unwrap(); // fills stage exactly
        assert_eq!(buf.pos, 8);
        buf.extend_str("9").unwrap(); // triggers flush
        assert_eq!(buf.out, b"12345678");
        assert_eq!(buf.pos, 1);
        assert_eq!(buf.finish(), b"123456789");
    }

    #[test]
    fn stagedbuf_large_write_bypasses_stage() {
        let mut buf = StagedBuf::<4>::new();
        buf.extend_str("12345678").unwrap(); // larger than stage
        assert_eq!(buf.out, b"12345678");
        assert_eq!(buf.pos, 0);
    }

    #[test]
    fn stagedbuf_clear_reuse() {
        let mut buf = StagedBuf::<16>::new();
        buf.extend_str("first").unwrap();
        buf.clear();
        assert_eq!(buf.len(), 0);
        buf.extend_str("second").unwrap();
        assert_eq!(buf.finish(), b"second");
    }

    #[test]
    fn smallbuf_reserve_triggers_spill() {
        let mut buf = SmallBuf::<8>::new();
        buf.extend_str("1234").unwrap();
        buf.reserve(16).unwrap(); // 4 + 16 > 8, should spill
        assert!(buf.is_spilled());
        assert_eq!(buf.as_slice(), b"1234"); // data preserved
    }
}

#[cfg(test)]
mod utf8_contract_tests {
    use super::*;

    #[test]
    fn raw_byte_writes_are_validated_where_they_are_written() {
        let mut buf = RenderBuffer::new();
        assert_eq!(buf.extend(&[0xff]), Err(WriteError::InvalidUtf8));
        // A lone lead byte: half of a character split across two writes.
        assert_eq!(buf.extend_static(&[0xC3]), Err(WriteError::InvalidUtf8));
        assert_eq!(buf.push(0xC3), Err(WriteError::InvalidUtf8));
        assert_eq!(
            buf.extend_shared(&Bytes::from_static(&[0xE2, 0x82])),
            Err(WriteError::InvalidUtf8)
        );
        assert!(buf.is_empty(), "a rejected write must not commit bytes");

        buf.extend("caf\u{e9}".as_bytes()).unwrap();
        buf.push(b'!').unwrap();
        assert_eq!(buf.into_string(), "caf\u{e9}!");
    }

    #[test]
    fn a_stale_checkpoint_cannot_cut_a_character_in_half() {
        let mut buf = RenderBuffer::new();
        buf.extend_str("ab").unwrap();
        let start = buf.checkpoint();
        buf.extend_str("cd").unwrap();
        let stale = buf.checkpoint(); // byte offset 4
        assert!(buf.rewind_to(start));
        // Offset 4 is now inside the first two-byte character.
        buf.extend_str("x\u{e9}\u{e9}").unwrap();
        assert!(!buf.rewind_to(stale));
        assert_eq!(buf.into_string(), "abx\u{e9}\u{e9}");
    }

    #[test]
    fn a_stale_checkpoint_cannot_cut_a_character_split_across_pages() {
        let mut buf = ChainedBumpWriter::<4>::new();
        buf.extend_str("wxyz").unwrap();
        let keep = buf.checkpoint();
        buf.extend_str("abcd").unwrap();
        let stale = buf.checkpoint(); // end of the second page
        assert!(buf.rewind_to(keep));
        // Larger than a page, so it is split mid-character: the second page
        // ends on the lead byte of U+00E9 and the third starts with the rest.
        buf.extend_str("abc\u{e9}z").unwrap();
        assert_eq!(buf.page_count(), 3);
        assert!(!buf.rewind_to(stale));
        assert_eq!(buf.into_string(), "wxyzabc\u{e9}z");
    }

    #[test]
    #[allow(unsafe_code)]
    fn the_in_place_writer_sees_zeros_on_fresh_and_reused_pages() {
        // Fresh page: allocated but never zero-filled.
        let mut buffer = RenderBuffer::new();
        // SAFETY: the closure writes ASCII digits.
        unsafe {
            buffer.extend_with(8, |window| {
                assert_eq!(window, [0u8; 8]);
                window[..2].copy_from_slice(b"42");
                2
            })
        }
        .unwrap();
        buffer.extend_str(&"secret".repeat(100)).unwrap();

        // Reused page: still holds the bytes written above.
        buffer.clear();
        // SAFETY: the closure writes ASCII digits.
        unsafe {
            buffer.extend_with(8, |window| {
                assert_eq!(window, [0u8; 8]);
                window[..1].copy_from_slice(b"7");
                1
            })
        }
        .unwrap();
        assert_eq!(buffer.into_string(), "7");
    }
}
