// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import React, { useState, type UIEvent } from "react";

type BaseLogPattern = {
  timestamp: string;
  message: string;
  severity: string;
  source: "React" | "OTel";
  spanId: string;
  payloadSize: number;
};

const LOG_PATTERNS: BaseLogPattern[] = [
  ["May 22, 15:16:40.283", "frontend_render_error.triggered", "error", "React", "a62bb7e21ef84c91", 734],
  ["May 22, 15:16:39.140", "checkout.tax_calculated", "info", "OTel", "28254f436d047dbc", 514],
  ["May 22, 15:16:39.140", "checkout.inventory_reserved", "info", "OTel", "df2ffe452e2cf688", 482],
  ["May 22, 15:16:39.139", "checkout.cart_validated", "info", "OTel", "13912f7d29b150ba", 469],
  ["May 22, 15:16:39.136", "checkout.started", "info", "OTel", "7fdce2d4bb21d024", 441],
  ["May 22, 15:16:39.132", "checkout.started", "debug", "React", "be491ad166d261a3", 389],
  ["May 22, 15:16:38.228", "checkout.expected_failure", "warning", "React", "cd35be756793f1d2", 526],
  ["May 22, 15:16:38.221", "checkout.payment_declined", "warning", "OTel", "d7760c5d9758cd7e", 552],
  ["May 22, 15:16:38.172", "checkout.inventory_miss", "warning", "OTel", "a85b7114d8ed15d0", 497],
  ["May 22, 15:16:37.944", "checkout.payment_slow_path", "info", "OTel", "bf218516fd922b90", 507],
  ["May 22, 15:16:37.621", "checkout.order_written", "info", "OTel", "904d0c29ad43a8f1", 613],
  ["May 22, 15:16:37.608", "checkout.completed", "info", "React", "f73ab2c4bdc771ec", 403],
].map(([timestamp, message, severity, source, spanId, payloadSize]) => ({
  timestamp: String(timestamp),
  message: String(message),
  severity: String(severity),
  source: source as "React" | "OTel",
  spanId: String(spanId),
  payloadSize: Number(payloadSize),
}));

export type SentryLogEntry = {
  // Reconciliation-only identity; never rendered as an application attribute.
  rowKey?: string;
  timestamp: string;
  message: string;
  severity: string;
  source: "React" | "OTel";
  spanId: string;
  payloadSize: number;
  checkoutTaxCents: number;
  checkoutTotalCents: number;
  codeLine: number;
  codePath: string;
  functionName: string;
  environment: string;
  instrumentationName: string;
  origin: string;
  serviceName: string;
  sdkLanguage: string;
  sdkName: string;
  sdkVersion: string;
  platform: string;
  traceId: string;
  expanded: boolean;
};

export function buildDefaultSentryLogs(count = 400): SentryLogEntry[] {
  return Array.from({ length: count }, (_, index) => {
    const pattern = LOG_PATTERNS[index % LOG_PATTERNS.length]!;
    return {
      rowKey: String(index),
      timestamp: pattern.timestamp,
      message: pattern.message,
      severity: pattern.severity,
      source: pattern.source,
      spanId: pattern.spanId,
      payloadSize: pattern.payloadSize,
      checkoutTaxCents: 528,
      checkoutTotalCents: 6928,
      codeLine: 108,
      codePath: "/srv/checkout/app/services/checkout.py",
      functionName: "calculate_tax",
      environment: "local",
      instrumentationName: "checkout_trace_lab",
      origin: "auto.otlp.logs",
      serviceName: "checkout-trace-lab-backend",
      sdkLanguage: "python",
      sdkName: "opentelemetry",
      sdkVersion: "1.42.0",
      platform: "python",
      traceId: "9f97da3a648f4d46a930c323a1aeaf7c",
      expanded: index % 24 === 1,
    };
  });
}

export const DEFAULT_SENTRY_LOGS = buildDefaultSentryLogs();

function SideRail() {
  return (
    <aside className="side-rail" aria-label="Sentry navigation">
      <a className="org-mark" href="#" aria-label="Sandbox Demo">SD</a>
      <nav className="primary-nav">
        <a href="#"><span className="nav-icon nav-icon-inbox" /><b>Issues</b></a>
        <a className="active" href="#"><span className="nav-icon nav-icon-compass" /><b>Explore</b></a>
        <a href="#"><span className="nav-icon nav-icon-grid" /><b>Dashboards</b></a>
        <a href="#"><span className="nav-icon nav-icon-bell" /><b>Monitors</b></a>
        <a href="#"><span className="nav-icon nav-icon-gear" /><b>Settings</b></a>
      </nav>
      <div className="rail-tools" aria-hidden="true">
        <span>⌕</span><span className="tool-active">✓</span><span>◭</span><span>◉</span><span>…</span><span className="avatar">PX</span>
      </div>
    </aside>
  );
}

