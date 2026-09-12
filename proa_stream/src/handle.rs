// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::{AtomicU64, Ordering};

use crate::request_id::RequestId;

/// Per-request token allocator and stream identity.
#[derive(Debug)]
pub struct StreamHandle {
    request_id: RequestId,
    next_token: AtomicU64,
}

impl StreamHandle {
    pub fn new(request_id: RequestId) -> Self {
        Self {
            request_id,
            next_token: AtomicU64::new(1),
        }
    }

    pub const fn request_id(&self) -> RequestId {
        self.request_id
    }

    pub fn next_token(&self) -> u64 {
        self.next_token.fetch_add(1, Ordering::Relaxed)
    }
}
