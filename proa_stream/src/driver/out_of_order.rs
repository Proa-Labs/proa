// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use std::future::Future;
use std::io::Write as _;
use std::pin::Pin;
use std::time::Instant;

use base64::{encoded_len, engine::general_purpose::STANDARD};
use futures::future::{AbortHandle, Abortable};
use futures::stream::{FuturesUnordered, StreamExt};
use futures::FutureExt;
use proa_core::{
    CspNonce, PendingBoundary, Priority, RenderArtifacts, RenderBuffer, RenderedBoundary,
    SendPendingBoundary, SendRenderedBoundary, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use tracing::error;

use crate::StreamContext;

type ScheduledBoundaryFuture =
    Pin<Box<dyn Future<Output = FinishedBoundary<RenderedBoundary>> + 'static>>;

struct ScheduledBoundary {
    priority: Priority,
    deadline: Option<Instant>,
    abort: AbortHandle,
}

/// Cache-resident deadline slots win for the common small boundary set. Above
/// this crossover, a lazy min-heap avoids rescanning every active deadline on
/// each wake. `scheduled` remains the source of truth for stale entries.
const DEADLINE_HEAP_THRESHOLD: usize = 64;

enum DeadlineIndex {
    Dense(Vec<(Instant, u64)>),
    Heap(BinaryHeap<Reverse<(Instant, u64)>>),
}

impl DeadlineIndex {
    fn new() -> Self {
        Self::Dense(Vec::new())
    }

    fn insert(&mut self, deadline: Instant, id: u64, scheduled: &BTreeMap<u64, ScheduledBoundary>) {
        match self {
            Self::Dense(entries) => {
                entries.push((deadline, id));
                if scheduled.len() > DEADLINE_HEAP_THRESHOLD {
                    *self = Self::Heap(build_deadline_heap(scheduled));
                }
            }
            Self::Heap(heap) => heap.push(Reverse((deadline, id))),
        }
    }

    fn next_deadline(&mut self, scheduled: &BTreeMap<u64, ScheduledBoundary>) -> Option<Instant> {
        self.rebalance(scheduled);
        match self {
            Self::Dense(entries) => {
                if entries.len() > DEADLINE_HEAP_THRESHOLD * 4 {
                    entries.clear();
                    entries.extend(scheduled.iter().filter_map(|(&id, boundary)| {
                        boundary.deadline.map(|deadline| (deadline, id))
                    }));
                }
                entries
                    .iter()
                    .filter(|&&(deadline, id)| deadline_is_current(scheduled, id, deadline))
                    .map(|&(deadline, _)| deadline)
                    .min()
            }
            Self::Heap(heap) => loop {
                let Reverse((deadline, id)) = *heap.peek()?;
                if deadline_is_current(scheduled, id, deadline) {
                    return Some(deadline);
                }
                heap.pop();
            },
        }
    }

    fn take_expired(
        &mut self,
        now: Instant,
        scheduled: &BTreeMap<u64, ScheduledBoundary>,
    ) -> Vec<u64> {
        self.rebalance(scheduled);
        let mut expired = BTreeSet::new();
        match self {
            Self::Dense(entries) => {
                entries.retain(|&(deadline, id)| {
                    if !deadline_is_current(scheduled, id, deadline) {
                        return false;
                    }
                    if deadline <= now {
                        expired.insert(id);
                        return false;
                    }
                    true
                });
            }
            Self::Heap(heap) => {
                while let Some(Reverse((deadline, id))) = heap.peek().copied() {
                    if !deadline_is_current(scheduled, id, deadline) {
                        heap.pop();
                        continue;
                    }
                    if deadline > now {
                        break;
                    }
                    heap.pop();
                    expired.insert(id);
                }
            }
        }
        expired.into_iter().collect()
    }

    fn rebalance(&mut self, scheduled: &BTreeMap<u64, ScheduledBoundary>) {
        if matches!(self, Self::Heap(_)) && scheduled.len() <= DEADLINE_HEAP_THRESHOLD / 2 {
            let entries = scheduled
                .iter()
                .filter_map(|(&id, boundary)| boundary.deadline.map(|deadline| (deadline, id)))
                .collect();
            *self = Self::Dense(entries);
        }
    }
}

fn build_deadline_heap(
    scheduled: &BTreeMap<u64, ScheduledBoundary>,
) -> BinaryHeap<Reverse<(Instant, u64)>> {
    scheduled
        .iter()
        .filter_map(|(&id, boundary)| boundary.deadline.map(|deadline| Reverse((deadline, id))))
        .collect()
}

fn deadline_is_current(
    scheduled: &BTreeMap<u64, ScheduledBoundary>,
    id: u64,
    deadline: Instant,
) -> bool {
    scheduled
        .get(&id)
        .is_some_and(|boundary| boundary.deadline == Some(deadline))
}

struct FinishedBoundary<R> {
    id: u64,
    priority: Priority,
    result: Result<Result<R, WriteError>, futures::future::Aborted>,
}

/// One observable out-of-order scheduling event.
pub enum BoundaryEvent<R = RenderedBoundary> {
    /// A boundary future finished, successfully or with a render error.
    Finished {
        id: u64,
        rendered: Result<R, WriteError>,
    },
    /// A boundary reached its configured deadline and was cancelled.
    TimedOut { id: u64 },
}

/// Runtime-independent scheduler for out-of-order boundaries.
///
/// All admitted futures are polled concurrently. When several futures are
/// ready in the same poll turn, events are ordered by
/// `Critical -> Optional -> Defer`, then by boundary id. Deadlines are data:
/// runtime adapters can inspect [`Self::next_deadline`] and call
/// [`Self::expire`] with their own clock.
pub struct OutOfOrderBoundaryDriver {
    in_flight: FuturesUnordered<ScheduledBoundaryFuture>,
    scheduled: BTreeMap<u64, ScheduledBoundary>,
    ready: VecDeque<BoundaryEvent>,
    deadlines: DeadlineIndex,
}

type SendScheduledBoundaryFuture =
    Pin<Box<dyn Future<Output = FinishedBoundary<SendRenderedBoundary>> + Send + 'static>>;

/// Send-capable boundary scheduler used by direct framework streams.
///
/// Its state is structurally separate from [`OutOfOrderBoundaryDriver`] so a
/// local future can never enter the direct path through type erasure.
pub struct SendOutOfOrderBoundaryDriver {
    in_flight: FuturesUnordered<SendScheduledBoundaryFuture>,
    scheduled: BTreeMap<u64, ScheduledBoundary>,
    ready: VecDeque<BoundaryEvent<SendRenderedBoundary>>,
    deadlines: DeadlineIndex,
}

impl Default for SendOutOfOrderBoundaryDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl SendOutOfOrderBoundaryDriver {
    pub fn new() -> Self {
        Self {
            in_flight: FuturesUnordered::new(),
            scheduled: BTreeMap::new(),
            ready: VecDeque::new(),
            deadlines: DeadlineIndex::new(),
        }
    }

    pub fn push(&mut self, boundary: SendPendingBoundary, now: Instant) {
        let SendPendingBoundary {
            id,
            future,
            priority,
            timeout,
        } = boundary;
        let deadline = timeout.map(|duration| now.checked_add(duration).unwrap_or(now));
        let (abort, registration) = AbortHandle::new_pair();
        if let Some(previous) = self.scheduled.insert(
            id,
            ScheduledBoundary {
                priority,
                deadline,
                abort,
            },
        ) {
            previous.abort.abort();
        }
        if let Some(deadline) = deadline {
            self.deadlines.insert(deadline, id, &self.scheduled);
        }
        self.in_flight.push(Box::pin(async move {
            FinishedBoundary {
                id,
                priority,
                result: Abortable::new(future, registration).await,
            }
        }));
    }

    pub fn extend<I>(&mut self, boundaries: I, now: Instant)
    where
        I: IntoIterator<Item = SendPendingBoundary>,
    {
        for boundary in boundaries {
            self.push(boundary, now);
        }
    }

    pub fn next_deadline(&mut self) -> Option<Instant> {
        self.deadlines.next_deadline(&self.scheduled)
    }

    pub fn expire(&mut self, now: Instant) -> Vec<BoundaryEvent<SendRenderedBoundary>> {
        let mut expired: Vec<(Priority, u64)> = self
            .deadlines
            .take_expired(now, &self.scheduled)
            .into_iter()
            .filter_map(|id| {
                self.scheduled
                    .get(&id)
                    .map(|boundary| (boundary.priority, id))
            })
            .collect();
        expired.sort_by_key(|(priority, id)| (priority_rank(*priority), *id));
        expired
            .into_iter()
            .filter_map(|(_, id)| {
                let boundary = self.scheduled.remove(&id)?;
                boundary.abort.abort();
                Some(BoundaryEvent::TimedOut { id })
            })
            .collect()
    }

    pub fn has_pending(&self) -> bool {
        !self.ready.is_empty() || !self.scheduled.is_empty()
    }

    pub async fn next_completed(&mut self) -> Option<BoundaryEvent<SendRenderedBoundary>> {
        if let Some(event) = self.ready.pop_front() {
            return Some(event);
        }

        loop {
            let first = self.in_flight.next().await?;
            let mut finished = vec![first];
            while let Some(Some(next)) = self.in_flight.next().now_or_never() {
                finished.push(next);
            }
            finished.sort_by_key(|boundary| (priority_rank(boundary.priority), boundary.id));
            for boundary in finished {
                let Ok(rendered) = boundary.result else {
                    continue;
                };
                if self.scheduled.remove(&boundary.id).is_some() {
                    self.ready.push_back(BoundaryEvent::Finished {
                        id: boundary.id,
                        rendered,
                    });
                }
            }
            if let Some(event) = self.ready.pop_front() {
                return Some(event);
            }
        }
    }

    #[cfg(feature = "rt-tokio")]
    pub async fn next_event(&mut self) -> Option<BoundaryEvent<SendRenderedBoundary>> {
        enum Wake {
            Completed(Option<BoundaryEvent<SendRenderedBoundary>>),
            Deadline,
        }

        loop {
            if let Some(event) = self.ready.pop_front() {
                return Some(event);
            }
            if !self.has_pending() {
                return None;
            }
            let Some(deadline) = self.next_deadline() else {
                return self.next_completed().await;
            };
            let wake = {
                let completed = self.next_completed();
                tokio::pin!(completed);
                tokio::select! {
                    biased;
                    event = &mut completed => Wake::Completed(event),
                    _ = tokio::time::sleep_until(deadline.into()) => Wake::Deadline,
                }
            };
            match wake {
                Wake::Completed(Some(event)) => return Some(event),
                Wake::Completed(None) => continue,
                Wake::Deadline => {
                    let expired = self.expire(Instant::now());
                    self.ready.extend(expired);
                }
            }
        }
    }

    #[cfg(not(feature = "rt-tokio"))]
    pub async fn next_event(&mut self) -> Option<BoundaryEvent<SendRenderedBoundary>> {
        if let Some(event) = self.ready.pop_front() {
            return Some(event);
        }
        let mut timed = Vec::new();
        for (&id, boundary) in &self.scheduled {
            if boundary.deadline.is_some() {
                timed.push((boundary.priority, id));
            }
        }
        timed.sort_by_key(|(priority, id)| (priority_rank(*priority), *id));
        for (_, id) in timed {
            if let Some(boundary) = self.scheduled.remove(&id) {
                boundary.abort.abort();
                self.ready.push_back(BoundaryEvent::TimedOut { id });
            }
        }
        if let Some(event) = self.ready.pop_front() {
            return Some(event);
        }
        self.next_completed().await
    }

    /// Return every event already ready in the same scheduler turn.
    pub async fn next_events(&mut self) -> Vec<BoundaryEvent<SendRenderedBoundary>> {
        let Some(first) = self.next_event().await else {
            return Vec::new();
        };
        let mut events = Vec::with_capacity(1 + self.ready.len());
        events.push(first);
        events.extend(self.ready.drain(..));
        events
    }
}

impl Default for OutOfOrderBoundaryDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl OutOfOrderBoundaryDriver {
    pub fn new() -> Self {
        Self {
            in_flight: FuturesUnordered::new(),
            scheduled: BTreeMap::new(),
            ready: VecDeque::new(),
            deadlines: DeadlineIndex::new(),
        }
    }

    /// Admit a boundary, starting its timeout window at `now`.
    pub fn push(&mut self, boundary: PendingBoundary, now: Instant) {
        let PendingBoundary {
            id,
            future,
            priority,
            timeout,
        } = boundary;
        let deadline = timeout.map(|duration| {
            // An unrepresentable deadline must not turn a requested timeout
            // into an unbounded wait. Expire it at admission instead.
            now.checked_add(duration).unwrap_or(now)
        });
        let (abort, registration) = AbortHandle::new_pair();
        if let Some(previous) = self.scheduled.insert(
            id,
            ScheduledBoundary {
                priority,
                deadline,
                abort,
            },
        ) {
            // Stream recorders issue unique ids. Stay safe if a custom
            // recorder violates that contract by cancelling the older work.
            previous.abort.abort();
        }
        if let Some(deadline) = deadline {
            self.deadlines.insert(deadline, id, &self.scheduled);
        }
        self.in_flight.push(Box::pin(async move {
            FinishedBoundary {
                id,
                priority,
                result: Abortable::new(future, registration).await,
            }
        }));
    }

    pub fn extend<I>(&mut self, boundaries: I, now: Instant)
    where
        I: IntoIterator<Item = PendingBoundary>,
    {
        for boundary in boundaries {
            self.push(boundary, now);
        }
    }

    /// Earliest active deadline, if any.
    pub fn next_deadline(&mut self) -> Option<Instant> {
        self.deadlines.next_deadline(&self.scheduled)
    }

    /// Cancel every boundary whose deadline is at or before `now`.
    ///
    /// Expired events use the same deterministic priority/id order as ready
    /// completions. Dropping the aborted future prevents a stalled child from
    /// keeping the stream open.
    pub fn expire(&mut self, now: Instant) -> Vec<BoundaryEvent> {
        let mut expired: Vec<(Priority, u64)> = self
            .deadlines
            .take_expired(now, &self.scheduled)
            .into_iter()
            .filter_map(|id| {
                self.scheduled
                    .get(&id)
                    .map(|boundary| (boundary.priority, id))
            })
            .collect();
        expired.sort_by_key(|(priority, id)| (priority_rank(*priority), *id));
        expired
            .into_iter()
            .filter_map(|(_, id)| {
                let boundary = self.scheduled.remove(&id)?;
                boundary.abort.abort();
                Some(BoundaryEvent::TimedOut { id })
            })
            .collect()
    }

    /// Whether any completion or timeout event can still be produced.
    pub fn has_pending(&self) -> bool {
        !self.ready.is_empty() || !self.scheduled.is_empty()
    }

    /// Return the next completed boundary, batching every result ready in the
    /// same poll turn before applying the priority tie-break.
    pub async fn next_completed(&mut self) -> Option<BoundaryEvent> {
        if let Some(event) = self.ready.pop_front() {
            return Some(event);
        }

        loop {
            let first = self.in_flight.next().await?;
            let mut finished = vec![first];
            while let Some(Some(next)) = self.in_flight.next().now_or_never() {
                finished.push(next);
            }
            finished.sort_by_key(|boundary| (priority_rank(boundary.priority), boundary.id));

            for boundary in finished {
                let Ok(rendered) = boundary.result else {
                    // `expire` already emitted the timeout event.
                    continue;
                };
                if self.scheduled.remove(&boundary.id).is_some() {
                    self.ready.push_back(BoundaryEvent::Finished {
                        id: boundary.id,
                        rendered,
                    });
                }
            }

            if let Some(event) = self.ready.pop_front() {
                return Some(event);
            }
        }
    }

    /// Drive one event with the default Tokio clock adapter.
    #[cfg(feature = "rt-tokio")]
    pub async fn next_event(&mut self) -> Option<BoundaryEvent> {
        enum Wake {
            Completed(Option<BoundaryEvent>),
            Deadline,
        }

        loop {
            if let Some(event) = self.ready.pop_front() {
                return Some(event);
            }
            if !self.has_pending() {
                return None;
            }

            let Some(deadline) = self.next_deadline() else {
                return self.next_completed().await;
            };
            let wake = {
                let completed = self.next_completed();
                tokio::pin!(completed);
                tokio::select! {
                    // A boundary already ready at its deadline wins over
                    // cancellation; ready peers are then priority-sorted.
                    biased;
                    event = &mut completed => Wake::Completed(event),
                    _ = tokio::time::sleep_until(deadline.into()) => Wake::Deadline,
                }
            };

            match wake {
                Wake::Completed(Some(event)) => return Some(event),
                Wake::Completed(None) => continue,
                Wake::Deadline => {
                    let expired = self.expire(Instant::now());
                    self.ready.extend(expired);
                }
            }
        }
    }

    /// A portable build has no clock adapter. It conservatively expires every
    /// configured timeout at admission, so a timeout can never become an
    /// unbounded wait. Hosts that want elapsed-time semantics should instead
    /// drive `next_deadline`/`expire` with their runtime clock.
    #[cfg(not(feature = "rt-tokio"))]
    pub async fn next_event(&mut self) -> Option<BoundaryEvent> {
        if let Some(event) = self.ready.pop_front() {
            return Some(event);
        }
        let mut timed = Vec::new();
        for (&id, boundary) in &self.scheduled {
            if boundary.deadline.is_some() {
                timed.push((boundary.priority, id));
            }
        }
        timed.sort_by_key(|(priority, id)| (priority_rank(*priority), *id));
        for (_, id) in timed {
            if let Some(boundary) = self.scheduled.remove(&id) {
                boundary.abort.abort();
                self.ready.push_back(BoundaryEvent::TimedOut { id });
            }
        }
        if let Some(event) = self.ready.pop_front() {
            return Some(event);
        }
        self.next_completed().await
    }

    /// Return every event already ready in the same scheduler turn.
    ///
    /// No batching timer is used: this only drains completions discovered by
    /// the same poll that produced the first event.
    pub async fn next_events(&mut self) -> Vec<BoundaryEvent> {
        let Some(first) = self.next_event().await else {
            return Vec::new();
        };
        let mut events = Vec::with_capacity(1 + self.ready.len());
        events.push(first);
        events.extend(self.ready.drain(..));
        events
    }
}

const fn priority_rank(priority: Priority) -> u8 {
    match priority {
        Priority::Critical => 0,
        Priority::Optional => 1,
        Priority::Defer => 2,
    }
}

/// One transport-framed out-of-order boundary and its committed browser facts.
///
/// Successful boundaries carry the exact [`RenderArtifacts`] committed by the
/// subtree that produced `bytes`. Render failures and timeouts carry an empty
/// artifact snapshot because their speculative subtree is not published.
///
/// A framework adapter must consume the artifacts before sending `bytes`:
/// serialize the boundary query-seed fragment and response-local
/// `PageRuntimePlan` delta into a metadata prefix, then publish that prefix
/// immediately **before** `bytes`. The swap script at the end of `bytes`
/// inserts the boundary DOM and dispatches `proa:boundary-ready`; metadata
/// appended after the frame arrives too late for that event. Keeping the
/// fields together prevents boundary-only islands from losing their hydration
/// assets or observing them after hydration starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutOfOrderBoundaryChunk {
    /// Boundary id used by the swap/timeout transport frame.
    pub id: u64,
    /// Complete swap/timeout transport bytes for this boundary.
    ///
    /// Any React bootstrap, query seed, SharedState seed, or runtime-plan
    /// metadata derived from `artifacts` must be emitted before these bytes,
    /// never
    /// appended after them.
    pub bytes: Vec<u8>,
    /// Committed artifacts belonging to the published boundary HTML.
    pub artifacts: RenderArtifacts,
}

