// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Table formatting for the bench harness. Mirrors the column-padded
// style of `proa_demo/bundle_size.mjs` so output reads consistently.

export function pad(s, w) {
    s = String(s);
    return s.length >= w ? s : s + " ".repeat(w - s.length);
}

export function rpad(s, w) {
    s = String(s);
    return s.length >= w ? s : " ".repeat(w - s.length) + s;
}

export function fmtMs(n) {
    if (n == null || Number.isNaN(n)) return "—";
    if (n >= 1000) return (n / 1000).toFixed(2) + "s";
    if (n >= 100) return n.toFixed(0) + "ms";
    if (n >= 10) return n.toFixed(1) + "ms";
    return n.toFixed(2) + "ms";
}

export function fmtBytes(n) {
    if (n == null || Number.isNaN(n)) return "—";
    if (n >= 1024 * 1024) return (n / 1024 / 1024).toFixed(2) + "MB";
    if (n >= 1024) return (n / 1024).toFixed(1) + "KB";
    return n + "B";
}

// Compute median + p95 of a numeric array. Returns NaN for empty input.
export function stats(values) {
    const xs = values.filter((v) => typeof v === "number" && !Number.isNaN(v)).sort((a, b) => a - b);
    if (xs.length === 0) return { median: NaN, p95: NaN, n: 0 };
    const median = xs[Math.floor(xs.length / 2)];
    // p95 with linear interpolation; falls back to last value for n < 20.
    const idx = Math.min(xs.length - 1, Math.ceil(xs.length * 0.95) - 1);
    const p95 = xs[Math.max(0, idx)];
    return { median, p95, n: xs.length };
}

// Print a per-(cpu, cache) section table with one row per engine.
//
// `summary` shape:
//   {
//     cpu: "unthrottled" | "4x" | "6x",
//     cache: "cold" | "warm",
//     rows: [{ engine, ttfb, fcp, lcp, tti, scriptCpu, transferred }]
//   }
//
// Each metric value is itself { median, p95, n }.
export function printSection(summary) {
    console.log("");
    const net = summary.network ? `  net=${summary.network}` : "";
    console.log(
        `═══ CPU=${summary.cpu}${net}  cache=${summary.cache}  (n=${summary.runs} runs/engine) ═══`,
    );
    console.log("");
    const cols = [
        ["engine", 14],
        ["TTFB", 9],
        ["FCP", 9],
        ["LCP", 9],
        ["TTI", 9],
        ["scriptCPU", 11],
        ["longTasks", 11],
        ["bytes", 10],
    ];
    console.log(cols.map(([h, w]) => rpad(h, w)).join(""));
    console.log(cols.map(([, w]) => rpad("─".repeat(w - 2), w)).join(""));
    for (const r of summary.rows) {
        const cells = [
            pad(r.engine, 14),
            rpad(fmtMs(r.ttfb.median), 9),
            rpad(fmtMs(r.fcp.median), 9),
            rpad(fmtMs(r.lcp.median), 9),
            rpad(fmtMs(r.tti.median), 9),
            rpad(fmtMs(r.scriptCpu.median), 11),
            rpad(fmtMs(r.longTasks.median), 11),
            rpad(fmtBytes(r.transferred.median), 10),
        ];
        console.log(cells.join(""));
    }
}

// Print a single grand-comparison table grouped by (cpu, cache).
export function printAll(allSummaries) {
    for (const s of allSummaries) printSection(s);
}

// Quick winner callout per profile. Useful for spot-checking that
// numbers look plausible (e.g. proa-rsjs FCP < react FCP under 6x).
export function printWinners(allSummaries) {
    console.log("");
    console.log("═══ winners by FCP (lower = better) ═══");
    console.log("");
    for (const s of allSummaries) {
        const ranked = [...s.rows]
            .filter((r) => !Number.isNaN(r.fcp.median))
            .sort((a, b) => a.fcp.median - b.fcp.median);
        if (ranked.length === 0) continue;
        const best = ranked[0];
        const worst = ranked[ranked.length - 1];
        const ratio = worst.fcp.median / best.fcp.median;
        const net = s.network ? ` net=${pad(s.network, 6)}` : "";
        console.log(
            `  CPU=${pad(s.cpu, 12)}${net} cache=${pad(s.cache, 6)}  ${pad(best.engine, 12)} ${fmtMs(best.fcp.median)}  vs  ${pad(worst.engine, 12)} ${fmtMs(worst.fcp.median)}  (${ratio.toFixed(2)}×)`,
        );
    }
}
