// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Legacy page-pool-backed writer retained for low-level compatibility.
//!
//! Standard sync, async, Markdown, and streaming rendering now use the owned
//! reusable `RenderBuffer`. These types remain temporarily available to code
//! that explicitly selected the old low-level writer:
//!
//! - [`PagePool`] owns recyclable [`SinkPage`] allocations.
//! - [`SinkWriter`] is a `WriteBuf` that checks pages out of a pool.
//! - [`StreamChunkFlusher`] remains the transport-facing erased boundary.
//!
//! They are not used by framework, `Suspense`, `ErrorBoundary`, or standard
//! context paths.

#![allow(deprecated)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::render::{StreamChunkFlusher, StreamChunkView};
use crate::shared::write_buf::{CheckpointWriteBuf, WriteBuf, WriteError};

// ============================================================================
// SinkPage
// ============================================================================

/// A single fixed-capacity writable page allocated from a [`PagePool`].
#[deprecated(note = "legacy SinkWriter compatibility; use RenderBuffer")]
pub struct SinkPage<const N: usize> {
    buf: Box<[u8]>,
    len: usize,
}

impl<const N: usize> SinkPage<N> {
    /// Allocate a new zeroed page.
    pub fn new() -> Self {
        Self {
            buf: vec![0u8; N].into_boxed_slice(),
            len: 0,
        }
    }

    /// View of the bytes written into this page.
    #[inline]
    pub fn as_written(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Total page capacity.
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Bytes written so far.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// True if no bytes have been written.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Reset the write cursor to 0 without freeing the underlying buffer.
    #[inline]
    pub fn reset(&mut self) {
        self.len = 0;
    }

    /// Append bytes to the page. Returns `Err(OutOfCapacity)` if the bytes
    /// would not fit; the page is left unchanged in that case.
    #[inline]
    pub fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        let new_len = self.len + bytes.len();
        if new_len > N {
            return Err(WriteError::OutOfCapacity);
        }
        self.buf[self.len..new_len].copy_from_slice(bytes);
        self.len = new_len;
        Ok(())
    }

    /// Spare capacity remaining in this page.
    #[inline]
    pub const fn remaining(&self) -> usize {
        N - self.len
    }
}

impl<const N: usize> Default for SinkPage<N> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PagePool
// ============================================================================

/// Request-scoped recyclable page pool for streaming writers.
#[deprecated(note = "RenderBuffer owns and recycles its pages directly")]
pub struct PagePool<const N: usize> {
    spare: RefCell<Vec<SinkPage<N>>>,
    max_idle: usize,
}

impl<const N: usize> PagePool<N> {
    /// Create a new pool with a default idle-page cap of 16.
    pub fn new() -> Self {
        Self::with_max_idle(16)
    }

    /// Create a new pool, capping the number of retained spare pages.
    pub fn with_max_idle(max_idle: usize) -> Self {
        Self {
            spare: RefCell::new(Vec::with_capacity(max_idle)),
            max_idle,
        }
    }

    /// Checkout a page from the pool, allocating on miss.
    pub fn checkout_page(&self) -> SinkPage<N> {
        self.spare.borrow_mut().pop().unwrap_or_default()
    }

    /// Return a page to the pool.
    pub fn return_page(&self, mut page: SinkPage<N>) {
        page.reset();
        let mut spare = self.spare.borrow_mut();
        if spare.len() < self.max_idle {
            spare.push(page);
        }
    }

    /// Number of spare pages currently retained.
    pub fn spare_len(&self) -> usize {
        self.spare.borrow().len()
    }
}

impl<const N: usize> Default for PagePool<N> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SinkWriter
// ============================================================================

/// A `WriteBuf` implementation that draws pages from a concrete [`PagePool`].
#[deprecated(note = "standard rendering and streaming use RenderBuffer")]
pub struct SinkWriter<const N: usize> {
    active: Option<SinkPage<N>>,
    sealed: Vec<SinkPage<N>>,
    page_pool: Rc<PagePool<N>>,
    total_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[deprecated(note = "legacy SinkWriter compatibility; use RenderBuffer checkpoints")]
pub struct SinkCheckpoint {
    sealed_len: usize,
    active_len: usize,
    total_len: usize,
}

struct SinkPagesView<'a, const N: usize> {
    pages: &'a [SinkPage<N>],
    total_len: usize,
}

impl<'a, const N: usize> StreamChunkView for SinkPagesView<'a, N> {
    fn total_len(&self) -> usize {
        self.total_len
    }