/// Drain pending boundaries without a CSP nonce while preserving artifacts.
pub async fn drain_pending_boundaries_with_artifacts<F, Fut>(stream: &StreamContext, emit: F)
where
    F: FnMut(OutOfOrderBoundaryChunk) -> Fut,
    Fut: Future<Output = bool>,
{
    drain_pending_boundaries_with_nonce_and_artifacts(stream, None, emit).await;
}

/// Drain pending boundaries with a CSP nonce while preserving artifacts.
///
/// The callback receives one structured chunk per successful, failed, or
/// timed-out boundary. Only a successful rendered boundary carries non-empty
/// `artifacts`. The callback must write the corresponding React bootstrap,
/// query seed, SharedState seed, and runtime-plan metadata immediately before
/// `chunk.bytes`; writing it after the swap frame is too late because that
/// frame dispatches `proa:boundary-ready`.
pub async fn drain_pending_boundaries_with_nonce_and_artifacts<F, Fut>(
    stream: &StreamContext,
    nonce: Option<&CspNonce>,
    mut emit: F,
) where
    F: FnMut(OutOfOrderBoundaryChunk) -> Fut,
    Fut: Future<Output = bool>,
{
    let mut driver = OutOfOrderBoundaryDriver::new();
    let mut pending_boundaries = Vec::new();

    loop {
        stream.drain_boundaries_into(&mut pending_boundaries);
        driver.extend(pending_boundaries.drain(..), Instant::now());
        let Some(event) = driver.next_event().await else {
            break;
        };

        let chunk = match event {
            BoundaryEvent::Finished {
                id,
                rendered: Ok(rendered),
            } => {
                let RenderedBoundary {
                    html,
                    artifacts,
                    nested,
                } = rendered;
                pending_boundaries.extend(nested);
                OutOfOrderBoundaryChunk {
                    id,
                    bytes: format_boundary_buffer_chunk_with_nonce(id, &html, nonce),
                    artifacts,
                }
            }
            BoundaryEvent::Finished {
                id,
                rendered: Err(err),
            } => {
                error!(
                    boundary_id = id,
                    ?err,
                    "uncaught streaming boundary failed; blanking boundary"
                );
                OutOfOrderBoundaryChunk {
                    id,
                    bytes: format_boundary_chunk_with_nonce(id, &[], nonce),
                    artifacts: RenderArtifacts::default(),
                }
            }
            BoundaryEvent::TimedOut { id } => {
                error!(
                    boundary_id = id,
                    "streaming boundary timed out; cancelling and preserving fallback"
                );
                OutOfOrderBoundaryChunk {
                    id,
                    bytes: format_boundary_timeout_chunk(id),
                    artifacts: RenderArtifacts::default(),
                }
            }
        };
        if !emit(chunk).await {
            return;
        }
    }
}

