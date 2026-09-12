// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Entry point for the proa-bench harness.
//
// Boots playwright chromium, drives N runs against each engine under
// every (CPU profile × cache profile) combination, and prints a
// comparison table.
//
// Companion servers must already be running:
//   - proa playground          (default :5000, override PROA_PORT)
//   - marko 6 SSR              (default :5300, override MARKO_PORT)
//   - react 19 SSR             (default :5401, override REACT_PORT)
//   - Next.js SSR              (default :5402, override NEXT_PORT)
//
// Run:
//   cd proa_demo/bench && npm install && node run.mjs
//
// Tunables via env:
//   BENCH_RUNS=5         runs per (engine, cpu, network, cache)
//   BENCH_CPU=4x         comma list from {unthrottled,4x,6x}; default all
//   BENCH_NETWORK=4g     comma list from {wifi,4g,fast3g,slow3g}; default wifi,4g
//   BENCH_CACHE=cold     comma list from {cold,warm}; default both
//   BENCH_PAGE=ebay-rsjs page from {ebay-rsjs,nike-catalog,nike-pdp}
//   BENCH_ENGINES=proa-rsjs,next 16 comma list of engine names for this page

import { chromium } from "playwright";
import { measureEngine } from "./measure.mjs";
import { stats, printAll, printWinners } from "./format.mjs";

const PROA_PORT = process.env.PROA_PORT || 5000;
const MARKO_PORT = process.env.MARKO_PORT || 5300;
const HONO_PORT = process.env.HONO_PORT || 5100;
const REACT_PORT = process.env.REACT_PORT || 5401;
const NEXT_PORT = process.env.NEXT_PORT || 5402;
const RUNS = parseInt(process.env.BENCH_RUNS || "5", 10);
const PAGE = process.env.BENCH_PAGE || "ebay-rsjs";

const CPU_PROFILES = (process.env.BENCH_CPU || "unthrottled,4x,6x")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
const NETWORK_PROFILES = (process.env.BENCH_NETWORK || "wifi,4g")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
const CACHE_PROFILES = (process.env.BENCH_CACHE || "cold,warm")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

const CPU_RATES = { unthrottled: 1, "4x": 4, "6x": 6 };

// Network profiles match Chrome DevTools' built-in throttling presets.
// `null` = no throttling (loopback). All others apply CDP
// `Network.emulateNetworkConditions` with the listed values.
//
// downloadKbps / uploadKbps are in kbps; latency in ms RTT.
const NETWORK_PROFILES_MAP = {
    wifi: null,
    "4g": { downloadKbps: 10 * 1024, uploadKbps: 1024, latency: 100 },
    fast3g: { downloadKbps: 1500, uploadKbps: 750, latency: 562 },
    slow3g: { downloadKbps: 400, uploadKbps: 400, latency: 2000 },
};

