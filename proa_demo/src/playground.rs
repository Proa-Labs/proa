// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Proa Playground — interactive demo server.
//!
//! Run:
//!   cargo proa run -p proa_demo --bin proa-playground --features server --profile release
//!
//! Opens http://localhost:5000 with a JSON editor + live preview + timing.

use axum::{
    extract::Json,
    http::{header, HeaderMap},
    response::{Html, IntoResponse},
    routing::{get, post},
    Extension, Router,
};
use proa_core::{RenderBuffer, WebContext, WriteBuf};
use proa_showcase::airbnb;
use proa_showcase::nike;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// Return the root directory for serving public assets.
///
/// **Packaged distribution**: looks for a `public/` directory next to the
/// running binary (set up by the distribution packaging step).
///
/// **Development** (`cargo run`): falls back to this repository's
/// `proa_marketing/public`.
fn assets_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        // Packaged: `public/` next to the binary
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let packaged = exe_dir.join("public");
                if packaged.exists() {
                    return packaged;
                }
            }
        }
        // Standalone-checkout fallback.
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("proa_marketing")
            .join("public")
    })
}

/// Return the root directory for the Hono SSR companion server.
///
/// Same resolution order as [`assets_root`]: binary-relative first, then
/// `CARGO_MANIFEST_DIR`.
fn hono_root() -> Option<&'static PathBuf> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        // Packaged: `hono-ssr/` next to the binary
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let packaged = exe_dir.join("hono-ssr");
                if packaged.exists() {
                    return Some(packaged);
                }
            }
        }
        // Dev fallback
        let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("hono-ssr");
        if dev.exists() {
            Some(dev)
        } else {
            None
        }
    })
    .as_ref()
}

/// Return the root directory for the React SSR companion server.
fn react_ssr_root() -> Option<&'static PathBuf> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let packaged = exe_dir.join("react-ssr");
                if packaged.exists() {
                    return Some(packaged);
                }
            }
        }
        let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("react-ssr");
        if dev.exists() {
            Some(dev)
        } else {
            None
        }
    })
    .as_ref()
}

/// Return the root directory for the Next.js SSR companion app.
fn next_root() -> Option<&'static PathBuf> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let packaged = exe_dir.join("next-ssr");
                if packaged.exists() {
                    return Some(packaged);
                }
            }
        }
        let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("next-ssr");
        if dev.exists() {
            Some(dev)
        } else {
            None
        }
    })
    .as_ref()
}

fn localhost_port_open(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(150)).is_ok()
}

/// High-resolution monotonic timestamp in microseconds.
///
/// On Unix, uses `CLOCK_THREAD_CPUTIME_ID` (immune to OS preemption and
/// context switches). On other platforms, falls back to `Instant::now()`.
fn thread_cpu_us() -> f64 {
    #[cfg(unix)]
    {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
        ts.tv_sec as f64 * 1_000_000.0 + ts.tv_nsec as f64 / 1_000.0
    }
    #[cfg(not(unix))]
    {
        use std::time::Instant;
        static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let epoch = *EPOCH.get_or_init(Instant::now);
        epoch.elapsed().as_secs_f64() * 1_000_000.0
    }
}

/// Measure a closure's wall-clock cost in microseconds.
#[inline]
fn measure_cpu<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let start = std::time::Instant::now();
    let result = f();
    let us = start.elapsed().as_secs_f64() * 1_000_000.0;
    (result, us)
}

/// Measure a closure's thread CPU cost in microseconds.
#[inline]
fn measure_thread_cpu<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let start = thread_cpu_us();
    let result = f();
    let us = thread_cpu_us() - start;
    (result, us)
}

#[derive(Deserialize)]
#[serde(default)]
struct SentryOtelPlaygroundProps {
    logs: Vec<proa_showcase::sentry_otel::SentryLogEntry>,
    trace_name: String,
    trace_id: String,
    issues: u32,
    spans: u32,
    age: String,
    root_duration: String,
    log_count: u32,
    client_ip: String,
    browser: String,
    environment: String,
}

impl Default for SentryOtelPlaygroundProps {
    fn default() -> Self {
        Self {
            logs: Vec::new(),
            trace_name: "pageload".to_string(),
            trace_id: "9f97da3a648f".to_string(),
            issues: 2,
            spans: 102,
            age: "37m".to_string(),
            root_duration: "108.00ms".to_string(),
            log_count: 40,
            client_ip: "192.0.2.42".to_string(),
            browser: "Chrome 148.0.0".to_string(),
            environment: "local".to_string(),
        }
    }
}

impl SentryOtelPlaygroundProps {
    fn page_props(&self) -> proa_showcase::sentry_otel::SentryPageProps<'_> {
        proa_showcase::sentry_otel::SentryPageProps {
            row_count: self.logs.len(),
            logs: Some(&self.logs),
            trace_name: &self.trace_name,
            trace_id: &self.trace_id,
            issues: self.issues,
            spans: self.spans,
            age: &self.age,
            root_duration: &self.root_duration,
            log_count: self.log_count,
            client_ip: &self.client_ip,
            browser: &self.browser,
            environment: &self.environment,
        }
    }
}

fn espn_default_props_json() -> serde_json::Value {
    serde_json::to_value(&*proa_showcase::espn::GAME)
        .expect("embedded ESPN game fixture must serialize")
}

fn sentry_otel_default_props_json() -> serde_json::Value {
    let logs = (0..proa_showcase::sentry_otel::DEFAULT_ROW_COUNT)
        .map(|index| {
            let pattern = &proa_showcase::sentry_otel::LOG_PATTERNS
                [index % proa_showcase::sentry_otel::LOG_PATTERNS.len()];
            let seconds = 40_u32.saturating_sub((index as u32) % 40);
            let millis = 283_u32.saturating_sub((index as u32 * 17) % 280);
            serde_json::json!({
                "timestamp": format!("May 22, 15:16:{seconds:02}.{millis:03}"),
                "message": pattern.message,
                "severity": pattern.severity,
                "source": pattern.source_label,
                "span_id": format!("{:016x}", 0xa62b_b7e2_1ef8_4c91_u64.wrapping_add(index as u64)),
                "payload_size": u32::from(pattern.payload_size) + (index as u32 % 97),
                "checkout_tax_cents": 528 + (index as u32 % 31),
                "checkout_total_cents": 6_928 + (index as u32 * 13),
                "code_line": 108 + (index as u32 % 19),
                "code_path": if pattern.source_label == "React" { "/srv/checkout/web/src/checkout.tsx" } else { "/srv/checkout/app/services/checkout.py" },
                "function_name": if pattern.source_label == "React" { "submitCheckout" } else { "calculate_tax" },
                "environment": "local",
                "instrumentation_name": "checkout_trace_lab",
                "origin": if pattern.source_label == "React" { "auto.browser.logs" } else { "auto.otlp.logs" },
                "service_name": if pattern.source_label == "React" { "checkout-trace-lab-frontend" } else { "checkout-trace-lab-backend" },
                "sdk_language": if pattern.source_label == "React" { "javascript" } else { "python" },
                "sdk_name": "opentelemetry",
                "sdk_version": if pattern.source_label == "React" { "2.0.1" } else { "1.42.0" },
                "platform": if pattern.source_label == "React" { "javascript" } else { "python" },
                "trace_id": "9f97da3a648f4d46a930c323a1aeaf7c",
                "expanded": index % 24 == 1
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "logs": logs,
        "trace_name": "pageload",
        "trace_id": "9f97da3a648f",
        "issues": 2,
        "spans": 102,
        "age": "37m",
        "root_duration": "108.00ms",
        "log_count": proa_showcase::sentry_otel::DEFAULT_ROW_COUNT,
        "client_ip": "192.0.2.42",
        "browser": "Chrome 148.0.0",
        "environment": "local"
    })
}

thread_local! {
    static RENDER_BUF: RefCell<RenderBuffer> = RefCell::new(RenderBuffer::new());
}

/// Default props as raw JSON strings per page — avoids Value intermediary.
static DEFAULT_PROPS_RAW: std::sync::OnceLock<std::collections::HashMap<String, String>> =
    std::sync::OnceLock::new();

fn default_props_raw() -> &'static std::collections::HashMap<String, String> {
    DEFAULT_PROPS_RAW.get_or_init(|| {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../showcase-props.json")).unwrap();
        if let Some(obj) = value.as_object_mut() {
            obj.insert("ebay-srp".to_string(), build_ebay_default_props_json());
            obj.insert("ebay-srp-rsjs".to_string(), build_ebay_default_props_json());
            obj.insert(
                "github-pr-diff".to_string(),
                proa_demo::github_pr_diff::default_props_json(),
            );
            obj.insert("espn-gamecast".to_string(), espn_default_props_json());
            obj.insert(
                "sentry-otel-logs".to_string(),
                sentry_otel_default_props_json(),
            );
        }
        value
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::to_string(v).unwrap()))
            .collect()
    })
}

use proa_demo::json_types;

// ---------------------------------------------------------------------------
// API types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RenderRequest {
    page: String,
    props: Option<Box<serde_json::value::RawValue>>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct PlaygroundOptions {
    pub load_tests_enabled: bool,
}

impl PlaygroundOptions {
    pub const fn standalone() -> Self {
        Self {
            load_tests_enabled: true,
        }
    }

    pub fn marketing() -> Self {
        Self {
            load_tests_enabled: env_flag("PROA_CONSOLE_LOAD_TESTS", false),
        }
    }
}

fn env_flag(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" | "enabled" => true,
            "0" | "false" | "no" | "off" | "disabled" => false,
            _ => default,
        },
        Err(_) => default,
    }
}

// proa-perf-ok(scope): cold console configuration rewrites one trusted static shell outside render paths
fn load_tests_disabled_html() -> String {
    PLAYGROUND_HTML
        .replacen(
            "</head>",
            r#"<style>
  #loadtest-preset,
  #loadtest-btn { display: none !important; }
  #loadtest-disabled-note {
    display: inline-flex;
    min-height: 42px;
    align-items: center;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: rgba(255, 255, 255, 0.035);
    color: var(--text-muted);
    padding: 0 14px;
    font-family: var(--font);
    font-size: 12px;
  }
</style></head>"#,
            1,
        )
        .replacen(
            r#"<select id="loadtest-preset""#,
            r#"<span id="loadtest-disabled-note">Load tests disabled</span><select id="loadtest-preset""#,
            1,
        )
}

async fn index(Extension(options): Extension<PlaygroundOptions>) -> Html<Cow<'static, str>> {
    if options.load_tests_enabled {
        Html(Cow::Borrowed(PLAYGROUND_HTML))
    } else {
        Html(Cow::Owned(load_tests_disabled_html()))
    }
}

async fn render_page(headers: HeaderMap, Json(req): Json<RenderRequest>) -> impl IntoResponse {
    let page = req.page.to_string();
    let silent = headers
        .get("x-proa-silent")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

    // Get raw JSON string — either from the request or from pre-stored defaults.
    let props_str: String = match &req.props {
        Some(raw) => raw.get().to_string(),
        None => default_props_raw()
            .get(page.as_str())
            .cloned()
            .unwrap_or_else(|| "null".to_string()),
    };

    // Move CPU work to a blocking thread so the event loop stays responsive.
    let page_name = page.clone();
    let result = tokio::task::spawn_blocking(move || {
        let e2e_start = std::time::Instant::now();

        match page.as_str() {
            "home" => {
                let (owned, deser_us) = match measure_cpu(|| {
                    serde_json::from_str::<json_types::HomeDataOwned>(&props_str)
                }) {
                    (Ok(d), us) => (d, us),
                    (Err(e), _) => return Err(e.to_string()),
                };
                let borrowed = owned.borrow_collections();
                let data = owned.to_home_data(&borrowed);
                let (html, render_us) = render_response_with_bump(|cx| {
                    airbnb::home::render_into(cx, &data);
                });
                let e2e_us = e2e_start.elapsed().as_secs_f64() * 1_000_000.0;
                Ok((html, render_us, render_us, deser_us, e2e_us))
            }
            "search" => {
                let (owned, deser_us) = match measure_cpu(|| {
                    serde_json::from_str::<json_types::SearchDataOwned>(&props_str)
                }) {
                    (Ok(d), us) => (d, us),
                    (Err(e), _) => return Err(e.to_string()),
                };
                let borrowed = owned.borrow_collections();
                let data = owned.to_search_data(&borrowed);
                let (html, render_us) = render_response_with_bump(|cx| {
                    airbnb::search::render_into(cx, &data);
                });
                let e2e_us = e2e_start.elapsed().as_secs_f64() * 1_000_000.0;
                Ok((html, render_us, render_us, deser_us, e2e_us))
            }
            "listing" => {
                let (owned, deser_us) = match measure_cpu(|| {
                    serde_json::from_str::<json_types::ListingDataOwned>(&props_str)
                }) {
                    (Ok(d), us) => (d, us),
                    (Err(e), _) => return Err(e.to_string()),
                };
                let borrowed = owned.borrow_collections();
                let data = owned.to_listing_data(&borrowed);
                let (html, render_us) = render_response_with_bump(|cx| {
                    let _ = airbnb::listing::render_into(cx, &data, None);
                });
                let e2e_us = e2e_start.elapsed().as_secs_f64() * 1_000_000.0;
                Ok((html, render_us, render_us, deser_us, e2e_us))
            }
            "nike-catalog" => {
                let (owned, deser_us) = match measure_cpu(|| {
                    serde_json::from_str::<json_types::NikeCatalogDataOwned>(&props_str)
                }) {
                    (Ok(d), us) => (d, us),
                    (Err(e), _) => return Err(e.to_string()),
                };
                let borrowed = owned.borrow_collections();
                let products = owned.build_products(&borrowed);
                let data = nike::catalog::CatalogData {
                    title: &owned.title,
                    products: &products,
                };
                let (html, render_us) = render_response_with_bump(|cx| {
                    nike::catalog::render_into(cx, &data);
                });
                let e2e_us = e2e_start.elapsed().as_secs_f64() * 1_000_000.0;
                Ok((html, render_us, render_us, deser_us, e2e_us))
            }
            "nike-pdp" => {
                let (owned, deser_us) = match measure_cpu(|| {
                    serde_json::from_str::<json_types::NikePdpDataOwned>(&props_str)
                }) {
                    (Ok(d), us) => (d, us),
                    (Err(e), _) => return Err(e.to_string()),
                };
                let borrowed = owned.borrow_collections();
                let data = owned.to_product_data(&borrowed);
                let (html, render_us) = render_response_with_bump(|cx| {
                    nike::product::render_into(cx, &data);
                });
                let e2e_us = e2e_start.elapsed().as_secs_f64() * 1_000_000.0;
                Ok((html, render_us, render_us, deser_us, e2e_us))
            }
            "ebay-srp" => {
                // Time the JSON-driven listings build (matches the
                // `measure_cpu` shape used by the airbnb / nike pages so
                // the playground's "JSON deser" stat populates).
                let (listings, deser_us) = measure_cpu(|| parse_ebay_props(&props_str));
                // Single-pass render: doctype, head, chrome, cards, all
                // into one buffer with no intermediate copies.
                let (html, render_us) = measure_thread_cpu(|| {
                    render_srp_response(
                        proa_demo::ebay_srp_chrome_real::DEFAULT_QUERY,
                        &listings,
                        proa_demo::ebay_srp::ListingsList {
                            listings: &listings,
                        },
                        (),
                    )
                });
                let e2e_us = e2e_start.elapsed().as_secs_f64() * 1_000_000.0;
                Ok((html, render_us, render_us, deser_us, e2e_us))
            }
            "ebay-srp-rsjs" => {
                let (listings, deser_us) = measure_cpu(|| parse_ebay_props(&props_str));
                // Single-pass render: the response owner snapshots the
                // artifacts recorded while the river renders, then inserts
                // the shared runtime plan at the document tail.
                let (html, render_us) = measure_thread_cpu(|| {
                    render_srp_response(
                        proa_demo::ebay_srp_chrome_real::DEFAULT_QUERY,
                        &listings,
                        SrpRsjsRiver {
                            chips: proa_demo::ebay_srp_islands::FilterChips::default(),
                            listings: &listings,
                        },
                        SrpUiEnhancements,
                    )
                });
                let e2e_us = e2e_start.elapsed().as_secs_f64() * 1_000_000.0;
                Ok((html, render_us, render_us, deser_us, e2e_us))
            }
            "github-pr-diff" => {
                let (props, deser_us) = measure_cpu(|| {
                    proa_demo::github_pr_diff::GithubPrDiffProps::from_json(&props_str)
                });
                let (html, render_us) = render_response_with_bump(|cx| {
                    use proa_core::WebRenderSync;
                    let _ =
                        proa_demo::github_pr_diff::GithubPrDiffPage { props: &props }.render(cx);
                });
                let e2e_us = e2e_start.elapsed().as_secs_f64() * 1_000_000.0;
                Ok((html, render_us, render_us, deser_us, e2e_us))
            }
            "espn-gamecast" => {
                let (props, deser_us) = match measure_cpu(|| {
                    serde_json::from_str::<proa_showcase::espn::GameData>(&props_str)
                }) {
                    (Ok(props), us) => (props, us),
                    (Err(error), _) => return Err(error.to_string()),
                };
                let (html, render_us) = render_response_with_bump(|cx| {
                    let _ = proa_showcase::espn::render_into(cx, &props);
                });
                let e2e_us = e2e_start.elapsed().as_secs_f64() * 1_000_000.0;
                Ok((html, render_us, render_us, deser_us, e2e_us))
            }
            "sentry-otel-logs" => {
                let (props, deser_us) = match measure_cpu(|| {
                    serde_json::from_str::<SentryOtelPlaygroundProps>(&props_str)
                }) {
                    (Ok(props), us) => (props, us),
                    (Err(error), _) => return Err(error.to_string()),
                };
                let (html, render_us) = render_response_with_bump(|cx| {
                    let _ =
                        proa_showcase::sentry_otel::render_into_with_props(cx, props.page_props());
                });
                let e2e_us = e2e_start.elapsed().as_secs_f64() * 1_000_000.0;
                Ok((html, render_us, render_us, deser_us, e2e_us))
            }
            _ => Err("unknown page".to_string()),
        }
    })
    .await;

    match result {
        Ok(Ok((html, render_us, doc_us, deser_us, e2e_us))) => {
            if !silent {
                log_render("proa", &page_name, render_us, deser_us, e2e_us, html.len());
            }
            // Return raw HTML with timing in headers — avoids JSON escaping
            // of the entire HTML body (saves ~1-3ms on large pages).
            let html_size = html.len();
            axum::response::Response::builder()
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .header("x-render-us", format!("{render_us:.3}"))
                .header("x-doc-us", format!("{doc_us:.3}"))
                .header("x-deser-us", format!("{deser_us:.3}"))
                .header("x-e2e-us", format!("{e2e_us:.3}"))
                .header("x-html-size", html_size.to_string())
                .body(axum::body::Body::from(html))
                .unwrap()
                .into_response()
        }
        Ok(Err(e)) => err_response_str(&e),
        Err(e) => err_response_str(&e.to_string()),
    }
}

/// Number of timed render iterations — we report the minimum to eliminate noise.
const RENDER_ITERS: usize = 10;

/// Render N times into the thread-local chained bump writer, return HTML + min render time.
/// The minimum filters out CPU frequency scaling, cache cold-starts, and scheduling jitter.
fn render_with_bump(f: impl Fn(&mut RenderBuffer)) -> (String, f64) {
    RENDER_BUF.with(|cell| {
        let mut buf = cell.borrow_mut();

        // Timed: take the minimum of N iterations.
        // Uses thread_cpu_us() (CLOCK_THREAD_CPUTIME_ID on Unix) to measure
        // pure CPU time, immune to context switches and scheduler preemption.
        let mut min_us = f64::MAX;
        for _ in 0..RENDER_ITERS {
            buf.clear();
            let t = thread_cpu_us();
            f(&mut buf);
            let us = thread_cpu_us() - t;
            if us < min_us {
                min_us = us;
            }
        }
        let html = String::from_utf8(buf.to_vec()).unwrap_or_default();
        (html, min_us)
    })
}

/// Render a complete HTTP response into the reusable buffer and let the
/// response owner finalize the rsjs artifacts recorded by that render.
///
/// Runtime planning is deliberately outside the timed render span. The
/// playground reports template-render cost separately from response asset
/// finalization, just as the framework does.
fn render_response_with_bump(
    f: impl Fn(&mut WebContext<::proa_core::NoLoader, RenderBuffer>),
) -> (String, f64) {
    RENDER_BUF.with(|cell| {
        let mut buf = cell.borrow_mut();
        let mut min_us = f64::MAX;
        let mut artifacts = proa_core::RsjsRenderArtifacts::default();

        for _ in 0..RENDER_ITERS {
            buf.clear();
            let mut cx = WebContext::with_buffer(std::mem::take(&mut *buf));
            let started = thread_cpu_us();
            f(&mut cx);
            let elapsed = thread_cpu_us() - started;
            artifacts = cx.rsjs_artifacts_snapshot();
            *buf = cx.into_output();
            min_us = min_us.min(elapsed);
        }

        let html = String::from_utf8(buf.to_vec()).unwrap_or_default();
        (finalize_rsjs_html(html, &artifacts), min_us)
    })
}

/// Render one owned response and finalize its recorded rsjs artifacts.
fn render_owned_response(f: impl FnOnce(&mut WebContext)) -> String {
    let mut cx = WebContext::new();
    f(&mut cx);
    let artifacts = cx.rsjs_artifacts_snapshot();
    let out = cx.into_output();
    finalize_rsjs_html(out.into_string(), &artifacts)
}

/// Render the eBay reference document through an externally owned
/// context so the response finalizer—not a tail component—owns rsjs planning.
fn render_srp_response<R, T>(
    query: &str,
    listings: &[proa_demo::ebay_srp::Listing],
    river: R,
    tail: T,
) -> String
where
    R: proa_core::WebRenderSync,
    T: proa_core::WebRenderSync,
{
    use proa_core::WebRenderSync;

    let total = 117_406 + (listings.len() as u64) * 7;
    render_owned_response(move |cx| {
        let _ = proa_demo::ebay_srp_chrome_real::RealSrpDocument {
            query,
            total,
            river,
            tail,
        }
        .render(cx);
    })
}

fn err_response_str(e: &str) -> axum::response::Response {
    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": e })),
    )
        .into_response()
}

async fn default_props() -> impl IntoResponse {
    // Inject the full eBay fixture array (all 60 cards expanded with
    // every field) so the user can edit any listing, not just the first few.
    let mut v: serde_json::Value = serde_json::from_str(include_str!("../showcase-props.json"))
        .unwrap_or(serde_json::Value::Null);
    if let Some(obj) = v.as_object_mut() {
        obj.insert("ebay-srp".to_string(), build_ebay_default_props_json());
        obj.insert("ebay-srp-rsjs".to_string(), build_ebay_default_props_json());
        obj.insert(
            "github-pr-diff".to_string(),
            proa_demo::github_pr_diff::default_props_json(),
        );
        obj.insert("espn-gamecast".to_string(), espn_default_props_json());
        obj.insert(
            "sentry-otel-logs".to_string(),
            sentry_otel_default_props_json(),
        );
    }
    (
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string()),
    )
}