function TopBar({ traceId }: { traceId: string }) {
  return (
    <header className="top-bar">
      <div className="breadcrumbs">
        <a href="#">Traces</a><span>/</span><i className="sdk-icon">⚛</i><b>{traceId}</b>
      </div>
      <div className="top-actions">
        <button type="button" className="ask-seer"><span>△</span>Ask Seer<kbd>⌘ /</kbd></button>
        <button type="button" className="feedback-button" aria-label="Give feedback">⚑</button>
      </div>
    </header>
  );
}

function TraceHeader({ props }: { props: Required<SentryOtelProps> }) {
  return (
    <section className="trace-header">
      <div className="trace-title-row">
        <div className="trace-title"><h1>{props.traceName}</h1><p>/</p></div>
        <a className="replay-link" href="#">▷ 1 Replay</a>
        <dl className="trace-stats">
          <div><dt>Issues</dt><dd className="purple">{props.issues}</dd></div>
          <div><dt>Spans</dt><dd>{props.spans}</dd></div>
          <div><dt>Age</dt><dd>{props.age}</dd></div>
          <div><dt>Root Duration</dt><dd>{props.rootDuration}</dd></div>
          <div><dt>Logs</dt><dd>{props.logCount}</dd></div>
        </dl>
      </div>
      <div className="trace-context">
        <span className="help-dot">?</span><span>{props.clientIp}</span><span className="browser-dot" />
        <span>{props.browser}</span><span className="apple-mark">●</span><span>Mac OS X &gt;=10.15.7</span>
        <span className="briefcase">▱</span><span>{props.environment}</span>
        <span className="project-badges"><i className="sdk-icon">⚛</i><i className="otel-icon">↗</i></span>
      </div>
    </section>
  );
}

function AttributeRow({ name, value = "", level = 0, purple = false }: { name: string; value?: string | number; level?: number; purple?: boolean }) {
  const className = level ? `nested level-${level}` : undefined;
  return <div className={className}><dt>{name}</dt><dd className={purple ? "purple" : undefined}>{value}</dd></div>;
}

function LogRow({ entry, index }: { entry: SentryLogEntry; index: number }) {
  const severityClass = ["error", "warning", "debug"].includes(entry.severity) ? `severity-${entry.severity}` : "severity-info";
  const sourceClass = entry.source === "React" ? "source-react" : "source-otel";
  return (
    <details className={`log-row ${severityClass}`} data-log-row="" data-batch={Math.floor(index / 40) + 1} data-row={index} open={entry.expanded}>
      <summary>
        <span className="row-chevron">›</span><span className="severity-dot" />
        <span className={`log-source ${sourceClass}`} title={entry.source}>{entry.source === "React" ? "⚛" : "↗"}</span>
        <time>{entry.timestamp}</time><code>{entry.message}</code>
      </summary>
      <div className="log-detail">
        <header><code>{entry.message}</code><span className="copy-json">{"{} Copy as JSON"}</span></header>
        <div className="attribute-columns">
          <dl>
            <AttributeRow name="payload_size" value={`${entry.payloadSize} B`} />
            <AttributeRow name="checkout_tax_cents" value={entry.checkoutTaxCents} />
            <AttributeRow name="checkout_total_cents" value={entry.checkoutTotalCents} />
            <AttributeRow name="code" />
            <AttributeRow name="line" level={1} />
            <AttributeRow name="number" value={entry.codeLine} level={2} />
            <AttributeRow name="file" level={1} />
            <AttributeRow name="path" value={entry.codePath} level={2} />
            <AttributeRow name="function" level={1} />
            <AttributeRow name="name" value={entry.functionName} level={2} />
            <AttributeRow name="environment" value={entry.environment} />
            <AttributeRow name="instrumentation" />
            <AttributeRow name="name" value={entry.instrumentationName} level={1} />
            <AttributeRow name="origin" value={entry.origin} />
          </dl>
          <dl>
            <AttributeRow name="resource" />
            <AttributeRow name="deployment" level={1} />
            <AttributeRow name="environment" value={entry.environment} level={2} />
            <AttributeRow name="service" level={1} />
            <AttributeRow name="name" value={entry.serviceName} level={2} />
            <AttributeRow name="telemetry" level={1} />
            <AttributeRow name="sdk" level={2} />
            <AttributeRow name="language" value={entry.sdkLanguage} level={3} />
            <AttributeRow name="name" value={entry.sdkName} level={3} />
            <AttributeRow name="version" value={entry.sdkVersion} level={3} />
            <AttributeRow name="platform" value={entry.platform} />
            <AttributeRow name="span_id" value={entry.spanId} />
            <AttributeRow name="severity" value={entry.severity} purple />
            <AttributeRow name="trace" value={entry.traceId} purple />
          </dl>
        </div>
      </div>
    </details>
  );
}

