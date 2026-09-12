// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end Axum benchmark for the Suspense response adapter.
//!
//! Unlike the Criterion render-only targets, this exercises the complete
//! `Router -> RouteResponse -> FrameworkBody -> Axum Body` path. It records
//! response-ready, first-byte, and completion latency so executor and bridge
//! changes can be compared against a durable pre-change baseline.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use http_body_util::BodyExt as _;
use proa_core::{
    render_child, render_send_child, NoLoader, RenderBuffer, RenderOutcome, SendWebContext,
    SyncToAsync, WebContext, WebRender, WebRenderSend, WebRenderSync, WriteBuf, WriteError,
};
use proa_framework_axum::manifest::IslandManifest;
use proa_framework_axum::{
    router_from_spec, LocalStreamExecutorConfig, Route, RouteContextLayer, RouteResponse,
};
use proa_stream::Suspense;
use serde_json::{json, Value};
use tokio::sync::Barrier;
use tower::ServiceExt;

const DEFAULT_ITERATIONS: usize = 10_000;
const DEFAULT_WARMUP: usize = 250;
const DEFAULT_REPEATS: usize = 3;
const DEFAULT_WORKERS: &[usize] = &[1, 4];
const DEFAULT_CONCURRENCY: &[usize] = &[1, 32, 128];

#[derive(Clone, Copy)]
struct BenchmarkCase {
    name: &'static str,
    path: &'static str,
    boundaries: usize,
    streamed: bool,
    execution_path: &'static str,
}

const CASES: &[BenchmarkCase] = &[
    BenchmarkCase {
        name: "buffered",
        path: "/buffered",
        boundaries: 0,
        streamed: false,
        execution_path: "buffered",
    },
    BenchmarkCase {
        name: "stream_0",
        path: "/stream/0",
        boundaries: 0,
        streamed: true,
        execution_path: "local",
    },
    BenchmarkCase {
        name: "stream_1",
        path: "/stream/1",
        boundaries: 1,
        streamed: true,
        execution_path: "local",
    },
    BenchmarkCase {
        name: "stream_8",
        path: "/stream/8",
        boundaries: 8,
        streamed: true,
        execution_path: "local",
    },
    BenchmarkCase {
        name: "stream_32",
        path: "/stream/32",
        boundaries: 32,
        streamed: true,
        execution_path: "local",
    },
    BenchmarkCase {
        name: "stream_128",
        path: "/stream/128",
        boundaries: 128,
        streamed: true,
        execution_path: "local",
    },
    BenchmarkCase {
        name: "send_stream_0",
        path: "/send-stream/0",
        boundaries: 0,
        streamed: true,
        execution_path: "send",
    },
    BenchmarkCase {
        name: "send_stream_1",
        path: "/send-stream/1",
        boundaries: 1,
        streamed: true,
        execution_path: "send",
    },
    BenchmarkCase {
        name: "send_stream_8",
        path: "/send-stream/8",
        boundaries: 8,
        streamed: true,
        execution_path: "send",
    },
    BenchmarkCase {
        name: "send_stream_32",
        path: "/send-stream/32",
        boundaries: 32,
        streamed: true,
        execution_path: "send",
    },
    BenchmarkCase {
        name: "send_stream_128",
        path: "/send-stream/128",
        boundaries: 128,
        streamed: true,
        execution_path: "send",
    },
];

struct TinyChild;

// proa-perf-ok(scope): benchmark fixture intentionally writes fixed protocol bytes directly
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TinyChild {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WriteBuf::extend_static(&mut cx.out, b"<span>x</span>")
    }
}

struct EmptyFallback;

struct SendTinyChild;

struct SendEmptyFallback;

impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for EmptyFallback {
    fn render(self, _cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

impl<L: proa_core::DataLoader, B: WriteBuf + Send> WebRenderSend<L, B> for SendTinyChild {
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(WriteBuf::extend_static(
            &mut cx.out,
            b"<span>x</span>",
        ))
    }
}

impl<L: proa_core::DataLoader, B: WriteBuf + Send> WebRenderSend<L, B> for SendEmptyFallback {
    fn render_send(
        self,
        _cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(Ok(()))
    }
}

#[derive(Clone, Copy)]
struct BenchmarkPage {
    boundaries: usize,
}

// proa-perf-ok(scope): benchmark fixture intentionally writes fixed protocol bytes directly
impl WebRender<NoLoader, RenderBuffer> for BenchmarkPage {
    fn render(
        self,
        cx: &mut WebContext<NoLoader, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            WriteBuf::extend_static(&mut cx.out, b"<!DOCTYPE html><main>")?;
            for _ in 0..self.boundaries {
                render_child(cx, Suspense::new(SyncToAsync(TinyChild), EmptyFallback)).await?;
            }
            WriteBuf::extend_static(&mut cx.out, b"</main>")
        })
    }
}

