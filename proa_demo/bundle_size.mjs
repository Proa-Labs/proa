// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Measure framework JS shipped per page across engines.
//
// Hits each engine's full-page URL, extracts the `<script src="...">`
// and `<link rel="modulepreload" href="...">` references, walks static
// JavaScript imports from those entry assets, and prints a side-by-side table:
//
//     raw bytes
//     raw bytes after esbuild minify (parity normalisation)
//     gzip-9 of raw
//     gzip-9 of minified
//     brotli-11 of raw
//     brotli-11 of minified
//
// Only first-party framework assets are counted. The shared synthetic shell
// contains no third-party scripts, so the totals isolate framework code.
//
// Run with: node bundle_size.mjs
//
// Optional rsjs budget gates:
//   PROA_RSJS_TOTAL_MIN_GZIP_BUDGET=12000 node bundle_size.mjs
//   PROA_RSJS_FRAGMENT_MIN_GZIP_BUDGET=4000 node bundle_size.mjs
//
// Add or remove engines by editing the `engines` array. The script
// expects each engine's server to already be running.

import { transform as esbuildTransform } from "../proa_demo/marko-server/node_modules/esbuild/lib/main.js";
import { gzipSync, brotliCompressSync, constants as zlibConstants } from "node:zlib";

// Each engine's `frameworkRoutes` is the set of URL-path patterns that
// represent JS shipped by this framework specifically. Any unrelated
// application script is filtered out so the comparison reflects only
// framework-emitted bytes.
// Ports match what the playground binary picks at startup. The proa
// playground walks 5000-5010 to find a free port; the companion marko
// node server is auto-spawned on 5300, react-bun on 5200, etc. The
// react-ssr companion is started manually at the dev's discretion on
// 5401, and the Next.js companion is started manually on 5402. If
// you're running on a non-default port, override these via env vars:
// `PROA_PORT=5001 node bundle_size.mjs`.
const PROA_PORT = process.env.PROA_PORT || 5000;
const MARKO_NODE_PORT = process.env.MARKO_NODE_PORT || 5300;
const REACT_SSR_PORT = process.env.REACT_SSR_PORT || 5401;
const NEXT_PORT = process.env.NEXT_PORT || 5402;

const engines = [
    {
        name: "proa-rsjs",
        url: `http://localhost:${PROA_PORT}/showcase/ebay/proa-rsjs`,
        // Match both legacy un-hashed shapes
        // (`/rsjs-runtime.js`, `/rsjs-runtime-query.js`,
        // `/rsjs/island/<asset_id>.js`) and content-hashed shapes
        // (`/rsjs-runtime.<hash8>.js`, `/rsjs-runtime-query.<hash8>.js`,
        // `/rsjs/island/<asset_id>.<hash8>.js`). The `(?:\.[0-9a-f]{6,})?`
        // optional segment captures the 8-hex content hash inserted
        // before `.js` once the route asset graph + immutable cache
        // headers ship.
        frameworkRoutes: [
            /^\/rsjs-runtime(?:-query)?(?:\.[0-9a-f]{6,})?\.js$/,
            /^\/rsjs\/island\//,
        ],
    },
    {
        name: "marko 6",
        url: `http://localhost:${MARKO_NODE_PORT}/showcase/ebay/marko`,
        frameworkRoutes: [/^\/marko-hydrate\.js$/],
    },
    {
        name: "react 19",
        url: `http://localhost:${REACT_SSR_PORT}/showcase/ebay/react`,
        frameworkRoutes: [/^\/client\.bundle\.js$/],
    },
    {
        name: "next 16",
        url: `http://localhost:${NEXT_PORT}/showcase/ebay/next`,
        frameworkRoutes: [/^\/_next\/static\/.*\.js$/],
    },
];

