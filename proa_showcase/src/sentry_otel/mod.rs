// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Sentry trace-associated OpenTelemetry logs showcase.
//!
//! The visual target is Sentry's May 2026 OpenTelemetry article. The page
//! keeps the captured 1920x1280 composition while using a deterministic,
//! allocation-free checkout telemetry fixture for scalable SSR benchmarks.

mod fixture;
mod page;

pub use fixture::{LogPattern, LOG_PATTERNS};
#[cfg(feature = "rsjs-pages")]
pub use page::{render, render_into, render_into_with_props, render_rows, SentryOtelPage};
pub use page::{render_benchmark_rows, SentryLogEntry, SentryPageProps};

pub const CSS: &str = include_str!("sentry_otel.css");
pub const DEFAULT_ROW_COUNT: usize = 400;
