// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::{stream, StreamExt};
use proa_framework_core::{FrameworkError, FrameworkLocalStreamFactory, FrameworkSendStream};
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tokio_stream::wrappers::ReceiverStream;

/// Resource limits for the pinned local streaming executor.
///
/// The pool owns exactly `worker_threads` long-lived threads. `max_in_flight`
/// bounds queued plus active responses, while both a chunk count and byte
/// budget bound each response's cross-thread transport. The renderer may hold
/// one not-yet-published chunk while waiting for byte capacity; when that
/// single chunk is larger than `response_buffer_bytes`, it reserves the whole
/// budget and remains the documented one-in-hand exception to the byte bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalStreamExecutorConfig {
    pub worker_threads: usize,
    pub max_in_flight: usize,
    pub response_buffer_bytes: usize,
    pub response_buffer_chunks: usize,
    pub cooperative_yield_interval: usize,
}

impl Default for LocalStreamExecutorConfig {
    fn default() -> Self {
        Self {
            worker_threads: 1,
            max_in_flight: 1_024,
            response_buffer_bytes: 1024 * 1024,
            response_buffer_chunks: 32,
            cooperative_yield_interval: 32,
        }
    }
}

impl LocalStreamExecutorConfig {
    fn validate(self) -> Result<Self, Arc<str>> {
        if self.worker_threads == 0 {
            return Err(Arc::from("local stream worker_threads must be non-zero"));
        }
        if self.max_in_flight == 0 {
            return Err(Arc::from("local stream max_in_flight must be non-zero"));
        }
        if self.response_buffer_bytes == 0 || self.response_buffer_bytes > u32::MAX as usize {
            return Err(Arc::from(
                "local stream response_buffer_bytes must be in 1..=u32::MAX",
            ));
        }
        if self.response_buffer_chunks == 0 {
            return Err(Arc::from(
                "local stream response_buffer_chunks must be non-zero",
            ));
        }
        if self.cooperative_yield_interval == 0 {
            return Err(Arc::from(
                "local stream cooperative_yield_interval must be non-zero",
            ));
        }
        Ok(self)
    }
}

#[derive(Debug)]
pub(super) enum LocalStreamPrepareError {
    Overloaded,
    Unavailable(Arc<str>),
    FirstItem(FrameworkError),
}

#[derive(Clone)]
pub(super) struct LocalStreamExecutor {
    inner: Result<Arc<PoolState>, Arc<str>>,
}

impl LocalStreamExecutor {
    pub(super) fn new(config: LocalStreamExecutorConfig) -> Self {
        let inner = config.validate().and_then(PoolState::start);
        Self { inner }
    }

    #[cfg(test)]
    pub(super) async fn prepare(
        &self,
        make: FrameworkLocalStreamFactory,
    ) -> Result<FrameworkSendStream, LocalStreamPrepareError> {
        self.prepare_with_head(make, None).await
    }

    pub(super) async fn prepare_with_head(
        &self,
        make: FrameworkLocalStreamFactory,
        head: Option<proa_framework_core::managed_response::PreparedHead>,
    ) -> Result<FrameworkSendStream, LocalStreamPrepareError> {
        let pool = self
            .inner
            .as_ref()
            .map_err(|error| LocalStreamPrepareError::Unavailable(Arc::clone(error)))?;
        let admission = match Arc::clone(&pool.admission).try_acquire_owned() {
            Ok(permit) => permit,
            Err(TryAcquireError::NoPermits) => {
                return Err(LocalStreamPrepareError::Overloaded);
            }
            Err(TryAcquireError::Closed) => {
                return Err(LocalStreamPrepareError::Unavailable(Arc::from(
                    "local stream admission is closed",
                )));
            }
        };

        let (first_tx, first_rx) = oneshot::channel();
        let (tail_tx, tail_rx) = mpsc::channel(pool.config.response_buffer_chunks);
        let byte_budget = ByteBudget::new(pool.config.response_buffer_bytes);
        let assigned_worker = pool.choose_worker();
        let job = LocalJob {
            id: pool.next_job_id.fetch_add(1, Ordering::Relaxed),
            make,
            head,
            first: first_tx,
            tail: tail_tx,
            byte_budget,
            admission,
            assigned_worker,
            cooperative_yield_interval: pool.config.cooperative_yield_interval,
        };

        pool.enqueue(job)?;
        let first = first_rx.await.map_err(|_| {
            LocalStreamPrepareError::Unavailable(Arc::from(
                "local stream worker stopped before preparing the response",
            ))
        })?;
        let first = first.map_err(LocalStreamPrepareError::FirstItem)?;

        let tail = ReceiverStream::new(tail_rx).map(|item| item.map(BufferedChunk::into_bytes));
        let first = stream::iter(first.into_iter().map(Ok));
        let keep_pool_alive = Arc::clone(pool);
        Ok(Box::pin(first.chain(tail).map(move |item| {
            let _keep_pool_alive = &keep_pool_alive;
            item
        })))
    }
}

