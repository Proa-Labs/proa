// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Hono-style React SSR server. Renders the full SRP document (header,
// rail, cards, footer) to HTML on each request: every element is a React
// element, no pre-rendered shell strings. Serves a single client-side
// hydrate bundle (built once at startup via esbuild).

import { build as esbuild } from "esbuild";
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { ComponentType } from "react";
import { renderToString } from "react-dom/server";
import { App } from "./page.tsx";
import type { Listing } from "./page.tsx";
import { BODY_CLASS, DEFAULT_QUERY, RealHeadAssets, RealSrpChrome } from "./chrome.generated.tsx";
import { NikeCatalogPage } from "../react-ssr-shared/src/nike-catalog";
import { NikePdpPage } from "../react-ssr-shared/src/nike-pdp";
import { EspnGamecastDocument, type EspnGamecastData } from "../react-ssr-shared/src/espn-gamecast";
import { buildDefaultSentryLogs, DEFAULT_SENTRY_LOGS, SentryOtelDocument, type SentryOtelProps } from "../react-ssr-shared/src/sentry-otel";
import { GithubPrDiffPage, GITHUB_PR_DIFF_CSS, GITHUB_PR_DIFF_JS, DEFAULT_GITHUB_PR_DIFF_PROPS } from "../react-ssr-shared/src/github-pr-diff";
import { NIKE_CSS } from "../react-ssr-shared/src/nike-css";
// @ts-expect-error — plain ESM, no .d.ts shipped with the marko helper.
import { fmtCount } from "../marko-server/helpers_srp_real.mjs";
// Reuse the marko-server helper so React, Marko, and Proa all render
// from the exact same listing dataset (real eBay image hashes, titles,
// subtitle parts, caption variants, etc.). Keeping the data identical
// across engines is what makes the side-by-side comparison meaningful.
// @ts-expect-error — plain ESM, no .d.ts shipped with the marko helper.
import { buildListings as buildListingsHelper } from "../marko-server/helpers_srp_real.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PACKAGE_DIR = __dirname.endsWith("/dist") ? dirname(__dirname) : __dirname;
const REPO_ROOT = join(PACKAGE_DIR, "..", "..");
const PORT = Number(process.env.PORT || 5401);
const HOST = process.env.HOST || "127.0.0.1";
const DATA_DIR =
    process.env.SHELL_DIR ||
    process.env.MARKETPLACE_DATA_DIR ||
    join(REPO_ROOT, "proa_showcase", "src", "ebay", "data");

// Page styles: the captured page's own stylesheet (extracted by
// scripts/gen_srp_chrome.py) plus the chips toolbar stylesheet.
const STYLE_MAIN = readFileSync(join(DATA_DIR, "srp_real_style.css"), "utf8");
const STYLE_CHIPS = readFileSync(join(DATA_DIR, "srp_chips_style.css"), "utf8");
const ESPN_CSS = readFileSync(
    join(REPO_ROOT, "proa_showcase", "src", "espn", "espn.css"),
    "utf8",
);
const SENTRY_OTEL_CSS = readFileSync(
    join(REPO_ROOT, "proa_showcase", "src", "sentry_otel", "sentry_otel.css"),
    "utf8",
);
const DEFAULT_ESPN_DATA = JSON.parse(
    readFileSync(join(REPO_ROOT, "proa_showcase", "src", "espn", "data", "game.json"), "utf8"),
) as unknown;
const DEFAULT_PROPS: Record<string, unknown> = JSON.parse(
    readFileSync(join(PACKAGE_DIR, "..", "showcase-props.json"), "utf8"),
);

// Build the client-side hydrate bundle once at startup.
const hydrateOut = join(PACKAGE_DIR, "client.bundle.js");
await esbuild({
    entryPoints: [join(PACKAGE_DIR, "client.tsx")],
    outfile: hydrateOut,
    bundle: true,
    format: "esm",
    platform: "browser",
    minify: true,
    treeShaking: true,
    legalComments: "none",
    target: ["es2020"],
    define: { "process.env.NODE_ENV": '"production"' },
    loader: { ".tsx": "tsx", ".ts": "ts" },
    jsx: "automatic",
});
const hydrateBundle = readFileSync(hydrateOut, "utf8");

const espnHydrateOut = join(PACKAGE_DIR, "espn-client.bundle.js");
await esbuild({
    entryPoints: [join(PACKAGE_DIR, "espn-client.tsx")],
    outfile: espnHydrateOut,
    bundle: true,
    format: "esm",
    platform: "browser",
    minify: true,
    treeShaking: true,
    legalComments: "none",
    target: ["es2020"],
    define: { "process.env.NODE_ENV": '"production"' },
    loader: { ".tsx": "tsx", ".ts": "ts" },
    jsx: "automatic",
});
const espnHydrateBundle = readFileSync(espnHydrateOut, "utf8");