fn build_ebay_default_props_json() -> serde_json::Value {
    use proa_demo::ebay_srp::{default_listing, CaptionSignal, ListingFormat};
    const COUNT: usize = 60;
    let listings: Vec<serde_json::Value> = (0..COUNT)
        .map(|i| {
            let l = default_listing(i);
            let caption = match l.caption {
                CaptionSignal::None => "None",
                CaptionSignal::NewLowPrice => "NewLowPrice",
                CaptionSignal::Trending => "Trending",
                CaptionSignal::EndsSoon => "EndsSoon",
                CaptionSignal::EditorsPick => "EditorsPick",
            };
            let format = match l.format {
                ListingFormat::BuyItNow => "BuyItNow",
                ListingFormat::Auction => "Auction",
                ListingFormat::BestOffer => "BestOffer",
            };
            let subtitle = l
                .subtitle_parts
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(" · ");
            serde_json::json!({
                "title": l.title.as_ref(),
                "subtitle": subtitle,
                "imageHash": l.image_hash.as_ref(),
                "priceCents": l.price_cents,
                "originalPriceCents": l.original_price_cents,
                "freeDelivery": l.delivery_cents == 0,
                "deliveryCents": l.delivery_cents,
                "location": l.location.as_ref(),
                "watcherCount": l.watcher_count,
                "couponPct": l.coupon_pct,
                "starsFull": l.stars_full,
                "hasHalfStar": l.has_half_star,
                "reviewCount": l.review_count,
                "format": format,
                "caption": caption,
                "sellerName": l.seller_name.as_ref(),
                "sellerFeedbackPctX10": l.seller_feedback_pct_x10,
                "sellerFeedbackCount": l.seller_feedback_count,
            })
        })
        .collect();
    serde_json::json!({ "count": COUNT, "listings": listings })
}

async fn health() -> impl IntoResponse {
    axum::Json(serde_json::json!({"status": "ok", "engine": "proa"}))
}

async fn openapi_spec() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        include_str!("../openapi.json"),
    )
}

async fn api_docs() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1.0"/>
<title>Proa Playground — API Docs</title>
</head>
<body>
<script id="api-reference" data-url="/api/openapi.json"></script>
<script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
</body>
</html>"#,
    )
}

// ---------------------------------------------------------------------------
// Proxy helpers — shared across Node and Bun handlers
// ---------------------------------------------------------------------------

/// Rewrite `/showcase/{airbnb,nike}/proa` links in HTML to use the given engine.
/// The React components render with `/proa` paths; the proxy swaps them so that
/// links stay within the same engine (node or bun) when viewing those versions.
fn rewrite_links_for_engine(html: &[u8], engine: &str) -> Vec<u8> {
    let s = String::from_utf8_lossy(html);
    let rewritten = s
        .replace(
            "/showcase/airbnb/proa",
            &format!("/showcase/airbnb/{engine}"),
        )
        .replace("/showcase/nike/proa", &format!("/showcase/nike/{engine}"));
    rewritten.into_bytes()
}

/// Declare one SSR companion origin plus its URL builder.
///
/// The playground renders Proa in-process and reaches every other engine over
/// HTTP, so each companion needs an address. The defaults are the local-dev
/// ports; the environment variable is what lets a deployment say where the
/// companion actually runs. Without it the origin is compiled in, and an
/// engine that is not listening on that exact port can only fail.
///
/// Keep companions on loopback where you can. These timings are published as
/// a comparison against in-process Proa, so putting a companion on the far
/// side of a network hop charges it for latency Proa never pays.
macro_rules! companion_origin {
    ($origin_fn:ident, $url_fn:ident, $env:literal, $default:literal) => {
        #[doc = concat!("Origin of the SSR companion, from `", $env, "` (default `", $default, "`).")]
        fn $origin_fn() -> &'static str {
            static ORIGIN: OnceLock<String> = OnceLock::new();
            ORIGIN.get_or_init(|| {
                std::env::var($env)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| $default.to_string())
                    .trim_end_matches('/')
                    .to_string()
            })
        }

        #[doc = concat!("Build a URL against the `", $env, "` companion origin.")]
        fn $url_fn(path: &str) -> String {
            format!("{}{}", $origin_fn(), path)
        }
    };
}

companion_origin!(
    node_origin,
    node_url,
    "PROA_NODE_SSR_ORIGIN",
    "http://localhost:5100"
);
companion_origin!(
    bun_origin,
    bun_url,
    "PROA_BUN_SSR_ORIGIN",
    "http://localhost:5200"
);
companion_origin!(
    marko_node_origin,
    marko_node_url,
    "PROA_MARKO_NODE_ORIGIN",
    "http://localhost:5300"
);
companion_origin!(
    marko_bun_origin,
    marko_bun_url,
    "PROA_MARKO_BUN_ORIGIN",
    "http://localhost:5400"
);
companion_origin!(
    react_ssr_origin,
    react_ssr_url,
    "PROA_REACT_SSR_ORIGIN",
    "http://localhost:5401"
);
companion_origin!(
    next_origin,
    next_url,
    "PROA_NEXT_SSR_ORIGIN",
    "http://localhost:5402"
);

/// Fetch a URL and return its body as HTML, rewriting links for the given engine.
async fn proxy_html(url: &str, engine: &str) -> axum::response::Response {
    match reqwest::get(url).await {
        Ok(resp) => {
            let status = axum::http::StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
            let body = resp.bytes().await.unwrap_or_default();
            let rewritten = rewrite_links_for_engine(&body, engine);
            (
                status,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string())],
                rewritten,
            )
                .into_response()
        }
        Err(_) => (axum::http::StatusCode::BAD_GATEWAY, "Server not ready").into_response(),
    }
}

/// Fetch a React SSR HTML page through the server-side companion process.
async fn proxy_react_ssr_html(path: &str) -> axum::response::Response {
    let url = react_ssr_url(path);
    match reqwest::get(&url).await {
        Ok(resp) => {
            let status = axum::http::StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
            let body = resp.bytes().await.unwrap_or_default();
            let html = rewrite_links_for_engine(&body, "react");
            let s = String::from_utf8_lossy(&html);
            let rewritten = s
                .replace(
                    "src=\"/client.bundle.js\"",
                    "src=\"/react-ssr/client.bundle.js\"",
                )
                .replace(
                    "src=\"/espn-client.bundle.js\"",
                    "src=\"/react-ssr/espn-client.bundle.js\"",
                )
                .replace(
                    "src=\"/sentry-otel-client.bundle.js\"",
                    "src=\"/react-ssr/sentry-otel-client.bundle.js\"",
                );
            (
                status,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string())],
                rewritten.into_bytes(),
            )
                .into_response()
        }
        Err(_) => (axum::http::StatusCode::BAD_GATEWAY, "Server not ready").into_response(),
    }
}

/// Fetch a Next.js HTML page, rewrite showcase links, and route framework
/// assets through a playground-prefixed proxy so `/showcase/.../next` works.
async fn proxy_next_html(url: &str) -> axum::response::Response {
    match reqwest::get(url).await {
        Ok(resp) => {
            let status = axum::http::StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
            let body = resp.bytes().await.unwrap_or_default();
            let html = rewrite_links_for_engine(&body, "next");
            let s = String::from_utf8_lossy(&html);
            let rewritten = s
                .replace("src=\"/_next/", "src=\"/next-assets/_next/")
                .replace("href=\"/_next/", "href=\"/next-assets/_next/");
            (
                status,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string())],
                rewritten.into_bytes(),
            )
                .into_response()
        }
        Err(_) => (axum::http::StatusCode::BAD_GATEWAY, "Server not ready").into_response(),
    }
}

/// Log a render result with consistent formatting.
/// Page names: "home", "search", "listing" (airbnb), "nike-catalog", "nike-pdp" (nike).
fn log_render(engine: &str, page: &str, render_us: f64, deser_us: f64, e2e_us: f64, size: usize) {
    fn fmt_us(us: f64) -> String {
        if us < 1000.0 {
            format!("{us:.1}µs")
        } else {
            format!("{:.2}ms", us / 1000.0)
        }
    }
    let (showcase, short_page) = match page {
        "nike-catalog" => ("nike", "catalog"),
        "nike-pdp" => ("nike", "pdp"),
        "github-pr-diff" => ("github-pr", "diff"),
        "espn-gamecast" => ("espn", "gamecast"),
        "sentry-otel-logs" => ("sentry-otel", "logs"),
        p => ("airbnb", p),
    };
    // Pad the tag to a fixed 26 visible chars so the columns after it always align.
    // Longest tag: [proa][airbnb][listing] = 24 chars → pad to 26
    let tag = format!("[{engine}][{showcase}][{short_page}]");
    eprintln!(
        "  \x1b[36m{tag:<26}\x1b[0m  render \x1b[1m{:>9}\x1b[0m  deser {:>9}  e2e {:>9}  \x1b[2m{:>4}KB\x1b[0m",
        fmt_us(render_us),
        fmt_us(deser_us),
        fmt_us(e2e_us),
        size / 1024,
    );
}

/// Forward a POST render request, log timing, and pass back response headers.
async fn proxy_render_to(
    url: &str,
    engine: &str,
    body: axum::body::Bytes,
) -> axum::response::Response {
    // Extract page name from the JSON body for logging
    let page = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("page")?.as_str().map(String::from))
        .unwrap_or_default();

    let client = reqwest::Client::new();
    match client
        .post(url)
        .header("content-type", "application/json")
        .body(body.to_vec())
        .send()
        .await
    {
        Ok(resp) => {
            let status = axum::http::StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);

            // Extract timing headers for logging
            let hdr = |name: &str| -> f64 {
                resp.headers()
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0)
            };
            let render_us = hdr("x-render-us");
            let deser_us = hdr("x-deser-us");
            let e2e_us = hdr("x-e2e-us");
            let html_size: usize = resp
                .headers()
                .get("x-html-size")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);

            let mut builder = axum::response::Response::builder().status(status);
            for key in &[
                "content-type",
                "x-render-us",
                "x-doc-us",
                "x-deser-us",
                "x-e2e-us",
                "x-html-size",
            ] {
                if let Some(val) = resp.headers().get(*key) {
                    builder = builder.header(*key, val.clone());
                }
            }
            let body = resp.bytes().await.unwrap_or_default();

            if render_us > 0.0 {
                log_render(engine, &page, render_us, deser_us, e2e_us, html_size);
            }

            builder.body(axum::body::Body::from(body)).unwrap()
        }
        Err(_) => (axum::http::StatusCode::BAD_GATEWAY, "Server not ready").into_response(),
    }
}

// ---------------------------------------------------------------------------
// Node proxy handlers (Hono on port 5100)
// ---------------------------------------------------------------------------

async fn proxy_node_root() -> impl IntoResponse {
    proxy_html(&node_url("/"), "node").await
}

async fn proxy_node(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    proxy_html(&node_url(&format!("/{path}")), "node").await
}

async fn proxy_node_nike_root() -> impl IntoResponse {
    proxy_html(&node_url("/nike"), "node").await
}

async fn proxy_node_nike(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    proxy_html(&node_url(&format!("/nike/{path}")), "node").await
}

async fn proxy_node_render(body: axum::body::Bytes) -> impl IntoResponse {
    proxy_render_to(&node_url("/api/render"), "node", body).await
}

async fn proxy_node_assets(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    let url = node_url(&format!("/_next/{path}"));
    match reqwest::get(&url).await {
        Ok(resp) => {
            let status = axum::http::StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            let body = resp.bytes().await.unwrap_or_default();
            (status, [(header::CONTENT_TYPE, content_type)], body).into_response()
        }
        Err(_) => (axum::http::StatusCode::BAD_GATEWAY, "Server not ready").into_response(),
    }
}

// ---------------------------------------------------------------------------
// Bun proxy handlers (same Hono server, Bun runtime on port 5200)
// ---------------------------------------------------------------------------

async fn proxy_bun_root() -> impl IntoResponse {
    proxy_html(&bun_url("/"), "bun").await
}

async fn proxy_bun(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    proxy_html(&bun_url(&format!("/{path}")), "bun").await
}

async fn proxy_bun_nike_root() -> impl IntoResponse {
    proxy_html(&bun_url("/nike"), "bun").await
}

async fn proxy_bun_nike(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    proxy_html(&bun_url(&format!("/nike/{path}")), "bun").await
}

async fn proxy_bun_render(body: axum::body::Bytes) -> impl IntoResponse {
    proxy_render_to(&bun_url("/api/render"), "bun", body).await
}

// Marko (Node) proxy handler.
async fn proxy_marko_node_render(body: axum::body::Bytes) -> impl IntoResponse {
    proxy_render_to(&marko_node_url("/api/render"), "marko-node", body).await
}

// Marko (Bun) proxy handler.
async fn proxy_marko_bun_render(body: axum::body::Bytes) -> impl IntoResponse {
    proxy_render_to(&marko_bun_url("/api/render"), "marko-bun", body).await
}

// React SSR proxy handler. The companion server lives in
// `proa_demo/react-ssr/`; production can override the loopback origin
// with PROA_REACT_SSR_ORIGIN. Same `{page, props}` contract as the
// Marko / Hono servers above.
async fn proxy_react_ssr_render(body: axum::body::Bytes) -> impl IntoResponse {
    proxy_render_to(&react_ssr_url("/api/render"), "react-ssr", body).await
}

async fn proxy_react_ssr_nike_root() -> impl IntoResponse {
    proxy_react_ssr_html("/showcase/nike/react").await
}

async fn proxy_react_ssr_nike(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    proxy_react_ssr_html(&format!("/showcase/nike/react/{path}")).await
}

async fn proxy_react_ssr_ebay() -> impl IntoResponse {
    proxy_react_ssr_html("/showcase/ebay/react").await
}

async fn proxy_react_ssr_github_pr_diff() -> impl IntoResponse {
    proxy_react_ssr_html("/showcase/github-pr-diff/react").await
}

async fn proxy_react_ssr_espn() -> impl IntoResponse {
    proxy_react_ssr_html("/showcase/espn/react").await
}

async fn proxy_react_ssr_sentry_otel() -> impl IntoResponse {
    proxy_react_ssr_html("/showcase/sentry-otel/react").await
}

async fn proxy_react_ssr_bundle(path: &str) -> axum::response::Response {
    let url = react_ssr_url(path);
    match reqwest::get(&url).await {
        Ok(resp) => {
            let status = axum::http::StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/javascript; charset=utf-8")
                .to_string();
            let body = resp.bytes().await.unwrap_or_default();
            (status, [(header::CONTENT_TYPE, content_type)], body).into_response()
        }
        Err(_) => (axum::http::StatusCode::BAD_GATEWAY, "Server not ready").into_response(),
    }
}

async fn proxy_react_ssr_client_bundle() -> impl IntoResponse {
    proxy_react_ssr_bundle("/client.bundle.js").await
}

async fn proxy_react_ssr_espn_bundle() -> impl IntoResponse {
    proxy_react_ssr_bundle("/espn-client.bundle.js").await
}

async fn proxy_react_ssr_sentry_otel_bundle() -> impl IntoResponse {
    proxy_react_ssr_bundle("/sentry-otel-client.bundle.js").await
}

// Next.js proxy handlers. The companion app lives in
// `proa_demo/next-ssr/` and listens on :5402.
async fn proxy_next_render(body: axum::body::Bytes) -> impl IntoResponse {
    proxy_render_to(&next_url("/api/render"), "next", body).await
}

async fn proxy_next_nike_root() -> impl IntoResponse {
    proxy_next_html(&next_url("/showcase/nike/next")).await
}

async fn proxy_next_nike(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    proxy_next_html(&next_url(&format!("/showcase/nike/next/{path}"))).await
}

async fn proxy_next_ebay() -> impl IntoResponse {
    proxy_next_html(&next_url("/showcase/ebay/next")).await
}

async fn proxy_next_assets(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    let url = next_url(&format!("/_next/{path}"));
    match reqwest::get(&url).await {
        Ok(resp) => {
            let status = axum::http::StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            let body = resp.bytes().await.unwrap_or_default();
            (status, [(header::CONTENT_TYPE, content_type)], body).into_response()
        }
        Err(_) => (axum::http::StatusCode::BAD_GATEWAY, "Server not ready").into_response(),
    }
}

// ---------------------------------------------------------------------------
// Bundle-size endpoint — fetches each engine's first-party JS for the
// given page, gzips the bytes server-side (avoiding browser CORS), and
// returns a breakdown the playground UI displays in the "Client JS"
// stat cell. Mirrors the logic of `proa_demo/bundle_size.mjs` but
// runs in-process so the UI doesn't have to spawn a Node worker.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct BundleSizeQuery {
    engine: String,
    page: String,
}

#[derive(Serialize)]
struct BundleAssetReport {
    url: String,
    raw: usize,
    gzip: usize,
}

#[derive(Serialize)]
struct BundleSizeReport {
    engine: String,
    page: String,
    total_raw: usize,
    total_gzip: usize,
    assets: Vec<BundleAssetReport>,
}

/// The playground's own bound port, captured at startup so the
/// bundle-size endpoint can build self-pointing URLs without the
/// caller having to plumb the port through every handler.
static PLAYGROUND_PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();

fn collect_attr_urls(html: &str, attr_prefix: &str, origin: &str, out: &mut Vec<String>) {
    let mut rest = html;
    while let Some(idx) = rest.find(attr_prefix) {
        let after = &rest[idx + attr_prefix.len()..];
        if let Some(end) = after.find('"') {
            let path = &after[..end];
            if path.starts_with("/_next/") && path.contains(".js") {
                let url = format!("{origin}{path}");
                if !out.contains(&url) {
                    out.push(url);
                }
            }
            rest = &after[end..];
        } else {
            break;
        }
    }
}

async fn next_bundle_urls_for_page(page: &str) -> Vec<String> {
    let path = match page {
        "ebay-srp" | "ebay-srp-rsjs" => "/showcase/ebay/next",
        "nike-catalog" => "/showcase/nike/next",
        "nike-pdp" => "/showcase/nike/next/product",
        _ => return Vec::new(),
    };
    let origin = next_origin();
    let url = format!("{origin}{path}");
    let Ok(resp) = reqwest::get(&url).await else {
        return Vec::new();
    };
    let Ok(html) = resp.text().await else {
        return Vec::new();
    };
    let mut urls = Vec::new();
    collect_attr_urls(&html, "src=\"", origin, &mut urls);
    collect_attr_urls(&html, "href=\"", origin, &mut urls);
    urls
}

fn capture_rsjs_artifacts(
    f: impl FnOnce(&mut WebContext<::proa_core::NoLoader, Vec<u8>>),
) -> proa_core::RsjsRenderArtifacts {
    let mut cx = WebContext::with_buffer(Vec::new());
    f(&mut cx);
    cx.rsjs_artifacts_snapshot()
}

/// Build the same shared runtime plan used by response finalization for the
/// default workload behind a bundle-size report.
fn default_page_runtime_plan(page: &str) -> Option<rsjs::PageRuntimePlan> {
    use proa_core::WebRenderSync;

    let artifacts = match page {
        "ebay-srp-rsjs" => {
            let listings = proa_demo::ebay_srp::build_listings(60);
            capture_rsjs_artifacts(|cx| {
                let _ = proa_demo::ebay_srp_islands::FilterChips::default().render(cx);
                let _ = proa_demo::ebay_srp::InteractiveListingsList {
                    listings: &listings,
                }
                .render(cx);
            })
        }
        "espn-gamecast" => capture_rsjs_artifacts(|cx| {
            let _ = proa_showcase::espn::render_into(cx, &proa_showcase::espn::GAME);
        }),
        "sentry-otel-logs" => capture_rsjs_artifacts(|cx| {
            let _ = proa_showcase::sentry_otel::render_into(
                cx,
                proa_showcase::sentry_otel::DEFAULT_ROW_COUNT,
            );
        }),
        _ => return None,
    };

    let urls = HashedRsjsAssetUrls::new(rsjs_asset_index());
    Some(
        rsjs::manifest()
            .page_runtime_plan_with_urls(artifacts.rendered_islands(), &urls)
            .expect("rendered playground islands must resolve through the linked rsjs manifest"),
    )
}

/// URLs fetched up front by a page runtime plan. Deferred island modules are
/// intentionally omitted until their hydration trigger fires.
fn initial_runtime_urls(plan: &rsjs::PageRuntimePlan) -> Vec<String> {
    let mut urls = Vec::new();
    let mut push_unique = |url: &str| {
        if !urls.iter().any(|candidate| candidate == url) {
            urls.push(url.to_owned());
        }
    };
    if let Some(runtime) = &plan.runtime {
        push_unique(&runtime.base);
        if let Some(query) = runtime.query.as_deref() {
            push_unique(query);
        }
    }
    for url in &plan.module_preloads {
        push_unique(url);
    }
    urls
}

async fn bundle_urls_for(engine: &str, page: &str) -> Vec<String> {
    let self_port = PLAYGROUND_PORT.get().copied().unwrap_or(5000);
    match (engine, page) {
        // Static SSR — no client JS at all.
        ("proa", "ebay-srp") => Vec::new(),
        ("proa", "github-pr-diff") => Vec::new(),
        ("proa", page @ ("ebay-srp-rsjs" | "espn-gamecast" | "sentry-otel-logs")) => {
            default_page_runtime_plan(page)
                .map(|plan| {
                    initial_runtime_urls(&plan)
                        .into_iter()
                        .map(|url| format!("http://localhost:{self_port}{url}"))
                        .collect()
                })
                .unwrap_or_default()
        }
        ("marko-node", _) => vec![marko_node_url("/marko-hydrate.js")],
        ("marko-bun", _) => vec![marko_bun_url("/marko-hydrate.js")],
        ("react-ssr", "ebay-srp" | "ebay-srp-rsjs") => {
            vec![react_ssr_url("/client.bundle.js")]
        }
        ("react-ssr", "espn-gamecast") => {
            vec![react_ssr_url("/espn-client.bundle.js")]
        }
        ("react-ssr", "sentry-otel-logs") => {
            vec![react_ssr_url("/sentry-otel-client.bundle.js")]
        }
        ("react-ssr", _) => Vec::new(),
        ("next", page) => next_bundle_urls_for_page(page).await,
        // Other React showcases use bundles chunked under /_next/*; this
        // comparison currently measures only the eBay workload.
        _ => Vec::new(),
    }
}

fn gzip_max(bytes: &[u8]) -> usize {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::with_capacity(bytes.len() / 4), Compression::best());
    enc.write_all(bytes).ok();
    let out = enc.finish().unwrap_or_default();
    out.len()
}

async fn bundle_size(
    axum::extract::Query(q): axum::extract::Query<BundleSizeQuery>,
) -> axum::response::Response {
    let urls = bundle_urls_for(&q.engine, &q.page).await;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let mut report = BundleSizeReport {
        engine: q.engine.clone(),
        page: q.page.clone(),
        total_raw: 0,
        total_gzip: 0,
        assets: Vec::new(),
    };
    for url in urls {
        let bytes = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r.bytes().await.ok(),
            _ => None,
        };
        if let Some(b) = bytes {
            let raw = b.len();
            let gz = gzip_max(&b);
            report.total_raw += raw;
            report.total_gzip += gz;
            report.assets.push(BundleAssetReport { url, raw, gzip: gz });
        }
    }
    (
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&report).unwrap_or_else(|_| "{}".to_string()),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Load test handler
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LoadTestRequest {
    page: String,
    concurrency: Option<usize>,
    iterations: Option<usize>,
    port: Option<u16>,
}

#[derive(Serialize)]
struct LatencyStats {
    avg: f64,
    p50: f64,
    p99: f64,
    min: f64,
    max: f64,
}