struct WorkerLoad {
    active: AtomicUsize,
    assigned: AtomicUsize,
}

struct WorkerControl {
    wake: mpsc::Sender<()>,
    queue: Arc<Mutex<VecDeque<LocalJob>>>,
    load: Arc<WorkerLoad>,
}

struct PoolState {
    config: LocalStreamExecutorConfig,
    admission: Arc<Semaphore>,
    workers: Box<[WorkerControl]>,
    joins: Mutex<Vec<std::thread::JoinHandle<()>>>,
    shutdown: Arc<AtomicBool>,
    next_tie: AtomicUsize,
    next_job_id: AtomicU64,
}

impl PoolState {
    fn start(config: LocalStreamExecutorConfig) -> Result<Arc<Self>, Arc<str>> {
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut receivers = Vec::with_capacity(config.worker_threads);
        let mut workers = Vec::with_capacity(config.worker_threads);
        for _ in 0..config.worker_threads {
            let (wake, receiver) = mpsc::channel(1);
            receivers.push(receiver);
            workers.push(WorkerControl {
                wake,
                queue: Arc::new(Mutex::new(VecDeque::new())),
                load: Arc::new(WorkerLoad {
                    active: AtomicUsize::new(0),
                    assigned: AtomicUsize::new(0),
                }),
            });
        }

        let pool = Arc::new(Self {
            config,
            admission: Arc::new(Semaphore::new(config.max_in_flight)),
            workers: workers.into_boxed_slice(),
            joins: Mutex::new(Vec::with_capacity(config.worker_threads)),
            shutdown,
            next_tie: AtomicUsize::new(0),
            next_job_id: AtomicU64::new(0),
        });
        let loads: Arc<[Arc<WorkerLoad>]> = pool
            .workers
            .iter()
            .map(|worker| Arc::clone(&worker.load))
            .collect::<Vec<_>>()
            .into();
        let queues: Arc<[Arc<Mutex<VecDeque<LocalJob>>>]> = pool
            .workers
            .iter()
            .map(|worker| Arc::clone(&worker.queue))
            .collect::<Vec<_>>()
            .into();

        for (index, receiver) in receivers.into_iter().enumerate() {
            let queues = Arc::clone(&queues);
            let shutdown = Arc::clone(&pool.shutdown);
            let loads = Arc::clone(&loads);
            let active = Arc::clone(&pool.workers[index].load);
            let thread = std::thread::Builder::new()
                .name(format!("proa-stream-{index}"))
                .spawn(move || run_worker(index, receiver, queues, shutdown, loads, active))
                .map_err(|error| {
                    Arc::<str>::from(format!(
                        "failed to start local stream worker {index}: {error}"
                    ))
                })?;
            pool.joins
                .lock()
                .expect("local stream join mutex poisoned")
                .push(thread);
        }

        Ok(pool)
    }

    fn choose_worker(&self) -> usize {
        let tie = self.next_tie.fetch_add(1, Ordering::Relaxed);
        self.workers
            .iter()
            .enumerate()
            .min_by_key(|(index, worker)| {
                (
                    worker.load.active.load(Ordering::Relaxed)
                        + worker.load.assigned.load(Ordering::Relaxed),
                    (index
                        .wrapping_add(self.workers.len())
                        .wrapping_sub(tie % self.workers.len()))
                        % self.workers.len(),
                )
            })
            .map_or(0, |(index, _)| index)
    }

