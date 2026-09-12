// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::fixture::{LogPattern, LOG_PATTERNS};
use super::{CSS, DEFAULT_ROW_COUNT};
use proa_core::{
    ClassList, DataLoader, RenderBuffer, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;
#[cfg(feature = "rsjs-pages")]
use rsjs::{event_handler, signal};
#[cfg(feature = "rsjs-pages")]
use rsjs_macros::rsjs;

#[cfg(feature = "rsjs-pages")]
pub fn render() -> String {
    render_rows(DEFAULT_ROW_COUNT)
}

/// Renders and finalizes an owned Sentry document with `row_count` log rows.
#[cfg(feature = "rsjs-pages")]
pub fn render_rows(row_count: usize) -> String {
    let mut out = RenderBuffer::new();
    let (artifacts, nonce) = {
        let mut cx = WebContext::with_buffer(&mut out);
        render_into(&mut cx, row_count).expect("Sentry showcase page should render");
        (cx.rsjs_artifacts_snapshot(), cx.csp_nonce_cloned())
    };
    let mut html = out.into_string();
    crate::finalize_rsjs_document(&mut html, &artifacts, nonce.as_ref())
        .expect("Sentry rsjs runtime plan should render");
    html
}

/// Renders the hydration-independent document used by the React/Proa SSR
/// comparison. Both benchmarks time this same application DOM; framework
/// hydration transports are deliberately outside the SSR comparison.
pub fn render_benchmark_rows(row_count: usize) -> String {
    let mut out = RenderBuffer::new();
    let mut cx = WebContext::with_buffer(&mut out);
    SentrySsrBenchmarkPage {
        props: SentryPageProps {
            row_count,
            ..SentryPageProps::default()
        },
    }
    .render(&mut cx)
    .expect("Sentry SSR benchmark page should render");
    out.into_string()
}

#[cfg(feature = "rsjs-pages")]
pub fn render_into<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    row_count: usize,
) -> Result<(), WriteError> {
    render_into_with_props(
        cx,
        SentryPageProps {
            row_count,
            ..SentryPageProps::default()
        },
    )
}

#[cfg(feature = "rsjs-pages")]
pub fn render_into_with_props<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    props: SentryPageProps<'_>,
) -> Result<(), WriteError> {
    SentryOtelPage { props }.render(cx)
}

#[derive(Debug, Clone, Copy)]
pub struct SentryPageProps<'a> {
    pub row_count: usize,
    pub logs: Option<&'a [SentryLogEntry]>,
    pub trace_name: &'a str,
    pub trace_id: &'a str,
    pub issues: u32,
    pub spans: u32,
    pub age: &'a str,
    pub root_duration: &'a str,
    pub log_count: u32,
    pub client_ip: &'a str,
    pub browser: &'a str,
    pub environment: &'a str,
}

impl Default for SentryPageProps<'static> {
    fn default() -> Self {
        Self {
            row_count: DEFAULT_ROW_COUNT,
            logs: None,
            trace_name: "pageload",
            trace_id: "9f97da3a648f",
            issues: 2,
            spans: 102,
            age: "37m",
            root_duration: "108.00ms",
            log_count: 40,
            client_ip: "192.0.2.42",
            browser: "Chrome 148.0.0",
            environment: "local",
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SentryLogEntry {
    pub timestamp: String,
    pub message: String,
    pub severity: String,
    pub source: String,
    pub span_id: String,
    pub payload_size: u32,
    pub checkout_tax_cents: u32,
    pub checkout_total_cents: u32,
    pub code_line: u32,
    pub code_path: String,
    pub function_name: String,
    pub environment: String,
    pub instrumentation_name: String,
    pub origin: String,
    pub service_name: String,
    pub sdk_language: String,
    pub sdk_name: String,
    pub sdk_version: String,
    pub platform: String,
    pub trace_id: String,
    #[serde(default)]
    pub expanded: bool,
}

#[cfg(feature = "rsjs-pages")]
pub struct SentryOtelPage<'a> {
    pub props: SentryPageProps<'a>,
}

#[cfg(feature = "rsjs-pages")]
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for SentryOtelPage<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let log_stream = LogStream {
            row_count: self.props.row_count,
            logs: self.props.logs,
        };
        SentryDocument {
            props: self.props,
            log_stream,
        }
        .render(cx)
    }
}

