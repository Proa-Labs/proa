// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Exact attempted-output accounting for render buffers.
//!
//! [`OutputMeter`] is the allocation-free, stack-owned meter used by ordinary
//! synchronous renders. [`SharedOutputMeter`] is selected up front when owned
//! deferred work must retain the activation meter across threads. Both issue
//! [`OutputReservation`] values through the same sealed
//! [`OutputMeterHandle`] contract.
//!
//! [`MeteredWriteBuf`] charges unreserved component writes before delegating to
//! its inner buffer. Known-length semantic operations reserve once and render
//! through [`ReservedWriteBuf`], which consumes only local reservation credit
//! and therefore cannot double-debit the activation meter.

use std::cell::Cell;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use bytes::Bytes;

use super::write_buf::{
    CheckpointWriteBuf, FlushMark, StreamingChunk, StreamingZeroCopyBuf, WriteBuf, WriteError,
};

mod private {
    pub trait Sealed {}
}

/// A handle capable of atomically reserving exact attempted-output bytes.
///
/// This trait is sealed so metered buffers cannot be constructed with a handle
/// that silently bypasses accounting. Use [`OutputMeter::borrow`] for the
/// stack-only sync path or clone a [`SharedOutputMeter`] for deferred work.
pub trait OutputMeterHandle: private::Sealed + Clone {
    /// The activation's original output limit.
    fn limit(&self) -> u64;

    /// Output credit that has not yet been reserved.
    fn remaining(&self) -> u64;

    /// Indivisibly compare and debit `requested` bytes.
    ///
    /// A failed reservation does not modify the meter.
    fn reserve_exact(&self, requested: u64) -> Result<OutputReservation, WriteError>;

    /// Bytes successfully reserved since this activation began.
    #[inline]
    fn consumed(&self) -> u64 {
        self.limit().saturating_sub(self.remaining())
    }
}

/// Stack-owned exact output meter for a synchronous render activation.
///
/// Constructing and borrowing this meter never allocates. The [`Cell`] makes
/// the local mode intentionally single-threaded; renders that can spawn owned
/// deferred work must choose [`SharedOutputMeter`] before rendering begins.
#[derive(Debug)]
pub struct OutputMeter {
    limit: u64,
    remaining: Cell<u64>,
}

impl OutputMeter {
    /// Create an allocation-free meter with `limit` bytes of output credit.
    #[inline]
    pub const fn new(limit: u64) -> Self {
        Self {
            limit,
            remaining: Cell::new(limit),
        }
    }

    /// Borrow the stack meter for a scoped render.
    #[inline]
    pub const fn borrow(&self) -> BorrowedOutputMeter<'_> {
        BorrowedOutputMeter { meter: self }
    }

    /// The activation's original output limit.
    #[inline]
    pub const fn limit(&self) -> u64 {
        self.limit
    }

    /// Output credit that has not yet been reserved.
    #[inline]
    pub fn remaining(&self) -> u64 {
        self.remaining.get()
    }

    /// Bytes successfully reserved since this activation began.
    #[inline]
    pub fn consumed(&self) -> u64 {
        self.limit.saturating_sub(self.remaining())
    }

    /// Indivisibly compare and debit `requested` bytes from this local meter.
    #[inline]
    pub fn reserve_exact(&self, requested: u64) -> Result<OutputReservation, WriteError> {
        self.borrow().reserve_exact(requested)
    }
}

/// Copyable scoped handle for a stack-owned [`OutputMeter`].
#[derive(Debug, Clone, Copy)]
pub struct BorrowedOutputMeter<'a> {
    meter: &'a OutputMeter,
}

impl BorrowedOutputMeter<'_> {
    /// The activation's original output limit.
    #[inline]
    pub const fn limit(self) -> u64 {
        self.meter.limit
    }

    /// Output credit that has not yet been reserved.
    #[inline]
    pub fn remaining(self) -> u64 {
        self.meter.remaining.get()
    }

    /// Bytes successfully reserved since this activation began.
    #[inline]
    pub fn consumed(self) -> u64 {
        self.limit().saturating_sub(self.remaining())
    }

    /// Indivisibly compare and debit `requested` bytes.
    #[inline]
    pub fn reserve_exact(self, requested: u64) -> Result<OutputReservation, WriteError> {
        let remaining = self.meter.remaining.get();
        if requested > remaining {
            return Err(WriteError::OutputLimitExceeded {
                requested,
                remaining,
            });
        }

        self.meter.remaining.set(remaining - requested);
        Ok(OutputReservation::new(requested))
    }
}

