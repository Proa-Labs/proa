#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//
// Proa Demo — WASM Benchmark Harness
//
// Usage:
//   cargo build -p proa_demo --release --target wasm32-unknown-unknown
//   node bench.mjs
//
// Requires: Node.js 18+

import { readFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

// Resolve the WASM binary — check optimized path first, then raw build output
const WASM_PATHS = [
  resolve(__dirname, "proa_demo.wasm"), // wasm-opt output (shipped)
  resolve(
    __dirname,
    "..",
    "target",
    "wasm32-unknown-unknown",
    "release",
    "proa_demo.wasm"
  ),
];

async function loadWasm() {
  for (const p of WASM_PATHS) {
    try {
      const bytes = await readFile(p);
      console.log(`Loaded WASM from: ${p} (${(bytes.length / 1024).toFixed(1)} KB)\n`);
      return bytes;
    } catch {
      // try next
    }
  }
  console.error(
    "WASM binary not found. Build it first:\n" +
      "  cargo build -p proa_demo --release --target wasm32-unknown-unknown\n"
  );
  process.exit(1);
}

// Read a UTF-8 string from WASM memory
function readString(memory, ptr, len) {
  return new TextDecoder().decode(new Uint8Array(memory.buffer, ptr, len));
}

// Percentile helper (array must be sorted)
function percentile(sorted, p) {
  const idx = Math.ceil((p / 100) * sorted.length) - 1;
  return sorted[Math.max(0, idx)];
}

async function main() {
  const wasmBytes = await loadWasm();

  // Instantiate with minimal imports
  const { instance } = await WebAssembly.instantiate(wasmBytes, {
    env: {},
  });

  const {
    memory,
    wasm_alloc,
    wasm_dealloc,
    render_airbnb_home,
    render_airbnb_search,
    render_airbnb_listing,
    proa_free,
  } = instance.exports;

  const WARMUP = 1_000;
  const ITERATIONS = 10_000;

  const pages = [
    { name: "Home", fn: render_airbnb_home },
    { name: "Search", fn: render_airbnb_search },
    { name: "Listing", fn: render_airbnb_listing },
  ];

  // Allocate out-param space (2 × 4 bytes for ptr + len as i32 on wasm32)
  const outPtrAddr = wasm_alloc(4);
  const outLenAddr = wasm_alloc(4);

  console.log("=".repeat(68));
  console.log("  Proa SSR Benchmark — WASM");
  console.log("=".repeat(68));
  console.log(
    `  Warmup: ${WARMUP.toLocaleString()} iterations | Bench: ${ITERATIONS.toLocaleString()} iterations`
  );
  console.log("=".repeat(68));
  console.log();

  for (const page of pages) {
    // --- Warmup ---
    for (let i = 0; i < WARMUP; i++) {
      page.fn(outPtrAddr, outLenAddr);
      // Re-create DataView after render (memory may have grown)
      const view32 = new DataView(memory.buffer);
      const ptr = view32.getUint32(outPtrAddr, true);
      const len = view32.getUint32(outLenAddr, true);
      proa_free(ptr, len);
    }

    // --- Benchmark ---
    const timings = new Float64Array(ITERATIONS);
    let lastLen = 0;
    let lastHtml = "";

    for (let i = 0; i < ITERATIONS; i++) {
      const start = performance.now();
      page.fn(outPtrAddr, outLenAddr);
      const elapsed = performance.now() - start;
      timings[i] = elapsed * 1000; // convert to µs

      // Re-create DataView after render (memory may have grown)
      const view32 = new DataView(memory.buffer);
      const ptr = view32.getUint32(outPtrAddr, true);
      const len = view32.getUint32(outLenAddr, true);

      if (i === ITERATIONS - 1) {
        lastLen = len;
        lastHtml = readString(memory, ptr, Math.min(len, 200));
      }

      proa_free(ptr, len);
    }

    // Sort for percentiles
    const sorted = Array.from(timings).sort((a, b) => a - b);
    const avg = sorted.reduce((a, b) => a + b, 0) / sorted.length;
    const p50 = percentile(sorted, 50);
    const p99 = percentile(sorted, 99);
    const min = sorted[0];
    const max = sorted[sorted.length - 1];

    console.log(`  ${page.name}`);
    console.log(`  ${"─".repeat(50)}`);
    console.log(`  avg: ${avg.toFixed(2)} µs   p50: ${p50.toFixed(2)} µs   p99: ${p99.toFixed(2)} µs`);
    console.log(`  min: ${min.toFixed(2)} µs   max: ${max.toFixed(2)} µs`);
    console.log(`  HTML size: ${(lastLen / 1024).toFixed(1)} KB (${lastLen.toLocaleString()} bytes)`);
    console.log(`  Preview: ${lastHtml.slice(0, 80)}…`);
    console.log();
  }

  // Clean up out-param allocations
  wasm_dealloc(outPtrAddr, 4);
  wasm_dealloc(outLenAddr, 4);

  console.log("=".repeat(68));
  console.log("  Done.");
  console.log("=".repeat(68));
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
