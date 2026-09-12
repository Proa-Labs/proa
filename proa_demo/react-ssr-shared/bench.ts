#!/usr/bin/env tsx
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//
// React SSR Benchmark — renderToString() harness
//
// Usage:
//   pnpm install
//   pnpm bench
//
// Methodology matches Proa's bench.mjs:
//   - Warmup: 1,000 iterations
//   - Measure: 10,000 iterations
//   - Reports avg, p50, p99, min, max in µs + HTML size in KB

import { performance } from "node:perf_hooks";
import { createElement } from "react";
import { renderToString } from "react-dom/server";

import { HomePage } from "./src/home";
import { ListingPage } from "./src/listing";
import { NikePdpPage } from "./src/nike-pdp";
import { NIKE_PDP_TITLE, wrapInNikeShell } from "./src/nike-shell";
import { SearchPage } from "./src/search";
import { wrapInDocumentShell } from "./src/shared";

if (process.env.NODE_ENV !== "production") {
	throw new Error(
		"React SSR benchmark requires NODE_ENV=production; run it through `npm run bench`.",
	);
}

function percentile(sorted: number[], p: number): number {
	const idx = Math.ceil((p / 100) * sorted.length) - 1;
	return sorted[Math.max(0, idx)];
}

const WARMUP = 1_000;
const ITERATIONS = 10_000;

const allPages = [
	{
		id: "home",
		name: "Home",
		title: "Airbnb - Vacation Rentals, Cabins, Beach Houses",
		element: createElement(HomePage),
		wrap: wrapInDocumentShell,
	},
	{
		id: "search",
		name: "Search",
		title: "Malibu - Vacation Rentals | Airbnb",
		element: createElement(SearchPage),
		wrap: wrapInDocumentShell,
	},
	{
		id: "listing",
		name: "Listing",
		title: "Breathtaking Malibu Ocean View Sanctuary | Airbnb",
		element: createElement(ListingPage),
		wrap: wrapInDocumentShell,
	},
	{
		id: "nike-pdp",
		name: "Nike PDP",
		title: NIKE_PDP_TITLE,
		element: createElement(NikePdpPage),
		wrap: wrapInNikeShell,
	},
];
const requestedPage = process.argv[2]?.toLowerCase();
const pages = requestedPage
	? allPages.filter(({ id }) => id === requestedPage)
	: allPages;
if (pages.length === 0) {
	throw new Error(`unknown page ${process.argv[2]}`);
}

console.log("=".repeat(68));
console.log("  React SSR Benchmark — renderToString()");
console.log("=".repeat(68));
console.log(
	`  Warmup: ${WARMUP.toLocaleString()} iterations | Bench: ${ITERATIONS.toLocaleString()} iterations`,
);
console.log("=".repeat(68));
console.log();

for (const page of pages) {
	// --- Warmup ---
	for (let i = 0; i < WARMUP; i++) {
		page.wrap(page.title, renderToString(page.element));
	}

	// --- Benchmark ---
	const timings = new Float64Array(ITERATIONS);
	let lastHtml = "";

	for (let i = 0; i < ITERATIONS; i++) {
		const start = performance.now();
		const bodyHtml = renderToString(page.element);
		const html = page.wrap(page.title, bodyHtml);
		const elapsed = performance.now() - start;
		timings[i] = elapsed * 1000; // convert to µs

		if (i === ITERATIONS - 1) {
			lastHtml = html;
		}
	}

	// Sort for percentiles
	const sorted = Array.from(timings).sort((a, b) => a - b);
	const avg = sorted.reduce((a, b) => a + b, 0) / sorted.length;
	const p50 = percentile(sorted, 50);
	const p99 = percentile(sorted, 99);
	const min = sorted[0];
	const max = sorted[sorted.length - 1];
	const sizeBytes = Buffer.byteLength(lastHtml, "utf8");

	console.log(`  ${page.name}`);
	console.log(`  ${"─".repeat(50)}`);
	console.log(
		`  avg: ${avg.toFixed(2)} µs   p50: ${p50.toFixed(2)} µs   p99: ${p99.toFixed(2)} µs`,
	);
	console.log(`  min: ${min.toFixed(2)} µs   max: ${max.toFixed(2)} µs`);
	console.log(
		`  HTML size: ${(sizeBytes / 1024).toFixed(1)} KB (${sizeBytes.toLocaleString()} bytes)`,
	);
	console.log(`  Preview: ${lastHtml.slice(0, 80)}…`);
	console.log();
}

console.log("=".repeat(68));
console.log("  Done.");
console.log("=".repeat(68));
