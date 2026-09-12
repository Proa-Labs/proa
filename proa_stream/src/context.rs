// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use proa_core::{
    PendingBoundary, RenderMode, SendPendingBoundary, SendStreamBoundaryRecorder,
    StreamBoundaryRecorder,
};

use crate::{RequestId, StreamHandle};

/// Request-scoped streaming state shared by future drivers.
#[derive(Clone)]
pub struct StreamContext {
    handle: Arc<StreamHandle>,
    mode: RenderMode,
    pending_boundaries: Rc<RefCell<Vec<PendingBoundary>>>,
}

impl StreamContext {
    pub fn new(mode: RenderMode) -> Self {
        Self {
            handle: Arc::new(StreamHandle::new(RequestId::new())),
            mode,
            pending_boundaries: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn handle(&self) -> &Arc<StreamHandle> {
        &self.handle
    }

    pub const fn mode(&self) -> RenderMode {
        self.mode
    }

    pub fn take_boundaries(&self) -> Vec<PendingBoundary> {
        std::mem::take(&mut *self.pending_boundaries.borrow_mut())
    }

    /// Drain newly registered work into a reusable staging vector.
    ///
    /// Both vectors keep their allocations, so repeated nested-boundary
    /// admission does not replace the registry's backing storage on every
    /// driver turn.
    pub fn drain_boundaries_into(&self, target: &mut Vec<PendingBoundary>) {
        target.append(&mut self.pending_boundaries.borrow_mut());
    }

    pub fn has_pending_boundaries(&self) -> bool {
        !self.pending_boundaries.borrow().is_empty()
    }
}

impl std::fmt::Debug for StreamContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamContext")
            .field("request_id", &self.handle.request_id())
            .field("mode", &self.mode)
            .field(
                "pending_boundaries",
                &self.pending_boundaries.borrow().len(),
            )
            .finish()
    }
}

impl StreamBoundaryRecorder for StreamContext {
    fn next_boundary_id(&self) -> u64 {
        self.handle.next_token()
    }

    fn push_boundary(&self, boundary: PendingBoundary) {
        self.pending_boundaries.borrow_mut().push(boundary);
    }
}

/// Branch-owned local registry used while a detached boundary is speculative.
/// IDs still come from the response-wide recorder, but nested futures do not
/// enter the live scheduler until their parent boundary commits.
pub(crate) struct LocalBoundaryBranch {
    id_source: Rc<dyn StreamBoundaryRecorder>,
    pending_boundaries: RefCell<Vec<PendingBoundary>>,
}

impl LocalBoundaryBranch {
    pub(crate) fn new(id_source: Rc<dyn StreamBoundaryRecorder>) -> Self {
        Self {
            id_source,
            pending_boundaries: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn take_boundaries(&self) -> Vec<PendingBoundary> {
        std::mem::take(&mut *self.pending_boundaries.borrow_mut())
    }
}

impl StreamBoundaryRecorder for LocalBoundaryBranch {
    fn next_boundary_id(&self) -> u64 {
        self.id_source.next_boundary_id()
    }

    fn push_boundary(&self, boundary: PendingBoundary) {
        self.pending_boundaries.borrow_mut().push(boundary);
    }
}

/// Request-scoped boundary registry for a compiler-proved `Send` stream.
#[derive(Clone)]
pub struct SendStreamContext {
    handle: Arc<StreamHandle>,
    mode: RenderMode,
    pending_boundaries: Arc<std::sync::Mutex<Vec<SendPendingBoundary>>>,
}

impl SendStreamContext {
    pub fn new(mode: RenderMode) -> Self {
        Self {
            handle: Arc::new(StreamHandle::new(RequestId::new())),
            mode,
            pending_boundaries: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn handle(&self) -> &Arc<StreamHandle> {
        &self.handle
    }

    pub const fn mode(&self) -> RenderMode {
        self.mode
    }

    pub fn take_boundaries(&self) -> Vec<SendPendingBoundary> {
        std::mem::take(
            &mut *self
                .pending_boundaries
                .lock()
                .expect("send boundary registry mutex poisoned"),
        )
    }

    pub fn drain_boundaries_into(&self, target: &mut Vec<SendPendingBoundary>) {
        target.append(
            &mut self
                .pending_boundaries
                .lock()
                .expect("send boundary registry mutex poisoned"),
        );
    }

    pub fn has_pending_boundaries(&self) -> bool {
        !self
            .pending_boundaries
            .lock()
            .expect("send boundary registry mutex poisoned")
            .is_empty()
    }
}

impl SendStreamBoundaryRecorder for SendStreamContext {
    fn next_boundary_id(&self) -> u64 {
        self.handle.next_token()
    }

    fn push_boundary(&self, boundary: SendPendingBoundary) {
        self.pending_boundaries
            .lock()
            .expect("send boundary registry mutex poisoned")
            .push(boundary);
    }
}

/// Branch-owned send-safe registry for speculative nested boundaries.
pub(crate) struct SendBoundaryBranch {
    id_source: Arc<dyn SendStreamBoundaryRecorder>,
    pending_boundaries: std::sync::Mutex<Vec<SendPendingBoundary>>,
}

impl SendBoundaryBranch {
    pub(crate) fn new(id_source: Arc<dyn SendStreamBoundaryRecorder>) -> Self {
        Self {
            id_source,
            pending_boundaries: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn take_boundaries(&self) -> Vec<SendPendingBoundary> {
        std::mem::take(
            &mut *self
                .pending_boundaries
                .lock()
                .expect("send boundary branch mutex poisoned"),
        )
    }
}

impl SendStreamBoundaryRecorder for SendBoundaryBranch {
    fn next_boundary_id(&self) -> u64 {
        self.id_source.next_boundary_id()
    }

    fn push_boundary(&self, boundary: SendPendingBoundary) {
        self.pending_boundaries
            .lock()
            .expect("send boundary branch mutex poisoned")
            .push(boundary);
    }
}