impl private::Sealed for BorrowedOutputMeter<'_> {}

impl OutputMeterHandle for BorrowedOutputMeter<'_> {
    #[inline]
    fn limit(&self) -> u64 {
        (*self).limit()
    }

    #[inline]
    fn remaining(&self) -> u64 {
        (*self).remaining()
    }

    #[inline]
    fn reserve_exact(&self, requested: u64) -> Result<OutputReservation, WriteError> {
        (*self).reserve_exact(requested)
    }
}

#[derive(Debug)]
struct SharedOutputMeterState {
    limit: u64,
    remaining: AtomicU64,
}

/// Cloneable, thread-safe output meter selected for deferred render modes.
///
/// Construction allocates the shared state once. Cloning only increments the
/// `Arc` reference count, and all reservations use one atomic compare/debit so
/// concurrent branches cannot oversubscribe the remaining credit.
#[derive(Debug, Clone)]
pub struct SharedOutputMeter {
    state: Arc<SharedOutputMeterState>,
}

impl SharedOutputMeter {
    /// Create a shared meter with `limit` bytes of output credit.
    #[inline]
    pub fn new(limit: u64) -> Self {
        Self {
            state: Arc::new(SharedOutputMeterState {
                limit,
                remaining: AtomicU64::new(limit),
            }),
        }
    }

    /// The activation's original output limit.
    #[inline]
    pub fn limit(&self) -> u64 {
        self.state.limit
    }

    /// Output credit that has not yet been reserved.
    #[inline]
    pub fn remaining(&self) -> u64 {
        self.state.remaining.load(Ordering::Acquire)
    }

    /// Bytes successfully reserved since this activation began.
    #[inline]
    pub fn consumed(&self) -> u64 {
        self.limit().saturating_sub(self.remaining())
    }

    /// Atomically compare and debit `requested` bytes.
    pub fn reserve_exact(&self, requested: u64) -> Result<OutputReservation, WriteError> {
        let mut remaining = self.state.remaining.load(Ordering::Acquire);
        loop {
            if requested > remaining {
                return Err(WriteError::OutputLimitExceeded {
                    requested,
                    remaining,
                });
            }

            match self.state.remaining.compare_exchange_weak(
                remaining,
                remaining - requested,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(OutputReservation::new(requested)),
                Err(observed) => remaining = observed,
            }
        }
    }
}

impl private::Sealed for SharedOutputMeter {}

impl OutputMeterHandle for SharedOutputMeter {
    #[inline]
    fn limit(&self) -> u64 {
        self.limit()
    }

    #[inline]
    fn remaining(&self) -> u64 {
        self.remaining()
    }

    #[inline]
    fn reserve_exact(&self, requested: u64) -> Result<OutputReservation, WriteError> {
        self.reserve_exact(requested)
    }
}

/// Exact output credit granted by an [`OutputMeterHandle`].
///
/// The activation meter is debited before this value is returned. Dropping a
/// reservation or encountering an underlying write failure never refunds that
/// debit.
#[derive(Debug)]
#[must_use = "an output reservation must be consumed by a reservation-scoped write"]
pub struct OutputReservation {
    reserved: u64,
    remaining: u64,
}

impl OutputReservation {
    #[inline]
    const fn new(reserved: u64) -> Self {
        Self {
            reserved,
            remaining: reserved,
        }
    }

    /// Total credit originally granted.
    #[inline]
    pub const fn reserved(&self) -> u64 {
        self.reserved
    }

    /// Credit not yet consumed by writes.
    #[inline]
    pub const fn remaining(&self) -> u64 {
        self.remaining
    }