struct SentrySsrBenchmarkPage<'a> {
    props: SentryPageProps<'a>,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for SentrySsrBenchmarkPage<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let log_stream = StaticLogStream {
            row_count: self.props.row_count,
            logs: self.props.logs,
        };
        SentryDocument {
            props: self.props,
            log_stream,
        }
        .render(cx)
    }
}

struct SentryDocument<'a, S> {
    props: SentryPageProps<'a>,
    log_stream: S,
}

impl<B, L, S> WebRenderSync<L, B> for SentryDocument<'_, S>
where
    B: WriteBuf,
    L: DataLoader,
    S: WebRenderSync<L, B>,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <meta name="robots" content="noindex, nofollow" />
                    <title>"Trace Logs — Sentry OpenTelemetry Showcase"</title>
                    <style>{StaticRaw(CSS)}</style>
                </head>
                <body data-sentry-otel>
                    <div class="sentry-app">
                        {SideRail}
                        <main class="sentry-main">
                            {TopBar { trace_id: self.props.trace_id }}
                            {TraceHeader { props: self.props }}
                            {self.log_stream}
                        </main>
                    </div>
                </body>
            </html>
        }
        .render(cx)
    }
}

struct SideRail;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for SideRail {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <aside class="side-rail" aria-label="Sentry navigation">
                <a class="org-mark" href="#" aria-label="Sandbox Demo">"SD"</a>
                <nav class="primary-nav">
                    <a href="#">
                        <span class="nav-icon nav-icon-inbox"></span>
                        <b>"Issues"</b>
                    </a>
                    <a class="active" href="#">
                        <span class="nav-icon nav-icon-compass"></span>
                        <b>"Explore"</b>
                    </a>
                    <a href="#">
                        <span class="nav-icon nav-icon-grid"></span>
                        <b>"Dashboards"</b>
                    </a>
                    <a href="#">
                        <span class="nav-icon nav-icon-bell"></span>
                        <b>"Monitors"</b>
                    </a>
                    <a href="#">
                        <span class="nav-icon nav-icon-gear"></span>
                        <b>"Settings"</b>
                    </a>
                </nav>
                <div class="rail-tools" aria-hidden="true">
                    <span>"⌕"</span>
                    <span class="tool-active">"✓"</span>
                    <span>"◭"</span>
                    <span>"◉"</span>
                    <span>"…"</span>
                    <span class="avatar">"PX"</span>
                </div>
            </aside>
        }
        .render(cx)
    }
}

struct TopBar<'a> {
    trace_id: &'a str,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for TopBar<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <header class="top-bar">
                <div class="breadcrumbs">
                    <a href="#">"Traces"</a>
                    <span>"/"</span>
                    <i class="sdk-icon">"⚛"</i>
                    <b>{self.trace_id}</b>
                </div>
                <div class="top-actions">
                    <button type="button" class="ask-seer">
                        <span>"△"</span>
                        "Ask Seer"
                        <kbd>"⌘ /"</kbd>
                    </button>
                    <button type="button" class="feedback-button" aria-label="Give feedback">
                        "⚑"
                    </button>
                </div>
            </header>
        }
        .render(cx)
    }
}

struct TraceHeader<'a> {
    props: SentryPageProps<'a>,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for TraceHeader<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section class="trace-header">
                <div class="trace-title-row">
                    <div class="trace-title">
                        <h1>{self.props.trace_name}</h1>
                        <p>"/"</p>
                    </div>
                    <a class="replay-link" href="#">"▷ 1 Replay"</a>
                    <dl class="trace-stats">
                        <div>
                            <dt>"Issues"</dt>
                            <dd class="purple">{self.props.issues}</dd>
                        </div>
                        <div>
                            <dt>"Spans"</dt>
                            <dd>{self.props.spans}</dd>
                        </div>
                        <div>
                            <dt>"Age"</dt>
                            <dd>{self.props.age}</dd>
                        </div>
                        <div>
                            <dt>"Root Duration"</dt>
                            <dd>{self.props.root_duration}</dd>
                        </div>
                        <div>
                            <dt>"Logs"</dt>
                            <dd>{self.props.log_count}</dd>
                        </div>
                    </dl>
                </div>
                <div class="trace-context">
                    <span class="help-dot">"?"</span>
                    <span>{self.props.client_ip}</span>
                    <span class="browser-dot"></span>
                    <span>{self.props.browser}</span>
                    <span class="apple-mark">"●"</span>
                    <span>"Mac OS X >=10.15.7"</span>
                    <span class="briefcase">"▱"</span>
                    <span>{self.props.environment}</span>
                    <span class="project-badges">
                        <i class="sdk-icon">"⚛"</i>
                        <i class="otel-icon">"↗"</i>
                    </span>
                </div>
            </section>
        }
        .render(cx)
    }
}

