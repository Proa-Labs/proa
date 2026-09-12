// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Chunk timeline trace for streaming SSR.
//!
//! This focuses on an eBay-like marketplace PDP where several page sections
//! resolve at different times. It compares:
//! - buffered async SSR (`RouteResponse::ssr_async`)
//! - streamed SSR (`RouteResponse::ssr_stream`)
//!
//! Run with:
//! `cargo run -p proa_bench --release --bin stream_trace -- --iterations 25`

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use http_body_util::BodyExt as _;
use proa_bench::streaming_marketplace::{
    marketplace_loader, marketplace_preload_keys, MarketplaceBufferedPage, MarketplacePanelSpec,
    MarketplaceStreamPage, MARKETPLACE_PANEL_SPECS, MARKETPLACE_SHELL_MARKER,
};
use proa_framework_axum::manifest::IslandManifest;
use proa_framework_axum::{router_from_spec, Route, RouteContextLayer, RouteResponse};
use serde_json::json;
use tower::ServiceExt;

const DEFAULT_ITERATIONS: usize = 20;
const DEFAULT_WARMUP: usize = 3;

#[derive(Clone, Copy, Debug)]
enum Mode {
    Buffered,
    OutOfOrder,
    InOrder,
}

#[derive(Debug, Clone)]
struct TraceIteration {
    response_ready_ms: f64,
    first_chunk_ms: f64,
    complete_ms: f64,
    shell_visible_ms: Option<f64>,
    fallback_visible_ms: BTreeMap<String, f64>,
    ready_visible_ms: BTreeMap<String, f64>,
}

#[derive(Debug, Clone)]
struct StatSummary {
    avg_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    min_ms: f64,
    max_ms: f64,
}

fn empty_manifest() -> std::sync::Arc<IslandManifest> {
    std::sync::Arc::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: Default::default(),
    })
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iterations = parse_iterations(std::env::args().skip(1));

    let routes = vec![
        Route::endpoint(
            "/buffered",
            get(|| async {
                RouteResponse::ssr_async_with_loader_dyn(
                    MarketplaceBufferedPage,
                    marketplace_loader(1),
                )
            }),
        ),
        Route::endpoint(
            "/streamed",
            get(|| async {
                RouteResponse::ssr_stream_with_loader_dyn(
                    MarketplaceStreamPage,
                    marketplace_loader(1),
                )
            }),
        ),
        Route::endpoint(
            "/streamed-in-order",
            get(|| async {
                RouteResponse::ssr_stream_with_loader_dyn(
                    MarketplaceStreamPage,
                    marketplace_loader(1),
                )
                .with_preload_keys(marketplace_preload_keys())
                .with_render_mode(proa_core::RenderMode::ChunkedInOrder)
            }),
        ),
    ];

    let manifest = empty_manifest();
    let (router, computed_metadata) = router_from_spec(routes);
    let app = router.layer(RouteContextLayer::new(manifest, computed_metadata));

    println!("Streaming trace: marketplace PDP");
    println!("Warmup iterations: {}", DEFAULT_WARMUP);
    println!("Measured iterations: {}", iterations);
    println!();

    warmup(&app).await?;

    let buffered = run_mode(&app, Mode::Buffered, iterations).await?;
    let out_of_order = run_mode(&app, Mode::OutOfOrder, iterations).await?;
    let in_order = run_mode(&app, Mode::InOrder, iterations).await?;

    print_summary("Buffered async SSR", &buffered);
    println!();
    print_summary("Out-of-order streamed SSR", &out_of_order);
    println!();
    print_summary("In-order streamed SSR", &in_order);
    println!();
    print_comparison(&buffered, &out_of_order, &in_order);

    let results_path = PathBuf::from("proa_bench/results/proa-stream-marketplace.json");
    let payload = json!({
        "scenario": "marketplace_pdp_stream_trace",
        "warmup_iterations": DEFAULT_WARMUP,
        "iterations": iterations,
        "panels": MARKETPLACE_PANEL_SPECS.iter().map(panel_json).collect::<Vec<_>>(),
        "buffered": summary_json(&buffered),
        "out_of_order": summary_json(&out_of_order),
        "in_order": summary_json(&in_order),
    });
    fs::write(&results_path, serde_json::to_vec_pretty(&payload)?)?;
    println!("Results written to {}", results_path.display());

    Ok(())
}

async fn warmup(app: &axum::Router) -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..DEFAULT_WARMUP {
        let _ = trace_iteration(app.clone(), Mode::Buffered).await?;
        let _ = trace_iteration(app.clone(), Mode::OutOfOrder).await?;
        let _ = trace_iteration(app.clone(), Mode::InOrder).await?;
    }
    Ok(())
}