function LogStream({ logs }: { logs: SentryLogEntry[] }) {
  const [visibleBatches, setVisibleBatches] = useState(1);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const onScroll = (event: UIEvent<HTMLDivElement>) => {
    const scrollTop = event.currentTarget.scrollTop;
    setVisibleBatches(current => current < 10 && scrollTop > current * 850 ? current + 1 : current);
  };
  return (
    <section className="trace-content" data-log-stream="" data-visible-batches={visibleBatches}>
      <div className="trace-tabs-row">
        <nav className="trace-tabs" aria-label="Trace views"><button type="button">Waterfall</button><button type="button" className="active" aria-current="page">Logs</button></nav>
        <div className="web-vitals"><span><b>LCP</b>108ms</span><span><b>FCP</b>108ms</span><span><b>INP</b>64ms</span><span><b>CLS</b>0</span><span><b>TTFB</b>12ms</span></div>
      </div>
      <div className="logs-toolbar">
        <label className="logs-search"><span>⌕</span><input type="search" placeholder="Search logs for this event" /></label>
        <button type="button" className="open-logs">Open in Logs</button>
        <button type="button" className="auto-refresh" aria-pressed={autoRefresh} onClick={() => setAutoRefresh(enabled => !enabled)}><i />Auto-refresh</button>
      </div>
      <div className="logs-viewport" onScroll={onScroll}>
        {logs.map((entry, index) => <LogRow entry={entry} index={index} key={entry.rowKey ?? entry.spanId} />)}
        <div className="loading-more"><i /><span>Streaming more trace logs</span></div>
      </div>
    </section>
  );
}

export type SentryOtelProps = {
  logs?: SentryLogEntry[];
  traceName?: string;
  traceId?: string;
  issues?: number;
  spans?: number;
  age?: string;
  rootDuration?: string;
  logCount?: number;
  clientIp?: string;
  browser?: string;
  environment?: string;
};

export function SentryOtelDocument({
  css,
  logs = DEFAULT_SENTRY_LOGS,
  traceName = "pageload",
  traceId = "9f97da3a648f",
  issues = 2,
  spans = 102,
  age = "37m",
  rootDuration = "108.00ms",
  logCount = 40,
  clientIp = "192.0.2.42",
  browser = "Chrome 148.0.0",
  environment = "local",
  hydrate = true,
}: { css: string; hydrate?: boolean } & SentryOtelProps) {
  const props = { logs, traceName, traceId, issues, spans, age, rootDuration, logCount, clientIp, browser, environment };
  const seedJson = hydrate ? JSON.stringify(props).replace(/</g, "\\u003c") : "";
  return (
    <html lang="en">
      <head>
        <meta charSet="utf-8" /><meta name="viewport" content="width=device-width, initial-scale=1" />
        <meta name="robots" content="noindex, nofollow" /><title>Trace Logs — Sentry OpenTelemetry Showcase</title>
        <style id={hydrate ? "sentry-otel-css" : undefined} dangerouslySetInnerHTML={{ __html: css }} />
      </head>
      <body
        data-sentry-otel=""
        data-row-count={hydrate ? logs.length : undefined}
        data-trace-name={hydrate ? traceName : undefined}
        data-trace-id={hydrate ? traceId : undefined}
        data-issues={hydrate ? issues : undefined}
        data-spans={hydrate ? spans : undefined}
        data-age={hydrate ? age : undefined}
        data-root-duration={hydrate ? rootDuration : undefined}
        data-log-count={hydrate ? logCount : undefined}
        data-client-ip={hydrate ? clientIp : undefined}
        data-browser={hydrate ? browser : undefined}
        data-environment={hydrate ? environment : undefined}
      >
        <div className="sentry-app"><SideRail /><main className="sentry-main"><TopBar traceId={traceId} /><TraceHeader props={props} /><LogStream logs={logs} /></main></div>
        {hydrate ? <script id="sentry-otel-props" type="application/json" dangerouslySetInnerHTML={{ __html: seedJson }} /> : null}
        {hydrate ? <script type="module" src="/sentry-otel-client.bundle.js" /> : null}
      </body>
    </html>
  );
}