#[cfg(feature = "rsjs-pages")]
pub struct LogStream<'a> {
    pub row_count: usize,
    pub logs: Option<&'a [SentryLogEntry]>,
}

#[cfg(feature = "rsjs-pages")]
#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for LogStream<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let visible_batches = signal(1_i32);
        let auto_refresh = signal(true);
        let reveal_on_scroll = event_handler(|event: rsjs::Event| {
            if visible_batches.get() < 10_i32
                && event.current_target_scroll_top() > f64::from(visible_batches.get() * 850_i32)
            {
                visible_batches.update(|count| count + 1_i32);
            }
        });
        let toggle_auto_refresh = event_handler(|_: rsjs::MouseEvent<rsjs::elem::Button>| {
            auto_refresh.update(|enabled| !enabled);
        });
        let log_rows = LogRows {
            row_count: self.row_count,
            logs: self.logs,
        };

        html_sync! {
            <section
                class="trace-content"
                data_log_stream
                data_visible_batches={visible_batches.get()}
            >
                <div class="trace-tabs-row">
                    <nav class="trace-tabs" aria_label="Trace views">
                        <button type="button">"Waterfall"</button>
                        <button type="button" class="active" aria_current="page">"Logs"</button>
                    </nav>
                    <div class="web-vitals">
                        <span>
                            <b>"LCP"</b>
                            "108ms"
                        </span>
                        <span>
                            <b>"FCP"</b>
                            "108ms"
                        </span>
                        <span>
                            <b>"INP"</b>
                            "64ms"
                        </span>
                        <span>
                            <b>"CLS"</b>
                            "0"
                        </span>
                        <span>
                            <b>"TTFB"</b>
                            "12ms"
                        </span>
                    </div>
                </div>
                <div class="logs-toolbar">
                    <label class="logs-search">
                        <span>"⌕"</span>
                        <input type="search" placeholder="Search logs for this event" />
                    </label>
                    <button type="button" class="open-logs">"Open in Logs"</button>
                    <button
                        type="button"
                        class="auto-refresh"
                        aria_pressed={auto_refresh.get()}
                        on_click={toggle_auto_refresh()}
                    >
                        <i></i>
                        "Auto-refresh"
                    </button>
                </div>
                <div class="logs-viewport" on_scroll={reveal_on_scroll()}>
                    {log_rows}
                    <div class="loading-more">
                        <i></i>
                        <span>"Streaming more trace logs"</span>
                    </div>
                </div>
            </section>
        }
        .render(cx)
    }
}

struct StaticLogStream<'a> {
    row_count: usize,
    logs: Option<&'a [SentryLogEntry]>,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for StaticLogStream<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let log_rows = LogRows {
            row_count: self.row_count,
            logs: self.logs,
        };
        html_sync! {
            <section class="trace-content" data-log-stream data-visible-batches="1">
                {TraceTabsRow}
                <div class="logs-toolbar">
                    <label class="logs-search">
                        <span>"⌕"</span>
                        <input type="search" placeholder="Search logs for this event" />
                    </label>
                    <button type="button" class="open-logs">"Open in Logs"</button>
                    <button
                        type="button"
                        class="auto-refresh"
                        aria-pressed="true"
                    >
                        <i></i>
                        "Auto-refresh"
                    </button>
                </div>
                <div class="logs-viewport">{log_rows}{LoadingMore}</div>
            </section>
        }
        .render(cx)
    }
}

struct TraceTabsRow;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for TraceTabsRow {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="trace-tabs-row">
                <nav class="trace-tabs" aria-label="Trace views">
                    <button type="button">"Waterfall"</button>
                    <button type="button" class="active" aria-current="page">"Logs"</button>
                </nav>
                <div class="web-vitals">
                    <span>
                        <b>"LCP"</b>
                        "108ms"
                    </span>
                    <span>
                        <b>"FCP"</b>
                        "108ms"
                    </span>
                    <span>
                        <b>"INP"</b>
                        "64ms"
                    </span>
                    <span>
                        <b>"CLS"</b>
                        "0"
                    </span>
                    <span>
                        <b>"TTFB"</b>
                        "12ms"
                    </span>
                </div>
            </div>
        }
        .render(cx)
    }
}