const sentryOtelHydrateOut = join(PACKAGE_DIR, "sentry-otel-client.bundle.js");
await esbuild({
    entryPoints: [join(PACKAGE_DIR, "sentry-otel-client.tsx")],
    outfile: sentryOtelHydrateOut,
    bundle: true,
    format: "esm",
    platform: "browser",
    minify: true,
    treeShaking: true,
    legalComments: "none",
    target: ["es2020"],
    define: { "process.env.NODE_ENV": '"production"' },
    loader: { ".tsx": "tsx", ".ts": "ts" },
    jsx: "automatic",
});
const sentryOtelHydrateBundle = readFileSync(sentryOtelHydrateOut, "utf8");

// Delegates to the shared marko helper so the dataset matches what
// every other engine renders.
function buildListings(n: number): Listing[] {
    return buildListingsHelper(n) as Listing[];
}

function applyOverrides(listings: Listing[], overrides: unknown): Listing[] {
    if (!Array.isArray(overrides)) return listings;
    for (let i = 0; i < overrides.length && i < listings.length; i++) {
        const ov = overrides[i] || {};
        const listing = listings[i];
        if (typeof ov.title === "string") listing.title = ov.title;
        if (typeof ov.subtitle === "string") {
            listing.subtitleParts = ov.subtitle
                .split(/[·|]/)
                .map((part: string) => part.trim())
                .filter(Boolean);
        }
        if (typeof ov.imageHash === "string") listing.imageHash = ov.imageHash;
        if (typeof ov.priceCents === "number") listing.priceCents = ov.priceCents;
        if (Object.prototype.hasOwnProperty.call(ov, "originalPriceCents")) {
            listing.originalPriceCents =
                ov.originalPriceCents == null ? null : Number(ov.originalPriceCents);
        }
        if (typeof ov.freeDelivery === "boolean" && ov.freeDelivery) listing.deliveryCents = 0;
        if (typeof ov.deliveryCents === "number") listing.deliveryCents = ov.deliveryCents;
        if (typeof ov.location === "string") listing.location = ov.location;
        if (typeof ov.watcherCount === "number") listing.watcherCount = ov.watcherCount;
        if (Object.prototype.hasOwnProperty.call(ov, "couponPct")) {
            listing.couponPct = ov.couponPct == null ? null : Number(ov.couponPct);
        }
        if (typeof ov.starsFull === "number") listing.starsFull = Math.min(5, ov.starsFull);
        if (typeof ov.hasHalfStar === "boolean") listing.hasHalfStar = ov.hasHalfStar;
        if (typeof ov.reviewCount === "number") listing.reviewCount = ov.reviewCount;
        if (typeof ov.format === "string") listing.format = ov.format;
        if (typeof ov.caption === "string") listing.caption = ov.caption === "None" ? null : ov.caption;
        if (typeof ov.sellerName === "string") listing.sellerName = ov.sellerName;
        if (typeof ov.sellerFeedbackPctX10 === "number") {
            listing.sellerFeedbackPctX10 = ov.sellerFeedbackPctX10;
        }
        if (typeof ov.sellerFeedbackCount === "number") {
            listing.sellerFeedbackCount = ov.sellerFeedbackCount;
        }
    }
    return listings;
}

function buildListingsFromProps(props: { count?: number; listings?: unknown } | undefined): Listing[] {
    const count = Math.max(1, Math.min(1000, Number(props?.count) || 60));
    return applyOverrides(buildListings(count), props?.listings);
}

function readBody(req: import("node:http").IncomingMessage): Promise<string> {
    return new Promise((resolve, reject) => {
        const chunks: Buffer[] = [];
        req.on("data", (c) => chunks.push(c));
        req.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
        req.on("error", reject);
    });
}

function SrpDocument({ listings, query }: { listings: Listing[]; query: string }) {
    const totalFmt = fmtCount(117_406 + listings.length * 7);
    return (
        <html lang="en">
            <head>
                <meta charSet="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>{`${query} for sale | eBay`}</title>
                <RealHeadAssets styleMain={STYLE_MAIN} styleChips={STYLE_CHIPS} />
            </head>
            <body className={BODY_CLASS}>
                <RealSrpChrome
                    query={query}
                    totalFmt={totalFmt}
                    styleMain={STYLE_MAIN}
                    styleChips={STYLE_CHIPS}
                    river={<App listings={listings} />}
                />
            </body>
        </html>
    );
}

