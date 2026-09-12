// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Tokio-backed boundary supervision for framework response streams.
//!
//! Untimed boundaries stay inline in the response driver's
//! `FuturesUnordered`. A timed boundary is owned by a runtime task, so its
//! timeout can still fire and drop its child future while the HTTP body is
//! backpressured. The local variant uses `spawn_local` and is enabled only on
//! pinned workers that explicitly enter [`LocalBoundarySupervisionGuard`].

use std::cell::Cell;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::stream::{FuturesUnordered, StreamExt};
use futures::FutureExt;
use proa_core::{
    PendingBoundary, Priority, RenderedBoundary, SendPendingBoundary, SendRenderedBoundary,
    WriteError,
};

use super::out_of_order::{BoundaryEvent, OutOfOrderBoundaryDriver, SendOutOfOrderBoundaryDriver};

thread_local! {
    static LOCAL_SUPERVISION_ENABLED: Cell<bool> = const { Cell::new(false) };
}

/// Marks a dedicated local-executor thread as safe for `spawn_local` boundary
/// supervision. Restores the previous value when dropped.
pub struct LocalBoundarySupervisionGuard {
    previous: bool,
}

impl LocalBoundarySupervisionGuard {
    pub fn enter() -> Self {
        let previous = LOCAL_SUPERVISION_ENABLED.with(|enabled| enabled.replace(true));
        Self { previous }
    }
}

impl Drop for LocalBoundarySupervisionGuard {
    fn drop(&mut self) {
        LOCAL_SUPERVISION_ENABLED.with(|enabled| enabled.set(self.previous));
    }
}

fn local_supervision_enabled() -> bool {
    LOCAL_SUPERVISION_ENABLED.with(Cell::get)
}

enum TaskOutcome<R> {
    Finished(Result<R, WriteError>),
    TimedOut,
}

struct CompletedTask<R> {
    id: u64,
    priority: Priority,
    outcome: TaskOutcome<R>,
}

struct SupervisedTask<R> {
    id: u64,
    priority: Priority,
    handle: tokio::task::JoinHandle<TaskOutcome<R>>,
}

impl<R> Future for SupervisedTask<R> {
    type Output = CompletedTask<R>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.handle).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(outcome)) => Poll::Ready(CompletedTask {
                id: self.id,
                priority: self.priority,
                outcome,
            }),
            Poll::Ready(Err(error)) => Poll::Ready(CompletedTask {
                id: self.id,
                priority: self.priority,
                outcome: TaskOutcome::Finished(Err(WriteError::render(format!(
                    "boundary task failed: {error}"
                )))),
            }),
        }
    }
}

impl<R> Drop for SupervisedTask<R> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

struct SupervisedTasks<R> {
    in_flight: FuturesUnordered<SupervisedTask<R>>,
}

impl<R> SupervisedTasks<R> {
    fn new() -> Self {
        Self {
            in_flight: FuturesUnordered::new(),
        }
    }

    fn push(
        &mut self,
        id: u64,
        priority: Priority,
        handle: tokio::task::JoinHandle<TaskOutcome<R>>,
    ) {
        self.in_flight.push(SupervisedTask {
            id,
            priority,
            handle,
        });
    }

    fn is_empty(&self) -> bool {
        self.in_flight.is_empty()
    }

    async fn next_events(&mut self) -> Vec<BoundaryEvent<R>> {
        let Some(first) = self.in_flight.next().await else {
            return Vec::new();
        };
        let mut completed = Vec::with_capacity(1);
        completed.push(first);
        while let Some(Some(next)) = self.in_flight.next().now_or_never() {
            completed.push(next);
        }
        completed.sort_by_key(|task| (priority_rank(task.priority), task.id));
        completed
            .into_iter()
            .map(|task| match task.outcome {
                TaskOutcome::Finished(rendered) => BoundaryEvent::Finished {
                    id: task.id,
                    rendered,
                },
                TaskOutcome::TimedOut => BoundaryEvent::TimedOut { id: task.id },
            })
            .collect()
    }
}