    fn for_each_slice(
        &self,
        f: &mut dyn FnMut(&[u8]) -> Result<(), WriteError>,
    ) -> Result<(), WriteError> {
        for page in self.pages {
            if !page.is_empty() {
                f(page.as_written())?;
            }
        }
        Ok(())
    }
}

impl<const N: usize> SinkWriter<N> {
    /// Create a new writer backed by `page_pool`.
    pub fn new(page_pool: Rc<PagePool<N>>) -> Self {
        let active = page_pool.checkout_page();
        Self {
            active: Some(active),
            sealed: Vec::new(),
            page_pool,
            total_len: 0,
        }
    }

    /// Number of pages currently held by this writer (active + sealed).
    pub fn page_count(&self) -> usize {
        self.sealed.len() + usize::from(self.active.is_some())
    }

    /// Shared pool backing this writer.
    pub fn page_pool(&self) -> Rc<PagePool<N>> {
        Rc::clone(&self.page_pool)
    }

    fn active(&self) -> &SinkPage<N> {
        self.active
            .as_ref()
            .expect("sink writer always has an active page")
    }

    fn active_mut(&mut self) -> &mut SinkPage<N> {
        self.active
            .as_mut()
            .expect("sink writer always has an active page")
    }

    /// Seal the active page and check out a fresh one.
    fn rotate(&mut self) {
        let next = self.page_pool.checkout_page();
        let full = self
            .active
            .replace(next)
            .expect("sink writer always has an active page");
        self.sealed.push(full);
    }

    /// Take all sealed pages plus the active page (if non-empty), leaving the
    /// writer with a fresh active page.
    fn drain_pages(&mut self) -> Vec<SinkPage<N>> {
        let mut out = std::mem::take(&mut self.sealed);
        if !self.active().is_empty() {
            let next = self.page_pool.checkout_page();
            let active = self
                .active
                .replace(next)
                .expect("sink writer always has an active page");
            out.push(active);
        }
        out
    }

    /// Flush all currently-buffered bytes through `flusher`.
    ///
    /// The pages themselves remain concrete and are returned to the page pool
    /// immediately after the transport-facing flush returns.
    pub fn flush_into_flusher(
        &mut self,
        flusher: &dyn StreamChunkFlusher,
    ) -> Result<(), WriteError> {
        let pages = self.drain_pages();
        if pages.is_empty() {
            return Ok(());
        }

        let total_len = pages.iter().map(SinkPage::len).sum();
        let view = SinkPagesView {
            pages: &pages,
            total_len,
        };
        flusher.flush_chunk_view(&view)?;

        for page in pages {
            self.page_pool.return_page(page);
        }
        self.total_len = 0;
        Ok(())
    }
}

impl<const N: usize> Drop for SinkWriter<N> {
    fn drop(&mut self) {
        for page in self.sealed.drain(..) {
            self.page_pool.return_page(page);
        }
        if let Some(page) = self.active.take() {
            self.page_pool.return_page(page);
        }
    }
}

impl<const N: usize> WriteBuf for SinkWriter<N> {
    #[inline(always)]
    fn capacity(&self) -> usize {
        usize::MAX
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.total_len
    }

    #[inline(always)]
    fn reserve(&mut self, _additional: usize) -> Result<(), WriteError> {
        Ok(())
    }

    #[inline]
    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        let active = self.active_mut();
        let new_len = active.len + bytes.len();
        if new_len <= N {
            active.buf[active.len..new_len].copy_from_slice(bytes);
            active.len = new_len;
            self.total_len += bytes.len();
            return Ok(());
        }

        let mut remaining = bytes;
        while !remaining.is_empty() {
            let active = self.active_mut();
            let space = N - active.len;
            let take = space.min(remaining.len());
            active.buf[active.len..active.len + take].copy_from_slice(&remaining[..take]);
            active.len += take;
            self.total_len += take;
            remaining = &remaining[take..];
            if !remaining.is_empty() {
                self.rotate();
            }
        }
        Ok(())
    }

    fn as_slice(&self) -> &[u8] {
        if self.sealed.is_empty() {
            self.active().as_written()
        } else {
            &[]
        }
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.total_len);
        for page in &self.sealed {
            out.extend_from_slice(page.as_written());
        }
        out.extend_from_slice(self.active().as_written());
        out
    }

    fn clear(&mut self) {
        for page in self.sealed.drain(..) {
            self.page_pool.return_page(page);
        }
        self.active_mut().reset();
        self.total_len = 0;
    }
}