const pageConfigs = {
    "ebay-rsjs": {
        label: "Synthetic marketplace SRP with interactive islands/widgets",
        engines: [
            {
                name: "proa-rsjs",
                url: `http://localhost:${PROA_PORT}/showcase/ebay/proa-rsjs`,
                healthCheck: `http://localhost:${PROA_PORT}/`,
                startHint: `cargo run --release -p proa_demo --bin proa-playground --features server  (and ensure it's on :${PROA_PORT})`,
            },
            {
                name: "marko 6",
                url: `http://localhost:${MARKO_PORT}/showcase/ebay/marko`,
                healthCheck: `http://localhost:${MARKO_PORT}/`,
                startHint: `auto-spawned by the proa playground; if missing, check the playground's stdout for marko-server boot errors`,
            },
            {
                name: "react 19",
                url: `http://localhost:${REACT_PORT}/showcase/ebay/react`,
                healthCheck: `http://localhost:${REACT_PORT}/`,
                startHint: `PORT=${REACT_PORT} node --import tsx ../react-ssr/server.tsx`,
            },
            {
                name: "next 16",
                url: `http://localhost:${NEXT_PORT}/showcase/ebay/next`,
                healthCheck: `http://localhost:${NEXT_PORT}/health`,
                startHint: `cd ../next-ssr && npm install && npm run build && PORT=${NEXT_PORT} npm run start`,
            },
        ],
    },
    "nike-catalog": {
        label: "Footwear catalog",
        engines: [
            {
                name: "proa",
                url: `http://localhost:${PROA_PORT}/showcase/nike/proa`,
                healthCheck: `http://localhost:${PROA_PORT}/`,
                startHint: `cargo run --release -p proa_demo --bin proa-playground --features server  (and ensure it's on :${PROA_PORT})`,
            },
            {
                name: "hono react",
                url: `http://localhost:${HONO_PORT}/nike`,
                healthCheck: `http://localhost:${HONO_PORT}/health`,
                startHint: `auto-spawned by the proa playground, or PORT=${HONO_PORT} node --import tsx ../hono-ssr/server.ts`,
            },
            {
                name: "next 16",
                url: `http://localhost:${NEXT_PORT}/showcase/nike/next`,
                healthCheck: `http://localhost:${NEXT_PORT}/health`,
                startHint: `cd ../next-ssr && npm install && npm run build && PORT=${NEXT_PORT} npm run start`,
            },
        ],
    },
    "nike-pdp": {
        label: "Footwear product detail page",
        engines: [
            {
                name: "proa",
                url: `http://localhost:${PROA_PORT}/showcase/nike/proa/product`,
                healthCheck: `http://localhost:${PROA_PORT}/`,
                startHint: `cargo run --release -p proa_demo --bin proa-playground --features server  (and ensure it's on :${PROA_PORT})`,
            },
            {
                name: "hono react",
                url: `http://localhost:${HONO_PORT}/nike/product`,
                healthCheck: `http://localhost:${HONO_PORT}/health`,
                startHint: `auto-spawned by the proa playground, or PORT=${HONO_PORT} node --import tsx ../hono-ssr/server.ts`,
            },
            {
                name: "next 16",
                url: `http://localhost:${NEXT_PORT}/showcase/nike/next/product`,
                healthCheck: `http://localhost:${NEXT_PORT}/health`,
                startHint: `cd ../next-ssr && npm install && npm run build && PORT=${NEXT_PORT} npm run start`,
            },
        ],
    },
};

const pageConfig = pageConfigs[PAGE];
if (!pageConfig) {
    console.error(`unknown BENCH_PAGE: ${PAGE} (allowed: ${Object.keys(pageConfigs).join(", ")})`);
    process.exit(1);
}

