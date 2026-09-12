// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Lean-page bench harness. Same primitives as `proa_demo/bench/run.mjs`,
// but points playwright at the three lean servers (5501/5502/5503).
//
// Companion servers must already be running:
//   - proa-lean       (default :5501, override PROA_LEAN_PORT)
//   - marko-lean      (default :5502, override MARKO_LEAN_PORT)
//   - react-lean      (default :5503, override REACT_LEAN_PORT)
//
// Run:
//   cd proa_demo/bench && node lean-page/lean-run.mjs
//
// Tunables (same env vars as run.mjs):
//   BENCH_RUNS=5
//   BENCH_CPU=4x
//   BENCH_NETWORK=4g
//   BENCH_CACHE=cold
//   LEAN_N=60          number of widgets per page

import { chromium } from "playwright";
import { measureEngine } from "../measure.mjs";
import { stats, printAll, printWinners } from "../format.mjs";

const PROA_LEAN_PORT = process.env.PROA_LEAN_PORT || 5501;
const MARKO_LEAN_PORT = process.env.MARKO_LEAN_PORT || 5502;
const REACT_LEAN_PORT = process.env.REACT_LEAN_PORT || 5503;
const N = process.env.LEAN_N || 60;
const RUNS = parseInt(process.env.BENCH_RUNS || "5", 10);

const CPU_PROFILES = (process.env.BENCH_CPU || "unthrottled,4x,6x").split(",").map((s) => s.trim()).filter(Boolean);
const NETWORK_PROFILES = (process.env.BENCH_NETWORK || "wifi,4g").split(",").map((s) => s.trim()).filter(Boolean);
const CACHE_PROFILES = (process.env.BENCH_CACHE || "cold,warm").split(",").map((s) => s.trim()).filter(Boolean);

const CPU_RATES = { unthrottled: 1, "4x": 4, "6x": 6 };
const NETWORK_PROFILES_MAP = {
    wifi: null,
    "4g": { downloadKbps: 10 * 1024, uploadKbps: 1024, latency: 100 },
    fast3g: { downloadKbps: 1500, uploadKbps: 750, latency: 562 },
    slow3g: { downloadKbps: 400, uploadKbps: 400, latency: 2000 },
};

const engines = [
    {
        name: "proa-lean",
        url: `http://localhost:${PROA_LEAN_PORT}/?n=${N}`,
        healthCheck: `http://localhost:${PROA_LEAN_PORT}/health`,
        startHint: `cargo run --release -p proa-lean-server  (binds :${PROA_LEAN_PORT})`,
    },
    {
        name: "marko-lean",
        url: `http://localhost:${MARKO_LEAN_PORT}/?n=${N}`,
        healthCheck: `http://localhost:${MARKO_LEAN_PORT}/health`,
        startHint: `cd proa_demo/bench/lean-page/marko-server && PORT=${MARKO_LEAN_PORT} node server.mjs`,
    },
    {
        name: "react-lean",
        url: `http://localhost:${REACT_LEAN_PORT}/?n=${N}`,
        healthCheck: `http://localhost:${REACT_LEAN_PORT}/health`,
        startHint: `cd proa_demo/bench/lean-page/react-server && PORT=${REACT_LEAN_PORT} node --import tsx server.tsx`,
    },
];

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
        scriptCpu: stats(res.records.map((r) => r.scriptDuration)),
        longTasks: stats(res.records.map((r) => r.longTaskSum)),
        transferred: stats(res.records.map((r) => r.transferred)),
    }));
}

async function main() {
    console.log("═══ lean-bench: timing/CPU/network/cache harness (LEAN page, n=" + N + ") ═══");
    console.log(`runs=${RUNS}  cpu=${CPU_PROFILES.join(",")}  network=${NETWORK_PROFILES.join(",")}  cache=${CACHE_PROFILES.join(",")}`);
    console.log(`proa-lean=:${PROA_LEAN_PORT}  marko-lean=:${MARKO_LEAN_PORT}  react-lean=:${REACT_LEAN_PORT}`);

    await preflight();

    const browser = await chromium.launch({ headless: true });
    const allSummaries = [];

    try {
        for (const cpu of CPU_PROFILES) {
            const cpuRate = CPU_RATES[cpu];
            if (cpuRate == null) {
                console.error(`unknown CPU profile: ${cpu}`);
                process.exit(1);
            }
            for (const networkLabel of NETWORK_PROFILES) {
                if (!(networkLabel in NETWORK_PROFILES_MAP)) {
                    console.error(`unknown network profile: ${networkLabel}`);
                    process.exit(1);
                }
                const network = NETWORK_PROFILES_MAP[networkLabel];
                for (const cache of CACHE_PROFILES) {
                    if (cache !== "cold" && cache !== "warm") {
                        console.error(`unknown cache profile: ${cache}`);
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
    console.error("✘ lean-bench failed:", err.stack || err.message || err);
    process.exit(1);
});