function fullPage(listings: Listing[], query: string): string {
    const doc = renderToString(<SrpDocument listings={listings} query={query} />);
    const seed = `<script type="application/json" id="__app_data__">${JSON.stringify(listings).replace(/</g, "\\u003c")}</script>`;
    const hydrate = `<script type="module" src="/client.bundle.js"></script>`;
    // Seed + hydrate tags go just before </body> so the SSR'd DOM exists
    // before the hydration entry runs.
    return `<!DOCTYPE html>${doc.replace("</body>", `${seed}${hydrate}</body>`)}`;
}

type SharedPageConfig = {
    component: ComponentType<{ data?: unknown }>;
    title: string;
    shell: "nike" | "github";
};

const SHARED_PAGES: Record<string, SharedPageConfig> = {
    "nike-catalog": {
        component: NikeCatalogPage as ComponentType<{ data?: unknown }>,
        title: "Men's Shoes & Sneakers. Nike.com",
        shell: "nike",
    },
    "nike-pdp": {
        component: NikePdpPage as ComponentType<{ data?: unknown }>,
        title: "Nike Alphafly 3. Nike.com",
        shell: "nike",
    },
    "github-pr-diff": {
        component: GithubPrDiffPage as ComponentType<{ data?: unknown }>,
        title: "GitHub PR diff demo",
        shell: "github",
    },
};

function snakeToCamel(str: string): string {
    return str.replace(/_([a-z])/g, (_, c) => c.toUpperCase());
}

function convertKeys(obj: unknown): unknown {
    if (Array.isArray(obj)) return obj.map(convertKeys);
    if (obj && typeof obj === "object") {
        const result: Record<string, unknown> = {};
        for (const [key, value] of Object.entries(obj as Record<string, unknown>)) {
            result[snakeToCamel(key)] = convertKeys(value);
        }
        return result;
    }
    return obj;
}

