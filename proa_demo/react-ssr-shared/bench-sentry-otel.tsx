#!/usr/bin/env tsx
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { readFileSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { buildDefaultSentryLogs, SentryOtelDocument } from "./src/sentry-otel";

if (process.env.NODE_ENV !== "production") {
  throw new Error("Sentry OTel benchmark requires NODE_ENV=production");
}

const rowCount = Number(process.env.BENCH_ROWS || 1_000);
const defaultIterations = rowCount >= 10_000 ? 25 : rowCount >= 1_000 ? 200 : 1_000;
const warmup = Number(process.env.BENCH_WARMUP || Math.min(100, defaultIterations));
const iterations = Number(process.env.BENCH_ITERATIONS || defaultIterations);
const css = readFileSync(
  new URL("../../proa_showcase/src/sentry_otel/sentry_otel.css", import.meta.url),
  "utf8",
);
const logs = buildDefaultSentryLogs(rowCount);
const element = createElement(SentryOtelDocument, { css, logs, hydrate: false });
const renderDocument = () => `<!DOCTYPE html>${renderToString(element)}`;

for (let i = 0; i < warmup; i++) renderDocument();

const timings = new Float64Array(iterations);
let html = "";
for (let i = 0; i < iterations; i++) {
  const start = performance.now();
  html = renderDocument();
  timings[i] = (performance.now() - start) * 1_000;
}

const sorted = Array.from(timings).sort((a, b) => a - b);
const percentile = (p: number) => sorted[Math.max(0, Math.ceil(sorted.length * p) - 1)]!;
const avg = sorted.reduce((sum, value) => sum + value, 0) / sorted.length;

console.log(`SENTRY_OTEL_REACT_BENCH node_env=${process.env.NODE_ENV} rows=${rowCount} warmup=${warmup} iterations=${iterations}`);
console.log("react_renderToString");
console.log(`  avg: ${avg.toFixed(2)} us`);
console.log(`  p50: ${percentile(0.5).toFixed(2)} us`);
console.log(`  p99: ${percentile(0.99).toFixed(2)} us`);
console.log(`  min: ${sorted[0]!.toFixed(2)} us`);
console.log(`  max: ${sorted.at(-1)!.toFixed(2)} us`);
console.log(`  bytes: ${Buffer.byteLength(html)}`);