impl WebRenderSend<NoLoader, RenderBuffer> for BenchmarkPage {
    fn render_send(
        self,
        cx: &mut SendWebContext<NoLoader, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            WriteBuf::extend_static(&mut cx.out, b"<!DOCTYPE html><main>")?;
            for _ in 0..self.boundaries {
                render_send_child(cx, Suspense::new(SendTinyChild, SendEmptyFallback)).await?;
            }
            WriteBuf::extend_static(&mut cx.out, b"</main>")
        })
    }
}

#[derive(Debug)]
struct Config {
    iterations: usize,
    warmup: usize,
    repeats: usize,
    workers: Vec<usize>,
    concurrency: Vec<usize>,
    output: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            iterations: DEFAULT_ITERATIONS,
            warmup: DEFAULT_WARMUP,
            repeats: DEFAULT_REPEATS,
            workers: DEFAULT_WORKERS.to_vec(),
            concurrency: DEFAULT_CONCURRENCY.to_vec(),
            output: PathBuf::from("proa_bench/results/suspense-adapter.json"),
        }
    }
}

#[derive(Debug)]
struct Observation {
    response_ready_ns: u64,
    first_byte_ns: u64,
    completion_ns: u64,
    output_bytes: usize,
    output_chunks: usize,
}

#[derive(Debug)]
struct Measurement {
    elapsed_ns: u64,
    attempts: usize,
    observations: Vec<Observation>,
    errors: Vec<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_config()?;
    let mut rows = Vec::new();

    println!("Proa Suspense Axum adapter benchmark");
    println!(
        "iterations={} warmup={} repeats={} workers={:?} concurrency={:?}",
        config.iterations, config.warmup, config.repeats, config.workers, config.concurrency
    );

    for &worker_threads in &config.workers {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .enable_all()
            .build()?;

        for case in CASES {
            for &concurrency in &config.concurrency {
                runtime.block_on(run_case(
                    build_app(worker_threads),
                    case.path,
                    config.warmup,
                    concurrency,
                ))?;

                for repeat in 1..=config.repeats {
                    let measurement = runtime.block_on(run_case(
                        build_app(worker_threads),
                        case.path,
                        config.iterations,
                        concurrency,
                    ))?;
                    let row =
                        measurement_json(worker_threads, *case, concurrency, repeat, &measurement);
                    print_row(&row);
                    rows.push(row);
                }
            }
        }
    }

    let output = json!({
        "schema": "proa.suspense-adapter-benchmark.v1",
        "generated_unix_seconds": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        "source": {
            "commit": command_output("git", &["rev-parse", "HEAD"]),
            "tracked_worktree_dirty": !command_output("git", &["status", "--porcelain", "--untracked-files=no"])
                .unwrap_or_default()
                .is_empty(),
        },
        "host": {
            "os": command_output("uname", &["-srv"]),
            "architecture": std::env::consts::ARCH,
            "cpu": command_output("sysctl", &["-n", "machdep.cpu.brand_string"]),
            "logical_cpus": std::thread::available_parallelism().map(usize::from).ok(),
            "rustc": command_output("rustc", &["--version"]),
        },
        "configuration": {
            "profile": "release",
            "iterations_per_repeat": config.iterations,
            "warmup_iterations": config.warmup,
            "repeats": config.repeats,
            "worker_threads": config.workers,
            "local_stream_worker_threads": "matches worker_threads",
            "concurrency": config.concurrency,
            "boundary_counts": CASES.iter().map(|case| case.boundaries).collect::<Vec<_>>(),
            "readiness": "all-ready immediate children",
            "client": "in-process fast body collector",
        },
        "results": rows,
    });

    if let Some(parent) = config.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config.output, serde_json::to_vec_pretty(&output)?)?;
    println!("wrote {}", config.output.display());
    Ok(())
}