    fn enqueue(&self, job: LocalJob) -> Result<(), LocalStreamPrepareError> {
        let job_id = job.id;
        let assigned = job.assigned_worker;
        self.workers[assigned]
            .load
            .assigned
            .fetch_add(1, Ordering::Relaxed);
        self.workers[assigned]
            .queue
            .lock()
            .expect("local stream queue mutex poisoned")
            .push_back(job);

        match self.workers[assigned].wake.try_send(()) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(())) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(())) => {
                // Another live worker may steal the unopened factory. Wake it
                // directly; if no worker survives, reject the queued job.
                if self
                    .workers
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != assigned)
                    .any(|(_, worker)| {
                        matches!(
                            worker.wake.try_send(()),
                            Ok(()) | Err(mpsc::error::TrySendError::Full(()))
                        )
                    })
                {
                    return Ok(());
                }

                let mut queue = self.workers[assigned]
                    .queue
                    .lock()
                    .expect("local stream queue mutex poisoned");
                if let Some(index) = queue.iter().position(|queued| queued.id == job_id) {
                    queue.remove(index);
                    self.workers[assigned]
                        .load
                        .assigned
                        .fetch_sub(1, Ordering::Relaxed);
                }
                Err(LocalStreamPrepareError::Unavailable(Arc::from(
                    "all local stream workers have stopped",
                )))
            }
        }
    }
}

impl Drop for PoolState {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        for worker in &self.workers {
            let _ = worker.wake.try_send(());
        }
        for join in self
            .joins
            .get_mut()
            .expect("local stream join mutex poisoned")
            .drain(..)
        {
            let _ = join.join();
        }
    }
}

fn run_worker(
    index: usize,
    mut wake: mpsc::Receiver<()>,
    queues: Arc<[Arc<Mutex<VecDeque<LocalJob>>>]>,
    shutdown: Arc<AtomicBool>,
    loads: Arc<[Arc<WorkerLoad>]>,
    own_load: Arc<WorkerLoad>,
) {
    let _boundary_supervision = proa_stream::driver::LocalBoundarySupervisionGuard::enter();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!(?error, "failed to build local stream worker runtime");
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async move {
        loop {
            if shutdown.load(Ordering::Acquire) {
                return;
            }
            if wake.recv().await.is_none() {
                return;
            }

            loop {
                if shutdown.load(Ordering::Acquire) {
                    return;
                }
                let Some(job) = claim_unopened_job(index, &queues, &loads) else {
                    break;
                };
                if job.first.is_closed() || job.tail.is_closed() {
                    tokio::task::yield_now().await;
                    continue;
                }

                own_load.active.fetch_add(1, Ordering::Relaxed);
                let active = Arc::clone(&own_load);
                tokio::task::spawn_local(async move {
                    let _active = ActiveJobGuard(active);
                    drive_job(job).await;
                });
                // Give the newly pinned stream and peer workers a chance to
                // run before this worker claims another unopened factory.
                tokio::task::yield_now().await;
            }
        }
    });
}

/// Claim locally first, then steal the newest unopened job from the busiest
/// peer. Only factories move between workers; once `drive_job` invokes one,
/// its potentially `!Send` stream remains pinned to this worker forever.
fn claim_unopened_job(
    own_index: usize,
    queues: &[Arc<Mutex<VecDeque<LocalJob>>>],
    loads: &[Arc<WorkerLoad>],
) -> Option<LocalJob> {
    if let Some(job) = queues[own_index]
        .lock()
        .expect("local stream queue mutex poisoned")
        .pop_front()
    {
        loads[own_index].assigned.fetch_sub(1, Ordering::Relaxed);
        return Some(job);
    }

    let busiest = (0..queues.len())
        .filter(|index| *index != own_index)
        .max_by_key(|index| loads[*index].assigned.load(Ordering::Relaxed));
    for peer in busiest
        .into_iter()
        .chain((0..queues.len()).filter(|index| *index != own_index && Some(*index) != busiest))
    {
        let Some(job) = queues[peer]
            .lock()
            .expect("local stream queue mutex poisoned")
            .pop_back()
        else {
            continue;
        };
        loads[peer].assigned.fetch_sub(1, Ordering::Relaxed);
        return Some(job);
    }
    None
}

struct ActiveJobGuard(Arc<WorkerLoad>);

impl Drop for ActiveJobGuard {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Relaxed);
    }
}

struct LocalJob {
    id: u64,
    make: FrameworkLocalStreamFactory,
    head: Option<proa_framework_core::managed_response::PreparedHead>,
    first: oneshot::Sender<Result<Option<Bytes>, FrameworkError>>,
    tail: mpsc::Sender<Result<BufferedChunk, FrameworkError>>,
    byte_budget: ByteBudget,
    admission: OwnedSemaphorePermit,
    assigned_worker: usize,
    cooperative_yield_interval: usize,
}

