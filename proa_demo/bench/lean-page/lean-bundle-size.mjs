// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Wire-size measurement for the LEAN benchmark page (proa_demo/bench/lean-page).
//
// Mirrors `proa_demo/bundle_size.mjs` but points at the three lean
// servers (5501/5502/5503) and counts only the framework-shipped JS:
//   proa-lean : /rsjs-runtime.js + /rsjs/island/<asset>.js
//   marko-lean: /lean-hydrate.js
//   react-lean: /client.bundle.js
//
// Run with: node lean-bundle-size.mjs

import { transform as esbuildTransform } from "../../marko-server/node_modules/esbuild/lib/main.js";
import { gzipSync, brotliCompressSync, constants as zlibConstants } from "node:zlib";

const PROA_LEAN_PORT = process.env.PROA_LEAN_PORT || 5501;
const MARKO_LEAN_PORT = process.env.MARKO_LEAN_PORT || 5502;
const REACT_LEAN_PORT = process.env.REACT_LEAN_PORT || 5503;
const N = process.env.LEAN_N || 60;

const engines = [
    {
        name: "proa-lean",
        url: `http://localhost:${PROA_LEAN_PORT}/?n=${N}`,
        frameworkRoutes: [/^\/rsjs-runtime(?:-query)?\.js$/, /^\/rsjs\/island\//],
    },
    {
        name: "marko-lean",
        url: `http://localhost:${MARKO_LEAN_PORT}/?n=${N}`,
        frameworkRoutes: [/^\/lean-hydrate\.js$/],
    },
    {
        name: "react-lean",
        url: `http://localhost:${REACT_LEAN_PORT}/?n=${N}`,
        frameworkRoutes: [/^\/client\.bundle\.js$/],
    },
];

const SCRIPT_TAG_RE = /<script\b[^>]*\bsrc="([^"]+)"/gi;

async function fetchText(url) {
    const r = await fetch(url, { redirect: "follow" });
    if (!r.ok) throw new Error(`${url} → HTTP ${r.status}`);
    return r.text();
}
async function fetchBytes(url) {
    const r = await fetch(url, { redirect: "follow" });
    if (!r.ok) throw new Error(`${url} → HTTP ${r.status}`);
    return Buffer.from(await r.arrayBuffer());
}

function isFirstParty(scriptUrl, pageUrl) {
    try {
        const a = new URL(scriptUrl, pageUrl);
        const b = new URL(pageUrl);
        return a.origin === b.origin;
    } catch {
        return false;
    }
}
const gzip9 = (b) => gzipSync(b, { level: 9 }).length;
const brotli11 = (b) =>
    brotliCompressSync(b, { params: { [zlibConstants.BROTLI_PARAM_QUALITY]: 11 } }).length;
async function minifyBytes(bytes) {
    const r = await esbuildTransform(bytes.toString("utf8"), { minify: true, target: ["es2020"] });
    return Buffer.from(r.code, "utf8");
}
const fmt = (n) => n.toLocaleString("en-US");
const pad = (s, w) => (String(s).length >= w ? String(s) : String(s) + " ".repeat(w - String(s).length));
const rpad = (s, w) => (String(s).length >= w ? String(s) : " ".repeat(w - String(s).length) + String(s));

async function measureEngine(engine) {
    const html = await fetchText(engine.url);
    const seen = new Set();
    const scripts = [];
    for (const m of html.matchAll(SCRIPT_TAG_RE)) {
        const src = m[1];
        if (!isFirstParty(src, engine.url)) continue;
        const abs = new URL(src, engine.url).toString();
        const path = new URL(abs).pathname;
        if (engine.frameworkRoutes && !engine.frameworkRoutes.some((re) => re.test(path))) continue;
        if (seen.has(abs)) continue;
        seen.add(abs);
        scripts.push(abs);
    }
    const assets = [];
    for (const s of scripts) {
        const raw = await fetchBytes(s);
        const min = await minifyBytes(raw);
        assets.push({
            url: s,
            rawLen: raw.length,
            minLen: min.length,
            rawGz: gzip9(raw),
            minGz: gzip9(min),
            rawBr: brotli11(raw),
            minBr: brotli11(min),
        });
    }
    const total = assets.reduce(
        (acc, a) => ({
            rawLen: acc.rawLen + a.rawLen,
            minLen: acc.minLen + a.minLen,
            rawGz: acc.rawGz + a.rawGz,
            minGz: acc.minGz + a.minGz,
            rawBr: acc.rawBr + a.rawBr,
            minBr: acc.minBr + a.minBr,
        }),
        { rawLen: 0, minLen: 0, rawGz: 0, minGz: 0, rawBr: 0, minBr: 0 },
    );
    return { engine, html, assets, total };
}

