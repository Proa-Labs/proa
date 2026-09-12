// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Per-engine measurement primitives. One `measureRun` is a single
// fresh-page load with a configured CPU throttle and cache mode; it
// returns a flat record of timing metrics.
//
// All browser-side observation runs inside `page.evaluate` so the
// metrics are computed on the page's own performance timeline, not
// from playwright's external clock. CDP throttling is applied before
// navigation so SSR HTML parsing also pays the throttle cost.

const NAV_TIMEOUT_MS = 30_000;
// Quietness window for the TTI heuristic: TTI is the timestamp at
// which there are no long tasks (>50ms) for `TTI_QUIET_MS`. We use a
// shorter window than the canonical 5s because the marketplace fixture
// reaches a steady state quickly under throttling and the wall-clock
// cost of a full 5s wait dominates the run.
const TTI_QUIET_MS = 2_000;
// How long to wait after `load` for late LCP candidates and CPU
// settle before reading metrics out. Tuned to be long enough for
// React 19 hydration under 6x CPU throttle.
const SETTLE_MS = 3_000;

// Build the in-page collector. This script registers PerformanceObservers
// for `paint`, `largest-contentful-paint`, and `longtask`, plus a
// readout closure that returns the aggregated metrics. We attach via
// `addInitScript` so it runs *before* any page script — important so
// LCP entries that fire during initial HTML streaming are captured.
export const COLLECTOR_SCRIPT = `
(() => {
    if (window.__proaBench) return;
    const state = {
        fcp: null,
        lcp: null,
        longTasks: [],
        scriptDurations: [],
    };
    window.__proaBench = state;
    try {
        new PerformanceObserver((list) => {
            for (const e of list.getEntries()) {
                if (e.name === "first-contentful-paint" && state.fcp == null) {
                    state.fcp = e.startTime;
                }
            }
        }).observe({ type: "paint", buffered: true });
    } catch {}
    try {
        new PerformanceObserver((list) => {
            for (const e of list.getEntries()) {
                state.lcp = e.startTime;
            }
        }).observe({ type: "largest-contentful-paint", buffered: true });
    } catch {}
    try {
        new PerformanceObserver((list) => {
            for (const e of list.getEntries()) {
                state.longTasks.push({ start: e.startTime, duration: e.duration });
            }
        }).observe({ type: "longtask", buffered: true });
    } catch {}
})();
`;

// Pulled out so we can serialize it cleanly into page.evaluate without
// closing over Node-side variables.
function collectMetricsInPage(quietMs) {
    const state = window.__proaBench || {};
    const nav = performance.getEntriesByType("navigation")[0] || {};
    const ttfb = nav.responseStart || NaN;
    const loadEnd = nav.loadEventEnd || NaN;
    const transferred = nav.transferSize || 0;
    const encodedBody = nav.encodedBodySize || 0;
    // First-party transferred bytes: sum of same-origin resource
    // entries plus the HTML doc itself. Lined up with bundle_size.mjs.
    //
    // Note on scriptCpu: we used to derive a parse+eval proxy from
    // `PerformanceResourceTiming.duration - (responseEnd - startTime)`,
    // but that is mathematically zero by spec (`duration` IS defined
    // as `responseEnd - startTime`). The real source of truth is CDP
    // `Performance.getMetrics().ScriptDuration`, captured outside the
    // page in `runOnce`. Here we surface a complementary proxy: the
    // sum of long-task durations after FCP, which the user-perceived
    // hydration cost roughly correlates with.
    const origin = location.origin;
    const resources = performance.getEntriesByType("resource");
    let firstPartyTransferred = transferred;
    for (const r of resources) {
        const sameOrigin = r.name.startsWith(origin + "/") || r.name === origin;
        if (sameOrigin) firstPartyTransferred += r.transferSize || 0;
    }
    // TTI heuristic: scan long tasks; find earliest moment after FCP
    // where there is `quietMs` of silence with no long tasks.
    const tasks = (state.longTasks || []).slice().sort((a, b) => a.start - b.start);
    let tti = NaN;
    const fcp = state.fcp;
    if (fcp != null) {
        const after = tasks.filter((t) => t.start + t.duration >= fcp);
        let cursor = fcp;
        if (after.length === 0) {
            tti = fcp;
        } else {
            for (let i = 0; i < after.length; i++) {
                const gap = after[i].start - cursor;
                if (gap >= quietMs) {
                    tti = cursor;
                    break;
                }
                cursor = Math.max(cursor, after[i].start + after[i].duration);
            }
            if (Number.isNaN(tti)) {
                // Use the end of the last long task, or loadEnd, as a
                // conservative upper bound.
                const lastEnd = after.length ? after[after.length - 1].start + after[after.length - 1].duration : cursor;
                tti = Math.max(lastEnd, loadEnd || lastEnd);
            }
        }
    }
    // Long-task durations after FCP — this is what the user feels as
    // "page is janky during hydration." Sum of ms spent in tasks >50ms
    // that fired after FCP (so we count hydration work, not the
    // initial parse).
    let longTaskSum = 0;
    if (fcp != null) {
        for (const t of tasks) {
            if (t.start + t.duration >= fcp) longTaskSum += t.duration;
        }
    }
    return {
        ttfb,
        fcp,
        lcp: state.lcp,
        tti,
        longTaskSum,
        transferred: firstPartyTransferred,
        loadEnd,
        encodedBody,
        longTaskCount: tasks.length,
    };
}

