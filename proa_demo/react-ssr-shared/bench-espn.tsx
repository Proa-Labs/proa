#!/usr/bin/env tsx
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { performance } from "node:perf_hooks";
import { readFileSync } from "node:fs";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { EspnGamecastDocument } from "./src/espn-gamecast";

if (process.env.NODE_ENV !== "production") {
  throw new Error("ESPN benchmark requires NODE_ENV=production");
}

const warmup = Number(process.env.BENCH_WARMUP || 1_000);
const iterations = Number(process.env.BENCH_ITERATIONS || 10_000);
const css = readFileSync(
  new URL("../../proa_showcase/src/espn/espn.css", import.meta.url),
  "utf8",
);
const element = createElement(EspnGamecastDocument, { css });

for (let i = 0; i < warmup; i++) renderToString(element);

const timings = new Float64Array(iterations);
let html = "";
for (let i = 0; i < iterations; i++) {
  const start = performance.now();
  html = renderToString(element);
  timings[i] = (performance.now() - start) * 1_000;
}

const sorted = Array.from(timings).sort((a, b) => a - b);
const percentile = (p: number) =>
  sorted[Math.max(0, Math.ceil(sorted.length * p) - 1)];
const avg = sorted.reduce((sum, value) => sum + value, 0) / sorted.length;

console.log(
  `ESPN_REACT_BENCH node_env=${process.env.NODE_ENV} warmup=${warmup} iterations=${iterations}`,
);
console.log("react_renderToString");
console.log(`  avg: ${avg.toFixed(2)} us`);
console.log(`  p50: ${percentile(0.5).toFixed(2)} us`);
console.log(`  p99: ${percentile(0.99).toFixed(2)} us`);
console.log(`  min: ${sorted[0].toFixed(2)} us`);
console.log(`  max: ${sorted.at(-1)!.toFixed(2)} us`);
console.log(`  bytes: ${Buffer.byteLength(html)}`);