function wrapInNikeShell(title: string, bodyHtml: string): string {
    return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1.0"/>
<meta name="robots" content="noindex, nofollow"/>
<title>${title}</title>
<style>${NIKE_CSS}</style>
</head><body class="nike-page">${bodyHtml}</body></html>`;
}

// Mirrors the Proa GitHub PR-diff page's own <html>/<head> (lang,
// data-pr-diff-demo marker, metas, inline stylesheet) and trailing
// interaction <script> so the two engines serve near-identical documents.
function wrapInGithubShell(title: string, bodyHtml: string): string {
    return `<!DOCTYPE html><html lang="en" data-pr-diff-demo><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${title}</title><style>${GITHUB_PR_DIFF_CSS}</style></head><body>${bodyHtml}<script>${GITHUB_PR_DIFF_JS}</script></body></html>`;
}

function defaultPropsFor(page: string): unknown {
    if (page === "github-pr-diff") return DEFAULT_GITHUB_PR_DIFF_PROPS;
    if (page === "espn-gamecast") return DEFAULT_ESPN_DATA;
    if (page === "sentry-otel-logs") return {
        logs: DEFAULT_SENTRY_LOGS,
        trace_name: "pageload",
        trace_id: "9f97da3a648f",
        issues: 2,
        spans: 102,
        age: "37m",
        root_duration: "108.00ms",
        log_count: DEFAULT_SENTRY_LOGS.length,
        client_ip: "192.0.2.42",
        browser: "Chrome 148.0.0",
        environment: "local",
    };
    return DEFAULT_PROPS[page];
}

function renderSharedPage(page: string, rawProps?: unknown): { html: string; renderUs: number; docUs: number } {
    if (page === "espn-gamecast") {
        const data = convertKeys(rawProps ?? defaultPropsFor(page)) as EspnGamecastData;
        const start = process.hrtime.bigint();
        const html = `<!DOCTYPE html>${renderToString(<EspnGamecastDocument
            css={ESPN_CSS}
            data={data}
        />)}`;
        const renderUs = Number(process.hrtime.bigint() - start) / 1000;
        return { html, renderUs, docUs: renderUs };
    }
    if (page === "sentry-otel-logs") {
        const props = convertKeys(rawProps ?? defaultPropsFor(page)) as SentryOtelProps;
        const logs = Array.isArray(props.logs) ? props.logs : DEFAULT_SENTRY_LOGS;
        const start = process.hrtime.bigint();
        const html = `<!DOCTYPE html>${renderToString(<SentryOtelDocument
            css={SENTRY_OTEL_CSS}
            logs={logs}
            traceName={String(props.traceName ?? "pageload")}
            traceId={String(props.traceId ?? "9f97da3a648f")}
            issues={Number(props.issues ?? 2)}
            spans={Number(props.spans ?? 102)}
            age={String(props.age ?? "37m")}
            rootDuration={String(props.rootDuration ?? "108.00ms")}
            logCount={Number(props.logCount ?? logs.length)}
            clientIp={String(props.clientIp ?? "192.0.2.42")}
            browser={String(props.browser ?? "Chrome 148.0.0")}
            environment={String(props.environment ?? "local")}
        />)}`;
        const renderUs = Number(process.hrtime.bigint() - start) / 1000;
        return { html, renderUs, docUs: renderUs };
    }
    const config = SHARED_PAGES[page];
    if (!config) throw new Error(`unknown page: ${page}`);
    const data = convertKeys(rawProps ?? defaultPropsFor(page));
    const wrap = config.shell === "nike" ? wrapInNikeShell : wrapInGithubShell;
    const Component = config.component;
    const start = process.hrtime.bigint();
    const bodyHtml = renderToString(<Component data={data} />);
    const renderUs = Number(process.hrtime.bigint() - start) / 1000;
    const docStart = process.hrtime.bigint();
    const html = wrap(config.title, bodyHtml);
    const docUs = renderUs + Number(process.hrtime.bigint() - docStart) / 1000;
    return { html, renderUs, docUs };
}

const server = createServer(async (req, res) => {
    const e2eStart = process.hrtime.bigint();

    // Permissive CORS so the playground iframe (which loads HTML from a
    // Blob URL with origin "null") can still fetch /client.bundle.js for
    // hydration. This server is dev-only / loopback-only, so wildcarding
    // origins is fine.
    res.setHeader("Access-Control-Allow-Origin", "*");
    res.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    res.setHeader("Access-Control-Allow-Headers", "Content-Type");
    if (req.method === "OPTIONS") {
        res.statusCode = 204;
        res.end();
        return;
    }

    if (req.method === "GET" && req.url === "/health") {
        res.statusCode = 200;
        res.setHeader("content-type", "application/json");
        res.end(JSON.stringify({ status: "ok", engine: "react-ssr" }));
        return;
    }
    if (req.method === "GET" && req.url === "/client.bundle.js") {
        res.statusCode = 200;
        res.setHeader("content-type", "application/javascript; charset=utf-8");
        res.end(hydrateBundle);
        return;
    }
    if (req.method === "GET" && req.url === "/espn-client.bundle.js") {
        res.statusCode = 200;
        res.setHeader("content-type", "application/javascript; charset=utf-8");
        res.end(espnHydrateBundle);
        return;
    }
    if (req.method === "GET" && req.url === "/sentry-otel-client.bundle.js") {
        res.statusCode = 200;
        res.setHeader("content-type", "application/javascript; charset=utf-8");
        res.end(sentryOtelHydrateBundle);
        return;
    }
    if (req.method === "GET" && req.url?.startsWith("/images/espn/")) {
        const filename = req.url.slice("/images/espn/".length).split("?")[0];
        if (!/^[a-z0-9-]+\.(?:jpg|png)$/.test(filename)) {
            res.statusCode = 400;
            res.end("bad image path");
            return;
        }
        try {
            const image = readFileSync(join(REPO_ROOT, "proa_showcase", "src", "espn", "data", "images", "espn", filename));
            res.statusCode = 200;
            res.setHeader("content-type", filename.endsWith(".png") ? "image/png" : "image/jpeg");
            res.end(image);
        } catch {
            res.statusCode = 404;
            res.end("not found");
        }
        return;
    }
    if (req.method === "GET" && req.url?.startsWith("/showcase/espn/react")) {
        const html = `<!DOCTYPE html>${renderToString(<EspnGamecastDocument css={ESPN_CSS} />)}`;
        res.statusCode = 200;
        res.setHeader("content-type", "text/html; charset=utf-8");
        res.end(html);
        return;
    }
    if (req.method === "GET" && req.url?.startsWith("/showcase/sentry-otel/react")) {
        const params = new URL(req.url, "http://localhost").searchParams;
        const rowCount = Math.max(40, Math.min(10_000, Number(params.get("rows")) || 400));
        const html = `<!DOCTYPE html>${renderToString(<SentryOtelDocument css={SENTRY_OTEL_CSS} logs={buildDefaultSentryLogs(rowCount)} />)}`;
        res.statusCode = 200;
        res.setHeader("content-type", "text/html; charset=utf-8");
        res.end(html);
        return;
    }
    if (req.method === "GET" && req.url?.startsWith("/showcase/ebay/react")) {
        const params = new URL(req.url, "http://localhost").searchParams;
        const n = Math.max(1, Math.min(1000, Number(params.get("n")) || 60));
        const query = params.get("q") || DEFAULT_QUERY;
        const listings = buildListings(n);
        const html = fullPage(listings, query);
        res.statusCode = 200;
        res.setHeader("content-type", "text/html; charset=utf-8");
        res.end(html);
        return;
    }
    if (req.method === "GET" && req.url?.startsWith("/showcase/github-pr-diff/react")) {
        const { html } = renderSharedPage("github-pr-diff");
        res.statusCode = 200;
        res.setHeader("content-type", "text/html; charset=utf-8");
        res.end(html);
        return;
    }
    if (req.method === "GET" && req.url?.startsWith("/showcase/nike/react/product")) {
        const { html } = renderSharedPage("nike-pdp");
        res.statusCode = 200;
        res.setHeader("content-type", "text/html; charset=utf-8");
        res.end(html);
        return;
    }
    if (req.method === "GET" && req.url?.startsWith("/showcase/nike/react")) {
        const { html } = renderSharedPage("nike-catalog");
        res.statusCode = 200;
        res.setHeader("content-type", "text/html; charset=utf-8");
        res.end(html);
        return;
    }
    if (req.method === "POST" && req.url?.startsWith("/api/render")) {
        const body = await readBody(req);
        let payload: { page?: string; props?: unknown } = {};
        try {
            payload = JSON.parse(body);
        } catch {
            res.statusCode = 400;
            res.end("bad json");
            return;
        }
        // React handles both proa eBay variants identically; the
        // playground sends `page: "ebay-srp"` for the static-comparison
        // route and `page: "ebay-srp-rsjs"` for the islands route, but
        // the React render is the same component tree either way.
        if (payload.page !== "ebay-srp" && payload.page !== "ebay-srp-rsjs") {
            if (
                !payload.page ||
                (!SHARED_PAGES[payload.page] &&
                    payload.page !== "espn-gamecast" &&
                    payload.page !== "sentry-otel-logs")
            ) {
                res.statusCode = 400;
                res.end("unknown page");
                return;
            }
            const deserStart = process.hrtime.bigint();
            const props = payload.props ?? defaultPropsFor(payload.page);
            const deserUs = Number(process.hrtime.bigint() - deserStart) / 1000;
            const { html, renderUs, docUs } = renderSharedPage(payload.page, props);
            const e2eUs = Number(process.hrtime.bigint() - e2eStart) / 1000;
            const htmlSize = Buffer.byteLength(html, "utf8");
            res.statusCode = 200;
            res.setHeader("Content-Type", "text/html; charset=utf-8");
            res.setHeader("x-render-us", renderUs.toFixed(2));
            res.setHeader("x-doc-us", docUs.toFixed(2));
            res.setHeader("x-deser-us", deserUs.toFixed(2));
            res.setHeader("x-e2e-us", e2eUs.toFixed(2));
            res.setHeader("x-html-size", String(htmlSize));
            res.end(html);
            return;
        }
        const deserStart = process.hrtime.bigint();
        const listings = buildListingsFromProps(payload.props as { count?: number; listings?: unknown } | undefined);
        const deserUs = Number(process.hrtime.bigint() - deserStart) / 1000;

        const docStart = process.hrtime.bigint();
        const renderStart = process.hrtime.bigint();
        const html = fullPage(listings, DEFAULT_QUERY);
        const renderUs = Number(process.hrtime.bigint() - renderStart) / 1000;
        const docUs = Number(process.hrtime.bigint() - docStart) / 1000;

        const e2eUs = Number(process.hrtime.bigint() - e2eStart) / 1000;
        const htmlSize = Buffer.byteLength(html, "utf8");
        res.statusCode = 200;
        res.setHeader("Content-Type", "text/html; charset=utf-8");
        res.setHeader("x-render-us", renderUs.toFixed(2));
        res.setHeader("x-doc-us", docUs.toFixed(2));
        res.setHeader("x-deser-us", deserUs.toFixed(2));
        res.setHeader("x-e2e-us", e2eUs.toFixed(2));
        res.setHeader("x-html-size", String(htmlSize));
        res.end(html);
        return;
    }
    res.statusCode = 404;
    res.end("not found");
});

server.listen(PORT, HOST, () => {
    console.log(`react-ssr listening on ${HOST}:${PORT}, hydrate bundle = ${hydrateBundle.length} bytes`);
});