    /// Bytes already attempted through this reservation.
    #[inline]
    pub const fn consumed(&self) -> u64 {
        self.reserved - self.remaining
    }

    /// Whether the exact granted credit has been consumed.
    #[inline]
    pub const fn is_complete(&self) -> bool {
        self.remaining == 0
    }

    /// Validate that the writer consumed exactly the granted byte count.
    #[inline]
    pub fn validate_complete(&self) -> Result<(), WriteError> {
        if self.is_complete() {
            Ok(())
        } else {
            Err(WriteError::OutputReservationIncomplete {
                reserved: self.reserved,
                consumed: self.consumed(),
                remaining: self.remaining,
            })
        }
    }

    /// Consume this token after validating exact completion.
    #[inline]
    pub fn finish(self) -> Result<(), WriteError> {
        self.validate_complete()
    }

    #[inline]
    fn consume(&mut self, requested: u64) -> Result<(), WriteError> {
        if requested > self.remaining {
            return Err(WriteError::OutputReservationExceeded {
                requested,
                reserved: self.reserved,
                consumed: self.consumed(),
                remaining: self.remaining,
            });
        }

        self.remaining -= requested;
        Ok(())
    }
}

#[inline(always)]
fn byte_count(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// Write-buffer view that consumes one exact [`OutputReservation`].
///
/// This wrapper never accesses the activation meter: its credit has already
/// been debited. Each attempted write consumes local credit before delegating,
/// so an underlying capacity or transport failure cannot refund bytes.
pub struct ReservedWriteBuf<B> {
    inner: B,
    reservation: OutputReservation,
}

impl<B> ReservedWriteBuf<B> {
    /// Scope `inner` to an exact pre-debited reservation.
    #[inline]
    pub const fn new(inner: B, reservation: OutputReservation) -> Self {
        Self { inner, reservation }
    }

    /// Inspect the reservation's current accounting state.
    #[inline]
    pub const fn reservation(&self) -> &OutputReservation {
        &self.reservation
    }

    /// Validate exact completion without consuming this view.
    #[inline]
    pub fn validate_complete(&self) -> Result<(), WriteError> {
        self.reservation.validate_complete()
    }

    /// Finish the reservation and return the wrapped buffer.
    ///
    /// On an incomplete reservation the wrapper is dropped. Scoped callers
    /// normally wrap `&mut B`, so the underlying caller-owned buffer remains
    /// available after the borrow ends.
    #[inline]
    pub fn finish(self) -> Result<B, WriteError> {
        let Self { inner, reservation } = self;
        reservation.finish()?;
        Ok(inner)
    }

    /// Return the wrapped buffer without validating exact completion.
    ///
    /// This is intended for propagating an earlier underlying write error. A
    /// successful semantic target write should use [`ReservedWriteBuf::finish`].
    #[inline]
    pub fn into_inner_unchecked(self) -> B {
        self.inner
    }
}

impl<B: WriteBuf> WriteBuf for ReservedWriteBuf<B> {
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
        self.reservation.consume(byte_count(bytes.len()))?;
        self.inner.extend(bytes)
    }

    #[inline(always)]
    fn push(&mut self, byte: u8) -> Result<(), WriteError> {
        self.reservation.consume(1)?;
        self.inner.push(byte)
    }

    #[inline(always)]
    fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
        self.reservation.consume(byte_count(bytes.len()))?;
        self.inner.extend_static(bytes)
    }

    #[inline(always)]
    fn extend_shared(&mut self, bytes: &Bytes) -> Result<(), WriteError> {
        self.reservation.consume(byte_count(bytes.len()))?;
        self.inner.extend_shared(bytes)
    }

    // Deliberately inherit WriteBuf's bounded-stack `extend_with` default. It
    // invokes `extend` with the closure's actual byte count, not `max_len`.

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
    }
}

/// Wrapped checkpoint for a [`ReservedWriteBuf`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReservedCheckpoint<C> {
    inner: C,
}

impl<B: CheckpointWriteBuf> CheckpointWriteBuf for ReservedWriteBuf<B> {
    type Checkpoint = ReservedCheckpoint<B::Checkpoint>;

