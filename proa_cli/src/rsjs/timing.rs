// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Opt-in elapsed stage diagnostics, independent of compiler freshness inputs.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn profile_output() -> Option<&'static Mutex<File>> {
    static OUTPUT: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    OUTPUT
        .get_or_init(|| {
            let path = std::env::var_os("PROA_BUILD_PROFILE")?;
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
                .map(Mutex::new)
        })
        .as_ref()
}

/// Local investigation only: emit Chrome-compatible complete spans. This is
/// diagnostic output, never a cache freshness input or validation authority.
pub(crate) struct ProfileSpan {
    name: String,
    started: Instant,
    timestamp_us: u128,
    args: serde_json::Value,
}

impl ProfileSpan {
    pub(crate) fn new(name: impl Into<String>) -> Option<Self> {
        profile_output()?;
        Some(Self {
            name: name.into(),
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_micros(),
            started: Instant::now(),
            args: serde_json::json!({}),
        })
    }
}

impl Drop for ProfileSpan {
    fn drop(&mut self) {
        let event = serde_json::json!({
            "name": self.name, "ph": "X", "cat": "rsjs",
            "ts": self.timestamp_us, "dur": self.started.elapsed().as_micros(),
            "pid": std::process::id(), "tid": format!("{:?}", std::thread::current().id()),
            "args": self.args,
            "phase": std::env::var(super::build::WRAPPER_MODE_ENV).unwrap_or_else(|_| "runner".into()),
            "crate": std::env::var("CARGO_CRATE_NAME").unwrap_or_default(),
        });
        if let (Some(output), Ok(mut bytes)) = (profile_output(), serde_json::to_vec(&event)) {
            bytes.push(b'\n');
            if let Ok(mut output) = output.lock() {
                let _ = output.write_all(&bytes);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Work {
    Read,
    Hash,
    Write,
    Sync,
}

pub(crate) struct FileProfile {
    span: Option<ProfileSpan>,
    times: [Duration; 4],
    input_bytes: u64,
    passes: u64,
}

impl FileProfile {
    pub(crate) fn new(name: &str, path: &Path, passes: u64) -> Self {
        let mut span = ProfileSpan::new(name);
        if let Some(span) = &mut span {
            span.args = serde_json::json!({"path": path});
        }
        Self {
            span,
            times: [Duration::ZERO; 4],
            input_bytes: 0,
            passes,
        }
    }

    pub(crate) fn measure<T>(&mut self, kind: Work, work: impl FnOnce() -> T) -> T {
        if self.span.is_none() {
            return work();
        }
        let start = Instant::now();
        let result = work();
        self.times[kind as usize] += start.elapsed();
        result
    }

    pub(crate) fn bytes(&mut self, count: usize) {
        self.input_bytes += count as u64;
    }
}

impl Drop for FileProfile {
    fn drop(&mut self) {
        if let Some(span) = &mut self.span {
            span.args["input_bytes"] = self.input_bytes.into();
            span.args["hashed_bytes"] = (self.input_bytes * self.passes).into();
            for (name, time) in ["read_us", "hash_us", "write_us", "sync_us"]
                .into_iter()
                .zip(self.times)
            {
                span.args[name] = (time.as_micros() as u64).into();
            }
        }
    }
}

pub(crate) struct StageTimer(Option<Instant>);

impl StageTimer {
    pub(crate) fn new() -> Self {
        Self(std::env::var_os("PROA_BUILD_TIMINGS").map(|_| Instant::now()))
    }

    pub(crate) fn mark(&mut self, stage: &str) {
        if let Some(previous) = self.0.as_mut() {
            let now = Instant::now();
            if let Some(mut span) = ProfileSpan::new(stage) {
                let elapsed = now.duration_since(*previous);
                span.started = *previous;
                span.timestamp_us = span.timestamp_us.saturating_sub(elapsed.as_micros());
            }
            eprintln!(
                "rsjs timing: {stage}: {:.3} ms",
                now.duration_since(*previous).as_secs_f64() * 1000.0
            );
            *previous = now;
        }
    }
}