#[derive(Serialize)]
struct EngineResult {
    total_requests: usize,
    failed_requests: usize,
    duration_ms: f64,
    rps: f64,
    /// Pure SSR render time (template → bytes, no HTTP)
    ssr_us: LatencyStats,
    /// Full document assembly time (inner render + shell wrapping, no HTTP)
    doc_us: LatencyStats,
    /// Full HTTP roundtrip (request → response)
    roundtrip_us: LatencyStats,
}

#[derive(Serialize)]
struct LoadTestResult {
    proa: EngineResult,
    react: EngineResult,
    bun: EngineResult,
    react_ssr: EngineResult,
    next: EngineResult,
}

fn compute_stats(sorted: &[f64]) -> LatencyStats {
    let avg = if sorted.is_empty() {
        0.0
    } else {
        sorted.iter().sum::<f64>() / sorted.len() as f64
    };
    LatencyStats {
        avg,
        p50: percentile(sorted, 50.0),
        p99: percentile(sorted, 99.0),
        min: sorted.first().copied().unwrap_or(0.0),
        max: sorted.last().copied().unwrap_or(0.0),
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[idx.saturating_sub(1).min(sorted.len() - 1)]
}

fn zero_latency_stats() -> LatencyStats {
    LatencyStats {
        avg: 0.0,
        p50: 0.0,
        p99: 0.0,
        min: 0.0,
        max: 0.0,
    }
}

fn empty_engine_result() -> EngineResult {
    EngineResult {
        total_requests: 0,
        failed_requests: 0,
        duration_ms: 0.0,
        rps: 0.0,
        ssr_us: zero_latency_stats(),
        doc_us: zero_latency_stats(),
        roundtrip_us: zero_latency_stats(),
    }
}

/// Request body for the markdown render endpoint. Mirrors
/// [`RenderRequest`] but ignores `props` — the showcase pages'
/// `render_md()` functions use the default fixture data because
/// Markdown is a content-shape preview, not a benchmark.
#[derive(Deserialize)]
struct RenderMdRequest {
    page: String,
}

/// Render a showcase page as Markdown via the page module's
/// `render_md()` function. Only the Airbnb showcase trio currently
/// has a markdown renderer wired up; other pages return 501 with a
/// clear message so the UI can show "Markdown not yet supported"
/// instead of a blank pane.
///
/// Used by the playground's third "Markdown" tab in `.tab-bar`.
/// Pairs with the existing `/api/render` (HTML) endpoint so the
/// same showcase data renders to either output format through the
/// same component tree — proa's compile-time render-trait split is
/// the demo here.
async fn render_page_md(Json(req): Json<RenderMdRequest>) -> impl IntoResponse {
    let page = req.page.as_str();
    let md = match page {
        "home" => airbnb::home::render_md(),
        "search" => airbnb::search::render_md(),
        "listing" => airbnb::listing::render_md(),
        other => {
            return (
                axum::http::StatusCode::NOT_IMPLEMENTED,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                format!(
                    "Markdown rendering is not yet wired up for `{other}`.\n\nSupported pages: home, search, listing (Airbnb showcase).\n\nTo add a new page: implement `render_md()` in its module and add a match arm here."
                ),
            )
                .into_response();
        }
    };
    (
        axum::http::StatusCode::OK,
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        md,
    )
        .into_response()
}

async fn loadtest(
    Extension(options): Extension<PlaygroundOptions>,
    Json(req): Json<LoadTestRequest>,
) -> axum::response::Response {
    if !options.load_tests_enabled {
        return (
            axum::http::StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "load tests are disabled for this playground"
            })),
        )
            .into_response();
    }

    let iterations = req.iterations.unwrap_or(2000);
    let concurrency = req.concurrency.unwrap_or(50);
    let page = req.page.as_str();

    let default_json = default_props_raw()
        .get(page)
        .cloned()
        .unwrap_or_else(|| "null".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(concurrency)
        .build()
        .unwrap();
    // Include props in the HTTP body so Node/Bun render the full page
    let props_value: serde_json::Value =
        serde_json::from_str(&default_json).unwrap_or(serde_json::Value::Null);
    let proa_body = serde_json::json!({ "page": page, "props": props_value }).to_string();
    let react_body = proa_body.clone();

    // ── Proa: pure SSR (direct render, no HTTP) ──────────────────────
    // Warmup
    for _ in 0..50 {
        match page {
            "home" => {
                if let Ok(owned) = serde_json::from_str::<json_types::HomeDataOwned>(&default_json)
                {
                    let b = owned.borrow_collections();
                    let d = owned.to_home_data(&b);
                    render_with_bump(|buf| {
                        airbnb::home::render_into(&mut WebContext::with_buffer(buf), &d);
                    });
                }
            }
            "search" => {
                if let Ok(owned) =
                    serde_json::from_str::<json_types::SearchDataOwned>(&default_json)
                {
                    let b = owned.borrow_collections();
                    let d = owned.to_search_data(&b);
                    render_with_bump(|buf| {
                        airbnb::search::render_into(&mut WebContext::with_buffer(buf), &d);
                    });
                }
            }
            "listing" => {
                if let Ok(owned) =
                    serde_json::from_str::<json_types::ListingDataOwned>(&default_json)
                {
                    let b = owned.borrow_collections();
                    let d = owned.to_listing_data(&b);
                    render_with_bump(|buf| {
                        let _ = airbnb::listing::render_into(
                            &mut WebContext::with_buffer(buf),
                            &d,
                            None,
                        );
                    });
                }
            }
            "nike-catalog" => {
                if let Ok(owned) =
                    serde_json::from_str::<json_types::NikeCatalogDataOwned>(&default_json)
                {
                    let borrowed = owned.borrow_collections();
                    let products = owned.build_products(&borrowed);
                    let data = nike::catalog::CatalogData {
                        title: &owned.title,
                        products: &products,
                    };
                    render_with_bump(|buf| {
                        nike::catalog::render_into(&mut WebContext::with_buffer(buf), &data);
                    });
                }
            }
            "nike-pdp" => {
                if let Ok(owned) =
                    serde_json::from_str::<json_types::NikePdpDataOwned>(&default_json)
                {
                    let borrowed = owned.borrow_collections();
                    let data = owned.to_product_data(&borrowed);
                    render_with_bump(|buf| {
                        nike::product::render_into(&mut WebContext::with_buffer(buf), &data);
                    });
                }
            }
            "ebay-srp" | "ebay-srp-rsjs" => {
                let listings = proa_demo::ebay_srp::build_listings(60);
                render_with_bump(|buf| {
                    use proa_core::WebRenderSync;
                    proa_demo::ebay_srp::ListingsList {
                        listings: &listings,
                    }
                    .render(&mut WebContext::with_buffer(buf))
                    .ok();
                });
            }
            "github-pr-diff" => {
                let props = proa_demo::github_pr_diff::GithubPrDiffProps::from_json(&default_json);
                render_with_bump(|buf| {
                    use proa_core::WebRenderSync;
                    let _ = proa_demo::github_pr_diff::GithubPrDiffPage { props: &props }
                        .render(&mut WebContext::with_buffer(buf));
                });
            }
            "espn-gamecast" => {
                if let Ok(props) =
                    serde_json::from_str::<proa_showcase::espn::GameData>(&default_json)
                {
                    render_with_bump(|buf| {
                        let _ = proa_showcase::espn::render_into(
                            &mut WebContext::with_buffer(buf),
                            &props,
                        );
                    });
                }
            }
            "sentry-otel-logs" => {
                if let Ok(props) = serde_json::from_str::<SentryOtelPlaygroundProps>(&default_json)
                {
                    render_with_bump(|buf| {
                        let _ = proa_showcase::sentry_otel::render_into_with_props(
                            &mut WebContext::with_buffer(buf),
                            props.page_props(),
                        );
                    });
                }
            }
            _ => {}
        }
    }

    // Measure pure SSR render time only (deser once, then render N times).
    // Each iteration does a single render and reports that time — matching
    // how React's render_us measures a single renderToString call.
    let mut proa_ssr = Vec::with_capacity(iterations);

    // Helper: render once into the bump buffer, return CPU time in µs
    let render_once = |page: &str, json: &str| -> f64 {
        match page {
            "home" => {
                if let Ok(owned) = serde_json::from_str::<json_types::HomeDataOwned>(json) {
                    let b = owned.borrow_collections();
                    let d = owned.to_home_data(&b);
                    RENDER_BUF.with(|cell| {
                        let mut buf = cell.borrow_mut();
                        buf.clear();
                        let t = thread_cpu_us();
                        airbnb::home::render_into(&mut WebContext::with_buffer(&mut *buf), &d);
                        thread_cpu_us() - t
                    })
                } else {
                    0.0
                }
            }
            "search" => {
                if let Ok(owned) = serde_json::from_str::<json_types::SearchDataOwned>(json) {
                    let b = owned.borrow_collections();
                    let d = owned.to_search_data(&b);
                    RENDER_BUF.with(|cell| {
                        let mut buf = cell.borrow_mut();
                        buf.clear();
                        let t = thread_cpu_us();
                        airbnb::search::render_into(&mut WebContext::with_buffer(&mut *buf), &d);
                        thread_cpu_us() - t
                    })
                } else {
                    0.0
                }
            }
            "listing" => {
                if let Ok(owned) = serde_json::from_str::<json_types::ListingDataOwned>(json) {
                    let b = owned.borrow_collections();
                    let d = owned.to_listing_data(&b);
                    RENDER_BUF.with(|cell| {
                        let mut buf = cell.borrow_mut();
                        buf.clear();
                        let t = thread_cpu_us();
                        let _ = airbnb::listing::render_into(
                            &mut WebContext::with_buffer(&mut *buf),
                            &d,
                            None,
                        );
                        thread_cpu_us() - t
                    })
                } else {
                    0.0
                }
            }
            "nike-catalog" => {
                if let Ok(owned) = serde_json::from_str::<json_types::NikeCatalogDataOwned>(json) {
                    let borrowed = owned.borrow_collections();
                    let products = owned.build_products(&borrowed);
                    let data = nike::catalog::CatalogData {
                        title: &owned.title,
                        products: &products,
                    };
                    RENDER_BUF.with(|cell| {
                        let mut buf = cell.borrow_mut();
                        buf.clear();
                        let t = thread_cpu_us();
                        nike::catalog::render_into(&mut WebContext::with_buffer(&mut *buf), &data);
                        thread_cpu_us() - t
                    })
                } else {
                    0.0
                }
            }
            "nike-pdp" => {
                if let Ok(owned) = serde_json::from_str::<json_types::NikePdpDataOwned>(json) {
                    let borrowed = owned.borrow_collections();
                    let data = owned.to_product_data(&borrowed);
                    RENDER_BUF.with(|cell| {
                        let mut buf = cell.borrow_mut();
                        buf.clear();
                        let t = thread_cpu_us();
                        nike::product::render_into(&mut WebContext::with_buffer(&mut *buf), &data);
                        thread_cpu_us() - t
                    })
                } else {
                    0.0
                }
            }
            "ebay-srp" | "ebay-srp-rsjs" => {
                let listings = parse_ebay_props(json);
                RENDER_BUF.with(|cell| {
                    let mut buf = cell.borrow_mut();
                    buf.clear();
                    let t = thread_cpu_us();
                    use proa_core::WebRenderSync;
                    let _ = proa_demo::ebay_srp::ListingsList {
                        listings: &listings,
                    }
                    .render(&mut WebContext::with_buffer(&mut *buf));
                    thread_cpu_us() - t
                })
            }
            "espn-gamecast" => {
                if let Ok(props) = serde_json::from_str::<proa_showcase::espn::GameData>(json) {
                    RENDER_BUF.with(|cell| {
                        let mut buf = cell.borrow_mut();
                        buf.clear();
                        let t = thread_cpu_us();
                        let _ = proa_showcase::espn::render_into(
                            &mut WebContext::with_buffer(&mut *buf),
                            &props,
                        );
                        thread_cpu_us() - t
                    })
                } else {
                    0.0
                }
            }
            "sentry-otel-logs" => {
                if let Ok(props) = serde_json::from_str::<SentryOtelPlaygroundProps>(json) {
                    RENDER_BUF.with(|cell| {
                        let mut buf = cell.borrow_mut();
                        buf.clear();
                        let t = thread_cpu_us();
                        let _ = proa_showcase::sentry_otel::render_into_with_props(
                            &mut WebContext::with_buffer(&mut *buf),
                            props.page_props(),
                        );
                        thread_cpu_us() - t
                    })
                } else {
                    0.0
                }
            }
            "github-pr-diff" => {
                let props = proa_demo::github_pr_diff::GithubPrDiffProps::from_json(json);
                RENDER_BUF.with(|cell| {
                    let mut buf = cell.borrow_mut();
                    buf.clear();
                    let t = thread_cpu_us();
                    use proa_core::WebRenderSync;
                    let _ = proa_demo::github_pr_diff::GithubPrDiffPage { props: &props }
                        .render(&mut WebContext::with_buffer(&mut *buf));
                    thread_cpu_us() - t
                })
            }
            _ => 0.0,
        }
    };

    for _ in 0..iterations {
        proa_ssr.push(render_once(page, &default_json));
    }
    proa_ssr.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // ── Proa: HTTP roundtrip ──────────────────────────────────────────
    let (proa_rt, _proa_rt_ssr, proa_rt_doc, proa_rt_failed, proa_rt_duration) = bench_js_engine(
        &client,
        &format!(
            "http://localhost:{}/api/render",
            req.port
                .or_else(|| PLAYGROUND_PORT.get().copied())
                .unwrap_or(5000)
        ),
        &proa_body,
        iterations,
        concurrency,
        true,
    )
    .await;

    let proa_result = EngineResult {
        total_requests: proa_rt.len(),
        failed_requests: proa_rt_failed,
        duration_ms: proa_rt_duration.as_secs_f64() * 1000.0,
        rps: if proa_rt_duration.is_zero() {
            0.0
        } else {
            proa_rt.len() as f64 / proa_rt_duration.as_secs_f64()
        },
        ssr_us: compute_stats(&proa_ssr),
        doc_us: if matches!(page, "ebay-srp" | "ebay-srp-rsjs") {
            compute_stats(&proa_rt_doc)
        } else {
            compute_stats(&proa_ssr)
        },
        roundtrip_us: compute_stats(&proa_rt),
    };

    // ── Helper: benchmark an engine via HTTP ────────────────────────
    async fn bench_js_engine(
        client: &reqwest::Client,
        url: &str,
        body: &str,
        iterations: usize,
        concurrency: usize,
        check_health: bool,
    ) -> (Vec<f64>, Vec<f64>, Vec<f64>, usize, std::time::Duration) {
        if check_health {
            // Derive health URL from render URL.
            let health_url = url.rsplitn(2, "/api/").last().unwrap_or(url).to_string() + "/health";

            // Wait for server to be ready (up to 5 seconds).
            let ready_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut server_ready = false;
            while std::time::Instant::now() < ready_deadline {
                if let Ok(resp) = client.get(&health_url).send().await {
                    if resp.status().is_success() {
                        server_ready = true;
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            if !server_ready {
                return (
                    vec![],
                    vec![],
                    vec![],
                    iterations,
                    std::time::Duration::from_secs(0),
                );
            }
        }

        // Warmup
        for _ in 0..10 {
            let _ = client
                .post(url)
                .header("content-type", "application/json")
                .header("x-proa-silent", "1")
                .body(body.to_string())
                .send()
                .await;
        }

        let mut rt_times = Vec::with_capacity(iterations);
        let mut ssr_times = Vec::with_capacity(iterations);
        let mut doc_times = Vec::with_capacity(iterations);
        let mut failed = 0usize;
        let start = std::time::Instant::now();

        for chunk in (0..iterations).step_by(concurrency) {
            let end = (chunk + concurrency).min(iterations);
            let futs: Vec<_> = (chunk..end)
                .map(|_| {
                    let c = client.clone();
                    let b = body.to_string();
                    let u = url.to_string();
                    async move {
                        let s = std::time::Instant::now();
                        let resp = c
                            .post(&u)
                            .header("content-type", "application/json")
                            .header("x-proa-silent", "1")
                            .body(b)
                            .send()
                            .await;
                        let rt = s.elapsed().as_secs_f64() * 1_000_000.0;
                        match resp {
                            Ok(r) if r.status().is_success() => {
                                let ssr = r
                                    .headers()
                                    .get("x-render-us")
                                    .and_then(|v| v.to_str().ok())
                                    .and_then(|s| s.parse::<f64>().ok())
                                    .unwrap_or(0.0);
                                let doc = r
                                    .headers()
                                    .get("x-doc-us")
                                    .and_then(|v| v.to_str().ok())
                                    .and_then(|s| s.parse::<f64>().ok())
                                    .unwrap_or(ssr);
                                let _ = r.bytes().await;
                                // ok=true: include in stats (ssr may be 0 due to timer precision)
                                (rt, ssr, doc, true)
                            }
                            _ => (rt, 0.0, 0.0, false),
                        }
                    }
                })
                .collect();
            for (rt, ssr, doc, ok) in futures::future::join_all(futs).await {
                if ok {
                    rt_times.push(rt);
                    ssr_times.push(ssr);
                    doc_times.push(doc);
                } else {
                    failed += 1;
                }
            }
        }
        let duration = start.elapsed();

        rt_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ssr_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        doc_times.sort_by(|a, b| a.partial_cmp(b).unwrap());

        (rt_times, ssr_times, doc_times, failed, duration)
    }

    // ── JS engines: pick endpoints based on showcase ─────────────────
    // The eBay workload routes through Marko (Node :5300, Bun :5400);
    // other showcases use the React/Hono servers (Node :5100, Bun :5200).
    // Next.js is included where the Next demo implements the page.
    // Named to avoid shadowing the `node_url` / `bun_url` companion helpers.
    let (node_render_url, bun_render_url) = if matches!(page, "ebay-srp" | "ebay-srp-rsjs") {
        (marko_node_url("/api/render"), marko_bun_url("/api/render"))
    } else {
        (node_url("/api/render"), bun_url("/api/render"))
    };

    // ── JS / Node: HTTP roundtrip ───────────────────────────────────
    let (react_rt, react_ssr, react_doc, react_failed, react_duration) = bench_js_engine(
        &client,
        &node_render_url,
        &react_body,
        iterations,
        concurrency,
        true,
    )
    .await;

    let react_result = EngineResult {
        total_requests: react_rt.len(),
        failed_requests: react_failed,
        duration_ms: react_duration.as_secs_f64() * 1000.0,
        rps: if react_duration.is_zero() {
            0.0
        } else {
            react_rt.len() as f64 / react_duration.as_secs_f64()
        },
        ssr_us: compute_stats(&react_ssr),
        doc_us: compute_stats(&react_doc),
        roundtrip_us: compute_stats(&react_rt),
    };

    // ── JS / Bun: HTTP roundtrip ────────────────────────────────────
    let bun_body = proa_body.clone();

    let (bun_rt, bun_ssr, bun_doc, bun_failed, bun_duration) = bench_js_engine(
        &client,
        &bun_render_url,
        &bun_body,
        iterations,
        concurrency,
        true,
    )
    .await;

    let bun_result = EngineResult {
        total_requests: bun_rt.len(),
        failed_requests: bun_failed,
        duration_ms: bun_duration.as_secs_f64() * 1000.0,
        rps: if bun_duration.is_zero() {
            0.0
        } else {
            bun_rt.len() as f64 / bun_duration.as_secs_f64()
        },
        ssr_us: compute_stats(&bun_ssr),
        doc_us: compute_stats(&bun_doc),
        roundtrip_us: compute_stats(&bun_rt),
    };

    let react_ssr_result = if matches!(
        page,
        "ebay-srp"
            | "ebay-srp-rsjs"
            | "github-pr-diff"
            | "nike-catalog"
            | "nike-pdp"
            | "espn-gamecast"
            | "sentry-otel-logs"
    ) {
        let (react_ssr_rt, react_ssr_ssr, react_ssr_doc, react_ssr_failed, react_ssr_duration) =
            bench_js_engine(
                &client,
                &react_ssr_url("/api/render"),
                &proa_body,
                iterations,
                concurrency,
                true,
            )
            .await;

        EngineResult {
            total_requests: react_ssr_rt.len(),
            failed_requests: react_ssr_failed,
            duration_ms: react_ssr_duration.as_secs_f64() * 1000.0,
            rps: if react_ssr_duration.is_zero() {
                0.0
            } else {
                react_ssr_rt.len() as f64 / react_ssr_duration.as_secs_f64()
            },
            ssr_us: compute_stats(&react_ssr_ssr),
            doc_us: compute_stats(&react_ssr_doc),
            roundtrip_us: compute_stats(&react_ssr_rt),
        }
    } else {
        empty_engine_result()
    };

    let next_result = if matches!(
        page,
        "ebay-srp" | "ebay-srp-rsjs" | "nike-catalog" | "nike-pdp"
    ) {
        let (next_rt, next_ssr, next_doc, next_failed, next_duration) = bench_js_engine(
            &client,
            &next_url("/api/render"),
            &proa_body,
            iterations,
            concurrency,
            false,
        )
        .await;

        EngineResult {
            total_requests: next_rt.len(),
            failed_requests: next_failed,
            duration_ms: next_duration.as_secs_f64() * 1000.0,
            rps: if next_duration.is_zero() {
                0.0
            } else {
                next_rt.len() as f64 / next_duration.as_secs_f64()
            },
            ssr_us: compute_stats(&next_ssr),
            doc_us: compute_stats(&next_doc),
            roundtrip_us: compute_stats(&next_rt),
        }
    } else {
        empty_engine_result()
    };

    Json(LoadTestResult {
        proa: proa_result,
        react: react_result,
        bun: bun_result,
        react_ssr: react_ssr_result,
        next: next_result,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// Routes needed when the playground console is mounted inside another Axum
/// app. This keeps the console UI and APIs identical to the standalone
/// `proa-playground` binary without taking over the host app's root route or
/// first-party showcase/static asset routes.
pub fn marketing_router() -> Router {
    marketing_router_with_options(PlaygroundOptions::marketing())
}

pub fn marketing_router_with_options(options: PlaygroundOptions) -> Router {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(2222);
    let _ = PLAYGROUND_PORT.set(port);

    Router::new()
        .route("/console", get(index))
        .route("/console/*path", get(index))
        .route("/playground", get(index))
        .route("/playground/*path", get(index))
        .route("/api/render", post(render_page))
        .route("/api/render-md", post(render_page_md))
        .route("/api/loadtest", post(loadtest))
        .route("/api/props", get(default_props))
        .route("/api/openapi.json", get(openapi_spec))
        .route("/api/docs", get(api_docs))
        .route("/health", get(health))
        .route("/favicon.ico", get(serve_favicon))
        .route("/api/render-node", post(proxy_node_render))
        .route("/api/render-bun", post(proxy_bun_render))
        .route("/api/render-marko-node", post(proxy_marko_node_render))
        .route("/api/render-marko-bun", post(proxy_marko_bun_render))
        .route("/api/render-react-ssr", post(proxy_react_ssr_render))
        .route("/api/render-next", post(proxy_next_render))
        .route("/api/bundle-size", get(bundle_size))
        .route(
            "/react-ssr/client.bundle.js",
            get(proxy_react_ssr_client_bundle),
        )
        .route(
            "/react-ssr/espn-client.bundle.js",
            get(proxy_react_ssr_espn_bundle),
        )
        .route(
            "/react-ssr/sentry-otel-client.bundle.js",
            get(proxy_react_ssr_sentry_otel_bundle),
        )
        .route("/showcase/nike/react", get(proxy_react_ssr_nike_root))
        .route("/showcase/nike/react/*path", get(proxy_react_ssr_nike))
        .route("/showcase/ebay/react", get(proxy_react_ssr_ebay))
        .route(
            "/showcase/github-pr-diff/react",
            get(proxy_react_ssr_github_pr_diff),
        )
        .route("/showcase/espn/react", get(proxy_react_ssr_espn))
        .route(
            "/showcase/sentry-otel/react",
            get(proxy_react_ssr_sentry_otel),
        )
        .route("/showcase/airbnb/node", get(proxy_node_root))
        .route("/showcase/airbnb/node/*path", get(proxy_node))
        .route("/showcase/nike/node", get(proxy_node_nike_root))
        .route("/showcase/nike/node/*path", get(proxy_node_nike))
        .route("/_next/*path", get(proxy_node_assets))
        .route("/showcase/nike/next", get(proxy_next_nike_root))
        .route("/showcase/nike/next/*path", get(proxy_next_nike))
        .route("/showcase/ebay/next", get(proxy_next_ebay))
        .route("/next-assets/_next/*path", get(proxy_next_assets))
        .route("/showcase/airbnb/bun", get(proxy_bun_root))
        .route("/showcase/airbnb/bun/*path", get(proxy_bun))
        .route("/showcase/nike/bun", get(proxy_bun_nike_root))
        .route("/showcase/nike/bun/*path", get(proxy_bun_nike))
        .route("/showcase/ebay/proa-rsjs", get(showcase_ebay_srp_rsjs))
        .route(
            "/showcase/ebay/proa-rsjs-consolidated",
            get(showcase_ebay_srp_rsjs_consolidated),
        )
        .route("/showcase/github-pr-diff", get(showcase_github_pr_diff))
        .route("/showcase/ops-dashboard", get(showcase_ops_dashboard))
        .layer(Extension(options))
}

// Single worker thread for fair comparison with single-threaded Node/Bun.
// The tokio runtime still uses async I/O so concurrency works fine.
#[tokio::main(flavor = "multi_thread", worker_threads = 1)]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/playground", get(index))
        .route("/playground/*path", get(index))
        .route("/api/render", post(render_page))
        .route("/api/render-md", post(render_page_md))
        .route("/api/loadtest", post(loadtest))
        .route("/api/props", get(default_props))
        .route("/health", get(health))
        .route("/api/openapi.json", get(openapi_spec))
        .route("/api/docs", get(api_docs))
        .route("/favicon.ico", get(serve_favicon))
        // Static assets so the preview iframe can load CSS/JS
        .route("/proa.css", get(serve_css))
        .route("/proa.js", get(serve_js))
        .route("/assets/*path", get(serve_asset))
        .route("/fonts/:filename", get(serve_font))
        .route("/images/*path", get(serve_image))
        // Showcase pages — so in-page links work when navigating within the preview
        .route("/showcase", get(showcase_index))
        .route("/showcase/airbnb/proa", get(showcase_airbnb_home))
        .route("/showcase/airbnb/proa/search", get(showcase_airbnb_search))
        .route(
            "/showcase/airbnb/proa/listing",
            get(showcase_airbnb_listing),
        )
        .route("/showcase/nike/proa", get(showcase_nike_catalog))
        .route("/showcase/nike/proa/product", get(showcase_nike_pdp))
        // Hono render API proxy
        .route("/api/render-node", post(proxy_node_render))
        // Hono proxy routes for A/B comparison — Airbnb
        .route("/showcase/airbnb/node", get(proxy_node_root))
        .route("/showcase/airbnb/node/*path", get(proxy_node))
        // Hono proxy routes — Nike compatibility path
        .route("/showcase/nike/node", get(proxy_node_nike_root))
        .route("/showcase/nike/node/*path", get(proxy_node_nike))
        // Proxy Hono static assets (/_next/*)
        .route("/_next/*path", get(proxy_node_assets))
        // Bun render API proxy
        .route("/api/render-bun", post(proxy_bun_render))
        .route("/api/render-marko-node", post(proxy_marko_node_render))
        .route("/api/render-marko-bun", post(proxy_marko_bun_render))
        .route("/api/render-react-ssr", post(proxy_react_ssr_render))
        .route("/api/render-next", post(proxy_next_render))
        .route("/api/bundle-size", get(bundle_size))
        .route(
            "/react-ssr/client.bundle.js",
            get(proxy_react_ssr_client_bundle),
        )
        .route(
            "/react-ssr/espn-client.bundle.js",
            get(proxy_react_ssr_espn_bundle),
        )
        .route(
            "/react-ssr/sentry-otel-client.bundle.js",
            get(proxy_react_ssr_sentry_otel_bundle),
        )
        .route("/showcase/nike/react", get(proxy_react_ssr_nike_root))
        .route("/showcase/nike/react/*path", get(proxy_react_ssr_nike))
        .route("/showcase/ebay/react", get(proxy_react_ssr_ebay))
        .route(
            "/showcase/github-pr-diff/react",
            get(proxy_react_ssr_github_pr_diff),
        )
        .route("/showcase/espn/react", get(proxy_react_ssr_espn))
        .route(
            "/showcase/sentry-otel/react",
            get(proxy_react_ssr_sentry_otel),
        )
        // Next.js proxy routes
        .route("/showcase/nike/next", get(proxy_next_nike_root))
        .route("/showcase/nike/next/*path", get(proxy_next_nike))
        .route("/showcase/ebay/next", get(proxy_next_ebay))
        .route("/next-assets/_next/*path", get(proxy_next_assets))
        // Bun proxy routes for A/B comparison — Airbnb
        .route("/showcase/airbnb/bun", get(proxy_bun_root))
        .route("/showcase/airbnb/bun/*path", get(proxy_bun))
        // Bun proxy routes — Nike compatibility path
        .route("/showcase/nike/bun", get(proxy_bun_nike_root))
        .route("/showcase/nike/bun/*path", get(proxy_bun_nike))
        // Bare-path fallbacks (React components link without engine prefix)
        .route("/showcase/airbnb", get(showcase_airbnb_home))
        .route("/showcase/airbnb/search", get(showcase_airbnb_search))
        .route("/showcase/airbnb/listing", get(showcase_airbnb_listing))
        .route("/showcase/nike", get(showcase_nike_catalog))
        .route("/showcase/nike/product", get(showcase_nike_pdp))
        .route("/showcase/ebay", get(showcase_ebay_srp))
        .route("/showcase/ebay/proa", get(showcase_ebay_srp))
        .route("/showcase/ebay/proa-rsjs", get(showcase_ebay_srp_rsjs))
        .route(
            "/showcase/ebay/proa-rsjs-consolidated",
            get(showcase_ebay_srp_rsjs_consolidated),
        )
        .route("/showcase/github-pr-diff", get(showcase_github_pr_diff))
        .route("/showcase/ops-dashboard", get(showcase_ops_dashboard))
        .route("/showcase/settings-console", get(showcase_settings_console))
        // rsjs runtime + per-island JS. Runtime files are root-mounted
        // because the per-island JS imports them via `/rsjs-runtime.js`.
        // Two URL shapes per asset are served: the canonical un-hashed
        // path (kept for backward-compatibility with code that still
        // imports `/rsjs-runtime.js` directly, served with a 60-second
        // cache to discourage prod use) and the content-hashed path
        // (`/rsjs-runtime.<hash8>.js`, served with `Cache-Control:
        // public, max-age=31536000, immutable` so the browser never
        // revalidates a hashed URL).
        .route("/rsjs-runtime.js", get(serve_rsjs_runtime))
        .route("/rsjs-runtime-query.js", get(serve_rsjs_runtime_query))
        // Per-island handler accepts both `<asset_id>.js` (un-hashed)
        // and `<asset_id>.<hash8>.js` (content-hashed) shapes; the
        // handler dispatches by counting dots in the path segment.
        .route("/rsjs/island/:file", get(serve_rsjs_island))
        // Hashed-URL routes for the runtime modules.
        .route("/rsjs-runtime.:hash.js", get(serve_rsjs_runtime_hashed))
        .route(
            "/rsjs-runtime-query.:hash.js",
            get(serve_rsjs_runtime_query_hashed),
        )
        .layer(Extension(PlaygroundOptions::standalone()));

    // ── Spawn comparison engines ────────────────────────────────────────
    let hono_dir = hono_root();
    let react_ssr_dir = react_ssr_root();
    let next_dir = next_root();

    // Check Node version (--import flag requires >= 20)
    let node_ok = Command::new("node")
        .args([
            "--eval",
            "process.exit(Number(process.versions.node.split('.')[0]) < 18 ? 1 : 0)",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    // Check Bun availability (>= 1.3 required for Bun.serve + Hono compat)
    let bun_ok = Command::new("bun")
        .args(["--eval", "const v=Bun.version.split('.').map(Number); process.exit(v[0]<1||(v[0]===1&&v[1]<3)?1:0)"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let d = "\x1b[2m"; // dim
    let r = "\x1b[0m"; // reset
    let g = "\x1b[32m"; // green
    let y = "\x1b[33m"; // yellow
    let b = "\x1b[1m"; // bold
    eprintln!("  {d}┌─────────────────────────────────────────────┐{r}");
    eprintln!("  {d}│{r}  {b}Comparison Engines{r}                         {d}│{r}");
    eprintln!("  {d}├─────────────────────────────────────────────┤{r}");

    if let Some(hono_dir) = hono_dir {
        let shared_nm = hono_dir
            .parent()
            .unwrap()
            .join("react-ssr-shared")
            .join("node_modules");
        let hono_nm = hono_dir.join("node_modules");
        if !shared_nm.exists() && hono_nm.exists() {
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink(&hono_nm, &shared_nm);
            }
        }
    }

    let _hono_child = if let Some(hono_dir) = hono_dir {
        if !node_ok {
            eprintln!("  {d}│{r}  {y}▲{r}  React (Node)  {d}skipped, need Node >= 18{r}  {d}│{r}");
            None
        } else if localhost_port_open(5100) {
            eprintln!("  {d}│{r}  {g}●{r}  React (Node)  {d}already on :5100{r}      {d}│{r}");
            None
        } else {
            let tsx_bin = hono_dir.join("node_modules/.bin/tsx");
            match Command::new(&tsx_bin)
                .arg("server.ts")
                .current_dir(hono_dir)
                .env("PORT", "5100")
                .env("NODE_ENV", "production")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
            {
                Ok(child) => {
                    eprintln!(
                        "  {d}│{r}  {g}●{r}  React (Node)  {d}listening on :5100{r}        {d}│{r}"
                    );
                    Some(child)
                }
                Err(_) => {
                    eprintln!(
                        "  {d}│{r}  {y}▲{r}  React (Node)  {d}failed to start{r}           {d}│{r}"
                    );
                    None
                }
            }
        }
    } else {
        eprintln!("  {d}│  ○  React (Node)  hono-ssr not found{r}        {d}│{r}");
        None
    };

    let _react_ssr_child = if let Some(react_ssr_dir) = react_ssr_dir {
        if !node_ok {
            eprintln!("  {d}│{r}  {y}▲{r}  React SSR    {d}skipped, need Node >= 18{r}  {d}│{r}");
            None
        } else if localhost_port_open(5401) {
            eprintln!("  {d}│{r}  {g}●{r}  React SSR    {d}already on :5401{r}      {d}│{r}");
            None
        } else if !react_ssr_dir.join("node_modules").exists() {
            eprintln!("  {d}│{r}  {y}▲{r}  React SSR    {d}skipped (run npm install){r}{d}│{r}");
            None
        } else {
            match Command::new("node")
                .args(["--import", "tsx", "server.tsx"])
                .current_dir(react_ssr_dir)
                .env("PORT", "5401")
                .env("NODE_ENV", "production")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
            {
                Ok(child) => {
                    eprintln!(
                        "  {d}│{r}  {g}●{r}  React SSR    {d}listening on :5401{r}        {d}│{r}"
                    );
                    Some(child)
                }
                Err(_) => {
                    eprintln!(
                        "  {d}│{r}  {y}▲{r}  React SSR    {d}failed to start{r}           {d}│{r}"
                    );
                    None
                }
            }
        }
    } else {
        eprintln!("  {d}│  ○  React SSR    react-ssr not found{r}       {d}│{r}");
        None
    };

    // ── Marko (Node) on :5300 ────────────────────────────────────────────
    let marko_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("marko-server");
    let marko_node_child = if marko_dir.exists() && node_ok {
        if !marko_dir.join("node_modules").exists() {
            eprintln!("  {d}│{r}  {y}▲{r}  Marko (Node)  {d}skipped (run npm install){r} {d}│{r}");
            None
        } else if localhost_port_open(5300) {
            eprintln!("  {d}│{r}  {g}●{r}  Marko (Node)  {d}already on :5300{r}      {d}│{r}");
            None
        } else {
            match Command::new("node")
                .arg("server.mjs")
                .current_dir(&marko_dir)
                .env("PORT", "5300")
                .env("RUNTIME", "node")
                .env("NODE_ENV", "production")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
            {
                Ok(child) => {
                    eprintln!(
                        "  {d}│{r}  {g}●{r}  Marko (Node)  {d}listening on :5300{r}        {d}│{r}"
                    );
                    Some(child)
                }
                Err(_) => {
                    eprintln!(
                        "  {d}│{r}  {y}▲{r}  Marko (Node)  {d}failed to start{r}           {d}│{r}"
                    );
                    None
                }
            }
        }
    } else {
        None
    };
    let _ = marko_node_child;

    // ── Marko (Bun) on :5400 ─────────────────────────────────────────────
    let marko_bun_child = if marko_dir.exists() && bun_ok {
        if !marko_dir.join("node_modules").exists() {
            None
        } else if localhost_port_open(5400) {
            eprintln!("  {d}│{r}  {g}●{r}  Marko (Bun)   {d}already on :5400{r}      {d}│{r}");
            None
        } else {
            match Command::new("bun")
                .arg("server.mjs")
                .current_dir(&marko_dir)
                .env("PORT", "5400")
                .env("RUNTIME", "bun")
                .env("NODE_ENV", "production")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
            {
                Ok(child) => {
                    eprintln!(
                        "  {d}│{r}  {g}●{r}  Marko (Bun)   {d}listening on :5400{r}        {d}│{r}"
                    );
                    Some(child)
                }
                Err(_) => None,
            }
        }
    } else {
        None
    };
    let _ = marko_bun_child;

    let _bun_child = if let Some(hono_dir) = hono_dir {
        if !bun_ok {
            // Distinguish "not installed" from "too old"
            let has_bun = Command::new("bun")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok();
            if has_bun {
                eprintln!(
                    "  {d}│{r}  {y}▲{r}  React (Bun)   {d}skipped, need Bun >= 1.3{r}   {d}│{r}"
                );
            } else {
                eprintln!("  {d}│  ○  React (Bun)   bun not installed{r}         {d}│{r}");
            }
            None
        } else if localhost_port_open(5200) {
            eprintln!("  {d}│{r}  {g}●{r}  React (Bun)   {d}already on :5200{r}      {d}│{r}");
            None
        } else {
            match Command::new("bun")
                .arg("run")
                .arg("server.ts")
                .current_dir(hono_dir)
                .env("PORT", "5200")
                .env("NODE_ENV", "production")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
            {
                Ok(child) => {
                    eprintln!(
                        "  {d}│{r}  {g}●{r}  React (Bun)   {d}listening on :5200{r}        {d}│{r}"
                    );
                    Some(child)
                }
                Err(_) => {
                    eprintln!(
                        "  {d}│{r}  {y}▲{r}  React (Bun)   {d}failed to start{r}           {d}│{r}"
                    );
                    None
                }
            }
        }
    } else {
        None
    };

    let _next_child = if let Some(next_dir) = next_dir {
        if !node_ok {
            eprintln!("  {d}│{r}  {y}▲{r}  Next.js      {d}skipped, need Node >= 18{r}  {d}│{r}");
            None
        } else if localhost_port_open(5402) {
            eprintln!("  {d}│{r}  {g}●{r}  Next.js      {d}already on :5402{r}           {d}│{r}");
            None
        } else if !next_dir.join("node_modules").exists() {
            eprintln!("  {d}│{r}  {y}▲{r}  Next.js      {d}skipped (run npm install){r} {d}│{r}");
            None
        } else if !next_dir.join(".next").join("BUILD_ID").exists() {
            eprintln!("  {d}│{r}  {y}▲{r}  Next.js      {d}skipped (run npm run build){r}{d}│{r}");
            None
        } else {
            let next_bin = next_dir.join("node_modules/.bin/next");
            match Command::new(&next_bin)
                .arg("start")
                .arg("--hostname")
                .arg("0.0.0.0")
                .arg("--port")
                .arg("5402")
                .current_dir(next_dir)
                .env("NODE_ENV", "production")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
            {
                Ok(child) => {
                    eprintln!(
                        "  {d}│{r}  {g}●{r}  Next.js      {d}listening on :5402{r}        {d}│{r}"
                    );
                    Some(child)
                }
                Err(_) => {
                    eprintln!(
                        "  {d}│{r}  {y}▲{r}  Next.js      {d}failed to start{r}           {d}│{r}"
                    );
                    None
                }
            }
        }
    } else {
        eprintln!("  {d}│  ○  Next.js      next-ssr not found{r}        {d}│{r}");
        None
    };

    eprintln!("  {d}└─────────────────────────────────────────────┘{r}");

    // ── Bind port ────────────────────────────────────────────────────────
    let requested_port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok());
    let mut listener = None;
    let mut port = requested_port.unwrap_or(5000);

    if let Some(p) = requested_port {
        listener = Some(
            tokio::net::TcpListener::bind(format!("0.0.0.0:{}", p))
                .await
                .unwrap_or_else(|err| panic!("Could not bind to requested PORT={p}: {err}")),
        );
    } else {
        for p in 5000..=5010 {
            match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", p)).await {
                Ok(l) => {
                    listener = Some(l);
                    port = p;
                    break;
                }
                Err(_) => continue,
            }
        }
    }

    let listener = listener.expect("Could not bind to any port 5000-5010");
    let _ = PLAYGROUND_PORT.set(port);

    let d = "\x1b[2m";
    let r = "\x1b[0m";
    let b = "\x1b[1m";
    let c = "\x1b[36m";
    eprintln!();
    eprintln!("  {b}Routes{r}");
    eprintln!("  {d}Playground        {r} {c}http://localhost:{port}/playground{r}");
    eprintln!("  {d}Showcase          {r} {c}http://localhost:{port}/showcase{r}");
    eprintln!("  {d}API Docs (Scalar) {r} {c}http://localhost:{port}/api/docs{r}");
    eprintln!("  {d}OpenAPI Spec      {r} {c}http://localhost:{port}/api/openapi.json{r}");
    eprintln!("  {d}Health            {r} {c}http://localhost:{port}/health{r}");
    eprintln!();

    // Open browser
    let _ = open::that(format!("http://localhost:{port}"));

    axum::serve(listener, app).await.unwrap();
}

// ---------------------------------------------------------------------------
// Static assets — embedded at compile time
// ---------------------------------------------------------------------------

async fn serve_favicon() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/x-icon")],
        include_bytes!("../../proa_marketing/public/favicon.ico").as_slice(),
    )
}

async fn serve_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../proa_marketing/public/styles.css"),
    )
}

async fn serve_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        "// This standalone demo uses rsjs assets on their dedicated routes.\n",
    )
}