async fn run_mode(
    app: &axum::Router,
    mode: Mode,
    iterations: usize,
) -> Result<Vec<TraceIteration>, Box<dyn std::error::Error>> {
    let mut traces = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        traces.push(trace_iteration(app.clone(), mode).await?);
    }
    Ok(traces)
}

async fn trace_iteration(
    app: axum::Router,
    mode: Mode,
) -> Result<TraceIteration, Box<dyn std::error::Error>> {
    let uri = match mode {
        Mode::Buffered => "/buffered",
        Mode::OutOfOrder => "/streamed",
        Mode::InOrder => "/streamed-in-order",
    };

    let start = Instant::now();
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty())?)
        .await?;
    if response.status() != StatusCode::OK {
        return Err(format!("expected 200 OK, got {}", response.status()).into());
    }
    let response_ready_ms = elapsed_ms(start);

    let mut first_chunk_ms = None;
    let mut shell_visible_ms = None;
    let mut fallback_visible_ms = BTreeMap::new();
    let mut ready_visible_ms = BTreeMap::new();
    let mut body = response.into_body();
    let mut buffer = Vec::new();

    while let Some(frame) = body.frame().await {
        let frame = frame?;
        let Ok(data) = frame.into_data() else {
            continue;
        };

        let now_ms = elapsed_ms(start);
        first_chunk_ms.get_or_insert(now_ms);
        buffer.extend_from_slice(data.as_ref());

        let html = String::from_utf8_lossy(&buffer);
        if shell_visible_ms.is_none() && html.contains(MARKETPLACE_SHELL_MARKER) {
            shell_visible_ms = Some(now_ms);
        }

        for panel in MARKETPLACE_PANEL_SPECS {
            let fallback_marker = format!("data-panel-fallback=\"{}\"", panel.key);
            if !fallback_visible_ms.contains_key(panel.key) && html.contains(&fallback_marker) {
                fallback_visible_ms.insert(panel.key.to_string(), now_ms);
            }

            let ready_marker = format!("data-panel-ready=\"{}\"", panel.key);
            if !ready_visible_ms.contains_key(panel.key) && html.contains(&ready_marker) {
                ready_visible_ms.insert(panel.key.to_string(), now_ms);
            }
        }
    }

    Ok(TraceIteration {
        response_ready_ms,
        first_chunk_ms: first_chunk_ms.unwrap_or(response_ready_ms),
        complete_ms: elapsed_ms(start),
        shell_visible_ms,
        fallback_visible_ms,
        ready_visible_ms,
    })
}

fn print_summary(label: &str, traces: &[TraceIteration]) {
    println!("{label}");
    println!(
        "  response ready: {}",
        fmt_stat(&summarize(traces.iter().map(|t| t.response_ready_ms)))
    );
    println!(
        "  first chunk:    {}",
        fmt_stat(&summarize(traces.iter().map(|t| t.first_chunk_ms)))
    );
    println!(
        "  complete:       {}",
        fmt_stat(&summarize(traces.iter().map(|t| t.complete_ms)))
    );

    if traces.iter().any(|t| t.shell_visible_ms.is_some()) {
        println!(
            "  shell visible:  {}",
            fmt_stat(&summarize(traces.iter().filter_map(|t| t.shell_visible_ms)))
        );
    }

    println!("  panel fallbacks:");
    for panel in MARKETPLACE_PANEL_SPECS {
        let values = traces
            .iter()
            .filter_map(|trace| trace.fallback_visible_ms.get(panel.key).copied());
        let stat = summarize(values);
        println!("    {:<12} {}", panel.key, fmt_stat(&stat));
    }

    println!("  panel ready:");
    for panel in MARKETPLACE_PANEL_SPECS {
        let values = traces
            .iter()
            .filter_map(|trace| trace.ready_visible_ms.get(panel.key).copied());
        let stat = summarize(values);
        println!("    {:<12} {}", panel.key, fmt_stat(&stat));
    }
}