const requestedEngines = (process.env.BENCH_ENGINES || "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
const engines = requestedEngines.length
    ? pageConfig.engines.filter((engine) => requestedEngines.includes(engine.name))
    : pageConfig.engines;
if (engines.length === 0) {
    console.error(`BENCH_ENGINES selected no engines for page ${PAGE}. Available: ${pageConfig.engines.map((e) => e.name).join(", ")}`);
    process.exit(1);
}

async function preflight() {
    const failures = [];
    for (const e of engines) {
        try {
            const r = await fetch(e.url, { redirect: "follow" });
            if (!r.ok) failures.push({ engine: e, reason: `HTTP ${r.status} from ${e.url}` });
        } catch (err) {
            failures.push({ engine: e, reason: err.message || String(err) });
        }
    }
    if (failures.length > 0) {
        console.error("");
        console.error("✘ preflight failed; required servers are not reachable:");
        for (const f of failures) {
            console.error(`    ${f.engine.name}: ${f.reason}`);
            console.error(`        start with: ${f.engine.startHint}`);
        }
        console.error("");
        process.exit(1);
    }
}

function summarizeRows(perEngineResults) {
    return perEngineResults.map((res) => ({
        engine: res.engine.name,
        runs: res.runs,
        errors: res.errors,
        ttfb: stats(res.records.map((r) => r.ttfb)),
        fcp: stats(res.records.map((r) => r.fcp)),
        lcp: stats(res.records.map((r) => r.lcp)),
        tti: stats(res.records.map((r) => r.tti)),
        // CDP-sourced cumulative script execution time on the main
        // thread. Replaces the broken duration-minus-network proxy.
        scriptCpu: stats(res.records.map((r) => r.scriptDuration)),
        // Sum of long-task durations fired after FCP (in-page). The
        // "page felt janky during hydration" metric.
        longTasks: stats(res.records.map((r) => r.longTaskSum)),
        transferred: stats(res.records.map((r) => r.transferred)),
    }));
}

async function main() {
    console.log("═══ proa-bench: timing/CPU/network/cache harness ═══");
    console.log(`page=${PAGE} (${pageConfig.label})`);
    console.log(`runs=${RUNS}  cpu=${CPU_PROFILES.join(",")}  network=${NETWORK_PROFILES.join(",")}  cache=${CACHE_PROFILES.join(",")}`);
    console.log(`engines=${engines.map((e) => e.name).join(", ")}`);
    console.log(`proa=:${PROA_PORT}  marko=:${MARKO_PORT}  hono=:${HONO_PORT}  react=:${REACT_PORT}  next=:${NEXT_PORT}`);

    await preflight();

    const browser = await chromium.launch({ headless: true });
    const allSummaries = [];

    try {
        for (const cpu of CPU_PROFILES) {
            const cpuRate = CPU_RATES[cpu];
            if (cpuRate == null) {
                console.error(`unknown CPU profile: ${cpu} (allowed: unthrottled, 4x, 6x)`);
                process.exit(1);
            }
            for (const networkLabel of NETWORK_PROFILES) {
                if (!(networkLabel in NETWORK_PROFILES_MAP)) {
                    console.error(`unknown network profile: ${networkLabel} (allowed: wifi, 4g, fast3g, slow3g)`);
                    process.exit(1);
                }
                const network = NETWORK_PROFILES_MAP[networkLabel];
                for (const cache of CACHE_PROFILES) {
                    if (cache !== "cold" && cache !== "warm") {
                        console.error(`unknown cache profile: ${cache} (allowed: cold, warm)`);
                        process.exit(1);
                    }
                    const perEngine = [];
                    for (const e of engines) {
                        process.stdout.write(`  measuring ${e.name}  cpu=${cpu}  net=${networkLabel}  cache=${cache} ... `);
                        const res = await measureEngine({
                            browser,
                            engine: e,
                            cpuRate,
                            cpuLabel: cpu,
                            network,
                            networkLabel,
                            cache,
                            runs: RUNS,
                        });
                        process.stdout.write(`done (${res.runs}/${RUNS} ok${res.errors.length ? `, ${res.errors.length} errored` : ""})\n`);
                        perEngine.push(res);
                    }
                    const rows = summarizeRows(perEngine);
                    allSummaries.push({ cpu, network: networkLabel, cache, runs: RUNS, rows });
                }
            }
        }
    } finally {
        await browser.close().catch(() => {});
    }

    printAll(allSummaries);
    printWinners(allSummaries);

    // Echo any errors collected per-engine after the table so they
    // don't muddy the headline numbers but are still visible.
    let anyErrors = false;
    for (const s of allSummaries) {
        for (const r of s.rows) {
            if (r.errors && r.errors.length) {
                if (!anyErrors) {
                    console.log("");
                    console.log("═══ run errors (collected) ═══");
                    anyErrors = true;
                }
                console.log(`  ${r.engine} cpu=${s.cpu} net=${s.network} cache=${s.cache}: ${r.errors.length} error(s)`);
                for (const msg of r.errors.slice(0, 3)) console.log(`      ${msg}`);
            }
        }
    }
}

main().catch((err) => {
    console.error("✘ bench failed:", err.stack || err.message || err);
    process.exit(1);
});