impl<const N: usize> CheckpointWriteBuf for SinkWriter<N> {
    type Checkpoint = SinkCheckpoint;

    fn checkpoint(&self) -> Self::Checkpoint {
        SinkCheckpoint {
            sealed_len: self.sealed.len(),
            active_len: self.active().len,
            total_len: self.total_len,
        }
    }

    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
        if checkpoint.sealed_len > self.sealed.len() || checkpoint.total_len > self.total_len {
            return false;
        }

        if checkpoint.sealed_len == self.sealed.len() {
            if checkpoint.active_len > self.active().len {
                return false;
            }
            self.active_mut().len = checkpoint.active_len;
            self.total_len = checkpoint.total_len;
            return true;
        }

        if checkpoint.active_len > self.sealed[checkpoint.sealed_len].len {
            return false;
        }
        let mut restored_active = self.sealed.remove(checkpoint.sealed_len);
        restored_active.len = checkpoint.active_len;

        while self.sealed.len() > checkpoint.sealed_len {
            let page = self.sealed.pop().expect("sealed len checked above");
            self.page_pool.return_page(page);
        }

        if let Some(old_active) = self.active.replace(restored_active) {
            self.page_pool.return_page(old_active);
        }
        self.total_len = checkpoint.total_len;
        true
    }
}

// ============================================================================
// In-order streaming aliases
// ============================================================================

/// Page size used by Suspense's in-order child writers.
#[deprecated(note = "Suspense now reuses the standard RenderBuffer")]
pub const IN_ORDER_SINK_PAGE_SIZE: usize = 32 * 1024;

/// Concrete pool type shared across in-order Suspense boundaries.
#[deprecated(note = "Suspense now reuses the standard RenderBuffer")]
pub type InOrderPagePool = PagePool<IN_ORDER_SINK_PAGE_SIZE>;

/// Concrete writer type used by in-order Suspense child renders.
#[deprecated(note = "Suspense now reuses the standard RenderBuffer")]
pub type InOrderSinkWriter = SinkWriter<IN_ORDER_SINK_PAGE_SIZE>;

// ============================================================================
// MemorySink
// ============================================================================

/// In-memory stream chunk sink for tests and benchmarks.
#[deprecated(note = "legacy SinkWriter test helper; use a StreamChunkFlusher with RenderBuffer")]
pub struct MemorySink<const N: usize> {
    page_pool: Rc<PagePool<N>>,
    output: RefCell<Vec<u8>>,
}

impl<const N: usize> MemorySink<N> {
    /// Create a new sink with a default idle-page cap of 16.
    pub fn new() -> Self {
        Self::with_max_idle(16)
    }

    /// Create a new sink with a specific idle-page cap.
    pub fn with_max_idle(max_idle: usize) -> Self {
        Self {
            page_pool: Rc::new(PagePool::with_max_idle(max_idle)),
            output: RefCell::new(Vec::new()),
        }
    }

    /// Shared page pool backing writers that flush into this sink.
    pub fn page_pool(&self) -> Rc<PagePool<N>> {
        Rc::clone(&self.page_pool)
    }

    /// Snapshot of all bytes flushed so far.
    pub fn output(&self) -> Vec<u8> {
        self.output.borrow().clone()
    }

    /// Clear the collected output.
    pub fn clear_output(&self) {
        self.output.borrow_mut().clear();
    }

    /// Number of spare pages currently retained by the pool.
    pub fn pool_size(&self) -> usize {
        self.page_pool.spare_len()
    }
}