struct LoadingMore;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for LoadingMore {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="loading-more">
                <i></i>
                <span>"Streaming more trace logs"</span>
            </div>
        }
        .render(cx)
    }
}

struct LogRows<'a> {
    row_count: usize,
    logs: Option<&'a [SentryLogEntry]>,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for LogRows<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if let Some(logs) = self.logs {
            for (index, entry) in logs.iter().enumerate() {
                LogRow {
                    index,
                    batch: index / 40 + 1,
                    data: LogRowData::from_entry(entry),
                }
                .render(cx)?;
            }
            return Ok(());
        }
        for index in 0..self.row_count {
            let pattern = &LOG_PATTERNS[index % LOG_PATTERNS.len()];
            LogRow {
                index,
                batch: index / 40 + 1,
                data: LogRowData::from_fixture(pattern, index % 24 == 1),
            }
            .render(cx)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct LogRowData<'a> {
    timestamp: &'a str,
    message: &'a str,
    severity: &'a str,
    source: &'a str,
    span_id: &'a str,
    payload_size: u32,
    checkout_tax_cents: u32,
    checkout_total_cents: u32,
    code_line: u32,
    code_path: &'a str,
    function_name: &'a str,
    environment: &'a str,
    instrumentation_name: &'a str,
    origin: &'a str,
    service_name: &'a str,
    sdk_language: &'a str,
    sdk_name: &'a str,
    sdk_version: &'a str,
    platform: &'a str,
    trace_id: &'a str,
    expanded: bool,
}

impl<'a> LogRowData<'a> {
    fn from_entry(entry: &'a SentryLogEntry) -> Self {
        Self {
            timestamp: &entry.timestamp,
            message: &entry.message,
            severity: &entry.severity,
            source: &entry.source,
            span_id: &entry.span_id,
            payload_size: entry.payload_size,
            checkout_tax_cents: entry.checkout_tax_cents,
            checkout_total_cents: entry.checkout_total_cents,
            code_line: entry.code_line,
            code_path: &entry.code_path,
            function_name: &entry.function_name,
            environment: &entry.environment,
            instrumentation_name: &entry.instrumentation_name,
            origin: &entry.origin,
            service_name: &entry.service_name,
            sdk_language: &entry.sdk_language,
            sdk_name: &entry.sdk_name,
            sdk_version: &entry.sdk_version,
            platform: &entry.platform,
            trace_id: &entry.trace_id,
            expanded: entry.expanded,
        }
    }

    fn from_fixture(pattern: &'a LogPattern, expanded: bool) -> Self {
        Self {
            timestamp: pattern.timestamp,
            message: pattern.message,
            severity: pattern.severity,
            source: pattern.source_label,
            span_id: pattern.span_id,
            payload_size: u32::from(pattern.payload_size),
            checkout_tax_cents: 528,
            checkout_total_cents: 6_928,
            code_line: 108,
            code_path: "/srv/checkout/app/services/checkout.py",
            function_name: "calculate_tax",
            environment: "local",
            instrumentation_name: "checkout_trace_lab",
            origin: "auto.otlp.logs",
            service_name: "checkout-trace-lab-backend",
            sdk_language: "python",
            sdk_name: "opentelemetry",
            sdk_version: "1.42.0",
            platform: "python",
            trace_id: "9f97da3a648f4d46a930c323a1aeaf7c",
            expanded,
        }
    }

    fn severity_class(self) -> &'static str {
        match self.severity {
            "error" => "severity-error",
            "warning" => "severity-warning",
            "debug" => "severity-debug",
            _ => "severity-info",
        }
    }

    fn source_class(self) -> &'static str {
        if self.source == "React" {
            "source-react"
        } else {
            "source-otel"
        }
    }
}