// Apply CPU throttling + network conditions + cache state to a fresh
// CDP session and load the URL. Returns one metrics record. The
// `scriptDuration` metric in the result comes from CDP's
// `Performance.getMetrics()` (the only spec-supported way to read
// real script-execution CPU time from outside the page).
async function runOnce({ context, page, url, cdp, cpuRate, network, cache, prewarmedFor }) {
    if (cache === "cold") {
        await cdp.send("Network.clearBrowserCache");
        await cdp.send("Network.clearBrowserCookies");
    }
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: cpuRate });
    if (network) {
        await cdp.send("Network.emulateNetworkConditions", {
            offline: false,
            latency: network.latency,
            downloadThroughput: network.downloadKbps * 1024 / 8,
            uploadThroughput: network.uploadKbps * 1024 / 8,
        });
    }
    // Reset CDP performance counters so `getMetrics` after navigation
    // reads only this run's totals. Re-enabling clears the running
    // accumulator on most chromium builds.
    await cdp.send("Performance.disable").catch(() => {});
    await cdp.send("Performance.enable");
    if (cache === "warm" && prewarmedFor !== url) {
        await page.goto(url, { waitUntil: "load", timeout: NAV_TIMEOUT_MS });
        await page.waitForTimeout(500);
    }
    const t0 = Date.now();
    const resp = await page.goto(url, { waitUntil: "load", timeout: NAV_TIMEOUT_MS });
    if (!resp || !resp.ok()) {
        throw new Error(`navigate ${url} → status ${resp ? resp.status() : "no response"}`);
    }
    // Settle: wait for late LCP candidates to fire and any hydration
    // long tasks to finish before reading metrics. We extend the
    // settle on slow networks so we don't read mid-flight LCPs.
    const settleMs = network && network.downloadKbps < 1000 ? SETTLE_MS + 2000 : SETTLE_MS;
    await page.waitForTimeout(settleMs);
    const metrics = await page.evaluate(collectMetricsInPage, TTI_QUIET_MS);
    metrics.wallClock = Date.now() - t0;
    // Pull script-execution CPU time from CDP. `Performance.getMetrics`
    // returns cumulative ms-since-page-start, including main-thread V8
    // execution outside long tasks. This is the metric we actually
    // want for the "did proa avoid 200ms of React parse+eval?" question.
    try {
        const { metrics: cdpMetrics } = await cdp.send("Performance.getMetrics");
        const byName = Object.fromEntries(cdpMetrics.map((m) => [m.name, m.value]));
        // Values are in seconds per CDP spec; convert to ms for
        // consistency with the in-page metrics.
        metrics.scriptDuration = (byName.ScriptDuration || 0) * 1000;
        metrics.layoutDuration = (byName.LayoutDuration || 0) * 1000;
        metrics.recalcStyleDuration = (byName.RecalcStyleDuration || 0) * 1000;
        metrics.taskDuration = (byName.TaskDuration || 0) * 1000;
    } catch {
        metrics.scriptDuration = NaN;
    }
    return metrics;
}

// Drive N runs for a single engine under one (cpu, network, cache)
// profile. Each run uses a fresh `BrowserContext` so service workers,
// SW caches, and in-memory caches don't leak between runs.
export async function measureEngine({
    browser,
    engine,
    cpuRate,
    cpuLabel,
    network,
    networkLabel,
    cache,
    runs,
}) {
    const records = [];
    const errors = [];
    for (let i = 0; i < runs; i++) {
        let context;
        try {
            context = await browser.newContext({
                bypassCSP: true,
                serviceWorkers: "block",
            });
            await context.addInitScript({ content: COLLECTOR_SCRIPT });
            const page = await context.newPage();
            const cdp = await context.newCDPSession(page);
            await cdp.send("Network.enable");
            await cdp.send("Network.setCacheDisabled", { cacheDisabled: cache === "cold" });
            const m = await runOnce({
                context,
                page,
                url: engine.url,
                cdp,
                cpuRate,
                network,
                cache,
            });
            records.push(m);
        } catch (err) {
            errors.push(err.message || String(err));
        } finally {
            if (context) await context.close().catch(() => {});
        }
    }
    return { engine, cpuLabel, networkLabel, cache, runs: records.length, records, errors };
}