/// Drain pending boundaries and emit transport bytes only.
///
/// This compatibility helper discards every successful boundary's rendered
/// island ids, query seeds, and SharedState seeds. It is incorrect for
/// responses where deferred boundaries may render client work; use
/// [`drain_pending_boundaries_with_artifacts`] instead.
#[deprecated(
    since = "0.3.0",
    note = "drops committed RSJS island ids, query seeds, and SharedState seeds; use drain_pending_boundaries_with_artifacts"
)]
pub async fn drain_pending_boundaries<F, Fut>(stream: &StreamContext, mut emit: F)
where
    F: FnMut(Vec<u8>) -> Fut,
    Fut: Future<Output = bool>,
{
    drain_pending_boundaries_with_artifacts(stream, |chunk| emit(chunk.bytes)).await;
}

/// Drain pending boundaries with a nonce and emit transport bytes only.
///
/// This compatibility helper applies the nonce but discards every successful
/// boundary's rendered island ids, query seeds, and SharedState seeds. Use
/// [`drain_pending_boundaries_with_nonce_and_artifacts`] for response
/// finalization.
#[deprecated(
    since = "0.3.0",
    note = "drops committed RSJS island ids, query seeds, and SharedState seeds; use drain_pending_boundaries_with_nonce_and_artifacts"
)]
pub async fn drain_pending_boundaries_with_nonce<F, Fut>(
    stream: &StreamContext,
    nonce: Option<&CspNonce>,
    mut emit: F,
) where
    F: FnMut(Vec<u8>) -> Fut,
    Fut: Future<Output = bool>,
{
    drain_pending_boundaries_with_nonce_and_artifacts(stream, nonce, |chunk| emit(chunk.bytes))
        .await;
}