// ---------------------------------------------------------------------------
// rsjs runtime + per-island JS — served for the interactive eBay demo.
// ---------------------------------------------------------------------------

const RSJS_RUNTIME_JS: &str = rsjs::runtime_assets::CORE_JS;
const RSJS_RUNTIME_QUERY_JS: &str = rsjs::runtime_assets::QUERY_JS;

const HASHED_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
const UNHASHED_CACHE_CONTROL: &str = "public, max-age=60";

/// Compute a stable 8-char hex content hash for an asset body.
///
/// Uses sha-256 truncated to its first 4 bytes. 8 hex chars × 4 bits =
/// 32 bits of entropy, enough to keep cache busting deterministic
/// without inflating URLs. The first-load risk of accidental hash
/// collision is negligible at the rsjs island count we expect (a
/// handful per app, never tens of thousands).
fn content_hash8(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(8);
    for byte in &digest[..4] {
        // Hand-roll the hex emit (lowercase) to avoid pulling
        // `hex` in just for this.
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    out
}

/// One served JS asset: the served (minified-where-possible) bytes,
/// its content hash, and precompressed brotli/gzip siblings.
///
/// Built once at server startup. Handlers pick a variant based on
/// `Accept-Encoding`: `br > gzip > raw`.
struct RsjsAsset {
    /// 8-hex content hash of [`Self::raw`]. Used for the cache-busted
    /// URL shape (`/rsjs-runtime.<hash>.js`).
    hash: String,
    /// The bytes we serve when no compression is negotiated. After
    /// minification when esbuild is available, otherwise the raw
    /// source unchanged.
    raw: Vec<u8>,
    /// Brotli-11 of [`Self::raw`].
    br: Vec<u8>,
    /// Gzip-9 of [`Self::raw`].
    gz: Vec<u8>,
}

impl RsjsAsset {
    fn build(source: &[u8], esbuild: Option<&std::path::Path>) -> Self {
        let raw = match esbuild {
            Some(bin) => match minify_with_esbuild(bin, source) {
                Ok(min) => min,
                Err(e) => {
                    eprintln!(
                        "  ⚠  rsjs minify failed ({e}); falling back to unminified bytes for one asset"
                    );
                    source.to_vec()
                }
            },
            None => source.to_vec(),
        };
        let hash = content_hash8(&raw);
        let br = compress_brotli(&raw);
        let gz = compress_gzip(&raw);
        Self { hash, raw, br, gz }
    }
}

fn rewrite_component_fragment_imports(
    source: &str,
    fragment_hashes: &std::collections::HashMap<String, String>,
) -> String {
    let mut rewritten = source.to_string();
    for (stem, hash) in fragment_hashes {
        let from = format!("./{stem}.js");
        let to = format!("./{stem}.{hash}.js");
        rewritten = rewritten.replace(&from, &to);
    }
    rewritten
}

fn rewrite_rsjs_runtime_imports(
    source: &str,
    runtime_url: &str,
    runtime_query_url: &str,
) -> String {
    rsjs::rewrite_runtime_import_specifiers(source, runtime_url, runtime_query_url)
}

fn rewrite_rsjs_imports(
    source: &str,
    fragment_hashes: &std::collections::HashMap<String, String>,
    runtime_url: &str,
    runtime_query_url: &str,
) -> String {
    let rewritten = rewrite_rsjs_runtime_imports(source, runtime_url, runtime_query_url);
    rewrite_component_fragment_imports(&rewritten, fragment_hashes)
}

fn build_fragment_assets(
    fragments: &[&'static rsjs::ComponentFragmentManifestEntry],
    esbuild: Option<&std::path::Path>,
    runtime_url: &str,
    runtime_query_url: &str,
) -> std::collections::HashMap<String, RsjsAsset> {
    let sources: Vec<(String, &'static str)> = fragments
        .iter()
        .map(|fragment| {
            let filename = rsjs::manifest::component_fragment_filename(fragment.asset_id);
            let stem = filename
                .strip_suffix(".js")
                .unwrap_or(filename.as_str())
                .to_string();
            (stem, fragment.js_source)
        })
        .collect();

    let mut hashes = std::collections::HashMap::with_capacity(sources.len());
    let mut assets = std::collections::HashMap::with_capacity(sources.len());

    // Fragment chunks may eventually import other fragment chunks. Their
    // content hashes must include the final child import URL, so iterate to a
    // fixed point instead of hashing the raw compiler output once.
    for _ in 0..8 {
        let mut next_hashes = std::collections::HashMap::with_capacity(sources.len());
        let mut next_assets = std::collections::HashMap::with_capacity(sources.len());

        for (stem, source) in &sources {
            let rewritten = rewrite_rsjs_imports(source, &hashes, runtime_url, runtime_query_url);
            let asset = RsjsAsset::build(rewritten.as_bytes(), esbuild);
            next_hashes.insert(stem.clone(), asset.hash.clone());
            next_assets.insert(stem.clone(), asset);
        }

        if next_hashes == hashes {
            return next_assets;
        }

        hashes = next_hashes;
        assets = next_assets;
    }

    assets
}

#[cfg(test)]
mod rsjs_asset_pipeline_tests {
    use super::*;

    static CHILD_FRAGMENT: rsjs::ComponentFragmentManifestEntry =
        rsjs::ComponentFragmentManifestEntry {
            asset_id: "child",
            struct_name: "Child",
            export_name: "mount_child",
            js_source: "export function mount_child() {}\n",
        };

    static PARENT_FRAGMENT: rsjs::ComponentFragmentManifestEntry =
        rsjs::ComponentFragmentManifestEntry {
            asset_id: "parent",
            struct_name: "Parent",
            export_name: "mount_parent",
            js_source: "import { mount_child } from \"./component_child.js\";\nexport function mount_parent() {}\n",
        };

    #[test]
    fn rewrites_component_fragment_imports_to_hashed_urls() {
        let mut hashes = std::collections::HashMap::new();
        hashes.insert("component_child".to_string(), "abc12345".to_string());

        let rewritten = rewrite_component_fragment_imports(
            "import { mount_child } from \"./component_child.js\";",
            &hashes,
        );

        assert!(rewritten.contains("./component_child.abc12345.js"));
    }

    #[test]
    fn fragment_asset_hashes_include_rewritten_child_imports() {
        let assets = build_fragment_assets(
            &[&CHILD_FRAGMENT, &PARENT_FRAGMENT],
            None,
            "/rsjs-runtime.abc12345.js",
            "/rsjs-runtime-query.def67890.js",
        );
        let parent = assets
            .get("component_parent")
            .expect("parent fragment asset missing");
        let js = std::str::from_utf8(&parent.raw).expect("fragment JS should be utf-8");

        assert!(
            js.contains("./component_child."),
            "parent fragment should import the hashed child URL, got:\n{js}"
        );
        assert!(
            !js.contains("./component_child.js\""),
            "parent fragment should not keep the short-cache child URL, got:\n{js}"
        );
    }

    #[test]
    fn rewrites_runtime_imports_to_hashed_urls() {
        let rewritten = rewrite_rsjs_runtime_imports(
            "import { hydrate } from \"/rsjs-runtime.js\";",
            "/rsjs-runtime.abc12345.js",
            "/rsjs-runtime-query.def67890.js",
        );

        assert_eq!(
            rewritten,
            "import { hydrate } from \"/rsjs-runtime.abc12345.js\";"
        );
    }

    #[test]
    fn shared_page_plan_uses_recorded_artifacts_and_hashed_urls() {
        use proa_core::WebRenderSync;

        let artifacts = capture_rsjs_artifacts(|cx| {
            let _ = proa_demo::ebay_srp_islands::WatchHeart.render(cx);
        });
        let urls = HashedRsjsAssetUrls::new(rsjs_asset_index());
        let plan = rsjs::manifest()
            .page_runtime_plan_with_urls(artifacts.rendered_islands(), &urls)
            .expect("recorded watch-heart artifact should resolve");
        let tags = plan.to_html(None);

        assert!(
            tags.contains(r#"<script type="module" src="/rsjs-runtime."#),
            "expected base runtime tag, got:\n{tags}"
        );
        assert!(
            !tags.contains("rsjs-runtime-query."),
            "non-query island should not emit the query runtime, got:\n{tags}"
        );
        // Plain-cargo rendering emits the island module directly
        // (`/rsjs/island/watchheart_<id>.<hash>.js`); the linked pipeline
        // emits a shared component fragment plus an owner module
        // (`/rsjs/island/component_watchheart_<id>...`). Either spelling
        // proves WatchHeart's JS is in the plan with a hashed URL.
        assert!(
            tags.contains(r#"/rsjs/island/watchheart_"#)
                || tags.contains(r#"/rsjs/island/component_watchheart_"#),
            "expected WatchHeart island assets in the plan, got:\n{tags}"
        );

        let html = finalize_rsjs_html("<html><body>fixture</body></html>".to_string(), &artifacts);
        let runtime_pos = html.find("/rsjs-runtime.").expect("runtime URL missing");
        let body_end = html.find("</body>").expect("closing body missing");
        assert!(runtime_pos < body_end, "runtime plan must precede </body>");
    }

    #[test]
    fn shared_page_plan_uses_hashed_query_runtime_when_required() {
        // Resolve a query island by its manifest flag rather than by struct
        // name: the linked pipeline registers islands behind owner modules
        // with their own asset ids, so `uses_queries` is the only spelling
        // of "this island has reachable query work" that exists in both
        // serving modes.
        let manifest = rsjs::manifest();
        let query_island = manifest
            .islands
            .iter()
            .find(|entry| entry.uses_queries)
            .expect("a query-using island should be registered");
        let urls = HashedRsjsAssetUrls::new(rsjs_asset_index());
        let plan = manifest
            .page_runtime_plan_with_urls([query_island.asset_id], &urls)
            .expect("registered query island should resolve");
        let tags = plan.to_html(None);

        assert!(
            tags.contains(r#"<script type="module" src="/rsjs-runtime-query."#),
            "query island should emit the query runtime, got:\n{tags}"
        );
    }
}

/// Materialised view of the rsjs asset URLs this server serves,
/// keyed by stable identifier (asset_id for islands,
/// `component_<asset_id>` for component fragments, "runtime" /
/// "runtime_query" for the shared modules) and indexed by content
/// hash for cache-busted URL emission.
///
/// Built once at server startup from the linkme manifest and cached
/// in a `OnceLock` so subsequent requests do no work to look up the
/// hashed URL for a given asset_id.
struct RsjsAssetIndex {
    runtime: RsjsAsset,
    runtime_query: RsjsAsset,
    /// Per-island assets, keyed by asset_id.
    islands: std::collections::HashMap<&'static str, RsjsAsset>,
    /// Client helper modules, keyed by module asset stem.
    client_modules: std::collections::HashMap<String, RsjsAsset>,
    /// Explicit client-class modules, keyed by class asset stem.
    client_classes: std::collections::HashMap<String, RsjsAsset>,
    /// Reusable component fragments, keyed by `component_<asset_id>`.
    fragments: std::collections::HashMap<String, RsjsAsset>,
}

/// Locate an `esbuild` binary at startup.
///
/// Search order:
/// 1. `esbuild` on `PATH` (via `which`).
/// 2. `proa_demo/marko-server/node_modules/.bin/esbuild` (relative
///    to the running binary, then to `CARGO_MANIFEST_DIR`).
/// 3. `proa_demo/react-ssr/node_modules/.bin/esbuild` (same
///    fallbacks).
///
/// Returns `None` if none of the above exist; the asset pipeline
/// then falls back to serving unminified source with a loud
/// startup warning.
fn find_esbuild() -> Option<std::path::PathBuf> {
    // 1. PATH lookup.
    if let Ok(out) = Command::new("which").arg("esbuild").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                let p = std::path::PathBuf::from(s);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    // 2./3. node_modules fallbacks. Probe both binary-relative
    // (packaged distribution) and CARGO_MANIFEST_DIR (cargo run).
    let candidates = [
        "marko-server/node_modules/.bin/esbuild",
        "react-ssr/node_modules/.bin/esbuild",
    ];
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    roots.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    for root in &roots {
        for rel in &candidates {
            let p = root.join(rel);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Run esbuild over `source`, returning the minified bytes.
///
/// Uses `--minify --target=es2020 --loader=js` and feeds the source
/// over stdin so we don't have to materialise temp files. esbuild
/// writes the minified module to stdout.
fn minify_with_esbuild(bin: &std::path::Path, source: &[u8]) -> std::io::Result<Vec<u8>> {
    use std::io::Write;
    let mut child = Command::new(bin)
        .args(["--minify", "--target=es2020", "--loader=js"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(source)?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(std::io::Error::other(format!(
            "esbuild exited with status {}: {stderr}",
            out.status
        )));
    }
    Ok(out.stdout)
}

fn compress_brotli(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    // Quality 11 = max. lgwin 22 is the max recommended; the brotli
    // crate's default. Buffer size 4096 is fine.
    let mut out = Vec::with_capacity(bytes.len() / 4);
    {
        let mut enc = brotli::CompressorWriter::new(&mut out, 4096, 11, 22);
        if enc.write_all(bytes).is_err() {
            return Vec::new();
        }
    }
    out
}

fn compress_gzip(bytes: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::with_capacity(bytes.len() / 4), Compression::best());
    if enc.write_all(bytes).is_err() {
        return Vec::new();
    }
    enc.finish().unwrap_or_default()
}

fn rsjs_asset_index() -> &'static RsjsAssetIndex {
    static INDEX: std::sync::OnceLock<RsjsAssetIndex> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        let started = std::time::Instant::now();
        let esbuild = find_esbuild();
        match esbuild.as_deref() {
            Some(p) => eprintln!("  rsjs assets: minifying with esbuild at {}", p.display()),
            None => eprintln!(
                "  ⚠  rsjs assets: esbuild not found (looked on PATH, marko-server/node_modules, \
                 react-ssr/node_modules); serving UNMINIFIED JS. Install esbuild (e.g. \
                 `npm i -D esbuild` in proa_demo/marko-server) for production byte sizes."
            ),
        }
        let runtime = RsjsAsset::build(RSJS_RUNTIME_JS.as_bytes(), esbuild.as_deref());
        let runtime_query = RsjsAsset::build(RSJS_RUNTIME_QUERY_JS.as_bytes(), esbuild.as_deref());
        let runtime_url = hashed_runtime_url_from_hash(&runtime.hash);
        let runtime_query_url = hashed_runtime_query_url_from_hash(&runtime_query.hash);
        let manifest = rsjs::manifest();
        let fragments = build_fragment_assets(
            &manifest.component_fragments,
            esbuild.as_deref(),
            &runtime_url,
            &runtime_query_url,
        );
        let fragment_hashes: std::collections::HashMap<String, String> = fragments
            .iter()
            .map(|(stem, asset)| (stem.clone(), asset.hash.clone()))
            .collect();
        let mut islands = std::collections::HashMap::with_capacity(manifest.islands.len());
        for island in &manifest.islands {
            let js_source = rewrite_rsjs_imports(
                island.js_source,
                &fragment_hashes,
                &runtime_url,
                &runtime_query_url,
            );
            islands.insert(
                island.asset_id,
                RsjsAsset::build(js_source.as_bytes(), esbuild.as_deref()),
            );
        }
        let reachable_client_modules = manifest.reachable_client_modules();
        let mut client_modules =
            std::collections::HashMap::with_capacity(reachable_client_modules.len());
        for module in &reachable_client_modules {
            let filename = rsjs::client_module_filename(module.module_path);
            let stem = filename
                .strip_suffix(".js")
                .unwrap_or(filename.as_str())
                .to_string();
            let js_source = rsjs::client_module_js_source(module);
            let js_source = rewrite_rsjs_imports(
                &js_source,
                &fragment_hashes,
                &runtime_url,
                &runtime_query_url,
            );
            client_modules.insert(
                stem,
                RsjsAsset::build(js_source.as_bytes(), esbuild.as_deref()),
            );
        }
        let mut client_classes =
            std::collections::HashMap::with_capacity(manifest.client_classes.len());
        for class in &manifest.client_classes {
            let filename = rsjs::client_class_filename(class.module_path);
            let stem = filename
                .strip_suffix(".js")
                .unwrap_or(filename.as_str())
                .to_string();
            let js_source = rewrite_rsjs_imports(
                class.js_source,
                &fragment_hashes,
                &runtime_url,
                &runtime_query_url,
            );
            client_classes.insert(
                stem,
                RsjsAsset::build(js_source.as_bytes(), esbuild.as_deref()),
            );
        }
        let elapsed = started.elapsed();
        eprintln!(
            "  rsjs assets: built {} entries in {:?} (runtime raw={}B br={}B gz={}B)",
            2 + islands.len() + client_modules.len() + client_classes.len() + fragments.len(),
            elapsed,
            runtime.raw.len(),
            runtime.br.len(),
            runtime.gz.len(),
        );
        RsjsAssetIndex {
            runtime,
            runtime_query,
            islands,
            client_modules,
            client_classes,
            fragments,
        }
    })
}

/// Hashed URL helpers used by [`HashedRsjsAssetUrls`] so the shared runtime
/// planner targets assets served with `Cache-Control: immutable`.
fn hashed_runtime_url(idx: &RsjsAssetIndex) -> String {
    hashed_runtime_url_from_hash(&idx.runtime.hash)
}

fn hashed_runtime_query_url(idx: &RsjsAssetIndex) -> String {
    hashed_runtime_query_url_from_hash(&idx.runtime_query.hash)
}

fn hashed_runtime_url_from_hash(hash: &str) -> String {
    format!("/rsjs-runtime.{hash}.js")
}

fn hashed_runtime_query_url_from_hash(hash: &str) -> String {
    format!("/rsjs-runtime-query.{hash}.js")
}

fn hashed_island_url(idx: &RsjsAssetIndex, asset_id: &str) -> Option<String> {
    idx.islands
        .get(asset_id)
        .map(|asset| format!("/rsjs/island/{asset_id}.{}.js", asset.hash))
}

fn hashed_component_fragment_url(idx: &RsjsAssetIndex, asset_id: &str) -> Option<String> {
    let stem = format!("component_{asset_id}");
    idx.fragments
        .get(stem.as_str())
        .map(|asset| format!("/rsjs/island/{stem}.{}.js", asset.hash))
}

fn hashed_client_module_url(idx: &RsjsAssetIndex, module_path: &str) -> Option<String> {
    let filename = rsjs::client_module_filename(module_path);
    let stem = filename.strip_suffix(".js").unwrap_or(filename.as_str());
    idx.client_modules
        .get(stem)
        .map(|asset| format!("/rsjs/island/{stem}.{}.js", asset.hash))
}

fn hashed_client_class_url(idx: &RsjsAssetIndex, module_path: &str) -> Option<String> {
    let filename = rsjs::client_class_filename(module_path);
    let stem = filename.strip_suffix(".js").unwrap_or(filename.as_str());
    idx.client_classes
        .get(stem)
        .map(|asset| format!("/rsjs/island/{stem}.{}.js", asset.hash))
}

/// Playground-owned mapping from logical rsjs assets to the immutable,
/// content-hashed paths served by this process.
#[derive(Clone, Copy)]
struct HashedRsjsAssetUrls<'a> {
    index: &'a RsjsAssetIndex,
}

impl<'a> HashedRsjsAssetUrls<'a> {
    const fn new(index: &'a RsjsAssetIndex) -> Self {
        Self { index }
    }
}

impl rsjs::RsjsAssetUrlResolver for HashedRsjsAssetUrls<'_> {
    fn resolve(&self, asset: rsjs::RsjsAssetRef<'_>) -> String {
        match asset {
            rsjs::RsjsAssetRef::Runtime => hashed_runtime_url(self.index),
            rsjs::RsjsAssetRef::QueryRuntime => hashed_runtime_query_url(self.index),
            rsjs::RsjsAssetRef::Island { asset_id } => hashed_island_url(self.index, asset_id)
                .unwrap_or_else(|| format!("/rsjs/island/{asset_id}.js")),
            rsjs::RsjsAssetRef::ClientModule { module_path } => {
                hashed_client_module_url(self.index, module_path).unwrap_or_else(|| {
                    format!("/rsjs/island/{}", rsjs::client_module_filename(module_path))
                })
            }
            rsjs::RsjsAssetRef::ClientClass { module_path } => {
                hashed_client_class_url(self.index, module_path).unwrap_or_else(|| {
                    format!("/rsjs/island/{}", rsjs::client_class_filename(module_path))
                })
            }
            rsjs::RsjsAssetRef::ComponentFragment { asset_id } => {
                hashed_component_fragment_url(self.index, asset_id)
                    .unwrap_or_else(|| format!("/rsjs/island/component_{asset_id}.js"))
            }
        }
    }
}

/// Finalize one complete playground response from the artifacts recorded by
/// its `WebContext`.
///
/// Query seeds and the deterministic shared runtime plan are inserted at the
/// document boundary. Render components never inspect the response artifact
/// ledger or emit their own rsjs runtime tags.
// proa-perf-ok(scope): the top-level HTTP response owner inserts planner-produced trusted tags at the document boundary
fn finalize_rsjs_html(mut html: String, artifacts: &proa_core::RsjsRenderArtifacts) -> String {
    if artifacts.is_empty() {
        return html;
    }

    let urls = HashedRsjsAssetUrls::new(rsjs_asset_index());
    let plan = rsjs::manifest()
        .page_runtime_plan_with_urls(artifacts.rendered_islands(), &urls)
        .expect("rendered playground islands must resolve through the linked rsjs manifest");
    let mut tail = Vec::new();
    if let Some(seed_table) = artifacts.query_seed_table_json() {
        tail.extend_from_slice(br#"<script type="application/json" id="rsjs-query-seed-table">"#);
        tail.extend_from_slice(seed_table.as_bytes());
        tail.extend_from_slice(b"</script>");
    }
    plan.write_html(&mut tail, None)
        .expect("writing the playground rsjs runtime plan to Vec cannot fail");
    if tail.is_empty() {
        return html;
    }

    let tail = String::from_utf8(tail).expect("rsjs response artifacts must be UTF-8");
    if let Some(body_end) = html.rfind("</body>") {
        html.insert_str(body_end, &tail);
    } else {
        html.push_str(&tail);
    }
    html
}

/// Pick the best precompressed variant of `asset` given the request's
/// `Accept-Encoding` header.
///
/// Preference order: `br` > `gzip` > raw (the already-minified bytes).
/// Returns the body bytes and the value we should set on
/// `Content-Encoding` (or `None` if we're serving raw).
///
/// Notes:
/// - We don't parse q-values. Real-world `Accept-Encoding` strings
///   from browsers and curl are simple comma-separated lists; tokens
///   are matched case-insensitively. If a client truly wants to
///   exclude `br` it can omit the token.
/// - Empty/missing header → raw.
fn pick_encoding<'a>(
    asset: &'a RsjsAsset,
    accept_encoding: Option<&str>,
) -> (&'a [u8], Option<&'static str>) {
    let ae = accept_encoding.unwrap_or("").to_ascii_lowercase();
    if !asset.br.is_empty() && ae.split(',').any(|t| t.trim().starts_with("br")) {
        return (&asset.br, Some("br"));
    }
    if !asset.gz.is_empty() && ae.split(',').any(|t| t.trim().starts_with("gzip")) {
        return (&asset.gz, Some("gzip"));
    }
    (&asset.raw, None)
}

/// Build a JS response with optional content-encoding. Always sets
/// `Vary: Accept-Encoding` so caches don't serve a brotli body to a
/// client that didn't ask for it.
fn js_response(
    body: Vec<u8>,
    encoding: Option<&'static str>,
    cache_control: &'static str,
) -> axum::response::Response {
    use axum::http::HeaderValue;
    let mut resp = axum::response::Response::new(axum::body::Body::from(body));
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    h.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    if let Some(enc) = encoding {
        h.insert(header::CONTENT_ENCODING, HeaderValue::from_static(enc));
    }
    resp
}

fn accept_encoding_str(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

async fn serve_rsjs_runtime(headers: axum::http::HeaderMap) -> axum::response::Response {
    let idx = rsjs_asset_index();
    let ae = accept_encoding_str(&headers);
    let (body, enc) = pick_encoding(&idx.runtime, ae.as_deref());
    js_response(body.to_vec(), enc, UNHASHED_CACHE_CONTROL)
}

async fn serve_rsjs_runtime_query(headers: axum::http::HeaderMap) -> axum::response::Response {
    let idx = rsjs_asset_index();
    let ae = accept_encoding_str(&headers);
    let (body, enc) = pick_encoding(&idx.runtime_query, ae.as_deref());
    js_response(body.to_vec(), enc, UNHASHED_CACHE_CONTROL)
}

/// Hashed-URL handler for the base runtime. The router is
/// `/rsjs-runtime.:hash.js`; we accept any hash value and serve the
/// current bytes, so an old cached response still hits the same body
/// (the cache-immutable promise) until a new build invalidates the
/// hash. The current-build hash is recomputed lazily and compared
/// only for telemetry (404 on mismatch would defeat the purpose; a
/// stale hash from a prior deploy still serves the current bytes
/// because that is the worst we can do without a full asset
/// versioning story).
async fn serve_rsjs_runtime_hashed(
    axum::extract::Path(_hash): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let idx = rsjs_asset_index();
    let ae = accept_encoding_str(&headers);
    let (body, enc) = pick_encoding(&idx.runtime, ae.as_deref());
    js_response(body.to_vec(), enc, HASHED_CACHE_CONTROL)
}

async fn serve_rsjs_runtime_query_hashed(
    axum::extract::Path(_hash): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let idx = rsjs_asset_index();
    let ae = accept_encoding_str(&headers);
    let (body, enc) = pick_encoding(&idx.runtime_query, ae.as_deref());
    js_response(body.to_vec(), enc, HASHED_CACHE_CONTROL)
}

/// rsjs generated JS handler — accepts both top-level island files and
/// relative component-fragment imports. Top-level islands support the
/// legacy un-hashed URL (`/rsjs/island/<asset_id>.js`) and the
/// content-hashed URL (`/rsjs/island/<asset_id>.<hash8>.js`).
/// Component fragments are imported by generated island modules as
/// `./component_<asset_id>.<hash8>.js` after the playground asset
/// pipeline rewrites compiler output, so they use the same directory
/// and immutable cache path as top-level islands.
///
/// The shape is detected by counting dots in the trimmed `.js` stem:
/// `watchheart_3a1f0b2c` (one underscore-separated hash, no dot) is
/// un-hashed; `watchheart_3a1f0b2c.abcd1234` (one dot) is content
/// hashed. The hashed branch sets `Cache-Control: immutable`; the
/// un-hashed branch keeps a short max-age to nudge production
/// deployments toward the hashed shape.
async fn serve_rsjs_island(
    axum::extract::Path(file): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let stem = file.strip_suffix(".js").unwrap_or(&file);
    let (asset_id, is_hashed) = match stem.rsplit_once('.') {
        // `watchheart_3a1f0b2c.abcd1234` → asset_id =
        // `watchheart_3a1f0b2c`, hashed.
        Some((id, _hash)) => (id, true),
        // No `.` in the stem: legacy un-hashed.
        None => (stem, false),
    };
    let idx = rsjs_asset_index();
    match idx
        .islands
        .get(asset_id)
        .or_else(|| idx.client_modules.get(asset_id))
        .or_else(|| idx.client_classes.get(asset_id))
        .or_else(|| idx.fragments.get(asset_id))
    {
        Some(asset) => {
            let cache_control = if is_hashed {
                HASHED_CACHE_CONTROL
            } else {
                UNHASHED_CACHE_CONTROL
            };
            let ae = accept_encoding_str(&headers);
            let (body, enc) = pick_encoding(asset, ae.as_deref());
            js_response(body.to_vec(), enc, cache_control)
        }
        None => (axum::http::StatusCode::NOT_FOUND, "unknown rsjs asset").into_response(),
    }
}

async fn serve_asset(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    let assets_dir = assets_root().join("assets");
    let file_path = assets_dir.join(&path);

    // Security: ensure the resolved path stays within assets_dir
    if !file_path.starts_with(&assets_dir) {
        return (axum::http::StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    match std::fs::read(&file_path) {
        Ok(bytes) => {
            let content_type = if path.ends_with(".js") {
                "application/javascript; charset=utf-8"
            } else if path.ends_with(".css") {
                "text/css; charset=utf-8"
            } else {
                "application/octet-stream"
            };
            ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
        }
        Err(_) => (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

async fn serve_font(
    axum::extract::Path(filename): axum::extract::Path<String>,
) -> impl IntoResponse {
    let fonts_dir = assets_root().join("fonts");
    let file_path = fonts_dir.join(&filename);

    if !file_path.starts_with(&fonts_dir) {
        return (axum::http::StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    match tokio::fs::read(&file_path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "font/woff2")], bytes).into_response(),
        Err(_) => (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

async fn serve_image(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    let images_dir = assets_root().join("images");
    let file_path = images_dir.join(&path);

    if !file_path.starts_with(&images_dir) {
        return (axum::http::StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    let file_path = if file_path.is_file() {
        file_path
    } else if path.starts_with("espn/") {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("proa_showcase")
            .join("src")
            .join("espn")
            .join("data")
            .join("images")
            .join(&path)
    } else {
        file_path
    };

    let content_type = match file_path.extension().and_then(|e| e.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };

    match tokio::fs::read(&file_path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, content_type)], bytes).into_response(),
        Err(_) => (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

// ---------------------------------------------------------------------------
// Showcase page handlers
// ---------------------------------------------------------------------------

async fn showcase_index() -> Html<String> {
    use proa_core::WebRenderSync;
    use proa_demo::showcase_index::ShowcaseIndexPage;

    Html(render_owned_response(|cx| {
        let _ = ShowcaseIndexPage.render(cx);
    }))
}

async fn showcase_airbnb_home() -> Html<String> {
    Html(airbnb::home::render())
}

async fn showcase_airbnb_search() -> Html<String> {
    Html(airbnb::search::render())
}

async fn showcase_airbnb_listing() -> Html<String> {
    Html(airbnb::listing::render())
}

async fn showcase_nike_catalog() -> Html<String> {
    Html(nike::catalog::render())
}

async fn showcase_nike_pdp() -> Html<String> {
    Html(nike::product::render())
}

async fn showcase_github_pr_diff(
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Html<String> {
    use proa_core::WebRenderSync;
    use proa_demo::github_pr_diff::{GithubPrDiffPage, GithubPrDiffProps};

    let mut props = GithubPrDiffProps::default();
    if let Some(file_count) = query.get("files").and_then(|value| value.parse().ok()) {
        props.file_count = file_count;
    }
    if let Some(lines_per_file) = query.get("lines").and_then(|value| value.parse().ok()) {
        props.lines_per_file = lines_per_file;
    }
    let props = props.clamped();

    Html(render_owned_response(|cx| {
        let _ = GithubPrDiffPage { props: &props }.render(cx);
    }))
}

async fn showcase_ops_dashboard() -> Html<String> {
    use proa_core::WebRenderSync;
    use proa_demo::ops_dashboard::{default_data, OpsDashboard};

    let data = default_data();
    Html(render_owned_response(|cx| {
        let _ = OpsDashboard { data: &data }.render(cx);
    }))
}

async fn showcase_settings_console() -> Html<String> {
    use proa_core::WebRenderSync;
    use proa_demo::settings_console::{SettingsConsole, AUDIT_EVENTS};

    Html(render_owned_response(|cx| {
        let _ = SettingsConsole {
            audit_events: AUDIT_EVENTS,
        }
        .render(cx);
    }))
}

/// eBay search-results reference workload.
///
/// Renders 60 listings into the restored document chrome while
/// preserving the benchmark's full-page composition and repeated-card shape.
async fn showcase_ebay_srp(
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Html<String> {
    use proa_demo::ebay_srp;

    let (n, q) = srp_query_params(&query);
    let listings = ebay_srp::default_listings(n);
    Html(render_srp_response(
        q,
        listings,
        ebay_srp::ListingsList { listings },
        (),
    ))
}

/// Chips toolbar + interactive card list, rendered inline inside the SRP
/// document's app-root region (single pass, no intermediate buffer).
struct SrpRsjsRiver<'a> {
    chips: proa_demo::ebay_srp_islands::FilterChips,
    listings: &'a [proa_demo::ebay_srp::Listing],
}

impl<'a, B: WriteBuf, L: proa_core::DataLoader> proa_core::WebRenderSync<L, B>
    for SrpRsjsRiver<'a>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        self.chips.render(cx)?;
        proa_demo::ebay_srp::InteractiveListingsList {
            listings: self.listings,
        }
        .render(cx)
    }
}

/// Consolidated-island variant of [`SrpRsjsRiver`].
struct SrpConsolidatedRiver<'a> {
    chips: proa_demo::ebay_srp_islands::FilterChips,
    listings: &'a [proa_demo::ebay_srp::Listing; 60],
}

impl<'a, B: WriteBuf, L: proa_core::DataLoader> proa_core::WebRenderSync<L, B>
    for SrpConsolidatedRiver<'a>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        self.chips.render(cx)?;
        proa_demo::ebay_srp_islands::WatchListings60 {
            listings: self.listings,
        }
        .render(cx)
    }
}

/// Non-rsjs browser enhancements used by the interactive SRP fixture.
///
/// The response owner emits the rsjs runtime plan separately after rendering.
struct SrpUiEnhancements;

impl<B: WriteBuf, L: proa_core::DataLoader> proa_core::WebRenderSync<L, B> for SrpUiEnhancements {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        proa_core::StaticRaw(proa_demo::ebay_srp::RSJS_ISLAND_STYLE).render(cx)?;
        proa_core::StaticRaw(proa_demo::ebay_srp_filter_runtime::SRP_FILTER_RUNTIME_SCRIPT)
            .render(cx)
    }
}

/// Shared `?n=` / `?q=` parsing for the compatibility eBay routes.
/// The authored fixture defaults to 60 cards but accepts any 1..=1000.
fn srp_query_params(query: &std::collections::HashMap<String, String>) -> (usize, &str) {
    let n = query
        .get("n")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(60)
        .clamp(1, 1000);
    let q = query
        .get("q")
        .map(String::as_str)
        .unwrap_or(proa_demo::ebay_srp_chrome_real::DEFAULT_QUERY);
    (n, q)
}

/// Interactive variant: each card's watch-heart is an rsjs island.
/// Loads the shared rsjs runtime + the per-island JS so the page is
/// hydrated client-side. Used for the islands-vs-Marko bundle-size
/// comparison.
async fn showcase_ebay_srp_rsjs(
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Html<String> {
    use proa_demo::ebay_srp;
    use proa_demo::ebay_srp_islands::FilterChips;

    let (n, q) = srp_query_params(&query);
    let listings = ebay_srp::default_listings(n);

    // Round-trip URL → chip-visual sync: parse `?format=auction,buyitnow&shipping=free`
    // out of the request URL and seed the chip island's initial signal
    // values. The runtime coordinator (writes-side, in
    // `ebay_srp_filter_runtime`) handles updating the URL on subsequent
    // clicks; this hook is the reads-side that makes refresh + direct
    // navigation render the chips with their checked-visual already
    // matching the URL on first paint.
    let format_active = query
        .get("format")
        .map(|s| s.split(',').collect::<Vec<_>>())
        .unwrap_or_default();
    let chips = FilterChips {
        initial_buy_it_now: format_active.contains(&"buyitnow"),
        initial_auction: format_active.contains(&"auction"),
        initial_free_shipping: query.get("shipping").map(|s| s == "free").unwrap_or(false),
    };

    // Single pass: chips + cards render inline inside the document. The
    // response owner finalizes their recorded rsjs artifacts afterward.
    Html(render_srp_response(
        q,
        listings,
        SrpRsjsRiver { chips, listings },
        SrpUiEnhancements,
    ))
}

/// Variant of [`showcase_ebay_srp_rsjs`] that uses the consolidated
/// [`WatchListings60`](proa_demo::ebay_srp_islands::WatchListings60) island in
/// place of 60 per-card `WatchHeart` islands.
///
/// The headline bundle-size demo: 60 `<rsjs-island>` wrappers + 60
/// seed JSON blobs + 60 hydration cycles per page collapse into 1 of
/// each. The rendered card markup, click behaviour, filter
/// coordinator, and URL round-trip are otherwise identical to
/// `/showcase/ebay/proa-rsjs`. Wire size + bundle count are the only
/// observable differences.
async fn showcase_ebay_srp_rsjs_consolidated(
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Html<String> {
    use proa_demo::ebay_srp;
    use proa_demo::ebay_srp_islands::FilterChips;

    // The consolidated island is fixed at 60 cards, so this route ignores
    // `?n=` (the per-card-island route honours it).
    let (_, q) = srp_query_params(&query);
    let listings = ebay_srp::default_listings(60);
    let listings_array: &[ebay_srp::Listing; 60] = listings
        .try_into()
        .expect("default_listings(60) returns 60");

    let format_active = query
        .get("format")
        .map(|s| s.split(',').collect::<Vec<_>>())
        .unwrap_or_default();
    let chips = FilterChips {
        initial_buy_it_now: format_active.contains(&"buyitnow"),
        initial_auction: format_active.contains(&"auction"),
        initial_free_shipping: query.get("shipping").map(|s| s == "free").unwrap_or(false),
    };

    // Single pass; the consolidated island owns its own `<ul>` wrapper.
    Html(render_srp_response(
        q,
        listings,
        SrpConsolidatedRiver {
            chips,
            listings: listings_array,
        },
        SrpUiEnhancements,
    ))
}

/// Parse the eBay fixture props JSON into a fully-built listings vec.
///
/// Schema:
///   {
///     "count": 60,
///     "listings": [   // optional per-index overrides
///       { "title": "…", "priceCents": 99900, "couponPct": 7, … },
///       { "freeDelivery": true, "watcherCount": 999 },
///       …
///     ]
///   }
fn parse_ebay_props(props_json: &str) -> Vec<proa_demo::ebay_srp::Listing> {
    let v: serde_json::Value = serde_json::from_str(props_json).unwrap_or(serde_json::Value::Null);
    let count = v
        .get("count")
        .and_then(|n| n.as_u64())
        .map(|n| n.clamp(1, 1000) as usize)
        .unwrap_or(60);
    let mut listings = proa_demo::ebay_srp::build_listings(count);
    if let Some(overrides) = v.get("listings") {
        proa_demo::ebay_srp::apply_overrides(&mut listings, overrides);
    }
    listings
}

// ---------------------------------------------------------------------------
// Embedded HTML — the entire playground UI
// ---------------------------------------------------------------------------

const PLAYGROUND_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1.0"/>
<title>Proa Playground</title>
<style>
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
:root {
  --bg: #0b0b0d;
  --bg-card: rgba(18, 19, 22, 0.94);
  --border: rgba(255, 255, 255, 0.08);
  --border-strong: rgba(255, 255, 255, 0.14);
  --text: #f4f4ef;
  --text-muted: #a1a1aa;
  --accent: #ff6a13;
  --accent-hover: #ff7a31;
  --green: #86efac;
  --font: "Geist", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  --mono: "Geist Mono", "SF Mono", "Fira Code", "Cascadia Code", monospace;
}
body {
  font-family: var(--font);
  background-color: var(--bg);
  background-image: radial-gradient(rgba(255, 255, 255, 0.05) 0.75px, transparent 0.75px);
  background-size: 26px 26px;
  color: var(--text);
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}
header {
  display: grid;
  grid-template-columns: minmax(0, 1.1fr) auto;
  gap: 20px;
  padding: 16px 24px;
  border-bottom: 1px solid var(--border);
  background: rgba(11, 11, 13, 0.9);
  backdrop-filter: blur(16px);
  flex-shrink: 0;
}
.brand {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.page-breadcrumb {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  margin-bottom: 6px;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.14em;
  text-transform: uppercase;
}
.page-breadcrumb span {
  display: inline-flex;
  align-items: center;
}
.page-breadcrumb .separator {
  opacity: 0.45;
}
.showcase-select {
  background: var(--surface);
  color: var(--text);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 4px 8px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  cursor: pointer;
  outline: none;
}
.showcase-select:focus {
  border-color: var(--green);
}
.crumb-pages {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.crumb-page {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-height: 32px;
  padding: 0 12px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-muted);
  cursor: pointer;
  font-family: var(--font);
  font-size: 12px;
  font-weight: 500;
  letter-spacing: -0.01em;
  text-transform: none;
  transition: background 0.2s ease, border-color 0.2s ease, color 0.2s ease, transform 0.2s ease;
}
.crumb-page:hover {
  background: rgba(255, 255, 255, 0.08);
  border-color: var(--border-strong);
  color: var(--text);
  transform: translateY(-1px);
}
.crumb-page.active {
  background: rgba(255, 122, 24, 0.14);
  border-color: rgba(255, 122, 24, 0.4);
  color: var(--text);
}
.crumb-page-size {
  color: var(--text-muted);
  font-family: var(--mono);
  font-size: 11px;
}
header h1 {
  font-size: 28px;
  font-weight: 600;
  line-height: 1.02;
  letter-spacing: -0.04em;
}
header p {
  max-width: 560px;
  color: var(--text-muted);
  font-size: 13px;
  line-height: 1.5;
}
.header-tools {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 10px;
}
.controls {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.controls button:not(.engine-btn),
.controls a {
  font-family: var(--font);
  font-size: 13px;
  min-height: 42px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text);
  cursor: pointer;
  text-decoration: none;
  transition: background 0.2s ease, border-color 0.2s ease, transform 0.2s ease;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0 14px;
  white-space: nowrap;
}
.controls button:not(.engine-btn):hover,
.controls a:hover {
  background: rgba(255, 255, 255, 0.08);
  border-color: var(--border-strong);
  transform: translateY(-1px);
}
.controls button#render-btn {
  background: var(--accent);
  border-color: var(--accent);
  color: #111;
  font-weight: 600;
}
.controls button#render-btn:hover { background: var(--accent-hover); }
.stats {
  display: flex;
  gap: 10px;
  align-items: stretch;
  justify-content: flex-end;
  flex-wrap: wrap;
}
.stat {
  min-width: 108px;
  padding: 8px 10px;
  border-radius: 14px;
  border: 1px solid var(--border);
  background: var(--bg-card);
}
.stat-label {
  display: block;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.stat-label[title] {
  cursor: help;
}
.stat-value {
  display: block;
  margin-top: 5px;
  font-weight: 600;
  font-family: var(--mono);
  font-size: 13px;
}
.stat-value.time { color: var(--green); }
main {
  display: flex;
  flex: 1;
  min-height: 0;
  gap: 16px;
  padding: 16px 24px 24px;
}
.editor-pane {
  width: 40%;
  min-width: 320px;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border);
  border-radius: 24px;
  background: var(--bg-card);
  box-shadow: 0 30px 80px -55px rgba(0, 0, 0, 0.75);
  overflow: hidden;
}
.editor-pane .pane-header {
  padding: 14px 16px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.18em;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  background: rgba(255, 255, 255, 0.02);
}
.editor-pane textarea {
  flex: 1;
  width: 100%;
  padding: 16px;
  background: transparent;
  color: var(--text);
  border: none;
  resize: none;
  font-family: var(--mono);
  font-size: 13px;
  line-height: 1.6;
  tab-size: 2;
}
.editor-pane textarea:focus { outline: none; }
.preview-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border);
  border-radius: 24px;
  background: var(--bg-card);
  box-shadow: 0 30px 80px -55px rgba(0, 0, 0, 0.75);
  overflow: hidden;
}
.preview-pane .pane-header {
  padding: 14px 16px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.18em;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: rgba(255, 255, 255, 0.02);
}
.preview-pane iframe {
  flex: 1;
  width: 100%;
  border: none;
  background: #fff;
}
.source-view {
  display: none;
  flex: 1;
  width: 100%;
  padding: 16px;
  background: transparent;
  color: var(--text);
  border: none;
  resize: none;
  font-family: var(--mono);
  font-size: 13px;
  line-height: 1.6;
}
.tab-bar {
  display: flex;
  gap: 6px;
}
.tab-bar button {
  min-height: 34px;
  padding: 0 12px;
  font-size: 11px;
  border-radius: 999px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-family: var(--font);
  text-transform: uppercase;
  letter-spacing: 0.12em;
}
.tab-bar button.active {
  background: rgba(255, 255, 255, 0.08);
  border-color: var(--border-strong);
  color: var(--text);
}
.engine-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.engine-toggle .engine-slider {
  display: none;
}
.engine-btn {
  display: inline-flex;
  align-items: center;
  min-height: 32px;
  padding: 0 12px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-muted);
  cursor: pointer;
  font-family: var(--font);
  font-size: 12px;
  font-weight: 500;
  letter-spacing: -0.01em;
  white-space: nowrap;
  transition: background 0.2s ease, border-color 0.2s ease, color 0.2s ease, transform 0.2s ease;
  -webkit-appearance: none;
  appearance: none;
  outline: none;
  margin: 0;
  box-shadow: none;
}
.engine-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  border-color: var(--border-strong);
  color: var(--text);
  transform: translateY(-1px);
}
.engine-btn.active {
  background: rgba(255, 122, 24, 0.14);
  border-color: rgba(255, 122, 24, 0.4);
  color: var(--text);
  font-weight: 600;
}
@media (max-width: 1180px) {
  header {
    grid-template-columns: 1fr;
  }
  .header-tools {
    align-items: flex-start;
  }
  .stats {
    justify-content: flex-start;
  }
}
@media (max-width: 900px) {
  main {
    flex-direction: column;
  }
  .editor-pane {
    width: 100%;
    min-width: 0;
    min-height: 320px;
  }
  .preview-pane {
    min-height: 520px;
  }
}
</style>
</head>
<body>
<header>
  <div class="brand">
    <nav class="page-breadcrumb" aria-label="Current showcase">
      <span>Showcases</span>
      <span class="separator">/</span>
      <select id="showcase-select" class="showcase-select">
        <option value="airbnb">Airbnb</option>
        <option value="nike">Nike</option>
        <option value="ebay">eBay</option>
        <option value="ebay-rsjs">eBay (rsjs islands)</option>
        <option value="github-pr">GitHub PR Diff</option>
        <option value="espn">ESPN Gamecast</option>
        <option value="sentry-otel">Sentry + OpenTelemetry</option>
      </select>
      <span class="separator">/</span>
      <div class="crumb-pages" id="crumb-pages" role="group" aria-label="Pages">
      </div>
    </nav>
    <h1>Proa Playground</h1>
    <p>Edit props, render native HTML, and inspect the output.</p>
  </div>
  <div class="header-tools">
    <div class="controls">
      <div class="engine-toggle" role="group" aria-label="Rendering engine" data-active="proa">
        <div class="engine-slider"></div>
        <button type="button" class="engine-btn active" data-engine="proa" onclick="setEngine('proa', this)">Proa (Rust)</button>
        <button type="button" class="engine-btn" data-engine="node" onclick="setEngine('node', this)">React (Hono, Node)</button>
        <button type="button" class="engine-btn" data-engine="bun" onclick="setEngine('bun', this)">React (Hono, Bun)</button>
        <button type="button" class="engine-btn" data-engine="marko-node" onclick="setEngine('marko-node', this)">Marko (Node)</button>
        <button type="button" class="engine-btn" data-engine="react-ssr" onclick="setEngine('react-ssr', this)">React (SSR)</button>
        <button type="button" class="engine-btn" data-engine="next" onclick="setEngine('next', this)">Next.js</button>
        <!-- Marko (Bun) remains wired server-side for manual eBay runs
             and can be restored here alongside the SHOWCASES route below.
        <button type="button" class="engine-btn" data-engine="marko-bun" onclick="setEngine('marko-bun', this)">Marko (Bun)</button>
        -->
      </div>
      <a id="open-route-link" href="/showcase/airbnb/proa/listing" target="_blank" rel="noreferrer">Open full page</a>
      <button id="render-btn" onclick="doRender()">Render</button>
      <button id="copy-curl-btn" onclick="copyCurl()" title="Copy curl command to clipboard" style="font-family:var(--font);font-size:12px;min-height:42px;border-radius:999px;border:1px solid var(--border);background:rgba(255,255,255,0.04);color:var(--text);padding:0 16px;cursor:pointer">Copy curl</button>
      <select id="loadtest-preset" style="font-family:var(--font);font-size:12px;min-height:42px;border-radius:999px;border:1px solid var(--border);background:rgba(255,255,255,0.04) url('data:image/svg+xml,<svg xmlns=%22http://www.w3.org/2000/svg%22 width=%2212%22 height=%2212%22 viewBox=%220 0 24 24%22 fill=%22none%22 stroke=%22%23888%22 stroke-width=%222.5%22 stroke-linecap=%22round%22 stroke-linejoin=%22round%22><polyline points=%226 9 12 15 18 9%22/></svg>') no-repeat right 12px center;color:var(--text);padding:0 32px 0 12px;cursor:pointer;-webkit-appearance:none;appearance:none">
        <option value="light">Light &mdash; 500 requests, 10 concurrent</option>
        <option value="medium" selected>Medium &mdash; 2K requests, 50 concurrent</option>
        <option value="heavy">Heavy &mdash; 5K requests, 100 concurrent</option>
        <option value="stress">Stress &mdash; 10K requests, 200 concurrent</option>
      </select>
      <button id="loadtest-btn" onclick="runLoadTest()">Load Test</button>
    </div>
    <div class="stats">
      <div class="stat"><span class="stat-label">SSR</span><span class="stat-value time" id="stat-render">—</span></div>
      <div class="stat"><span class="stat-label" title="Time spent deserializing the JSON props before render">JSON deser</span><span class="stat-value" id="stat-deser">—</span></div>
      <div class="stat"><span class="stat-label">E2E</span><span class="stat-value" id="stat-e2e">—</span></div>
      <div class="stat"><span class="stat-label" title="Total client-side JS the browser downloads (sum of runtime + per-island/per-bundle JS, gzipped)">Client JS (gz)</span><span class="stat-value" id="stat-bundle">—</span></div>
    </div>
  </div>
</header>
<div style="padding:6px 16px;font-size:11px;color:var(--text-muted, #888);background:rgba(255,255,255,0.02);border-bottom:1px solid var(--border, #333);line-height:1.6">
  All timings here are slower than isolated benchmarks due to HTTP overhead, shared CPU between engines, browser activity, and server contention. For accurate numbers, use <code style="background:rgba(255,255,255,0.06);padding:1px 4px;border-radius:3px">curl</code> or the FFI library.
</div>
<main>
  <div class="editor-pane">
    <div class="pane-header">JSON Props — <span id="editor-label">listing</span></div>
    <textarea id="editor" spellcheck="false"></textarea>
  </div>
  <div class="preview-pane">
    <div class="pane-header">
      <span>Preview</span>
      <div class="tab-bar">
        <button class="active" onclick="setView('preview', this)">Preview</button>
        <button onclick="setView('source', this)">HTML Source</button>
        <button onclick="setView('markdown', this)">Markdown</button>
      </div>
    </div>
    <iframe id="preview-frame"></iframe>
    <textarea id="source-view" class="source-view" readonly></textarea>
  </div>
</main>
<script>
const editor = document.getElementById('editor');
const previewFrame = document.getElementById('preview-frame');
const sourceView = document.getElementById('source-view');
const statRender = document.getElementById('stat-render');
const statDeser = document.getElementById('stat-deser');
const statE2e = document.getElementById('stat-e2e');
const statBundle = document.getElementById('stat-bundle');
const openRouteLink = document.getElementById('open-route-link');
const showcaseSelect = document.getElementById('showcase-select');
const crumbPagesContainer = document.getElementById('crumb-pages');
let currentView = 'preview';
let lastHtml = '';
// Cached markdown rendering for the active page. Cleared whenever
// the active page changes or a fresh HTML render lands (since the
// underlying data may have changed too). The third tab fetches
// `/api/render-md` lazily so non-supporting pages don't pay the
// round-trip until the user actually asks for Markdown.
let lastMarkdown = null;

// Fetch the gzipped bundle size for the active engine + page combination
// and write it into the "Client JS (gz)" stat cell. Each engine has a
// fixed set of first-party JS URLs the page loads (rsjs runtime + per-
// island, marko-hydrate.js, react client.bundle.js, etc.); the server
// side endpoint fetches them and gzips so we don't fight CORS in the
// browser. Called from each doRender* path after a successful render.
async function updateBundleStat() {
  statBundle.textContent = '...';
  try {
    const r = await fetch('/api/bundle-size?engine=' + encodeURIComponent(currentEngine) + '&page=' + encodeURIComponent(currentPage));
    if (!r.ok) { statBundle.textContent = '—'; return; }
    const d = await r.json();
    const bytes = d.total_gzip || 0;
    if (bytes === 0) {
      statBundle.textContent = '0 B';
    } else if (bytes < 1024) {
      statBundle.textContent = bytes + ' B';
    } else {
      statBundle.textContent = (bytes / 1024).toFixed(1) + ' KB';
    }
  } catch (e) {
    statBundle.textContent = '—';
  }
}
let currentEngine = 'proa';

// Showcase definitions: pages + routes per showcase
const SHOWCASES = {
  airbnb: {
    pages: [
      { id: 'home', label: 'Home', size: '156 KB' },
      { id: 'search', label: 'Search', size: '47 KB' },
      { id: 'listing', label: 'Listing', size: '140 KB' },
    ],
    defaultPage: 'listing',
    routes: {
      proa:  { home: '/showcase/airbnb/proa', search: '/showcase/airbnb/proa/search', listing: '/showcase/airbnb/proa/listing' },
      node:  { home: '/showcase/airbnb/node', search: '/showcase/airbnb/node/search', listing: '/showcase/airbnb/node/listing' },
      bun:   { home: '/showcase/airbnb/bun',  search: '/showcase/airbnb/bun/search',  listing: '/showcase/airbnb/bun/listing' },
    },
  },
  nike: {
    pages: [
      { id: 'nike-catalog', label: 'Catalog', size: '103 KB' },
      { id: 'nike-pdp', label: 'Product', size: '56 KB' },
    ],
    defaultPage: 'nike-catalog',
    routes: {
      proa: { 'nike-catalog': '/showcase/nike/proa', 'nike-pdp': '/showcase/nike/proa/product' },
      node: { 'nike-catalog': '/showcase/nike/node', 'nike-pdp': '/showcase/nike/node/product' },
      bun:  { 'nike-catalog': '/showcase/nike/bun', 'nike-pdp': '/showcase/nike/bun/product' },
      'react-ssr': { 'nike-catalog': '/showcase/nike/react', 'nike-pdp': '/showcase/nike/react/product' },
      next: { 'nike-catalog': '/showcase/nike/next', 'nike-pdp': '/showcase/nike/next/product' },
    },
  },
  ebay: {
    pages: [
      { id: 'ebay-srp', label: 'Search Results', size: 'capture' },
    ],
    defaultPage: 'ebay-srp',
    routes: {
      proa: { 'ebay-srp': '/showcase/ebay/proa' },
      'marko-node': { 'ebay-srp': '/showcase/ebay/proa' },
      'react-ssr':  { 'ebay-srp': '/showcase/ebay/react' },
      next: { 'ebay-srp': '/showcase/ebay/next' },
      // Marko (Bun) intentionally commented out — see button above. Server
      // still runs on :5400, just hidden from the eBay UI.
      // 'marko-bun':  { 'ebay-srp': '/showcase/ebay/proa' },
    },
  },
  // Interactive variant: same eBay page, but every watch-heart is an
  // rsjs island and the toolbar carries three filter-chip islands.
  // Compares apples-to-apples client-JS bundle against Marko 6 hybrid
  // and React 19 SSR — see `proa_demo/bundle_size.mjs`.
  //
  // The page id `ebay-srp-rsjs` (not `ebay-srp`) is what /api/render
  // dispatches on; using a distinct id lets the playground select the
  // interactive code path without overloading the static page name.
  'ebay-rsjs': {
    pages: [
      { id: 'ebay-srp-rsjs', label: 'Search Results (interactive)', size: 'capture' },
    ],
    defaultPage: 'ebay-srp-rsjs',
    routes: {
      proa: { 'ebay-srp-rsjs': '/showcase/ebay/proa-rsjs' },
      'react-ssr': { 'ebay-srp-rsjs': '/showcase/ebay/react' },
      next: { 'ebay-srp-rsjs': '/showcase/ebay/next' },
    },
  },
  'github-pr': {
    pages: [
      { id: 'github-pr-diff', label: 'Files changed', size: 'generated' },
    ],
    defaultPage: 'github-pr-diff',
    routes: {
      proa: { 'github-pr-diff': '/showcase/github-pr-diff' },
      'react-ssr': { 'github-pr-diff': '/showcase/github-pr-diff/react' },
    },
  },
  espn: {
    pages: [
      { id: 'espn-gamecast', label: 'Gamecast', size: '33 KB' },
    ],
    defaultPage: 'espn-gamecast',
    routes: {
      proa: { 'espn-gamecast': '/showcase/espn/proa' },
      'react-ssr': { 'espn-gamecast': '/showcase/espn/react' },
    },
  },
  'sentry-otel': {
    pages: [
      { id: 'sentry-otel-logs', label: 'Trace Logs', size: '875 KB' },
    ],
    defaultPage: 'sentry-otel-logs',
    routes: {
      proa: { 'sentry-otel-logs': '/showcase/sentry-otel/proa' },
      'react-ssr': { 'sentry-otel-logs': '/showcase/sentry-otel/react' },
    },
  },
};

// Read showcase from URL: /playground/nike → "nike", /playground/airbnb/listing → "airbnb"
// Label for the JS comparison engines, varies by current showcase.
// eBay load tests hit Marko Node/Bun; everything else hits Hono React.
function jsEngineLabel(rt) {
  if (currentShowcase === 'ebay') {
    return rt === 'bun' ? 'Marko (Bun)' : 'Marko (Node)';
  }
  return rt === 'bun' ? 'Hono (Bun)' : 'Hono (Node)';
}

function getShowcaseFromUrl() {
  const m = location.pathname.match(/^\/playground\/([^/]+)/);
  return m && SHOWCASES[m[1]] ? m[1] : 'airbnb';
}
function getPageFromUrl() {
  const m = location.pathname.match(/^\/playground\/[^/]+\/(.+)/);
  if (m) {
    const sc = getShowcaseFromUrl();
    const found = SHOWCASES[sc]?.pages.find(p => p.id === m[1]);
    if (found) return found.id;
  }
  return null;
}

let currentShowcase = getShowcaseFromUrl();
let currentPage = getPageFromUrl() || SHOWCASES[currentShowcase].defaultPage;
let allProps = {};
const editorLabel = document.getElementById('editor-label');

function buildPageButtons() {
  const showcase = SHOWCASES[currentShowcase];
  crumbPagesContainer.innerHTML = '';
  showcase.pages.forEach(p => {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'crumb-page' + (p.id === currentPage ? ' active' : '');
    btn.dataset.page = p.id;
    btn.setAttribute('aria-pressed', p.id === currentPage ? 'true' : 'false');
    btn.innerHTML = '<span>' + p.label + '</span><span class="crumb-page-size">' + p.size + '</span>';
    btn.addEventListener('click', () => {
      if (p.id === currentPage) return;
      setCurrentPage(p.id);
      loadPropsForPage(currentPage);
      syncRouteLink();
      doRender();
    });
    crumbPagesContainer.appendChild(btn);
  });
}

function setCurrentPage(page) {
  currentPage = page;
  // Invalidate the Markdown cache: a new page means new content,
  // and the cached `render_md()` output no longer matches.
  lastMarkdown = null;
  crumbPagesContainer.querySelectorAll('.crumb-page').forEach(btn => {
    const active = btn.dataset.page === page;
    btn.classList.toggle('active', active);
    btn.setAttribute('aria-pressed', active ? 'true' : 'false');
  });
  pushPlaygroundUrl();
}

function pushPlaygroundUrl() {
  const url = '/playground/' + currentShowcase + '/' + currentPage;
  if (location.pathname !== url) {
    history.pushState(null, '', url);
  }
}

function setShowcase(showcase) {
  currentShowcase = showcase;
  showcaseSelect.value = showcase;
  currentPage = SHOWCASES[showcase].defaultPage;
  buildPageButtons();
  syncEngineButtons();
  loadPropsForPage(currentPage);
  syncRouteLink();
  pushPlaygroundUrl();
  doRender();
}

// Show only engine buttons supported by the current showcase. If the
// active engine isn't supported, snap back to Proa.
function syncEngineButtons() {
  const supported = SHOWCASES[currentShowcase] ? Object.keys(SHOWCASES[currentShowcase].routes) : ['proa'];
  let needsRepick = !supported.includes(currentEngine);
  document.querySelectorAll('.engine-btn').forEach(btn => {
    const ok = supported.includes(btn.dataset.engine);
    btn.style.display = ok ? '' : 'none';
    btn.disabled = !ok;
    if (!ok) btn.classList.remove('active');
  });
  if (needsRepick) {
    currentEngine = 'proa';
    const proaBtn = document.querySelector('.engine-btn[data-engine="proa"]');
    if (proaBtn) {
      document.querySelectorAll('.engine-btn').forEach(b => b.classList.remove('active'));
      proaBtn.classList.add('active');
    }
  }
  requestAnimationFrame(positionSlider);
}

showcaseSelect.addEventListener('change', () => {
  setShowcase(showcaseSelect.value);
});

// Calculate total JS the client needs for this page to be interactive.

function syncRouteLink() {
  const routes = SHOWCASES[currentShowcase]?.routes?.[currentEngine] || {};
  openRouteLink.href = routes[currentPage] || '#';
}

function positionSlider() {
  const toggle = document.querySelector('.engine-toggle');
  const slider = toggle.querySelector('.engine-slider');
  const activeBtn = toggle.querySelector('.engine-btn.active');
  if (!activeBtn || !slider) return;
  slider.style.left = activeBtn.offsetLeft + 'px';
  slider.style.width = activeBtn.offsetWidth + 'px';
}

function setEngine(engine, btn) {
  currentEngine = engine;
  document.querySelectorAll('.engine-btn').forEach(b => b.classList.remove('active'));
  btn.classList.add('active');
  positionSlider();

  syncRouteLink();
  doRender();
}

// Position slider on load
requestAnimationFrame(positionSlider);

// Build initial page buttons and sync URL
showcaseSelect.value = currentShowcase;
buildPageButtons();
syncEngineButtons();
pushPlaygroundUrl();

// Handle back/forward navigation
window.addEventListener('popstate', () => {
  const sc = getShowcaseFromUrl();
  const pg = getPageFromUrl() || SHOWCASES[sc].defaultPage;
  if (sc !== currentShowcase) {
    currentShowcase = sc;
    showcaseSelect.value = sc;
    currentPage = pg;
    buildPageButtons();
    loadPropsForPage(currentPage);
    syncRouteLink();
    doRender();
  } else if (pg !== currentPage) {
    currentPage = pg;
    setCurrentPage(pg);
    loadPropsForPage(currentPage);
    syncRouteLink();
    doRender();
  }
});

// Build a reverse lookup from showcase route paths → playground page IDs.
// e.g. "/showcase/airbnb/proa" → { showcase: "airbnb", page: "home" }
function buildRouteMap() {
  const map = {};
  for (const [sc, def] of Object.entries(SHOWCASES)) {
    for (const [engine, routes] of Object.entries(def.routes)) {
      for (const [pageId, route] of Object.entries(routes)) {
        map[route] = { showcase: sc, page: pageId };
      }
    }
  }
  return map;
}
const ROUTE_MAP = buildRouteMap();

// Returns a <script> tag that, when injected into the preview iframe,
// intercepts link clicks and posts a message to the parent playground
// instead of navigating inside the blob-origin iframe.
function buildNavInterceptScript() {
  return `<script>
(function(){
  var routeMap = ${JSON.stringify(ROUTE_MAP)};
  document.addEventListener('click', function(e) {
    var a = e.target.closest('a[href]');
    if (!a) return;
    var href = a.getAttribute('href');
    if (!href || href.startsWith('#') || href.startsWith('javascript:')) return;
    try { href = new URL(href, document.baseURI).pathname; } catch(ex) { return; }
    if (routeMap[href]) {
      e.preventDefault();
      e.stopPropagation();
      window.parent.postMessage({ type: 'proa-nav', href: href }, '*');
    }
  }, true);
  // Suppress Turbo Drive inside the iframe to prevent the progress bar.
  document.addEventListener('turbo:before-visit', function(e) { e.preventDefault(); }, true);
})();
<\/script>`;
}

// Listen for navigation messages from the preview iframe.
// Both the injected click-intercept script and airbnb-showcase.js
// send { type: 'proa-nav', href: '/showcase/...' }.
window.addEventListener('message', function(e) {
  if (!e.data || e.data.type !== 'proa-nav') return;
  var href = e.data.href;
  if (!href) return;
  var match = ROUTE_MAP[href];
  if (!match) return;
  var sc = match.showcase;
  var pg = match.page;
  if (sc !== currentShowcase) {
    setShowcase(sc);
    if (pg !== currentPage) {
      setCurrentPage(pg);
      loadPropsForPage(pg);
      syncRouteLink();
      doRender();
    }
  } else if (pg !== currentPage) {
    setCurrentPage(pg);
    loadPropsForPage(pg);
    syncRouteLink();
    doRender();
  }
});

function copyCurl() {
  const page = currentPage;
  let props;
  try { props = JSON.parse(editor.value); } catch { props = undefined; }
  const body = JSON.stringify(props ? { page, props } : { page });

  const apiPath = currentEngine === 'node' ? '/api/render-node'
    : currentEngine === 'bun' ? '/api/render-bun'
    : currentEngine === 'marko-node' ? '/api/render-marko-node'
    : currentEngine === 'marko-bun' ? '/api/render-marko-bun'
    : currentEngine === 'react-ssr' ? '/api/render-react-ssr'
    : currentEngine === 'next' ? '/api/render-next'
    : '/api/render';
  const origin = location.origin;

  const cmd = `curl -s -X POST ${origin}${apiPath} \\
  -H 'Content-Type: application/json' \\
  -d '${body.replace(/'/g, "'\\''")}'`;

  navigator.clipboard.writeText(cmd).then(() => {
    const btn = document.getElementById('copy-curl-btn');
    btn.textContent = 'Copied!';
    setTimeout(() => { btn.textContent = 'Copy curl'; }, 1500);
  });
}

// Load default props for all pages
fetch('/api/props')
  .then(r => r.json())
  .then(data => {
    allProps = data;
    loadPropsForPage(currentPage);
    syncRouteLink();
    doRender();
  });

function loadPropsForPage(page) {
  editorLabel.textContent = page;
  if (allProps[page]) {
    editor.value = JSON.stringify(allProps[page], null, 2);
  } else {
    editor.value = '{}';
  }
}

function setView(view, btn) {
  currentView = view;
  document.querySelectorAll('.tab-bar button').forEach(b => b.classList.remove('active'));
  btn.classList.add('active');
  if (view === 'preview') {
    previewFrame.style.display = 'block';
    sourceView.style.display = 'none';
    return;
  }
  previewFrame.style.display = 'none';
  sourceView.style.display = 'block';
  if (view === 'source') {
    sourceView.value = lastHtml;
    return;
  }
  if (view === 'markdown') {
    // Lazy-fetch on first click per page change. The endpoint
    // returns either the markdown body (200) or a "not supported"
    // message (501) — we show whichever directly in the source
    // pane so the user always sees a sensible response.
    if (lastMarkdown !== null) {
      sourceView.value = lastMarkdown;
      return;
    }
    sourceView.value = 'Rendering Markdown…';
    fetch('/api/render-md', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ page: currentPage }),
    })
      .then(r => r.text().then(t => ({ ok: r.ok, body: t })))
      .then(({ ok, body }) => {
        lastMarkdown = body;
        // If the view changed while the fetch was in flight, don't
        // clobber whatever the user is now looking at.
        if (currentView === 'markdown') {
          sourceView.value = body;
        }
        if (!ok) {
          // Surface the not-supported case clearly but don't error.
          console.info('Markdown render returned non-OK:', body);
        }
      })
      .catch(err => {
        sourceView.value = 'Markdown render failed: ' + err;
      });
  }
}

async function doRender() {
  if (currentEngine === 'node' || currentEngine === 'bun') {
    await doRenderReact(currentEngine);
    return;
  }
  if (currentEngine === 'marko-node' || currentEngine === 'marko-bun') {
    await doRenderMarko(currentEngine);
    return;
  }
  if (currentEngine === 'react-ssr') {
    await doRenderReactSsr();
    return;
  }
  if (currentEngine === 'next') {
    await doRenderNext();
    return;
  }
  const page = currentPage;
  let props = undefined;
  try { props = JSON.parse(editor.value); } catch(e) {
    statRender.textContent = 'JSON error';
    return;
  }
  statRender.textContent = '...';
  const res = await fetch('/api/render', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ page, props }),
  });
  // Proa returns raw HTML with timing in headers (no JSON escaping overhead)
  const html = await res.text();
  if (!res.ok) {
    showEngineUnavailable(res);
    return;
  }
  const renderUs = parseFloat(res.headers.get('x-render-us') || '0');
  const deserUs = parseFloat(res.headers.get('x-deser-us') || '0');
  const e2eUs = parseFloat(res.headers.get('x-e2e-us') || '0');
  const htmlSize = parseInt(res.headers.get('x-html-size') || '0', 10);
  lastHtml = html;
  const fmt = (us) => (us == null || isNaN(us) || us === 0) ? 'N/A' : us < 1000 ? us.toFixed(1) + ' µs' : (us / 1000).toFixed(2) + ' ms';
  statRender.textContent = fmt(renderUs);
  statDeser.textContent = deserUs > 0 ? fmt(deserUs) : '—';
  statE2e.textContent = fmt(e2eUs);
  updateBundleStat();
  // Display the rendered HTML in the preview iframe.
  // Inject a <base> tag so relative asset URLs resolve, plus a script that
  // intercepts in-page link clicks and routes them through the playground
  // instead of letting Turbo Drive (or the browser) navigate inside the
  // blob-origin iframe — which would hang with a progress bar.
  const baseTag = `<base href="${location.origin}/">`;
  const navScript = buildNavInterceptScript();
  const htmlWithBase = html.replace('<head>', '<head>' + baseTag + navScript);
  const blob = new Blob([htmlWithBase], { type: 'text/html' });
  previewFrame.src = URL.createObjectURL(blob);
  if (currentView === 'source') sourceView.value = lastHtml;
}

// A non-2xx from a render endpoint nearly always means that engine's SSR
// companion is not reachable. Every engine except Proa is a separate Node or
// Bun process that this server proxies to, and the proxy answers 502 when it
// cannot connect. Cloudflare then swaps our 502 body for its own error page,
// so the response text is a full HTML document that says nothing useful about
// the engine.
//
// Report the engine instead of that body, and blank the sibling stats: leaving
// the previous engine's render/deser/e2e numbers on screen next to a failed
// render reads as though the failed engine produced them.
function showEngineUnavailable(res) {
  const btn = document.querySelector('.engine-btn[data-engine="' + currentEngine + '"]');
  const label = btn ? btn.textContent.trim() : currentEngine;
  const unreachable = res.status === 502 || res.status === 503 || res.status === 504;
  statRender.textContent = unreachable ? 'unavailable' : 'HTTP ' + res.status;
  statDeser.textContent = '—';
  statE2e.textContent = '—';
  const detail = unreachable
    ? label + ' is not running in this deployment. It renders in a separate SSR companion process, and this server got HTTP ' + res.status + ' trying to reach it.'
    : label + ' returned HTTP ' + res.status + '.';
  lastHtml = '';
  const esc = (t) => String(t).replace(/&/g, '&amp;').replace(/</g, '&lt;');
  const doc = '<!doctype html><meta charset="utf-8"><body style="margin:0;padding:28px;font:14px/1.6 ui-sans-serif,system-ui,sans-serif;background:#0a0a0b;color:#a3a3a3">'
    + '<h2 style="margin:0 0 10px;font-size:15px;font-weight:600;color:#fafafa">' + esc(label) + ' unavailable</h2>'
    + '<p style="margin:0;max-width:62ch">' + esc(detail) + '</p></body>';
  previewFrame.src = URL.createObjectURL(new Blob([doc], { type: 'text/html' }));
  if (currentView === 'source') sourceView.value = detail;
}

async function doRenderReact(engine) {
  const page = currentPage;
  let props = undefined;
  try { props = JSON.parse(editor.value); } catch(e) {
    statRender.textContent = 'JSON error';
    return;
  }
  statRender.textContent = '...';

  const apiPath = engine === 'bun' ? '/api/render-bun' : '/api/render-node';
  const res = await fetch(apiPath, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ page, props }),
  });
  const html = await res.text();
  if (!res.ok) {
    showEngineUnavailable(res);
    return;
  }
  const renderUs = parseFloat(res.headers.get('x-render-us') || '0');
  const deserUs = parseFloat(res.headers.get('x-deser-us') || '0');
  const e2eUs = parseFloat(res.headers.get('x-e2e-us') || '0');
  const htmlSize = parseInt(res.headers.get('x-html-size') || '0', 10);
  lastHtml = html;
  const fmt = (us) => (us == null || isNaN(us) || us === 0) ? 'N/A' : us < 1000 ? us.toFixed(1) + ' \u00b5s' : (us / 1000).toFixed(2) + ' ms';
  statRender.textContent = fmt(renderUs);
  statDeser.textContent = deserUs > 0 ? fmt(deserUs) : '\u2014';
  statE2e.textContent = fmt(e2eUs);
  updateBundleStat();

  // Display in iframe via blob URL with base tag for asset resolution
  const basePath = engine === 'bun' ? '/showcase/airbnb/bun/' : '/showcase/airbnb/node/';
  const baseTag = '<base href="' + location.origin + basePath + '">';
  const navScript = buildNavInterceptScript();
  const htmlWithBase = html.replace('<head>', '<head>' + baseTag + navScript);
  const blob = new Blob([htmlWithBase], { type: 'text/html' });
  previewFrame.src = URL.createObjectURL(blob);
  if (currentView === 'source') sourceView.value = lastHtml;
}