async fn drive_job(mut job: LocalJob) {
    if job.first.is_closed() || job.tail.is_closed() {
        return;
    }
    let mut response = (job.make)();
    let first = tokio::select! {
        biased;
        () = job.first.closed() => return,
        item = proa_framework_core::managed_response::first_or_ready(&mut response, job.head.as_ref()) => item,
    };
    match first {
        None => {
            if job.first.send(Ok(None)).is_err() {
                return;
            }
            if job.head.as_ref().is_none_or(|h| h.get().is_none()) {
                return;
            }
        }
        Some(Err(error)) => {
            let _ = job.first.send(Err(error));
            return;
        }
        Some(Ok(bytes)) => {
            if job.first.send(Ok(Some(bytes))).is_err() {
                return;
            }
        }
    }

    // Keep admission for the response's entire active lifetime.
    let _admission = job.admission;
    let mut emitted = 0usize;
    loop {
        let next = tokio::select! {
            biased;
            () = job.tail.closed() => return,
            item = response.next() => item,
        };
        let Some(item) = next else {
            return;
        };
        match item {
            Ok(bytes) => {
                let permit = tokio::select! {
                    biased;
                    () = job.tail.closed() => return,
                    permit = job.byte_budget.reserve(bytes.len()) => permit,
                };
                if job
                    .tail
                    .send(Ok(BufferedChunk { bytes, permit }))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                let _ = job.tail.send(Err(error)).await;
                return;
            }
        }
        emitted += 1;
        if emitted.is_multiple_of(job.cooperative_yield_interval) {
            tokio::task::yield_now().await;
        }
    }
}

#[derive(Clone)]
struct ByteBudget {
    semaphore: Arc<Semaphore>,
    capacity: usize,
}

impl ByteBudget {
    fn new(capacity: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(capacity)),
            capacity,
        }
    }

    async fn reserve(&self, bytes: usize) -> Option<OwnedSemaphorePermit> {
        if bytes == 0 {
            return None;
        }
        let permits = bytes.min(self.capacity) as u32;
        Arc::clone(&self.semaphore)
            .acquire_many_owned(permits)
            .await
            .ok()
    }
}

struct BufferedChunk {
    bytes: Bytes,
    permit: Option<OwnedSemaphorePermit>,
}

impl BufferedChunk {
    fn into_bytes(self) -> Bytes {
        let Self { bytes, permit } = self;
        drop(permit);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Poll};
    use std::time::Duration;

    use futures::{stream, Stream, StreamExt};
    use proa_framework_core::{FrameworkLocalStream, FrameworkLocalStreamFactory};

    use super::*;

    fn factory(chunks: Vec<&'static [u8]>) -> FrameworkLocalStreamFactory {
        Box::new(move || {
            let stream: FrameworkLocalStream = Box::pin(stream::iter(
                chunks
                    .into_iter()
                    .map(|chunk| Ok(Bytes::from_static(chunk))),
            ));
            stream
        })
    }

    fn test_config() -> LocalStreamExecutorConfig {
        LocalStreamExecutorConfig {
            worker_threads: 1,
            max_in_flight: 2,
            response_buffer_bytes: 8,
            response_buffer_chunks: 1,
            cooperative_yield_interval: 1,
        }
    }

    #[tokio::test]
    async fn prepares_first_item_and_preserves_tail_order() {
        let executor = LocalStreamExecutor::new(test_config());
        let mut prepared = executor
            .prepare(factory(vec![b"first", b"second", b"third"]))
            .await
            .unwrap();
        let mut output = Vec::new();
        while let Some(item) = prepared.next().await {
            output.extend_from_slice(&item.unwrap());
        }
        assert_eq!(output, b"firstsecondthird");
    }