pub fn format_boundary_chunk(id: u64, html: &[u8]) -> Vec<u8> {
    format_boundary_chunk_with_nonce(id, html, None)
}

const BOUNDARY_TEMPLATE_OPEN: &[u8] = b"<template id=\"proa-tpl-";
const BOUNDARY_TEMPLATE_ENCODING: &[u8] = b"\" data-proa-encoding=\"base64-v1\">";
const BOUNDARY_TEMPLATE_CLOSE: &[u8] = b"</template>";
const BOUNDARY_SCRIPT_OPEN: &[u8] = b"<script";
const BOUNDARY_SWAP_OPEN: &[u8] = b">$proa(";
const BOUNDARY_SWAP_CLOSE: &[u8] = b");</script>";
const BOUNDARY_BATCH_OPEN: &[u8] = b">$proaBatch([";
const BOUNDARY_BATCH_SEPARATOR: &[u8] = b",";
const BOUNDARY_BATCH_CLOSE: &[u8] = b"]);</script>";
const MAX_U64_DECIMAL_BYTES: usize = 20;
const NONCE_ATTR_OVERHEAD: usize = b" nonce=\"\"".len();

/// Format one out-of-order boundary swap with the request's CSP nonce.
pub fn format_boundary_chunk_with_nonce(id: u64, html: &[u8], nonce: Option<&CspNonce>) -> Vec<u8> {
    let mut out = Vec::new();
    append_boundary_chunk_with_nonce(&mut out, id, html, nonce);
    out
}