    #[inline(always)]
    fn checkpoint(&self) -> Self::Checkpoint {
        ReservedCheckpoint {
            inner: self.inner.checkpoint(),
        }
    }

    #[inline(always)]
    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
        self.inner.rewind_to(checkpoint.inner)
    }
}

/// Write buffer that charges every unreserved attempted output byte.
///
/// `B` may be an owned buffer or a scoped `&mut B`; no `'static` bound is
/// required. `H` is inferred as either [`BorrowedOutputMeter`] or
/// [`SharedOutputMeter`].
pub struct MeteredWriteBuf<B, H: OutputMeterHandle> {
    inner: B,
    meter: H,
}

impl<B, H: OutputMeterHandle> MeteredWriteBuf<B, H> {
    /// Wrap `inner` with one activation meter handle.
    #[inline]
    pub const fn new(inner: B, meter: H) -> Self {
        Self { inner, meter }
    }

    /// Inspect the active meter handle.
    #[inline]
    pub const fn meter(&self) -> &H {
        &self.meter
    }

    /// Reserve one complete known-length semantic write.
    ///
    /// The returned token is intended for a reservation-scoped target writer,
    /// which consumes this already-debited credit without charging the meter a
    /// second time.
    #[inline]
    pub fn reserve_exact(&self, requested: u64) -> Result<OutputReservation, WriteError> {
        self.meter.reserve_exact(requested)
    }

    /// Access the physical buffer beneath this meter from a core adapter.
    ///
    /// Writes through the returned buffer bypass activation accounting. This
    /// low-level escape hatch exists for scoped target adapters that have
    /// already debited an [`OutputReservation`] from this meter and immediately
    /// install a [`ReservedWriteBuf`] over the returned borrow. Ordinary
    /// component and renderer code must continue writing through `self`.
    #[inline]
    pub(crate) fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }

    /// Remaining activation output credit.
    #[inline]
    pub fn remaining_output(&self) -> u64 {
        self.meter.remaining()
    }

    /// Consume the wrapper and return its inner buffer.
    #[inline]
    pub fn into_inner(self) -> B {
        self.inner
    }

    /// Consume the wrapper and return both parts, for example around a
    /// buffer-specific flush operation. Reusing the returned handle preserves
    /// the already-consumed activation credit.
    #[inline]
    pub fn into_parts(self) -> (B, H) {
        (self.inner, self.meter)
    }

    #[inline(always)]
    fn debit(&self, requested: usize) -> Result<OutputReservation, WriteError> {
        self.meter.reserve_exact(byte_count(requested))
    }
}

impl<B: WriteBuf, H: OutputMeterHandle> WriteBuf for MeteredWriteBuf<B, H> {
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
        let _reservation = self.debit(bytes.len())?;
        self.inner.extend(bytes)
    }

    #[inline(always)]
    fn push(&mut self, byte: u8) -> Result<(), WriteError> {
        let _reservation = self.debit(1)?;
        self.inner.push(byte)
    }

    #[inline(always)]
    fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
        let _reservation = self.debit(bytes.len())?;
        self.inner.extend_static(bytes)
    }

    #[inline(always)]
    fn extend_shared(&mut self, bytes: &Bytes) -> Result<(), WriteError> {
        let _reservation = self.debit(bytes.len())?;
        self.inner.extend_shared(bytes)
    }

    // Deliberately inherit WriteBuf's bounded-stack `extend_with` default. It
    // invokes `extend` with the closure's actual byte count, not `max_len`.

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
    }
}

/// Wrapped checkpoint for a [`MeteredWriteBuf`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeteredCheckpoint<C> {
    inner: C,
}

impl<B: CheckpointWriteBuf, H: OutputMeterHandle> CheckpointWriteBuf for MeteredWriteBuf<B, H> {
    type Checkpoint = MeteredCheckpoint<B::Checkpoint>;

    #[inline(always)]
    fn checkpoint(&self) -> Self::Checkpoint {
        MeteredCheckpoint {
            inner: self.inner.checkpoint(),
        }
    }

    #[inline(always)]
    fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
        self.inner.rewind_to(checkpoint.inner)
    }
}