async fn supervise<R>(
    future: impl Future<Output = Result<R, WriteError>>,
    timeout: Option<std::time::Duration>,
) -> TaskOutcome<R> {
    let future = std::panic::AssertUnwindSafe(future).catch_unwind();
    match timeout {
        Some(timeout) => {
            tokio::select! {
                biased;
                result = future => match result {
                    Ok(rendered) => TaskOutcome::Finished(rendered),
                    Err(_) => TaskOutcome::Finished(Err(WriteError::render("boundary task panicked"))),
                },
                _ = tokio::time::sleep(timeout) => TaskOutcome::TimedOut,
            }
        }
        None => match future.await {
            Ok(rendered) => TaskOutcome::Finished(rendered),
            Err(_) => TaskOutcome::Finished(Err(WriteError::render("boundary task panicked"))),
        },
    }
}

/// Local response driver that selects task supervision only on a pinned local
/// executor. Runtime-independent callers retain the inline polling driver.
enum RuntimeLocalDriver {
    Inline(OutOfOrderBoundaryDriver),
    Hybrid {
        inline: OutOfOrderBoundaryDriver,
        timed: SupervisedTasks<RenderedBoundary>,
        priorities: BTreeMap<u64, Priority>,
    },
}

pub struct RuntimeOutOfOrderBoundaryDriver {
    inner: RuntimeLocalDriver,
}

impl RuntimeOutOfOrderBoundaryDriver {
    pub fn new() -> Self {
        let inner = if local_supervision_enabled() {
            RuntimeLocalDriver::Hybrid {
                inline: OutOfOrderBoundaryDriver::new(),
                timed: SupervisedTasks::new(),
                priorities: BTreeMap::new(),
            }
        } else {
            RuntimeLocalDriver::Inline(OutOfOrderBoundaryDriver::new())
        };
        Self { inner }
    }

    pub fn extend<I>(&mut self, boundaries: I, now: std::time::Instant)
    where
        I: IntoIterator<Item = PendingBoundary>,
    {
        match &mut self.inner {
            RuntimeLocalDriver::Inline(driver) => driver.extend(boundaries, now),
            RuntimeLocalDriver::Hybrid {
                inline,
                timed,
                priorities,
            } => {
                for boundary in boundaries {
                    let PendingBoundary {
                        id,
                        future,
                        priority,
                        timeout,
                    } = boundary;
                    priorities.insert(id, priority);
                    if timeout.is_some() {
                        timed.push(
                            id,
                            priority,
                            tokio::task::spawn_local(supervise(future, timeout)),
                        );
                    } else {
                        inline.push(
                            PendingBoundary::with_options(id, future, priority, None),
                            now,
                        );
                    }
                }
            }
        }
    }

    pub async fn next_events(&mut self) -> Vec<BoundaryEvent> {
        match &mut self.inner {
            RuntimeLocalDriver::Inline(driver) => driver.next_events().await,
            RuntimeLocalDriver::Hybrid {
                inline,
                timed,
                priorities,
            } => {
                let mut events = if !inline.has_pending() {
                    timed.next_events().await
                } else if timed.is_empty() {
                    inline.next_events().await
                } else {
                    let inline_events = inline.next_events().fuse();
                    let timed_events = timed.next_events().fuse();
                    futures::pin_mut!(inline_events, timed_events);
                    match futures::future::select(inline_events, timed_events).await {
                        futures::future::Either::Left((mut events, other)) => {
                            if let Some(mut same_poll) = other.now_or_never() {
                                events.append(&mut same_poll);
                            }
                            events
                        }
                        futures::future::Either::Right((mut events, other)) => {
                            if let Some(mut same_poll) = other.now_or_never() {
                                events.append(&mut same_poll);
                            }
                            events
                        }
                    }
                };
                events.sort_by_key(|event| {
                    let id = boundary_event_id(event);
                    (
                        priorities.get(&id).copied().map_or(u8::MAX, priority_rank),
                        id,
                    )
                });
                for event in &events {
                    priorities.remove(&boundary_event_id(event));
                }
                events
            }
        }
    }
}