fn print_comparison(
    buffered: &[TraceIteration],
    out_of_order: &[TraceIteration],
    in_order: &[TraceIteration],
) {
    let buffered_complete = summarize(buffered.iter().map(|t| t.complete_ms));
    let out_of_order_shell = summarize(out_of_order.iter().filter_map(|t| t.shell_visible_ms));
    let out_of_order_complete = summarize(out_of_order.iter().map(|t| t.complete_ms));
    let in_order_shell = summarize(in_order.iter().filter_map(|t| t.shell_visible_ms));
    let in_order_complete = summarize(in_order.iter().map(|t| t.complete_ms));

    println!("Marketplace takeaways");
    println!(
        "  out-of-order shell lead vs buffered complete: {:.2} ms",
        buffered_complete.avg_ms - out_of_order_shell.avg_ms
    );
    println!(
        "  in-order shell lead vs buffered complete: {:.2} ms",
        buffered_complete.avg_ms - in_order_shell.avg_ms
    );
    println!(
        "  out-of-order complete overhead vs buffered complete: {:.2} ms",
        out_of_order_complete.avg_ms - buffered_complete.avg_ms
    );
    println!(
        "  in-order complete overhead vs buffered complete: {:.2} ms",
        in_order_complete.avg_ms - buffered_complete.avg_ms
    );
    for panel in MARKETPLACE_PANEL_SPECS {
        let out_of_order_ready = summarize(
            out_of_order
                .iter()
                .filter_map(|trace| trace.ready_visible_ms.get(panel.key).copied()),
        );
        let in_order_ready = summarize(
            in_order
                .iter()
                .filter_map(|trace| trace.ready_visible_ms.get(panel.key).copied()),
        );
        println!(
            "  {:<12} reveal lead vs buffered complete: {:.2} ms (out-of-order) / {:.2} ms (in-order)",
            panel.key,
            buffered_complete.avg_ms - out_of_order_ready.avg_ms,
            buffered_complete.avg_ms - in_order_ready.avg_ms
        );
    }
}

fn summary_json(traces: &[TraceIteration]) -> serde_json::Value {
    let fallback = MARKETPLACE_PANEL_SPECS
        .iter()
        .map(|panel| {
            (
                panel.key.to_string(),
                stat_json(&summarize(traces.iter().filter_map(|trace| {
                    trace.fallback_visible_ms.get(panel.key).copied()
                }))),
            )
        })
        .collect::<serde_json::Map<_, _>>();

    let ready = MARKETPLACE_PANEL_SPECS
        .iter()
        .map(|panel| {
            (
                panel.key.to_string(),
                stat_json(&summarize(traces.iter().filter_map(|trace| {
                    trace.ready_visible_ms.get(panel.key).copied()
                }))),
            )
        })
        .collect::<serde_json::Map<_, _>>();

    json!({
        "response_ready_ms": stat_json(&summarize(traces.iter().map(|t| t.response_ready_ms))),
        "first_chunk_ms": stat_json(&summarize(traces.iter().map(|t| t.first_chunk_ms))),
        "complete_ms": stat_json(&summarize(traces.iter().map(|t| t.complete_ms))),
        "shell_visible_ms": stat_json(&summarize(traces.iter().filter_map(|t| t.shell_visible_ms))),
        "fallback_visible_ms": fallback,
        "ready_visible_ms": ready,
    })
}

fn panel_json(panel: &MarketplacePanelSpec) -> serde_json::Value {
    json!({
        "key": panel.key,
        "title": panel.title,
        "delay_ms": panel.delay_ms,
        "fallback_label": panel.fallback_label,
    })
}

fn stat_json(stat: &StatSummary) -> serde_json::Value {
    json!({
        "avg_ms": stat.avg_ms,
        "p50_ms": stat.p50_ms,
        "p95_ms": stat.p95_ms,
        "min_ms": stat.min_ms,
        "max_ms": stat.max_ms,
    })
}

fn summarize(values: impl IntoIterator<Item = f64>) -> StatSummary {
    let mut values: Vec<f64> = values.into_iter().collect();
    if values.is_empty() {
        return StatSummary {
            avg_ms: 0.0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            min_ms: 0.0,
            max_ms: 0.0,
        };
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let len = values.len();
    let avg_ms = values.iter().sum::<f64>() / len as f64;

    StatSummary {
        avg_ms,
        p50_ms: percentile(&values, 0.50),
        p95_ms: percentile(&values, 0.95),
        min_ms: values[0],
        max_ms: values[len - 1],
    }
}

fn percentile(sorted_values: &[f64], percentile: f64) -> f64 {
    let idx = ((sorted_values.len() - 1) as f64 * percentile).round() as usize;
    sorted_values[idx]
}

fn fmt_stat(stat: &StatSummary) -> String {
    format!(
        "avg {:.2} ms | p50 {:.2} | p95 {:.2} | min {:.2} | max {:.2}",
        stat.avg_ms, stat.p50_ms, stat.p95_ms, stat.min_ms, stat.max_ms
    )
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn parse_iterations(args: impl Iterator<Item = String>) -> usize {
    let args: Vec<String> = args.collect();
    args.windows(2)
        .find_map(|window| {
            if window[0] == "--iterations" {
                window[1].parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(DEFAULT_ITERATIONS)
}