// Streaming flushes remove bytes from the physical buffer but must not restore
// activation credit. These forwarding methods keep the meter wrapper installed
// while preserving `StreamingZeroCopyBuf`'s transport-ready chunk API.
impl<H: OutputMeterHandle> MeteredWriteBuf<StreamingZeroCopyBuf, H> {
    /// Capture a streaming flush mark without changing output credit.
    #[inline]
    pub fn mark(&mut self) -> FlushMark {
        self.inner.mark()
    }

    /// Flush a prefix without replenishing the activation meter.
    #[inline]
    pub fn flush_through(&mut self, mark: FlushMark) -> Option<StreamingChunk> {
        self.inner.flush_through(mark)
    }

    /// Flush all buffered bytes without replenishing the activation meter.
    #[inline]
    pub fn flush_all(&mut self) -> Option<StreamingChunk> {
        self.inner.flush_all()
    }

    /// Freeze the remaining streaming buffer.
    #[inline]
    pub fn freeze(self) -> StreamingChunk {
        self.inner.freeze()
    }
}

impl<H: OutputMeterHandle> MeteredWriteBuf<&mut StreamingZeroCopyBuf, H> {
    /// Capture a streaming flush mark without changing output credit.
    #[inline]
    pub fn mark(&mut self) -> FlushMark {
        self.inner.mark()
    }

    /// Flush a prefix without replenishing the activation meter.
    #[inline]
    pub fn flush_through(&mut self, mark: FlushMark) -> Option<StreamingChunk> {
        self.inner.flush_through(mark)
    }