    #[tokio::test]
    async fn bounded_admission_rejects_excess_work() {
        let config = LocalStreamExecutorConfig {
            max_in_flight: 1,
            ..test_config()
        };
        let executor = LocalStreamExecutor::new(config);
        let entered = Arc::new(AtomicBool::new(false));
        let entered_for_factory = Arc::clone(&entered);
        let first: FrameworkLocalStreamFactory = Box::new(move || {
            entered_for_factory.store(true, Ordering::Release);
            Box::pin(stream::pending())
        });
        let first_prepare = executor.prepare(first);
        tokio::pin!(first_prepare);
        tokio::select! {
            _ = &mut first_prepare => panic!("stalled stream unexpectedly prepared"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
        assert!(entered.load(Ordering::Acquire));

        let error = match executor.prepare(factory(vec![b"extra"])).await {
            Ok(_) => panic!("excess work unexpectedly passed admission"),
            Err(error) => error,
        };
        assert!(matches!(error, LocalStreamPrepareError::Overloaded));
    }

    #[tokio::test]
    async fn dropping_queued_preparation_skips_factory_side_effects() {
        let config = LocalStreamExecutorConfig {
            max_in_flight: 2,
            ..test_config()
        };
        let executor = LocalStreamExecutor::new(config);
        let worker_blocked = Arc::new(AtomicBool::new(false));
        let worker_blocked_by_factory = Arc::clone(&worker_blocked);
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker: FrameworkLocalStreamFactory = Box::new(move || {
            worker_blocked_by_factory.store(true, Ordering::Release);
            release_rx.recv().unwrap();
            Box::pin(stream::pending())
        });
        let blocker_prepare = executor.prepare(blocker);
        tokio::pin!(blocker_prepare);
        while !worker_blocked.load(Ordering::Acquire) {
            tokio::select! {
                _ = &mut blocker_prepare => panic!("stalled stream unexpectedly prepared"),
                _ = tokio::task::yield_now() => {}
            }
        }

        let invoked = Arc::new(AtomicBool::new(false));
        let invoked_by_factory = Arc::clone(&invoked);
        let queued: FrameworkLocalStreamFactory = Box::new(move || {
            invoked_by_factory.store(true, Ordering::Release);
            Box::pin(stream::empty())
        });
        {
            let queued_prepare = executor.prepare(queued);
            tokio::pin!(queued_prepare);
            tokio::select! {
                _ = &mut queued_prepare => panic!("queued stream unexpectedly prepared"),
                _ = tokio::time::sleep(Duration::from_millis(1)) => {}
            }
        }
        release_tx.send(()).unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!invoked.load(Ordering::Acquire));
    }

    struct DropObservedStream {
        dropped: Arc<AtomicBool>,
    }

    impl Stream for DropObservedStream {
        type Item = Result<Bytes, FrameworkError>;

        fn poll_next(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            Poll::Pending
        }
    }

    impl Drop for DropObservedStream {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn dropping_active_preparation_cancels_and_drops_local_stream() {
        let executor = LocalStreamExecutor::new(test_config());
        let invoked = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let invoked_by_factory = Arc::clone(&invoked);
        let dropped_by_stream = Arc::clone(&dropped);
        let factory: FrameworkLocalStreamFactory = Box::new(move || {
            invoked_by_factory.store(true, Ordering::Release);
            Box::pin(DropObservedStream {
                dropped: dropped_by_stream,
            })
        });

        {
            let prepare = executor.prepare(factory);
            tokio::pin!(prepare);
            tokio::time::timeout(Duration::from_secs(1), async {
                while !invoked.load(Ordering::Acquire) {
                    tokio::select! {
                        _ = &mut prepare => panic!("pending stream unexpectedly prepared"),
                        _ = tokio::task::yield_now() => {}
                    }
                }
            })
            .await
            .expect("local worker should invoke the admitted factory");
        }

        tokio::time::timeout(Duration::from_secs(1), async {
            while !dropped.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancelling preparation should drop the active local stream");
    }

    #[tokio::test]
    async fn byte_budget_blocks_until_buffered_chunk_is_released() {
        let budget = ByteBudget::new(8);
        let first = budget.reserve(6).await.expect("six-byte reservation");
        assert_eq!(budget.semaphore.available_permits(), 2);

        let second = budget.reserve(3);
        tokio::pin!(second);
        tokio::select! {
            _ = &mut second => panic!("reservation exceeded the configured byte budget"),
            _ = tokio::task::yield_now() => {}
        }

        drop(first);
        let second = tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .expect("released bytes should wake the blocked producer")
            .expect("three-byte reservation");
        assert_eq!(budget.semaphore.available_permits(), 5);
        drop(second);
        assert_eq!(budget.semaphore.available_permits(), 8);

        let oversized = budget.reserve(64).await.expect("oversized reservation");
        assert_eq!(budget.semaphore.available_permits(), 0);
        drop(oversized);
        assert_eq!(budget.semaphore.available_permits(), 8);
    }
}