async function doRenderReactSsr() {
  const page = currentPage;
  let props = undefined;
  try { props = JSON.parse(editor.value); } catch(e) {
    statRender.textContent = 'JSON error';
    return;
  }
  statRender.textContent = '...';
  const res = await fetch('/api/render-react-ssr', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ page, props }),
  });
  const html = await res.text();
  if (!res.ok) {
    showEngineUnavailable(res);
    return;
  }
  const renderUs = parseFloat(res.headers.get('x-render-us') || '0');
  const deserUs = parseFloat(res.headers.get('x-deser-us') || '0');
  const e2eUs = parseFloat(res.headers.get('x-e2e-us') || '0');
  lastHtml = html;
  const fmt = (us) => (us == null || isNaN(us) || us === 0) ? 'N/A' : us < 1000 ? us.toFixed(1) + ' µs' : (us / 1000).toFixed(2) + ' ms';
  statRender.textContent = fmt(renderUs);
  statDeser.textContent = deserUs > 0 ? fmt(deserUs) : '—';
  statE2e.textContent = fmt(e2eUs);
  updateBundleStat();

  const baseTag = '<base href="' + location.origin + '/">';
  const navScript = buildNavInterceptScript();
  // Route the React hydrate bundle through this app; preview/prod browsers
  // cannot reach the loopback-only companion process directly.
  const htmlRewritten = html
    .replace('<head>', '<head>' + baseTag + navScript)
    .replace('src="/client.bundle.js"', 'src="/react-ssr/client.bundle.js"')
    .replace('src="/espn-client.bundle.js"', 'src="/react-ssr/espn-client.bundle.js"')
    .replace('src="/sentry-otel-client.bundle.js"', 'src="/react-ssr/sentry-otel-client.bundle.js"');
  const blob = new Blob([htmlRewritten], { type: 'text/html' });
  previewFrame.src = URL.createObjectURL(blob);
  if (currentView === 'source') sourceView.value = lastHtml;
}