impl<const N: usize> Default for MemorySink<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> StreamChunkFlusher for MemorySink<N> {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.output.borrow_mut().extend_from_slice(&chunk);
        Ok(())
    }

    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        chunk.for_each_slice(&mut |slice| {
            self.output.borrow_mut().extend_from_slice(slice);
            Ok(())
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: usize = 32;

    #[test]
    fn writer_writes_into_active_page() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        writer.extend(b"hello").unwrap();
        assert_eq!(writer.len(), 5);
        assert_eq!(writer.page_count(), 1);
        assert_eq!(writer.as_slice(), b"hello");
    }

    #[test]
    fn writer_rotates_pages_when_active_fills() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        let payload = vec![b'a'; 40];
        writer.extend(&payload).unwrap();

        assert_eq!(writer.len(), 40);
        assert_eq!(writer.page_count(), 2);
        assert_eq!(writer.to_vec(), payload);
    }

    #[test]
    fn writer_handles_multi_page_payload() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        let payload = vec![b'x'; PAGE * 4 + 7];
        writer.extend(&payload).unwrap();

        assert_eq!(writer.len(), payload.len());
        assert_eq!(writer.page_count(), 5);
        assert_eq!(writer.to_vec(), payload);
    }

    #[test]
    fn flush_ships_bytes_and_resets_writer() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        writer.extend(b"hello").unwrap();
        writer.flush_into_flusher(sink.as_ref()).unwrap();

        assert_eq!(sink.output(), b"hello");
        assert_eq!(writer.len(), 0);
        assert_eq!(writer.page_count(), 1);
    }

    #[test]
    fn flush_with_multiple_pages_preserves_order() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        let payload = vec![b'z'; PAGE * 3 + 5];
        writer.extend(&payload).unwrap();
        writer.flush_into_flusher(sink.as_ref()).unwrap();

        assert_eq!(sink.output(), payload);
    }

    #[test]
    fn empty_flush_is_noop() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        writer.flush_into_flusher(sink.as_ref()).unwrap();
        assert_eq!(sink.output(), b"");
    }

    #[test]
    fn drop_returns_pages_to_pool_without_flushing() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        assert_eq!(sink.pool_size(), 0);

        {
            let mut writer = SinkWriter::new(sink.page_pool());
            writer.extend(b"unflushed").unwrap();
        }

        assert_eq!(sink.output(), b"");
        assert_eq!(sink.pool_size(), 1);
    }

    #[test]
    fn pages_recycle_after_flush() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());

        {
            let mut writer = SinkWriter::new(sink.page_pool());
            writer.extend(&[b'1'; PAGE * 3]).unwrap();
            writer.flush_into_flusher(sink.as_ref()).unwrap();
        }
        let pool_after_first = sink.pool_size();
        assert!(
            pool_after_first >= 3,
            "expected at least 3 recycled pages, got {pool_after_first}"
        );

        {
            let mut writer = SinkWriter::new(sink.page_pool());
            writer.extend(&[b'2'; PAGE * 2]).unwrap();
            writer.flush_into_flusher(sink.as_ref()).unwrap();
        }

        assert_eq!(sink.output().len(), PAGE * 5);
    }

    #[test]
    fn two_writers_coexist_with_independent_pages() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());

        let mut a = SinkWriter::new(sink.page_pool());
        let mut b = SinkWriter::new(sink.page_pool());

        a.extend(b"alpha").unwrap();
        b.extend(b"beta").unwrap();

        assert_eq!(a.to_vec(), b"alpha");
        assert_eq!(b.to_vec(), b"beta");

        a.flush_into_flusher(sink.as_ref()).unwrap();
        b.flush_into_flusher(sink.as_ref()).unwrap();

        assert_eq!(sink.output(), b"alphabeta");
    }

    #[test]
    fn writebuf_extend_static_works() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        WriteBuf::extend_static(&mut writer, b"<html>").unwrap();
        assert_eq!(writer.as_slice(), b"<html>");
    }

    #[test]
    fn writebuf_push_works() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        WriteBuf::push(&mut writer, b'X').unwrap();
        assert_eq!(writer.as_slice(), b"X");
    }

    #[test]
    fn writebuf_clear_recycles_pages() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        writer.extend(&[b'q'; PAGE * 3]).unwrap();
        assert_eq!(writer.len(), PAGE * 3);
        assert_eq!(writer.page_count(), 3);

        WriteBuf::clear(&mut writer);
        assert_eq!(writer.len(), 0);
        assert_eq!(writer.page_count(), 1);
        assert_eq!(sink.pool_size(), 2);
    }

    #[test]
    fn as_slice_returns_active_only_when_no_sealed_pages() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        writer.extend(b"small").unwrap();
        assert_eq!(writer.as_slice(), b"small");

        writer.extend(&[b'.'; PAGE]).unwrap();
        assert_eq!(writer.as_slice(), b"");
    }

    #[test]
    fn pool_respects_max_idle_cap() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::with_max_idle(2));

        {
            let mut writer = SinkWriter::new(sink.page_pool());
            writer.extend(&[b'k'; PAGE * 5]).unwrap();
        }

        assert_eq!(sink.pool_size(), 2);
    }

    #[test]
    fn extend_static_falls_through_to_extend() {
        let sink: Rc<MemorySink<PAGE>> = Rc::new(MemorySink::new());
        let mut writer = SinkWriter::new(sink.page_pool());

        WriteBuf::extend_static(&mut writer, b"static-bytes").unwrap();
        assert_eq!(writer.as_slice(), b"static-bytes");
    }
}