function printPerEngineBreakdown(report) {
    console.log(`\n═══ ${report.engine.name} (${report.engine.url}) ═══`);
    const htmlBuf = Buffer.from(report.html);
    console.log(`page HTML: ${fmt(report.html.length)} bytes raw, ${fmt(gzip9(htmlBuf))} gz, ${fmt(brotli11(htmlBuf))} br`);
    if (report.assets.length === 0) {
        console.log("  (no first-party JS found)");
        return;
    }
    console.log("");
    console.log(`  ${pad("asset", 56)}${rpad("raw", 9)}${rpad("min", 9)}${rpad("rawGz", 9)}${rpad("minGz", 9)}${rpad("rawBr", 9)}${rpad("minBr", 9)}`);
    for (const a of report.assets) {
        const short = a.url.replace(report.engine.url.split("/").slice(0, 3).join("/"), "");
        console.log(`  ${pad(short.length > 56 ? "…" + short.slice(-55) : short, 56)}${rpad(fmt(a.rawLen), 9)}${rpad(fmt(a.minLen), 9)}${rpad(fmt(a.rawGz), 9)}${rpad(fmt(a.minGz), 9)}${rpad(fmt(a.rawBr), 9)}${rpad(fmt(a.minBr), 9)}`);
    }
    console.log(`  ${pad("TOTAL", 56)}${rpad(fmt(report.total.rawLen), 9)}${rpad(fmt(report.total.minLen), 9)}${rpad(fmt(report.total.rawGz), 9)}${rpad(fmt(report.total.minGz), 9)}${rpad(fmt(report.total.rawBr), 9)}${rpad(fmt(report.total.minBr), 9)}`);
}

function printComparisonTable(reports) {
    console.log("\n═══ comparison: total client JS shipped on the LEAN page (first-party only) ═══");
    console.log("");
    const cols = [
        ["engine", 14],
        ["raw (B)", 11],
        ["min (B)", 11],
        ["raw gzip", 11],
        ["min gzip", 11],
        ["raw brot", 11],
        ["min brot", 11],
    ];
    console.log(cols.map(([h, w]) => rpad(h, w)).join(""));
    console.log(cols.map(([_, w]) => rpad("─".repeat(w - 2), w)).join(""));
    for (const r of reports) {
        const cells = [
            pad(r.engine.name, 14),
            rpad(fmt(r.total.rawLen), 11),
            rpad(fmt(r.total.minLen), 11),
            rpad(fmt(r.total.rawGz), 11),
            rpad(fmt(r.total.minGz), 11),
            rpad(fmt(r.total.rawBr), 11),
            rpad(fmt(r.total.minBr), 11),
        ];
        console.log(cells.join(""));
    }
    if (reports.length >= 2) {
        const a = reports[0];
        for (let i = 1; i < reports.length; i++) {
            const b = reports[i];
            const ratio = b.total.minGz / Math.max(1, a.total.minGz);
            console.log("");
            console.log(`${a.engine.name} vs ${b.engine.name} (min gzip): ${fmt(a.total.minGz)} vs ${fmt(b.total.minGz)} = ${ratio.toFixed(2)}× ${ratio > 1 ? "smaller in " + a.engine.name : "smaller in " + b.engine.name}`);
        }
    }
}

const reports = [];
for (const e of engines) {
    try {
        const r = await measureEngine(e);
        reports.push(r);
        printPerEngineBreakdown(r);
    } catch (err) {
        console.error(`✘ ${e.name} (${e.url}): ${err.message}`);
    }
}
printComparisonTable(reports);