/// Append one contiguous boundary frame without a temporary allocation.
pub fn append_boundary_chunk_with_nonce(
    out: &mut Vec<u8>,
    id: u64,
    html: &[u8],
    nonce: Option<&CspNonce>,
) {
    append_boundary_template_slices(out, id, html.len(), std::iter::once(html));
    append_boundary_swap_script_with_nonce(out, id, nonce);
}

/// Format a paged render buffer without flattening its HTML before Base64.
pub fn format_boundary_buffer_chunk_with_nonce(
    id: u64,
    html: &RenderBuffer,
    nonce: Option<&CspNonce>,
) -> Vec<u8> {
    let mut out = Vec::new();
    append_boundary_buffer_chunk_with_nonce(&mut out, id, html, nonce);
    out
}

/// Append one boundary frame directly into an existing response batch.
pub fn append_boundary_buffer_chunk_with_nonce(
    out: &mut Vec<u8>,
    id: u64,
    html: &RenderBuffer,
    nonce: Option<&CspNonce>,
) {
    append_boundary_buffer_template(out, id, html);
    append_boundary_swap_script_with_nonce(out, id, nonce);
}

/// Append a paged boundary template without an application script.
///
/// Response drivers use this for a same-ready batch, then append one
/// [`append_boundary_batch_script_with_nonce`] after all templates.
pub fn append_boundary_buffer_template(out: &mut Vec<u8>, id: u64, html: &RenderBuffer) {
    append_boundary_template_slices(out, id, html.len(), html.slices());
}

/// Append a contiguous boundary template without an application script.
pub fn append_boundary_template(out: &mut Vec<u8>, id: u64, html: &[u8]) {
    append_boundary_template_slices(out, id, html.len(), std::iter::once(html));
}