    /// Flush all buffered bytes without replenishing the activation meter.
    #[inline]
    pub fn flush_all(&mut self) -> Option<StreamingChunk> {
        self.inner.flush_all()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;
    use crate::shared::write_buf::WriteCursor;

    #[derive(Default)]
    struct DelegationSpy {
        bytes: Vec<u8>,
        extend_calls: usize,
        push_calls: usize,
        static_calls: usize,
        shared_calls: usize,
    }

    impl WriteBuf for DelegationSpy {
        fn capacity(&self) -> usize {
            usize::MAX
        }

        fn len(&self) -> usize {
            self.bytes.len()
        }

        fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
            self.extend_calls += 1;
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }

        fn push(&mut self, byte: u8) -> Result<(), WriteError> {
            self.push_calls += 1;
            self.bytes.push(byte);
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

    impl CheckpointWriteBuf for DelegationSpy {
        type Checkpoint = usize;

        fn checkpoint(&self) -> Self::Checkpoint {
            self.bytes.len()
        }

        fn rewind_to(&mut self, checkpoint: Self::Checkpoint) -> bool {
            if checkpoint > self.bytes.len() {
                return false;
            }
            self.bytes.truncate(checkpoint);
            true
        }
    }

    #[test]
    fn local_meter_reserves_exactly_and_preserves_structured_failure() {
        let meter = OutputMeter::new(7);
        let reservation = meter.reserve_exact(5).unwrap();
        assert_eq!(reservation.reserved(), 5);
        assert_eq!(meter.limit(), 7);
        assert_eq!(meter.consumed(), 5);
        assert_eq!(meter.remaining(), 2);

        let err = meter.reserve_exact(3).unwrap_err();
        assert_eq!(
            err,
            WriteError::OutputLimitExceeded {
                requested: 3,
                remaining: 2,
            }
        );
        assert!(!err.is_boundary_catchable());
        assert_eq!(meter.remaining(), 2);
    }

    #[test]
    fn shared_meter_compare_and_debit_cannot_oversubscribe() {
        const THREADS: usize = 32;
        const LIMIT: u64 = 17;

        let meter = SharedOutputMeter::new(LIMIT);
        let barrier = Arc::new(Barrier::new(THREADS));
        let mut joins = Vec::with_capacity(THREADS);
        for _ in 0..THREADS {
            let meter = meter.clone();
            let barrier = Arc::clone(&barrier);
            joins.push(thread::spawn(move || {
                barrier.wait();
                meter.reserve_exact(1).is_ok()
            }));
        }

        let successful = joins
            .into_iter()
            .map(|join| join.join().unwrap())
            .filter(|success| *success)
            .count();
        assert_eq!(successful, LIMIT as usize);
        assert_eq!(meter.consumed(), LIMIT);
        assert_eq!(meter.remaining(), 0);
    }

    #[test]
    fn shared_meter_handle_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SharedOutputMeter>();
    }

    #[test]
    fn metered_buffer_preserves_static_and_shared_delegation() {
        static STATIC: &[u8] = b"static";
        let shared = Bytes::from_static(b"shared");
        let meter = OutputMeter::new(32);
        let mut out = MeteredWriteBuf::new(DelegationSpy::default(), meter.borrow());

        out.extend(b"plain").unwrap();
        out.push(b'!').unwrap();
        out.extend_static(STATIC).unwrap();
        out.extend_shared(&shared).unwrap();

        assert_eq!(out.remaining_output(), 32 - 5 - 1 - 6 - 6);
        let spy = out.into_inner();
        assert_eq!(spy.bytes, b"plain!staticshared");
        assert_eq!(spy.extend_calls, 1);
        assert_eq!(spy.push_calls, 1);
        assert_eq!(spy.static_calls, 1);
        assert_eq!(spy.shared_calls, 1);
    }

    #[test]
    fn over_budget_write_is_rejected_before_delegation() {
        let meter = OutputMeter::new(2);
        let mut out = MeteredWriteBuf::new(DelegationSpy::default(), meter.borrow());

        let err = out.extend_static(b"three").unwrap_err();
        assert_eq!(
            err,
            WriteError::OutputLimitExceeded {
                requested: 5,
                remaining: 2,
            }
        );
        assert_eq!(out.inner.bytes, b"");
        assert_eq!(out.inner.static_calls, 0);
        assert_eq!(meter.remaining(), 2);
    }

    #[test]
    fn failed_underlying_write_still_consumes_attempted_bytes() {
        let meter = OutputMeter::new(10);
        let mut backing = [0u8; 3];
        let cursor = WriteCursor::new(&mut backing);
        let mut out = MeteredWriteBuf::new(cursor, meter.borrow());

        assert_eq!(out.extend(b"four").unwrap_err(), WriteError::OutOfCapacity);
        assert_eq!(meter.remaining(), 6);
        assert_eq!(out.len(), 0);
    }

    #[test]
    fn reserve_and_extend_with_charge_only_attempted_output() {
        let meter = OutputMeter::new(10);
        let mut out = MeteredWriteBuf::new(Vec::new(), meter.borrow());

        out.reserve(100).unwrap();
        assert_eq!(meter.remaining(), 10);
        out.extend_with(8, |spare| {
            spare[..3].copy_from_slice(b"abc");
            3
        })
        .unwrap();

        assert_eq!(out.as_slice(), b"abc");
        assert_eq!(meter.remaining(), 7);
    }

    #[test]
    fn owned_and_scoped_borrowed_buffers_both_work() {
        let owned_meter = OutputMeter::new(3);
        let mut owned = MeteredWriteBuf::new(Vec::new(), owned_meter.borrow());
        owned.extend(b"abc").unwrap();
        assert_eq!(owned.into_inner(), b"abc");

        let borrowed_meter = OutputMeter::new(4);
        let mut backing = [0u8; 4];
        let mut cursor = WriteCursor::new(&mut backing);
        {
            let mut borrowed = MeteredWriteBuf::new(&mut cursor, borrowed_meter.borrow());
            borrowed.extend_static(b"test").unwrap();
        }
        assert_eq!(cursor.as_slice(), b"test");
        assert_eq!(borrowed_meter.remaining(), 0);
    }

    #[test]
    fn clear_and_rewind_never_refund_meter_credit() {
        let meter = OutputMeter::new(10);
        let mut out = MeteredWriteBuf::new(DelegationSpy::default(), meter.borrow());
        out.extend(b"abc").unwrap();
        let checkpoint = out.checkpoint();
        out.extend(b"de").unwrap();
        assert!(out.rewind_to(checkpoint));
        assert_eq!(out.as_slice(), b"abc");
        assert_eq!(meter.remaining(), 5);

        out.clear();
        assert!(out.is_empty());
        assert_eq!(meter.remaining(), 5);
        out.extend(b"12345").unwrap();
        assert_eq!(meter.remaining(), 0);
    }

    #[test]
    fn streaming_flush_never_refunds_meter_credit() {
        let meter = OutputMeter::new(10);
        let mut out = MeteredWriteBuf::new(StreamingZeroCopyBuf::new(), meter.borrow());
        out.extend_static(b"abc").unwrap();
        let mark = out.mark();
        let chunk = out.flush_through(mark).unwrap();
        assert_eq!(chunk.to_vec(), b"abc");
        assert_eq!(meter.remaining(), 7);

        out.extend(b"1234567").unwrap();
        assert_eq!(meter.remaining(), 0);
        let chunk = out.flush_all().unwrap();
        assert_eq!(chunk.to_vec(), b"1234567");
        assert_eq!(meter.remaining(), 0);
    }

    #[test]
    fn reserved_writer_completes_exactly_without_double_debit() {
        static STATIC: &[u8] = b"ab";
        let shared = Bytes::from_static(b"de");
        let meter = OutputMeter::new(9);
        let reservation = meter.reserve_exact(5).unwrap();
        assert_eq!(meter.remaining(), 4);

        let mut out = ReservedWriteBuf::new(DelegationSpy::default(), reservation);
        out.extend_static(STATIC).unwrap();
        out.push(b'c').unwrap();
        out.extend_shared(&shared).unwrap();
        assert!(out.reservation().is_complete());
        let spy = out.finish().unwrap();

        assert_eq!(spy.bytes, b"abcde");
        assert_eq!(spy.static_calls, 1);
        assert_eq!(spy.push_calls, 1);
        assert_eq!(spy.shared_calls, 1);
        assert_eq!(meter.remaining(), 4);
    }

    #[test]
    fn reserved_writer_reports_under_consumption_precisely() {
        let meter = OutputMeter::new(8);
        let reservation = meter.reserve_exact(6).unwrap();
        let mut out = ReservedWriteBuf::new(Vec::new(), reservation);
        out.extend(b"ab").unwrap();

        let err = out.validate_complete().unwrap_err();
        assert_eq!(
            err,
            WriteError::OutputReservationIncomplete {
                reserved: 6,
                consumed: 2,
                remaining: 4,
            }
        );
        assert!(!err.is_boundary_catchable());
        assert_eq!(meter.remaining(), 2);
    }

    #[test]
    fn reserved_writer_rejects_over_consumption_before_delegation() {
        let meter = OutputMeter::new(8);
        let reservation = meter.reserve_exact(3).unwrap();
        let mut out = ReservedWriteBuf::new(DelegationSpy::default(), reservation);
        out.extend(b"ab").unwrap();

        let err = out.extend_static(b"cd").unwrap_err();
        assert_eq!(
            err,
            WriteError::OutputReservationExceeded {
                requested: 2,
                reserved: 3,
                consumed: 2,
                remaining: 1,
            }
        );
        assert!(!err.is_boundary_catchable());
        assert_eq!(out.as_slice(), b"ab");
        assert_eq!(out.inner.static_calls, 0);

        out.push(b'c').unwrap();
        assert_eq!(out.finish().unwrap().bytes, b"abc");
        assert_eq!(meter.remaining(), 5);
    }

    #[test]
    fn reservation_credit_is_not_refunded_on_capacity_failure_or_rewind() {
        let meter = OutputMeter::new(4);
        let reservation = meter.reserve_exact(4).unwrap();
        let mut backing = [0u8; 2];
        let cursor = WriteCursor::new(&mut backing);
        let mut out = ReservedWriteBuf::new(cursor, reservation);
        let checkpoint = out.checkpoint();

        assert_eq!(out.extend(b"four").unwrap_err(), WriteError::OutOfCapacity);
        assert!(out.reservation().is_complete());
        assert!(out.rewind_to(checkpoint));
        assert!(out.reservation().is_complete());
        assert_eq!(meter.remaining(), 0);
    }
}