struct LogRow<'a> {
    index: usize,
    batch: usize,
    data: LogRowData<'a>,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for LogRow<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let row_classes = ClassList::<2>::new()
            .add("log-row")
            .add(self.data.severity_class());
        let source_classes = ClassList::<2>::new()
            .add("log-source")
            .add(self.data.source_class());
        html_sync! {
            <details
                class={row_classes}
                data-log-row
                data-batch={self.batch}
                data-row={self.index}
                open={self.data.expanded}
            >
                <summary>
                    <span class="row-chevron">"›"</span>
                    <span class="severity-dot"></span>
                    <span class={source_classes} title={self.data.source}>
                        {if self.data.source == "React" { "⚛" } else { "↗" }}
                    </span>
                    <time>{self.data.timestamp}</time>
                    <code>{self.data.message}</code>
                </summary>
                <div class="log-detail">
                    <header>
                        <code>{self.data.message}</code>
                        <span class="copy-json">"{} Copy as JSON"</span>
                    </header>
                    <div class="attribute-columns">
                        <dl>
                            <div>
                                <dt>"payload_size"</dt>
                                <dd>{self.data.payload_size}" B"</dd>
                            </div>
                            <div>
                                <dt>"checkout_tax_cents"</dt>
                                <dd>{self.data.checkout_tax_cents}</dd>
                            </div>
                            <div>
                                <dt>"checkout_total_cents"</dt>
                                <dd>{self.data.checkout_total_cents}</dd>
                            </div>
                            <div>
                                <dt>"code"</dt>
                                <dd></dd>
                            </div>
                            <div class="nested level-1">
                                <dt>"line"</dt>
                                <dd></dd>
                            </div>
                            <div class="nested level-2">
                                <dt>"number"</dt>
                                <dd>{self.data.code_line}</dd>
                            </div>
                            <div class="nested level-1">
                                <dt>"file"</dt>
                                <dd></dd>
                            </div>
                            <div class="nested level-2">
                                <dt>"path"</dt>
                                <dd>{self.data.code_path}</dd>
                            </div>
                            <div class="nested level-1">
                                <dt>"function"</dt>
                                <dd></dd>
                            </div>
                            <div class="nested level-2">
                                <dt>"name"</dt>
                                <dd>{self.data.function_name}</dd>
                            </div>
                            <div>
                                <dt>"environment"</dt>
                                <dd>{self.data.environment}</dd>
                            </div>
                            <div>
                                <dt>"instrumentation"</dt>
                                <dd></dd>
                            </div>
                            <div class="nested level-1">
                                <dt>"name"</dt>
                                <dd>{self.data.instrumentation_name}</dd>
                            </div>
                            <div>
                                <dt>"origin"</dt>
                                <dd>{self.data.origin}</dd>
                            </div>
                        </dl>
                        <dl>
                            <div>
                                <dt>"resource"</dt>
                                <dd></dd>
                            </div>
                            <div class="nested level-1">
                                <dt>"deployment"</dt>
                                <dd></dd>
                            </div>
                            <div class="nested level-2">
                                <dt>"environment"</dt>
                                <dd>{self.data.environment}</dd>
                            </div>
                            <div class="nested level-1">
                                <dt>"service"</dt>
                                <dd></dd>
                            </div>
                            <div class="nested level-2">
                                <dt>"name"</dt>
                                <dd>{self.data.service_name}</dd>
                            </div>
                            <div class="nested level-1">
                                <dt>"telemetry"</dt>
                                <dd></dd>
                            </div>
                            <div class="nested level-2">
                                <dt>"sdk"</dt>
                                <dd></dd>
                            </div>
                            <div class="nested level-3">
                                <dt>"language"</dt>
                                <dd>{self.data.sdk_language}</dd>
                            </div>
                            <div class="nested level-3">
                                <dt>"name"</dt>
                                <dd>{self.data.sdk_name}</dd>
                            </div>
                            <div class="nested level-3">
                                <dt>"version"</dt>
                                <dd>{self.data.sdk_version}</dd>
                            </div>
                            <div>
                                <dt>"platform"</dt>
                                <dd>{self.data.platform}</dd>
                            </div>
                            <div>
                                <dt>"span_id"</dt>
                                <dd>{self.data.span_id}</dd>
                            </div>
                            <div>
                                <dt>"severity"</dt>
                                <dd class="purple">{self.data.severity}</dd>
                            </div>
                            <div>
                                <dt>"trace"</dt>
                                <dd class="purple">{self.data.trace_id}</dd>
                            </div>
                        </dl>
                    </div>
                </div>
            </details>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod benchmark_tests {
    use super::*;

    #[test]
    fn benchmark_document_has_the_requested_rows_without_hydration_transport() {
        let html = render_benchmark_rows(25);

        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.ends_with("</body></html>"));
        assert_eq!(html.matches("<details class=\"log-row ").count(), 25);
        assert!(!html.contains("data-rsjs-"));
        assert!(!html.contains("/rsjs-runtime.js"));
    }
}