const SCRIPT_TAG_RE = /<script\b[^>]*\bsrc="([^"]+)"/gi;
const MODULEPRELOAD_TAG_RE = /<link\b[^>]*\brel="modulepreload"[^>]*\bhref="([^"]+)"/gi;
const STATIC_IMPORT_RE = /\bimport\s+(?:[^'"()]*?\s+from\s*)?["']([^"']+)["']/g;

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

function gzip9(bytes) {
    return gzipSync(bytes, { level: 9 }).length;
}

function brotli11(bytes) {
    return brotliCompressSync(bytes, {
        params: { [zlibConstants.BROTLI_PARAM_QUALITY]: 11 },
    }).length;
}

async function minifyBytes(bytes) {
    const r = await esbuildTransform(bytes.toString("utf8"), {
        minify: true,
        target: ["es2020"],
    });
    return Buffer.from(r.code, "utf8");
}

function pad(s, w) {
    s = String(s);
    return s.length >= w ? s : s + " ".repeat(w - s.length);
}

function rpad(s, w) {
    s = String(s);
    return s.length >= w ? s : " ".repeat(w - s.length) + s;
}

function fmt(n) {
    return n.toLocaleString("en-US");
}

function zeroTotal() {
    return { rawLen: 0, minLen: 0, rawGz: 0, minGz: 0, rawBr: 0, minBr: 0 };
}

function addTotals(acc, a) {
    acc.rawLen += a.rawLen;
    acc.minLen += a.minLen;
    acc.rawGz += a.rawGz;
    acc.minGz += a.minGz;
    acc.rawBr += a.rawBr;
    acc.minBr += a.minBr;
    return acc;
}

function sumAssets(assets) {
    return assets.reduce(addTotals, zeroTotal());
}

function classifyAsset(engine, path, source) {
    if (engine.name !== "proa-rsjs") return "framework";
    if (/^\/rsjs-runtime-query(?:\.[0-9a-f]{6,})?\.js$/.test(path)) return "query-runtime";
    if (/^\/rsjs-runtime(?:\.[0-9a-f]{6,})?\.js$/.test(path)) return "runtime";
    if (/^\/rsjs\/island\/component_/.test(path)) return "fragment";
    if (/^\/rsjs\/island\//.test(path) && source === "import") return "client-module";
    if (/^\/rsjs\/island\//.test(path)) return "island";
    return "framework";
}

function categoryTotals(assets) {
    const byCategory = new Map();
    for (const asset of assets) {
        if (!byCategory.has(asset.category)) byCategory.set(asset.category, zeroTotal());
        addTotals(byCategory.get(asset.category), asset);
    }
    return [...byCategory.entries()].sort(([a], [b]) => a.localeCompare(b));
}

function staticImports(source) {
    const imports = [];
    for (const match of source.matchAll(STATIC_IMPORT_RE)) {
        imports.push(match[1]);
    }
    return imports;
}

async function measureEngine(engine) {
    const html = await fetchText(engine.url);
    const seen = new Set();
    const queue = [];
    function collect(src, source = "html-script", baseUrl = engine.url) {
        const abs = new URL(src, baseUrl).toString();
        if (!isFirstParty(abs, engine.url)) return;
        const path = new URL(abs).pathname;
        if (engine.frameworkRoutes && !engine.frameworkRoutes.some((re) => re.test(path))) return;
        if (seen.has(abs)) return;
        seen.add(abs);
        queue.push({ url: abs, source });
    }
    for (const m of html.matchAll(SCRIPT_TAG_RE)) {
        collect(m[1], "html-script");
    }
    for (const m of html.matchAll(MODULEPRELOAD_TAG_RE)) {
        collect(m[1], "modulepreload");
    }
    const assets = [];
    for (let i = 0; i < queue.length; i += 1) {
        const item = queue[i];
        const raw = await fetchBytes(item.url);
        const min = await minifyBytes(raw);
        const path = new URL(item.url).pathname;
        assets.push({
            url: item.url,
            source: item.source,
            category: classifyAsset(engine, path, item.source),
            raw,
            min,
            rawLen: raw.length,
            minLen: min.length,
            rawGz: gzip9(raw),
            minGz: gzip9(min),
            rawBr: brotli11(raw),
            minBr: brotli11(min),
        });
        const js = raw.toString("utf8");
        for (const specifier of staticImports(js)) {
            collect(specifier, "import", item.url);
        }
    }
    const total = sumAssets(assets);
    return { engine, html, assets, total };
}

function printPerEngineBreakdown(report) {
    console.log(`\n═══ ${report.engine.name} (${report.engine.url}) ═══`);
    console.log(`page HTML: ${fmt(report.html.length)} bytes raw, ${fmt(gzip9(Buffer.from(report.html)))} gz, ${fmt(brotli11(Buffer.from(report.html)))} br`);
    if (report.assets.length === 0) {
        console.log("  (no first-party JS found)");
        return;
    }
    console.log("");
    console.log(
        `  ${pad("asset", 56)}${pad("category", 15)}${rpad("raw", 9)}${rpad("min", 9)}${rpad("rawGz", 9)}${rpad("minGz", 9)}${rpad("rawBr", 9)}${rpad("minBr", 9)}`,
    );
    for (const a of report.assets) {
        const short = a.url.replace(report.engine.url.split("/").slice(0, 3).join("/"), "");
        console.log(
            `  ${pad(short.length > 56 ? "…" + short.slice(-55) : short, 56)}${pad(a.category, 15)}${rpad(fmt(a.rawLen), 9)}${rpad(fmt(a.minLen), 9)}${rpad(fmt(a.rawGz), 9)}${rpad(fmt(a.minGz), 9)}${rpad(fmt(a.rawBr), 9)}${rpad(fmt(a.minBr), 9)}`,
        );
    }
    console.log(
        `  ${pad("TOTAL", 56)}${pad("", 15)}${rpad(fmt(report.total.rawLen), 9)}${rpad(fmt(report.total.minLen), 9)}${rpad(fmt(report.total.rawGz), 9)}${rpad(fmt(report.total.minGz), 9)}${rpad(fmt(report.total.rawBr), 9)}${rpad(fmt(report.total.minBr), 9)}`,
    );
    const categories = categoryTotals(report.assets);
    if (categories.length > 1) {
        console.log("");
        console.log(
            `  ${pad("by category", 15)}${rpad("raw", 9)}${rpad("min", 9)}${rpad("rawGz", 9)}${rpad("minGz", 9)}${rpad("rawBr", 9)}${rpad("minBr", 9)}`,
        );
        for (const [category, total] of categories) {
            console.log(
                `  ${pad(category, 15)}${rpad(fmt(total.rawLen), 9)}${rpad(fmt(total.minLen), 9)}${rpad(fmt(total.rawGz), 9)}${rpad(fmt(total.minGz), 9)}${rpad(fmt(total.rawBr), 9)}${rpad(fmt(total.minBr), 9)}`,
            );
        }
    }
}

function printComparisonTable(reports) {
    console.log("\n═══ comparison: total client JS shipped (first-party only) ═══");
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
        const b = reports[1];
        console.log("");
        console.log(
            `${a.engine.name} vs ${b.engine.name} (min gzip): ${fmt(a.total.minGz)} vs ${fmt(b.total.minGz)} = ${(b.total.minGz / a.total.minGz).toFixed(2)}× ${b.total.minGz > a.total.minGz ? "smaller in " + a.engine.name : "smaller in " + b.engine.name}`,
        );
    }
}

function parseBudget(name) {
    const raw = process.env[name];
    if (!raw) return null;
    const budget = Number(raw);
    if (!Number.isFinite(budget) || budget < 0) {
        throw new Error(`${name} must be a non-negative byte count, got ${raw}`);
    }
    return budget;
}

function enforceRsjsBudgets(reports) {
    const report = reports.find((r) => r.engine.name === "proa-rsjs");
    if (!report) return;

    const totalBudget = parseBudget("PROA_RSJS_TOTAL_MIN_GZIP_BUDGET");
    const fragmentBudget = parseBudget("PROA_RSJS_FRAGMENT_MIN_GZIP_BUDGET");
    const checks = [];
    if (totalBudget !== null) {
        checks.push(["total min gzip", report.total.minGz, totalBudget]);
    }
    if (fragmentBudget !== null) {
        const fragmentTotal = categoryTotals(report.assets).find(([category]) => category === "fragment")?.[1] || zeroTotal();
        checks.push(["fragment min gzip", fragmentTotal.minGz, fragmentBudget]);
    }

    for (const [label, actual, budget] of checks) {
        if (actual > budget) {
            console.error(`rsjs budget exceeded: ${label} ${fmt(actual)}B > ${fmt(budget)}B`);
            process.exitCode = 1;
        } else {
            console.log(`rsjs budget ok: ${label} ${fmt(actual)}B <= ${fmt(budget)}B`);
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
enforceRsjsBudgets(reports);