fn parse_config() -> Result<Config, Box<dyn std::error::Error>> {
    let mut config = Config::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = match arg.as_str() {
            "--iterations" | "--warmup" | "--repeats" | "--workers" | "--concurrency"
            | "--output" => args
                .next()
                .ok_or_else(|| format!("missing value for {arg}"))?,
            "--help" | "-h" => {
                println!(
                    "Usage: suspense_adapter_bench [--iterations N] [--warmup N] \
                     [--repeats N] [--workers 1,4] [--concurrency 1,32,128] \
                     [--output PATH]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        };

        match arg.as_str() {
            "--iterations" => config.iterations = positive(&value, &arg)?,
            "--warmup" => config.warmup = value.parse()?,
            "--repeats" => config.repeats = positive(&value, &arg)?,
            "--workers" => config.workers = parse_list(&value, &arg)?,
            "--concurrency" => config.concurrency = parse_list(&value, &arg)?,
            "--output" => config.output = PathBuf::from(value),
            _ => unreachable!(),
        }
    }
    Ok(config)
}

fn positive(value: &str, flag: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let parsed: usize = value.parse()?;
    if parsed == 0 {
        return Err(format!("{flag} must be greater than zero").into());
    }
    Ok(parsed)
}

fn parse_list(value: &str, flag: &str) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
    let values = value
        .split(',')
        .map(|part| positive(part.trim(), flag))
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err(format!("{flag} requires at least one value").into());
    }
    Ok(values)
}

fn build_app(local_stream_workers: usize) -> axum::Router {
    let routes = vec![
        Route::endpoint(
            "/buffered",
            get(|| async { RouteResponse::ssr_async(BenchmarkPage { boundaries: 0 }) }),
        ),
        stream_route("/stream/0", 0),
        stream_route("/stream/1", 1),
        stream_route("/stream/8", 8),
        stream_route("/stream/32", 32),
        stream_route("/stream/128", 128),
        send_stream_route("/send-stream/0", 0),
        send_stream_route("/send-stream/1", 1),
        send_stream_route("/send-stream/8", 8),
        send_stream_route("/send-stream/32", 32),
        send_stream_route("/send-stream/128", 128),
    ];
    let manifest = Arc::new(IslandManifest {
        islands: HashMap::new(),
        assets: Default::default(),
        import_map: HashMap::new(),
        ssr_bundle: None,
    });
    let (router, metadata) = router_from_spec(routes);
    router.layer(
        RouteContextLayer::new(manifest, metadata).with_local_stream_executor_config(
            LocalStreamExecutorConfig {
                worker_threads: local_stream_workers,
                ..LocalStreamExecutorConfig::default()
            },
        ),
    )
}

fn stream_route(path: &'static str, boundaries: usize) -> Route<()> {
    Route::endpoint(
        path,
        get(move || async move { RouteResponse::ssr_stream(BenchmarkPage { boundaries }) }),
    )
}

fn send_stream_route(path: &'static str, boundaries: usize) -> Route<()> {
    Route::endpoint(
        path,
        get(move || async move { RouteResponse::ssr_stream_send(BenchmarkPage { boundaries }) }),
    )
}

async fn run_case(
    app: axum::Router,
    path: &'static str,
    iterations: usize,
    concurrency: usize,
) -> Result<Measurement, Box<dyn std::error::Error>> {
    if iterations == 0 {
        return Ok(Measurement {
            elapsed_ns: 0,
            attempts: 0,
            observations: Vec::new(),
            errors: Vec::new(),
        });
    }

    let concurrency = concurrency.min(iterations);
    let next = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(concurrency + 1));
    let mut tasks = Vec::with_capacity(concurrency);

    for _ in 0..concurrency {
        let app = app.clone();
        let next = Arc::clone(&next);
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            let mut observations = Vec::new();
            barrier.wait().await;
            loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= iterations {
                    break;
                }
                observations.push(run_request(app.clone(), path).await);
            }
            observations
        }));
    }

    let started = Instant::now();
    barrier.wait().await;
    let mut observations = Vec::with_capacity(iterations);
    let mut errors = Vec::new();
    for task in tasks {
        for observation in task.await.map_err(|err| err.to_string())? {
            match observation {
                Ok(observation) => observations.push(observation),
                Err(err) => errors.push(err),
            }
        }
    }
    let elapsed_ns = nanos(started.elapsed());
    if observations.len() + errors.len() != iterations {
        return Err(format!(
            "expected {iterations} attempts, received {}",
            observations.len() + errors.len()
        )
        .into());
    }
    Ok(Measurement {
        elapsed_ns,
        attempts: iterations,
        observations,
        errors,
    })
}

