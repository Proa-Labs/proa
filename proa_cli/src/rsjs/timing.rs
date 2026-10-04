// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Opt-in elapsed stage diagnostics, independent of compiler freshness inputs.

use std::time::Instant;

pub(crate) struct StageTimer(Option<Instant>);

impl StageTimer {
    pub(crate) fn new() -> Self {
        Self(std::env::var_os("PROA_BUILD_TIMINGS").map(|_| Instant::now()))
    }

    pub(crate) fn mark(&mut self, stage: &str) {
        if let Some(previous) = self.0.as_mut() {
            let now = Instant::now();
            eprintln!(
                "rsjs timing: {stage}: {:.3} ms",
                now.duration_since(*previous).as_secs_f64() * 1000.0
            );
            *previous = now;
        }
    }
}