fn append_boundary_template_slices<'a>(
    out: &mut Vec<u8>,
    id: u64,
    html_len: usize,
    slices: impl IntoIterator<Item = &'a [u8]>,
) {
    let payload_len = encoded_len(html_len, true).expect("boundary payload is too large to encode");
    let additional = payload_len
        .checked_add(BOUNDARY_TEMPLATE_OPEN.len())
        .and_then(|len| len.checked_add(MAX_U64_DECIMAL_BYTES))
        .and_then(|len| len.checked_add(BOUNDARY_TEMPLATE_ENCODING.len()))
        .and_then(|len| len.checked_add(BOUNDARY_TEMPLATE_CLOSE.len()))
        .expect("boundary chunk is too large to allocate");
    out.reserve(additional);
    {
        let mut cx = WebContext::with_stateless_buffer(&mut *out);
        let _ = WriteBuf::extend_static(&mut cx.out, BOUNDARY_TEMPLATE_OPEN);
        let _ = WebRenderSync::render(id, &mut cx);
        let _ = WriteBuf::extend_static(&mut cx.out, BOUNDARY_TEMPLATE_ENCODING);
    }

    let payload_start = out.len();
    {
        let mut encoder = base64::write::EncoderWriter::new(&mut *out, &STANDARD);
        for slice in slices {
            encoder
                .write_all(slice)
                .expect("writing Base64 into a Vec cannot fail");
        }
        encoder
            .finish()
            .expect("finishing Base64 into a Vec cannot fail");
    }
    debug_assert_eq!(out.len() - payload_start, payload_len);

    {
        let mut cx = WebContext::with_stateless_buffer(&mut *out);
        let _ = WriteBuf::extend_static(&mut cx.out, BOUNDARY_TEMPLATE_CLOSE);
    }
}

fn append_boundary_swap_script_with_nonce(out: &mut Vec<u8>, id: u64, nonce: Option<&CspNonce>) {
    let nonce_len = nonce.map_or(0, |nonce| NONCE_ATTR_OVERHEAD + nonce.as_str().len());
    out.reserve(
        BOUNDARY_SCRIPT_OPEN.len()
            + nonce_len
            + BOUNDARY_SWAP_OPEN.len()
            + MAX_U64_DECIMAL_BYTES
            + BOUNDARY_SWAP_CLOSE.len(),
    );
    let mut cx = WebContext::with_stateless_buffer(&mut *out);
    let _ = WriteBuf::extend_static(&mut cx.out, BOUNDARY_SCRIPT_OPEN);
    if let Some(nonce) = nonce {
        let _ = nonce.write_html_attr(cx.out);
    }
    let _ = WriteBuf::extend_static(&mut cx.out, BOUNDARY_SWAP_OPEN);
    let _ = WebRenderSync::render(id, &mut cx);
    let _ = WriteBuf::extend_static(&mut cx.out, BOUNDARY_SWAP_CLOSE);
}

/// Append one browser application call for a same-ready server batch.
/// An empty id list emits nothing.
pub fn append_boundary_batch_script_with_nonce(
    out: &mut Vec<u8>,
    ids: &[u64],
    nonce: Option<&CspNonce>,
) {
    if ids.is_empty() {
        return;
    }
    let nonce_len = nonce.map_or(0, |nonce| NONCE_ATTR_OVERHEAD + nonce.as_str().len());
    let ids_len = ids
        .len()
        .checked_mul(MAX_U64_DECIMAL_BYTES + BOUNDARY_BATCH_SEPARATOR.len())
        .expect("boundary batch is too large to allocate");
    out.reserve(
        BOUNDARY_SCRIPT_OPEN.len()
            + nonce_len
            + BOUNDARY_BATCH_OPEN.len()
            + ids_len
            + BOUNDARY_BATCH_CLOSE.len(),
    );
    let mut cx = WebContext::with_stateless_buffer(&mut *out);
    let _ = WriteBuf::extend_static(&mut cx.out, BOUNDARY_SCRIPT_OPEN);
    if let Some(nonce) = nonce {
        let _ = nonce.write_html_attr(cx.out);
    }
    let _ = WriteBuf::extend_static(&mut cx.out, BOUNDARY_BATCH_OPEN);
    for (index, id) in ids.iter().copied().enumerate() {
        if index != 0 {
            let _ = WriteBuf::extend_static(&mut cx.out, BOUNDARY_BATCH_SEPARATOR);
        }
        let _ = WebRenderSync::render(id, &mut cx);
    }
    let _ = WriteBuf::extend_static(&mut cx.out, BOUNDARY_BATCH_CLOSE);
}

/// Format the observable timeout marker for one boundary.
///
/// Unlike an uncaught render error, a timeout preserves the fallback already
/// emitted in the shell. No swap is sent.
pub fn format_boundary_timeout_chunk(id: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    append_boundary_timeout_chunk(&mut out, id);
    out
}