async fn run_request(app: axum::Router, path: &'static str) -> Result<Observation, String> {
    let started = Instant::now();
    let response = app
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .map_err(|err| err.to_string())?,
        )
        .await
        .map_err(|err| err.to_string())?;
    let response_ready_ns = nanos(started.elapsed());
    if response.status() != StatusCode::OK {
        return Err(format!("{path} returned {}", response.status()));
    }

    let mut body = response.into_body();
    let mut first_byte_ns = None;
    let mut output_bytes = 0usize;
    let mut output_chunks = 0usize;
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|err| err.to_string())?;
        if let Some(data) = frame.data_ref() {
            if first_byte_ns.is_none() {
                first_byte_ns = Some(nanos(started.elapsed()));
            }
            output_bytes = output_bytes.saturating_add(data.len());
            output_chunks = output_chunks.saturating_add(1);
        }
    }
    let completion_ns = nanos(started.elapsed());
    Ok(Observation {
        response_ready_ns,
        first_byte_ns: first_byte_ns.unwrap_or(completion_ns),
        completion_ns,
        output_bytes,
        output_chunks,
    })
}

fn measurement_json(
    worker_threads: usize,
    case: BenchmarkCase,
    concurrency: usize,
    repeat: usize,
    measurement: &Measurement,
) -> Value {
    let response_ready = measurement
        .observations
        .iter()
        .map(|observation| observation.response_ready_ns)
        .collect::<Vec<_>>();
    let first_byte = measurement
        .observations
        .iter()
        .map(|observation| observation.first_byte_ns)
        .collect::<Vec<_>>();
    let completion = measurement
        .observations
        .iter()
        .map(|observation| observation.completion_ns)
        .collect::<Vec<_>>();
    let output_bytes = measurement
        .observations
        .iter()
        .map(|observation| observation.output_bytes)
        .collect::<Vec<_>>();
    let output_chunks = measurement
        .observations
        .iter()
        .map(|observation| observation.output_chunks)
        .collect::<Vec<_>>();

    json!({
        "worker_threads": worker_threads,
        "local_stream_worker_threads": worker_threads,
        "scenario": case.name,
        "streamed": case.streamed,
        "execution_path": case.execution_path,
        "boundaries": case.boundaries,
        "concurrency": concurrency,
        "repeat": repeat,
        "iterations": measurement.attempts,
        "successful_iterations": measurement.observations.len(),
        "error_count": measurement.errors.len(),
        "error_samples": measurement.errors.iter().take(8).collect::<Vec<_>>(),
        "elapsed_ns": measurement.elapsed_ns,
        "attempted_throughput_rps": throughput(measurement.attempts, measurement.elapsed_ns),
        "successful_throughput_rps": throughput(measurement.observations.len(), measurement.elapsed_ns),
        "output_bytes": range(&output_bytes),
        "output_chunks": range(&output_chunks),
        "response_ready_ns": summary(response_ready),
        "first_byte_ns": summary(first_byte),
        "completion_ns": summary(completion),
    })
}

fn summary(mut values: Vec<u64>) -> Value {
    values.sort_unstable();
    let count = values.len();
    let total = values.iter().map(|&value| u128::from(value)).sum::<u128>();
    json!({
        "min": values.first().copied().unwrap_or(0),
        "mean": if count == 0 { 0 } else { (total / count as u128) as u64 },
        "p50": percentile(&values, 50),
        "p95": percentile(&values, 95),
        "p99": percentile(&values, 99),
        "max": values.last().copied().unwrap_or(0),
    })
}

fn range(values: &[usize]) -> Value {
    json!({
        "min": values.iter().copied().min().unwrap_or(0),
        "max": values.iter().copied().max().unwrap_or(0),
    })
}

fn percentile(sorted: &[u64], percent: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = percent
        .saturating_mul(sorted.len())
        .saturating_add(99)
        .checked_div(100)
        .unwrap_or(1)
        .max(1);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn throughput(iterations: usize, elapsed_ns: u64) -> f64 {
    if elapsed_ns == 0 {
        return 0.0;
    }
    iterations as f64 * 1_000_000_000.0 / elapsed_ns as f64
}

fn nanos(duration: std::time::Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

fn print_row(row: &Value) {
    let completion = &row["completion_ns"];
    println!(
        "workers={} scenario={} concurrency={} repeat={} p50={:.2}us p99={:.2}us throughput={:.0}rps errors={}",
        row["worker_threads"],
        row["scenario"].as_str().unwrap_or("unknown"),
        row["concurrency"],
        row["repeat"],
        completion["p50"].as_u64().unwrap_or(0) as f64 / 1_000.0,
        completion["p99"].as_u64().unwrap_or(0) as f64 / 1_000.0,
        row["successful_throughput_rps"].as_f64().unwrap_or(0.0),
        row["error_count"],
    );
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