impl Default for RuntimeOutOfOrderBoundaryDriver {
    fn default() -> Self {
        Self::new()
    }
}

/// Send response driver. Within Tokio it supervises timed boundaries as tasks;
/// untimed boundaries and runtime-independent direct users use inline polling.
enum RuntimeSendDriver {
    Inline(SendOutOfOrderBoundaryDriver),
    Hybrid {
        inline: SendOutOfOrderBoundaryDriver,
        timed: SupervisedTasks<SendRenderedBoundary>,
        priorities: BTreeMap<u64, Priority>,
    },
}

pub struct RuntimeSendOutOfOrderBoundaryDriver {
    inner: RuntimeSendDriver,
}

impl RuntimeSendOutOfOrderBoundaryDriver {
    pub fn new() -> Self {
        let inner = if tokio::runtime::Handle::try_current().is_ok() {
            RuntimeSendDriver::Hybrid {
                inline: SendOutOfOrderBoundaryDriver::new(),
                timed: SupervisedTasks::new(),
                priorities: BTreeMap::new(),
            }
        } else {
            RuntimeSendDriver::Inline(SendOutOfOrderBoundaryDriver::new())
        };
        Self { inner }
    }

    pub fn extend<I>(&mut self, boundaries: I, now: std::time::Instant)
    where
        I: IntoIterator<Item = SendPendingBoundary>,
    {
        match &mut self.inner {
            RuntimeSendDriver::Inline(driver) => driver.extend(boundaries, now),
            RuntimeSendDriver::Hybrid {
                inline,
                timed,
                priorities,
            } => {
                for boundary in boundaries {
                    let SendPendingBoundary {
                        id,
                        future,
                        priority,
                        timeout,
                    } = boundary;
                    priorities.insert(id, priority);
                    if timeout.is_some() {
                        timed.push(id, priority, tokio::spawn(supervise(future, timeout)));
                    } else {
                        inline.push(
                            SendPendingBoundary::with_options(id, future, priority, None),
                            now,
                        );
                    }
                }
            }
        }
    }

    pub async fn next_events(&mut self) -> Vec<BoundaryEvent<SendRenderedBoundary>> {
        match &mut self.inner {
            RuntimeSendDriver::Inline(driver) => driver.next_events().await,
            RuntimeSendDriver::Hybrid {
                inline,
                timed,
                priorities,
            } => {
                let mut events = if !inline.has_pending() {
                    timed.next_events().await
                } else if timed.is_empty() {
                    inline.next_events().await
                } else {
                    let inline_events = inline.next_events().fuse();
                    let timed_events = timed.next_events().fuse();
                    futures::pin_mut!(inline_events, timed_events);
                    match futures::future::select(inline_events, timed_events).await {
                        futures::future::Either::Left((mut events, other)) => {
                            if let Some(mut same_poll) = other.now_or_never() {
                                events.append(&mut same_poll);
                            }
                            events
                        }
                        futures::future::Either::Right((mut events, other)) => {
                            if let Some(mut same_poll) = other.now_or_never() {
                                events.append(&mut same_poll);
                            }
                            events
                        }
                    }
                };
                events.sort_by_key(|event| {
                    let id = boundary_event_id(event);
                    (
                        priorities.get(&id).copied().map_or(u8::MAX, priority_rank),
                        id,
                    )
                });
                for event in &events {
                    priorities.remove(&boundary_event_id(event));
                }
                events
            }
        }
    }
}

impl Default for RuntimeSendOutOfOrderBoundaryDriver {
    fn default() -> Self {
        Self::new()
    }
}

const fn priority_rank(priority: Priority) -> u8 {
    match priority {
        Priority::Critical => 0,
        Priority::Optional => 1,
        Priority::Defer => 2,
    }
}

const fn boundary_event_id<R>(event: &BoundaryEvent<R>) -> u64 {
    match event {
        BoundaryEvent::Finished { id, .. } | BoundaryEvent::TimedOut { id } => *id,
    }
}