/// Append one observable timeout marker to an existing response batch.
pub fn append_boundary_timeout_chunk(out: &mut Vec<u8>, id: u64) {
    {
        let mut cx = WebContext::with_stateless_buffer(&mut *out);
        let _ = WriteBuf::extend_static(&mut cx.out, b"<!--proa:timeout:");
        let _ = WebRenderSync::render(id, &mut cx);
        let _ = WriteBuf::extend_static(&mut cx.out, b"-->");
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use base64::Engine as _;
    use futures::future;
    use proa_core::{RenderMode, RsjsRenderArtifacts, StreamBoundaryRecorder};

    use super::*;

    struct StartGate {
        id: u64,
        started: Rc<Cell<usize>>,
        expected: usize,
        admitted: bool,
    }

    impl Future for StartGate {
        type Output = Result<RenderedBoundary, WriteError>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if !self.admitted {
                self.admitted = true;
                self.started.set(self.started.get() + 1);
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            if self.started.get() == self.expected {
                Poll::Ready(Ok(RenderedBoundary::html(self.id.to_string().into_bytes())))
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    fn artifacts_with_island(asset_id: &'static str) -> RsjsRenderArtifacts {
        let mut out = Vec::new();
        let cx = WebContext::with_buffer(&mut out);
        cx.rsjs_island_used(asset_id);
        cx.rsjs_artifacts_snapshot()
    }

    #[tokio::test]
    async fn artifact_aware_drain_keeps_boundary_artifacts_with_nonce_framing() {
        let stream = StreamContext::new(RenderMode::ChunkedOutOfOrder);
        let artifacts = artifacts_with_island("deferred_counter_12345678");
        let mut boundary_html = RenderBuffer::new();
        boundary_html.extend(b"<button>ready</button>").unwrap();
        stream.push_boundary(PendingBoundary::new(
            11,
            Box::pin(future::ready(Ok(RenderedBoundary::new(
                boundary_html,
                RenderArtifacts {
                    rsjs: artifacts.clone(),
                    react_islands: vec!["DeferredChart".to_owned()],
                },
            )))),
        ));
        let nonce = CspNonce::new("Ym91bmRhcnktYXJ0aWZhY3Rz").unwrap();
        let mut chunks = Vec::new();

        drain_pending_boundaries_with_nonce_and_artifacts(&stream, Some(&nonce), |chunk| {
            chunks.push(chunk);
            future::ready(true)
        })
        .await;

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].id, 11);
        assert_eq!(chunks[0].artifacts.rsjs, artifacts);
        assert_eq!(chunks[0].artifacts.react_islands, ["DeferredChart"]);
        let island_id = chunks[0].artifacts.rsjs.rendered_islands().next().unwrap();
        let mut ordered_wire = format!("<!--rsjs-metadata:{island_id}-->").into_bytes();
        let metadata_end = ordered_wire.len();
        ordered_wire.extend_from_slice(&chunks[0].bytes);
        let wire = String::from_utf8(ordered_wire).unwrap();
        assert_eq!(
            &wire[..metadata_end],
            "<!--rsjs-metadata:deferred_counter_12345678-->"
        );
        assert!(wire[metadata_end..].starts_with("<template"));
        assert!(wire.contains(r#"nonce="Ym91bmRhcnktYXJ0aWZhY3Rz""#));
    }

    #[tokio::test]
    async fn failed_boundary_chunk_carries_no_speculative_artifacts() {
        let stream = StreamContext::new(RenderMode::ChunkedOutOfOrder);
        stream.push_boundary(PendingBoundary::new(
            13,
            Box::pin(future::ready(Err(WriteError::render("failed")))),
        ));
        let mut chunks = Vec::new();

        drain_pending_boundaries_with_artifacts(&stream, |chunk| {
            chunks.push(chunk);
            future::ready(true)
        })
        .await;

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].id, 13);
        assert!(chunks[0].artifacts.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&chunks[0].bytes),
            r#"<template id="proa-tpl-13" data-proa-encoding="base64-v1"></template><script>$proa(13);</script>"#
        );
    }

    #[test]
    fn boundary_swap_uses_the_request_nonce() {
        let nonce = CspNonce::new("Ym91bmRhcnktbm9uY2U=").unwrap();
        let chunk = format_boundary_chunk_with_nonce(7, b"<strong>ready</strong>", Some(&nonce));
        assert_eq!(
            String::from_utf8(chunk).unwrap(),
            r#"<template id="proa-tpl-7" data-proa-encoding="base64-v1">PHN0cm9uZz5yZWFkeTwvc3Ryb25nPg==</template><script nonce="Ym91bmRhcnktbm9uY2U=">$proa(7);</script>"#
        );
    }

    #[test]
    fn boundary_swap_without_nonce_uses_versioned_base64_transport() {
        assert_eq!(
            format_boundary_chunk_with_nonce(7, b"ready", None),
            br#"<template id="proa-tpl-7" data-proa-encoding="base64-v1">cmVhZHk=</template><script>$proa(7);</script>"#
        );
    }

    #[test]
    fn same_ready_templates_share_one_nonced_application_script() {
        let nonce = CspNonce::new("YmF0Y2gtbm9uY2U=").unwrap();
        let mut first = RenderBuffer::new();
        first.extend(b"first").unwrap();
        let mut second = RenderBuffer::new();
        second.extend(b"second").unwrap();
        let mut batch = Vec::new();
        append_boundary_buffer_template(&mut batch, 7, &first);
        append_boundary_buffer_template(&mut batch, 3, &second);
        append_boundary_batch_script_with_nonce(&mut batch, &[7, 3], Some(&nonce));

        let batch = String::from_utf8(batch).unwrap();
        assert_eq!(batch.matches("<script").count(), 1);
        assert!(batch.ends_with(r#"<script nonce="YmF0Y2gtbm9uY2U=">$proaBatch([7,3]);</script>"#));
    }

    #[test]
    fn paged_boundary_encoding_does_not_flatten_or_corrupt_page_edges() {
        let expected = (0..(proa_core::STANDARD_PAGE_SIZE * 3 + 17))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let mut html = RenderBuffer::new();
        html.extend(&expected).unwrap();
        assert!(html.slices().count() >= 4);

        let chunk = format_boundary_buffer_chunk_with_nonce(5, &html, None);
        let payload_start = chunk
            .windows(BOUNDARY_TEMPLATE_ENCODING.len())
            .position(|window| window == BOUNDARY_TEMPLATE_ENCODING)
            .unwrap()
            + BOUNDARY_TEMPLATE_ENCODING.len();
        let payload_end = chunk[payload_start..]
            .windows(BOUNDARY_TEMPLATE_CLOSE.len())
            .position(|window| window == BOUNDARY_TEMPLATE_CLOSE)
            .unwrap()
            + payload_start;
        assert_eq!(
            STANDARD.decode(&chunk[payload_start..payload_end]).unwrap(),
            expected
        );
    }

    #[test]
    fn boundary_transport_round_trips_unicode_and_adversarial_bytes() {
        let html = concat!(
            "<article>Grüße, 東京 🌊",
            "<template><p>balanced</p></template>",
            "</template><div>lowercase escape attempt</div>",
            "</TeMpLaTe ><div>mixed-case escape attempt</div>",
            "</template\t><div>tab escape attempt</div>",
            "\0</article>"
        )
        .as_bytes();
        let chunk = format_boundary_chunk(u64::MAX, html);
        let payload_start = chunk
            .windows(BOUNDARY_TEMPLATE_ENCODING.len())
            .position(|window| window == BOUNDARY_TEMPLATE_ENCODING)
            .unwrap()
            + BOUNDARY_TEMPLATE_ENCODING.len();
        let payload_end = chunk
            .windows(BOUNDARY_TEMPLATE_CLOSE.len())
            .position(|window| window == BOUNDARY_TEMPLATE_CLOSE)
            .unwrap();
        let encoded = &chunk[payload_start..payload_end];

        assert!(encoded
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')));
        assert!(!encoded.contains(&b'<'));
        assert_eq!(STANDARD.decode(encoded).unwrap(), html);
    }

    #[tokio::test]
    async fn ready_boundaries_use_priority_then_id_without_serializing_start() {
        let now = Instant::now();
        let mut driver = OutOfOrderBoundaryDriver::new();
        let started = Rc::new(Cell::new(0));
        for (id, priority) in [
            (4, Priority::Defer),
            (3, Priority::Critical),
            (2, Priority::Optional),
            (1, Priority::Critical),
        ] {
            driver.push(
                PendingBoundary::with_options(
                    id,
                    Box::pin(StartGate {
                        id,
                        started: Rc::clone(&started),
                        expected: 4,
                        admitted: false,
                    }),
                    priority,
                    None,
                ),
                now,
            );
        }

        let mut ids = Vec::new();
        while let Some(event) = driver.next_completed().await {
            let BoundaryEvent::Finished { id, rendered } = event else {
                panic!("ready boundary unexpectedly timed out");
            };
            assert!(rendered.is_ok());
            ids.push(id);
        }

        assert_eq!(ids, [1, 3, 2, 4]);
        assert_eq!(started.get(), 4);
    }

    #[test]
    fn expiry_is_deadline_data_and_cancels_stalled_work() {
        let now = Instant::now();
        let mut driver = OutOfOrderBoundaryDriver::new();
        driver.push(
            PendingBoundary::with_options(
                9,
                Box::pin(future::pending()),
                Priority::Optional,
                Some(Duration::from_secs(5)),
            ),
            now,
        );

        assert_eq!(
            driver.next_deadline(),
            now.checked_add(Duration::from_secs(5))
        );
        assert!(driver
            .expire(now.checked_add(Duration::from_secs(4)).unwrap())
            .is_empty());
        let expired = driver.expire(now.checked_add(Duration::from_secs(5)).unwrap());
        assert_eq!(expired.len(), 1);
        assert!(matches!(expired[0], BoundaryEvent::TimedOut { id: 9 }));
        assert!(!driver.has_pending());
    }

    #[test]
    fn deadline_index_preserves_order_above_the_heap_crossover() {
        let now = Instant::now();
        let mut driver = OutOfOrderBoundaryDriver::new();
        for id in 1..=(DEADLINE_HEAP_THRESHOLD as u64 + 8) {
            let (priority, timeout) = match id {
                1 => (Priority::Defer, Duration::from_millis(1)),
                2 => (Priority::Critical, Duration::from_millis(1)),
                3 => (Priority::Optional, Duration::from_millis(1)),
                _ => (Priority::Critical, Duration::from_secs(1)),
            };
            driver.push(
                PendingBoundary::with_options(
                    id,
                    Box::pin(future::pending()),
                    priority,
                    Some(timeout),
                ),
                now,
            );
        }

        assert_eq!(
            driver.next_deadline(),
            now.checked_add(Duration::from_millis(1))
        );
        let expired = driver.expire(now.checked_add(Duration::from_millis(1)).unwrap());
        let ids = expired
            .into_iter()
            .map(|event| match event {
                BoundaryEvent::TimedOut { id } => id,
                BoundaryEvent::Finished { .. } => panic!("pending boundary unexpectedly finished"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, [2, 3, 1]);
        assert_eq!(
            driver.next_deadline(),
            now.checked_add(Duration::from_secs(1))
        );
    }

    #[cfg(not(feature = "rt-tokio"))]
    #[tokio::test]
    async fn portable_adapter_expires_timed_work_at_admission() {
        let mut driver = OutOfOrderBoundaryDriver::new();
        driver.push(
            PendingBoundary::with_options(
                9,
                Box::pin(future::pending()),
                Priority::Optional,
                Some(Duration::from_secs(5)),
            ),
            Instant::now(),
        );

        assert!(matches!(
            driver.next_event().await,
            Some(BoundaryEvent::TimedOut { id: 9 })
        ));
        assert!(!driver.has_pending());
    }

    #[test]
    fn timeout_marker_preserves_fallback_without_a_script_or_swap() {
        let chunk = format_boundary_timeout_chunk(7);
        assert_eq!(String::from_utf8(chunk).unwrap(), "<!--proa:timeout:7-->");
    }
}