async function doRenderNext() {
  const page = currentPage;
  let props = undefined;
  try { props = JSON.parse(editor.value); } catch(e) {
    statRender.textContent = 'JSON error';
    return;
  }
  statRender.textContent = '...';
  const res = await fetch('/api/render-next', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ page, props }),
  });
  const html = await res.text();
  if (!res.ok) {
    showEngineUnavailable(res);
    return;
  }
  const renderUs = parseFloat(res.headers.get('x-render-us') || '0');
  const deserUs = parseFloat(res.headers.get('x-deser-us') || '0');
  const e2eUs = parseFloat(res.headers.get('x-e2e-us') || '0');
  lastHtml = html;
  const fmt = (us) => (us == null || isNaN(us) || us === 0) ? 'N/A' : us < 1000 ? us.toFixed(1) + ' µs' : (us / 1000).toFixed(2) + ' ms';
  statRender.textContent = fmt(renderUs);
  statDeser.textContent = deserUs > 0 ? fmt(deserUs) : '—';
  statE2e.textContent = fmt(e2eUs);
  updateBundleStat();

  const baseTag = '<base href="' + location.origin + '/">';
  const navScript = buildNavInterceptScript();
  const htmlWithBase = html.replace('<head>', '<head>' + baseTag + navScript);
  const blob = new Blob([htmlWithBase], { type: 'text/html' });
  previewFrame.src = URL.createObjectURL(blob);
  if (currentView === 'source') sourceView.value = lastHtml;
}