#[cfg(all(test, feature = "rsjs-pages"))]
mod tests {
    use super::*;

    #[test]
    fn renders_sentry_trace_logs_and_rsjs_markers() {
        let html = render();
        assert!(html.contains("data-sentry-otel"));
        assert!(html.contains("data-rsjs-island-root"));
        assert!(html.contains("/rsjs-runtime.js"));
        assert_eq!(
            html.matches(r#"<script type="module" src="/rsjs-runtime.js""#)
                .count(),
            1
        );
        assert!(
            html.find("/rsjs-runtime.js").unwrap() < html.rfind("</body>").unwrap(),
            "runtime plan must remain inside the document body"
        );
        assert!(html.len() > 250_000);
    }

    #[test]
    fn benchmark_row_count_is_respected() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        render_into(&mut cx, 1_000).unwrap();
        let html = String::from_utf8(out).unwrap();
        assert!(!html.contains("/rsjs-runtime.js"));
        assert_eq!(html.matches("<details class=\"log-row ").count(), 1_000);
    }

    #[test]
    fn owned_render_rows_finalizes_the_requested_document() {
        let html = render_rows(25);

        assert_eq!(html.matches("<details class=\"log-row ").count(), 25);
        assert_eq!(
            html.matches(r#"<script type="module" src="/rsjs-runtime.js""#)
                .count(),
            1
        );
        assert!(html.find("/rsjs-runtime.js").unwrap() < html.rfind("</body>").unwrap());
    }

    #[test]
    fn renders_playground_props() {
        let logs = [SentryLogEntry {
            timestamp: "Jul 10, 23:59:59.999".into(),
            message: "checkout.unique_row".into(),
            severity: "warning".into(),
            source: "React".into(),
            span_id: "unique-span-0001".into(),
            payload_size: 987,
            checkout_tax_cents: 654,
            checkout_total_cents: 9_876,
            code_line: 777,
            code_path: "/custom/checkout.tsx".into(),
            function_name: "customCheckout".into(),
            environment: "production".into(),
            instrumentation_name: "custom_otel".into(),
            origin: "manual.browser.logs".into(),
            service_name: "custom-checkout-frontend".into(),
            sdk_language: "typescript".into(),
            sdk_name: "custom-otel-sdk".into(),
            sdk_version: "9.8.7".into(),
            platform: "browser".into(),
            trace_id: "full-trace-cafef00d".into(),
            expanded: true,
        }];
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        render_into_with_props(
            &mut cx,
            SentryPageProps {
                row_count: logs.len(),
                logs: Some(&logs),
                trace_name: "checkout.submit",
                trace_id: "trace-cafef00d",
                issues: 9,
                spans: 321,
                age: "4s",
                root_duration: "912.4ms",
                log_count: 40,
                client_ip: "203.0.113.8",
                browser: "Firefox 140",
                environment: "production",
            },
        )
        .unwrap();
        let html = String::from_utf8(out).unwrap();
        assert!(html.contains("checkout.submit"));
        assert!(html.contains("trace-cafef00d"));
        assert!(html.contains("912.4ms"));
        assert!(html.contains("203.0.113.8"));
        assert!(html.contains("Firefox 140"));
        assert!(html.contains("production"));
        assert!(html.contains("checkout.unique_row"));
        assert!(html.contains("unique-span-0001"));
        assert!(html.contains("/custom/checkout.tsx"));
        assert!(html.contains("custom-checkout-frontend"));
        assert_eq!(html.matches("<details class=\"log-row ").count(), 1);
    }

    #[test]
    fn island_js_handles_streaming_and_auto_refresh() {
        let manifest = rsjs::manifest();
        let entry = manifest
            .islands
            .iter()
            .find(|entry| entry.struct_name == "LogStream")
            .expect("LogStream island should be registered");
        assert!(entry.js_source.contains("scrollTop"));
        assert!(entry.js_source.contains("aria-pressed"));
    }
}
