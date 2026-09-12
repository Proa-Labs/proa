#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Cross-engine parity check for the fully dynamic eBay SRP.
//
// Fetches the same page from the Proa, react-ssr, and Next.js engines and
// compares element counts, per-tag histograms, and key text content. The
// engines intentionally differ in hydration mechanics (rsjs islands vs React
// seed JSON vs RSC payload), so the check normalises those out and compares
// the server-rendered DOM surface.
//
// Usage: node parity_check.mjs [n]
//   PROA_URL  (default http://127.0.0.1:5002/showcase/ebay/proa-rsjs)
//   REACT_URL (default http://127.0.0.1:5401/showcase/ebay/react)
//   NEXT_URL  (default http://127.0.0.1:5402/showcase/ebay/next)

const n = Number(process.argv[2]) || 60;
const targets = {
  proa: (process.env.PROA_URL || "http://127.0.0.1:5002/showcase/ebay/proa-rsjs") + `?n=${n}`,
  react: (process.env.REACT_URL || "http://127.0.0.1:5401/showcase/ebay/react") + `?n=${n}`,
  next: (process.env.NEXT_URL || "http://127.0.0.1:5402/showcase/ebay/next") + `?n=${n}`,
  marko: (process.env.MARKO_URL || "http://127.0.0.1:5300/showcase/ebay/marko") + `?n=${n}`,
};

function strip(html, engine) {
  let out = html;
  // Inline RSC flight payload + framework script tags (Next), seed JSON +
  // hydrate tag (react-ssr), island scripts + runtime (proa) are hydration
  // transport, not server-rendered DOM. Remove all script tags for the DOM
  // surface comparison; their cost shows up in the byte columns instead.
  out = out.replace(/<script[\s\S]*?<\/script>/g, "");
  out = out.replace(/<link[^>]*>/g, "");
  if (engine === "proa") {
    // rsjs island wrappers contribute structure the JS engines express
    // differently (React keeps the button inline). Count them separately.
    out = out.replace(/<rsjs-island[^>]*>/g, "").replace(/<\/rsjs-island>/g, "");
  }
  return out;
}

function tagHistogram(html) {
  const hist = new Map();
  for (const m of html.matchAll(/<([a-zA-Z][a-zA-Z0-9-]*)/g)) {
    const tag = m[1].toLowerCase();
    hist.set(tag, (hist.get(tag) ?? 0) + 1);
  }
  return hist;
}

const results = {};
for (const [engine, url] of Object.entries(targets)) {
  try {
    const res = await fetch(url);
    const html = await res.text();
    const stripped = strip(html, engine);
    const hist = tagHistogram(stripped);
    const elements = [...hist.values()].reduce((a, b) => a + b, 0);
    results[engine] = { bytes: Buffer.byteLength(html), elements, hist, html };
  } catch (e) {
    console.log(`${engine}: FETCH FAILED ${url} (${e.message})`);
  }
}

const engines = Object.keys(results);
if (engines.length === 0) process.exit(1);

console.log(`\n=== parity check, n=${n} ===`);
console.log("engine   page bytes   DOM elements (scripts/links stripped)");
for (const e of engines) {
  console.log(
    e.padEnd(8),
    String(results[e].bytes).padStart(10),
    String(results[e].elements).padStart(12),
  );
}

// Per-tag diff vs proa
if (results.proa) {
  for (const other of engines.filter((e) => e !== "proa")) {
    const diffs = [];
    const tags = new Set([...results.proa.hist.keys(), ...results[other].hist.keys()]);
    for (const t of tags) {
      const a = results.proa.hist.get(t) ?? 0;
      const b = results[other].hist.get(t) ?? 0;
      if (a !== b) diffs.push(`${t}: proa=${a} ${other}=${b}`);
    }
    console.log(`\ntag diffs proa vs ${other}: ${diffs.length === 0 ? "none" : ""}`);
    for (const d of diffs.slice(0, 12)) console.log("  " + d);
  }
}

// Spot-check shared dynamic content renders identically.
const probes = [
  `results for`,
  "Copyright © 1995-2026 eBay Inc. All Rights Reserved.",
  "product ratings",
  "watchers",
  "x-refine__count",
  "gh-meganav__section",
];
console.log("\ncontent probes:");
for (const p of probes) {
  const row = engines.map((e) => `${e}=${results[e].html.includes(p) ? "ok" : "MISSING"}`).join("  ");
  console.log("  " + p.slice(0, 40).padEnd(42) + row);
}