async function doRenderMarko(engine) {
  const page = currentPage;
  let props = undefined;
  try { props = JSON.parse(editor.value); } catch(e) {
    statRender.textContent = 'JSON error';
    return;
  }
  statRender.textContent = '...';

  const apiPath = engine === 'marko-bun' ? '/api/render-marko-bun' : '/api/render-marko-node';
  const res = await fetch(apiPath, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ page, props }),
  });
  const html = await res.text();
  if (!res.ok) {
    showEngineUnavailable(res);
    return;
  }
  const renderUs = parseFloat(res.headers.get('x-render-us') || '0');
  const e2eUs = parseFloat(res.headers.get('x-e2e-us') || '0');
  lastHtml = html;
  const fmt = (us) => (us == null || isNaN(us) || us === 0) ? 'N/A' : us < 1000 ? us.toFixed(1) + ' \u00b5s' : (us / 1000).toFixed(2) + ' ms';
  statRender.textContent = fmt(renderUs);
  statDeser.textContent = '\u2014';
  statE2e.textContent = fmt(e2eUs);
  updateBundleStat();

  const baseTag = '<base href="' + location.origin + '/">';
  const navScript = buildNavInterceptScript();
  const htmlWithBase = html.replace('<head>', '<head>' + baseTag + navScript);
  const blob = new Blob([htmlWithBase], { type: 'text/html' });
  previewFrame.src = URL.createObjectURL(blob);
  if (currentView === 'source') sourceView.value = lastHtml;
}

// Ctrl/Cmd+Enter to render
editor.addEventListener('keydown', (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
    e.preventDefault();
    doRender();
  }
});

async function runLoadTest() {
  const page = currentPage;
  const btn = document.getElementById('loadtest-btn');
  const preset = document.getElementById('loadtest-preset').value;
  const configs = {
    light:  { iterations: 500,   concurrency: 10 },
    medium: { iterations: 2000,  concurrency: 50 },
    heavy:  { iterations: 5000,  concurrency: 100 },
    stress: { iterations: 10000, concurrency: 200 },
  };
  const cfg = configs[preset] || configs.medium;
  btn.textContent = 'Running (' + cfg.iterations + ' \u00d7 ' + cfg.concurrency + ')...';
  btn.disabled = true;

  try {
    const res = await fetch('/api/loadtest', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ page, iterations: cfg.iterations, concurrency: cfg.concurrency }),
    });
    const data = await res.json();
    showLoadTestResults(data);
  } catch (e) {
    alert('Load test failed: ' + e.message);
  } finally {
    btn.textContent = 'Load Test';
    btn.disabled = false;
  }
}

function showLoadTestResults(data) {
  const fmt = (us) => (us == null || isNaN(us) || us === 0) ? 'N/A' : us < 1000 ? us.toFixed(1) + ' \u00b5s' : (us / 1000).toFixed(2) + ' ms';
  const fmtRps = (rps) => (rps == null || isNaN(rps) || rps === 0) ? 'N/A' : rps > 1000 ? (rps / 1000).toFixed(1) + 'K' : rps.toFixed(0);

  let overlay = document.getElementById('loadtest-overlay');
  if (!overlay) {
    overlay = document.createElement('div');
    overlay.id = 'loadtest-overlay';
    overlay.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,0.7);z-index:9999;backdrop-filter:blur(4px);overflow-y:auto';
    overlay.onclick = (e) => { if (e.target === overlay) overlay.remove(); };
    document.body.appendChild(overlay);
  }

  const safeRatio = (a, b) => b > 0 && a > 0 ? (a / b).toFixed(0) : 'N/A';
  const emptyStats = { avg: 0, p50: 0, p99: 0, min: 0, max: 0 };
  const emptyEngine = { total_requests: 0, failed_requests: 0, duration_ms: 0, rps: 0, ssr_us: emptyStats, doc_us: emptyStats, roundtrip_us: emptyStats };
  const next = data.next || emptyEngine;
  const reactSsr = data.react_ssr || emptyEngine;
  data.proa.doc_us = data.proa.doc_us || data.proa.ssr_us;
  data.react.doc_us = data.react.doc_us || data.react.ssr_us;
  data.bun.doc_us = data.bun.doc_us || data.bun.ssr_us;
  next.doc_us = next.doc_us || next.ssr_us;
  reactSsr.doc_us = reactSsr.doc_us || reactSsr.ssr_us;
  const showNext = (next.total_requests + next.failed_requests) > 0 || next.ssr_us.avg > 0 || next.doc_us.avg > 0 || next.roundtrip_us.avg > 0;
  const showReactSsr = (reactSsr.total_requests + reactSsr.failed_requests) > 0 || reactSsr.ssr_us.avg > 0 || reactSsr.doc_us.avg > 0 || reactSsr.roundtrip_us.avg > 0;
  const ssrRatioNode = safeRatio(data.react.ssr_us.avg, data.proa.ssr_us.avg);
  const ssrRatioBun = safeRatio(data.bun.ssr_us.avg, data.proa.ssr_us.avg);
  const ssrRatioReactSsr = showReactSsr ? safeRatio(reactSsr.ssr_us.avg, data.proa.ssr_us.avg) : 'N/A';
  const ssrRatioNext = showNext ? safeRatio(next.ssr_us.avg, data.proa.ssr_us.avg) : 'N/A';
  const docRatioNode = safeRatio(data.react.doc_us.avg, data.proa.doc_us.avg);
  const docRatioBun = safeRatio(data.bun.doc_us.avg, data.proa.doc_us.avg);
  const docRatioReactSsr = showReactSsr ? safeRatio(reactSsr.doc_us.avg, data.proa.doc_us.avg) : 'N/A';
  const docRatioNext = showNext ? safeRatio(next.doc_us.avg, data.proa.doc_us.avg) : 'N/A';
  const rtRatioNode = safeRatio(data.react.roundtrip_us.avg, data.proa.roundtrip_us.avg);
  const rtRatioBun = safeRatio(data.bun.roundtrip_us.avg, data.proa.roundtrip_us.avg);
  const rtRatioReactSsr = showReactSsr ? safeRatio(reactSsr.roundtrip_us.avg, data.proa.roundtrip_us.avg) : 'N/A';
  const rtRatioNext = showNext ? safeRatio(next.roundtrip_us.avg, data.proa.roundtrip_us.avg) : 'N/A';

  const th = 'style="text-align:left;padding:8px 12px;color:var(--text-muted);font-weight:600;text-transform:uppercase;font-size:11px;letter-spacing:0.1em"';
  const thP = 'style="text-align:right;padding:8px 12px;color:var(--accent);font-weight:600;text-transform:uppercase;font-size:11px;letter-spacing:0.1em"';
  const thR = 'style="text-align:right;padding:8px 12px;color:var(--text-muted);font-weight:600;text-transform:uppercase;font-size:11px;letter-spacing:0.1em"';
  const thB = 'style="text-align:right;padding:8px 12px;color:#60a5fa;font-weight:600;text-transform:uppercase;font-size:11px;letter-spacing:0.1em"';
  const thS = 'style="text-align:right;padding:8px 12px;color:#22d3ee;font-weight:600;text-transform:uppercase;font-size:11px;letter-spacing:0.1em"';
  const thN = 'style="text-align:right;padding:8px 12px;color:#a78bfa;font-weight:600;text-transform:uppercase;font-size:11px;letter-spacing:0.1em"';
  const tdL = 'style="padding:10px 12px;color:var(--text-muted)"';
  const tdP = 'style="padding:10px 12px;text-align:right;color:var(--green)"';
  const tdR = 'style="padding:10px 12px;text-align:right"';
  const tdB = 'style="padding:10px 12px;text-align:right;color:#60a5fa"';
  const tdS = 'style="padding:10px 12px;text-align:right;color:#22d3ee"';
  const tdN = 'style="padding:10px 12px;text-align:right;color:#a78bfa"';
  const row = 'style="border-bottom:1px solid var(--border)"';

  // Preserve the historical eBay engine set while keeping the Marko/Bun
  // endpoint available for explicit manual comparisons.
  const hideBun = currentShowcase === 'ebay';

  function makeChart(title, subtitle, proa, react, bun, reactSsrEngine, nextEngine, ratioNode, ratioBun, ratioReactSsr, ratioNext) {
    const values = [proa.avg, react.avg];
    if (!hideBun) values.push(bun.avg);
    if (showReactSsr) values.push(reactSsrEngine.avg);
    if (showNext) values.push(nextEngine.avg);
    const maxVal = Math.max(...values);
    function bar(label, value, color, ratio) {
      const pct = maxVal > 0 ? (value / maxVal) * 100 : 0;
      const ratioText = ratio && ratio !== 'N/A' ? `<span style="color:var(--text-muted);font-size:11px;margin-left:8px">${ratio}x slower</span>` : (ratio === 'N/A' ? '<span style="color:#f59e0b;font-size:11px;margin-left:8px">N/A</span>' : '');
      return `<div style="margin-bottom:14px">
        <div style="display:flex;justify-content:space-between;align-items:baseline;margin-bottom:4px">
          <span style="font-size:13px;font-weight:500">${label}</span>
          <span style="font-size:13px;font-family:var(--mono)">${fmt(value)}${ratioText}</span>
        </div>
        <div style="background:var(--border);border-radius:6px;height:28px;overflow:hidden;position:relative">
          <div style="background:${color};height:100%;border-radius:6px;width:${Math.max(pct, 0.5)}%;transition:width 0.3s;min-width:3px"></div>
        </div>
      </div>`;
    }
    return `<div style="margin-bottom:28px">
      <div style="display:flex;align-items:baseline;gap:8px;margin-bottom:16px">
        <h3 style="font-size:15px;font-weight:600">${title}</h3>
        <span style="font-size:12px;color:var(--text-muted)">${subtitle}</span>
      </div>
      ${bar('Proa (Rust)', proa.avg, 'var(--green)', null)}
      ${bar(jsEngineLabel('node'), react.avg, '#e5e5e5', ratioNode)}
      ${showReactSsr ? bar('React (SSR)', reactSsrEngine.avg, '#22d3ee', ratioReactSsr) : ''}
      ${hideBun ? '' : bar(jsEngineLabel('bun'), bun.avg, '#60a5fa', ratioBun)}
      ${showNext ? bar('Next.js', nextEngine.avg, '#a78bfa', ratioNext) : ''}
      <p style="color:var(--text-muted);font-size:11px;margin-top:4px">Avg time \u00b7 lower is better</p>
    </div>`;
  }

  function makeTable(title, subtitle, proa, react, bun, reactSsrEngine, nextEngine, ratioNode, ratioBun, ratioReactSsr, ratioNext) {
    return `
      <div style="margin-bottom:24px">
        <div style="display:flex;align-items:baseline;gap:8px;margin-bottom:12px">
          <h3 style="font-size:15px;font-weight:600">${title}</h3>
          <span style="font-size:12px;color:var(--text-muted)">${subtitle}</span>
        </div>
        <table style="width:100%;border-collapse:collapse;font-size:13px">
          <thead>
            <tr ${row}>
              <th ${th}>Metric</th>
              <th ${thP}>Proa (Rust)</th>
              <th ${thR}>${jsEngineLabel('node')}</th>
              ${showReactSsr ? `<th ${thS}>React (SSR)</th>` : ''}
              ${hideBun ? '' : `<th ${thB}>${jsEngineLabel('bun')}</th>`}
              ${showNext ? `<th ${thN}>Next.js</th>` : ''}
            </tr>
          </thead>
          <tbody style="font-family:var(--mono)">
            <tr ${row}><td ${tdL}>Avg</td><td ${tdP}>${fmt(proa.avg)}</td><td ${tdR}>${fmt(react.avg)}</td>${showReactSsr ? `<td ${tdS}>${fmt(reactSsrEngine.avg)}</td>` : ''}${hideBun ? '' : `<td ${tdB}>${fmt(bun.avg)}</td>`}${showNext ? `<td ${tdN}>${fmt(nextEngine.avg)}</td>` : ''}</tr>
            <tr ${row}><td ${tdL}>P50</td><td ${tdP}>${fmt(proa.p50)}</td><td ${tdR}>${fmt(react.p50)}</td>${showReactSsr ? `<td ${tdS}>${fmt(reactSsrEngine.p50)}</td>` : ''}${hideBun ? '' : `<td ${tdB}>${fmt(bun.p50)}</td>`}${showNext ? `<td ${tdN}>${fmt(nextEngine.p50)}</td>` : ''}</tr>
            <tr ${row}><td ${tdL}>P99</td><td ${tdP}>${fmt(proa.p99)}</td><td ${tdR}>${fmt(react.p99)}</td>${showReactSsr ? `<td ${tdS}>${fmt(reactSsrEngine.p99)}</td>` : ''}${hideBun ? '' : `<td ${tdB}>${fmt(bun.p99)}</td>`}${showNext ? `<td ${tdN}>${fmt(nextEngine.p99)}</td>` : ''}</tr>
            <tr ${row}><td ${tdL}>Min</td><td ${tdP}>${fmt(proa.min)}</td><td ${tdR}>${fmt(react.min)}</td>${showReactSsr ? `<td ${tdS}>${fmt(reactSsrEngine.min)}</td>` : ''}${hideBun ? '' : `<td ${tdB}>${fmt(bun.min)}</td>`}${showNext ? `<td ${tdN}>${fmt(nextEngine.min)}</td>` : ''}</tr>
            <tr><td ${tdL}>Max</td><td ${tdP}>${fmt(proa.max)}</td><td ${tdR}>${fmt(react.max)}</td>${showReactSsr ? `<td ${tdS}>${fmt(reactSsrEngine.max)}</td>` : ''}${hideBun ? '' : `<td ${tdB}>${fmt(bun.max)}</td>`}${showNext ? `<td ${tdN}>${fmt(nextEngine.max)}</td>` : ''}</tr>
          </tbody>
        </table>
      </div>`;
  }

  const failureNotes = [
    data.react.failed_requests > 0 ? jsEngineLabel('node') + ': ' + data.react.failed_requests + '/' + (data.react.total_requests + data.react.failed_requests) + ' requests failed' : '',
    (!hideBun && data.bun.failed_requests > 0) ? jsEngineLabel('bun') + ': ' + data.bun.failed_requests + '/' + (data.bun.total_requests + data.bun.failed_requests) + ' requests failed' : '',
    (showReactSsr && reactSsr.failed_requests > 0) ? 'React (SSR): ' + reactSsr.failed_requests + '/' + (reactSsr.total_requests + reactSsr.failed_requests) + ' requests failed' : '',
    (showNext && next.failed_requests > 0) ? 'Next.js: ' + next.failed_requests + '/' + (next.total_requests + next.failed_requests) + ' requests failed' : '',
  ].filter(Boolean);

  overlay.innerHTML = `
    <div style="background:var(--bg-card);border:1px solid var(--border);border-radius:24px;padding:32px;max-width:900px;width:90%;color:var(--text);font-family:var(--font);margin:40px auto 40px">
      <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px">
        <h2 style="font-size:22px;font-weight:600;letter-spacing:-0.03em">Load Test Results</h2>
        <button onclick="this.closest('#loadtest-overlay').remove()" style="background:none;border:none;color:var(--text-muted);cursor:pointer;font-size:20px">&times;</button>
      </div>
      <p style="color:var(--text-muted);font-size:13px;margin-bottom:4px">${data.proa.total_requests + data.proa.failed_requests} iterations \u00b7 ${currentPage} page \u00b7 Throughput: Proa <span style="color:var(--green);font-weight:600">${fmtRps(data.proa.rps)} req/s</span> vs ${jsEngineLabel('node')} ${fmtRps(data.react.rps)} req/s${showReactSsr ? ` vs React (SSR) <span style=\"color:#22d3ee;font-weight:600\">${fmtRps(reactSsr.rps)} req/s</span>` : ''}${hideBun ? '' : ` vs ${jsEngineLabel('bun')} <span style=\"color:#60a5fa;font-weight:600\">${fmtRps(data.bun.rps)} req/s</span>`}${showNext ? ` vs Next.js <span style=\"color:#a78bfa;font-weight:600\">${fmtRps(next.rps)} req/s</span>` : ''}</p>
      ${failureNotes.length ? `<p style="color:#f59e0b;font-size:12px;margin-bottom:4px;padding:6px 10px;background:rgba(245,158,11,0.08);border:1px solid rgba(245,158,11,0.2);border-radius:6px">${failureNotes.join(' \u00b7 ')}. Results may be inaccurate \u2014 the JS server may be overloaded or not running.</p>` : ''}
      <p style="color:var(--text-muted);font-size:11px;margin-bottom:24px;padding:8px 12px;background:rgba(255,255,255,0.03);border:1px solid var(--border);border-radius:8px">All engines single-threaded, tested sequentially. Inner render is only the component/list render. Full document assembly adds shell wrapping and seed serialization. HTTP roundtrip also drains the full response body over localhost, which can dominate larger pages.</p>
      ${makeChart('Inner Render Time', 'avg, component/list \u2192 HTML bytes', data.proa.ssr_us, data.react.ssr_us, data.bun.ssr_us, reactSsr.ssr_us, next.ssr_us, ssrRatioNode, ssrRatioBun, ssrRatioReactSsr, ssrRatioNext)}
      ${makeChart('Document Assembly', 'avg, render + shell + seed', data.proa.doc_us, data.react.doc_us, data.bun.doc_us, reactSsr.doc_us, next.doc_us, docRatioNode, docRatioBun, docRatioReactSsr, docRatioNext)}
      ${makeChart('HTTP Roundtrip', 'avg, full request \u2192 response', data.proa.roundtrip_us, data.react.roundtrip_us, data.bun.roundtrip_us, reactSsr.roundtrip_us, next.roundtrip_us, rtRatioNode, rtRatioBun, rtRatioReactSsr, rtRatioNext)}
      ${makeTable('Inner Render Time', 'component/list \u2192 HTML bytes (no shell)', data.proa.ssr_us, data.react.ssr_us, data.bun.ssr_us, reactSsr.ssr_us, next.ssr_us, ssrRatioNode, ssrRatioBun, ssrRatioReactSsr, ssrRatioNext)}
      ${makeTable('Document Assembly', 'render + shell wrapping + seed serialization (no HTTP)', data.proa.doc_us, data.react.doc_us, data.bun.doc_us, reactSsr.doc_us, next.doc_us, docRatioNode, docRatioBun, docRatioReactSsr, docRatioNext)}
      ${makeTable('HTTP Roundtrip', 'full request \u2192 response (both via localhost)', data.proa.roundtrip_us, data.react.roundtrip_us, data.bun.roundtrip_us, reactSsr.roundtrip_us, next.roundtrip_us, rtRatioNode, rtRatioBun, rtRatioReactSsr, rtRatioNext)}
      <div style="border-top:1px solid var(--border);padding-top:16px;display:flex;justify-content:space-between;gap:12px;flex-wrap:wrap;font-size:13px;font-family:var(--mono)">
        <div><span style="color:var(--text-muted)">Total time</span> <span style="color:var(--green);margin-left:8px">${data.proa.duration_ms.toFixed(1)} ms</span> <span style="color:var(--text-muted);margin-left:4px">Proa</span></div>
        <div><span style="color:var(--text-muted)">${data.react.duration_ms.toFixed(1)} ms</span> <span style="color:var(--text-muted);margin-left:4px">${jsEngineLabel('node')}</span></div>
        ${showReactSsr ? `<div><span style=\"color:#22d3ee\">${reactSsr.duration_ms.toFixed(1)} ms</span> <span style=\"color:var(--text-muted);margin-left:4px\">React (SSR)</span></div>` : ''}
        ${hideBun ? '' : `<div><span style=\"color:#60a5fa\">${data.bun.duration_ms.toFixed(1)} ms</span> <span style=\"color:var(--text-muted);margin-left:4px\">${jsEngineLabel('bun')}</span></div>`}
        ${showNext ? `<div><span style=\"color:#a78bfa\">${next.duration_ms.toFixed(1)} ms</span> <span style=\"color:var(--text-muted);margin-left:4px\">Next.js</span></div>` : ''}
      </div>
    </div>
  `;
}
</script>
<footer style="text-align:center;padding:12px 0 16px;font-size:11px;color:var(--text-muted, #666);opacity:0.6">&copy; 2026 Proa Labs, Inc. All rights reserved.</footer>
</body>
</html>"##;
