// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { Hono } from "hono";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { HomePage } from "../react-ssr-shared/src/home";
import { SearchPage } from "../react-ssr-shared/src/search";
import { ListingPage } from "../react-ssr-shared/src/listing";
import { NikeCatalogPage } from "../react-ssr-shared/src/nike-catalog";
import { NikePdpPage } from "../react-ssr-shared/src/nike-pdp";
import { GithubPrDiffPage, GITHUB_PR_DIFF_CSS } from "../react-ssr-shared/src/github-pr-diff";
import { wrapInDocumentShell } from "../react-ssr-shared/src/shared";
import { SHARED_CSS } from "../react-ssr-shared/src/css";
import { wrapInNikeShell } from "../react-ssr-shared/src/nike-shell";

const app = new Hono();

// ---------------------------------------------------------------------------
// snake_case → camelCase key conversion (recursive)
// ---------------------------------------------------------------------------

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

function wrapInGithubShell(title: string, bodyHtml: string): string {
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1.0"/>
<meta name="robots" content="noindex, nofollow"/>
<title>${title}</title>
<style>${GITHUB_PR_DIFF_CSS}</style>
</head><body>${bodyHtml}</body></html>`;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const PAGES: Record<string, { component: React.ComponentType<any>; title: string; shell?: "nike" | "github" }> = {
  home: { component: HomePage, title: "Airbnb - Vacation Rentals, Cabins, Beach Houses" },
  search: { component: SearchPage, title: "Malibu - Vacation Rentals | Airbnb" },
  listing: { component: ListingPage, title: "Breathtaking Malibu Ocean View Sanctuary | Airbnb" },
  "nike-catalog": { component: NikeCatalogPage, title: "Men's Shoes & Sneakers. Nike.com", shell: "nike" },
  "nike-pdp": { component: NikePdpPage, title: "Nike Alphafly 3. Nike.com", shell: "nike" },
  "github-pr-diff": { component: GithubPrDiffPage, title: "GitHub PR diff demo", shell: "github" },
};

// Number of render iterations — report the minimum to filter noise.
// Matches Proa's RENDER_ITERS methodology.
const RENDER_ITERS = 5;

// High-resolution timer in microseconds.
// process.hrtime.bigint() gives nanosecond precision on both Node and Bun,
// unlike performance.now() which has only millisecond precision on some Bun builds.
function nowUs(): number {
  return Number(process.hrtime.bigint()) / 1000;
}

// The Proa JSON props encode sections as {title, subtitle, listing_indexes}
// objects, but the React components expect [title, subtitle, indexes] tuples.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function normalizeSections(data: any): any {
  if (!data || typeof data !== "object") return data;
  const sectionKeys = ["homesSections", "experiencesSections", "servicesSections"];
  for (const key of sectionKeys) {
    if (Array.isArray(data[key])) {
      data[key] = data[key].map((s: any) =>
        Array.isArray(s) ? s : [s.title ?? "", s.subtitle ?? "", s.listingIndexes ?? []]
      );
    }
  }
  return data;
}

function renderPage(page: string, props?: unknown): { html: string; renderUs: number } {
  const config = PAGES[page];
  if (!config) throw new Error(`Unknown page: ${page}`);

  const converted = props ? convertKeys(props) : undefined;
  const componentProps = converted ? { data: normalizeSections(converted) } : {};

  const wrap = config.shell === "nike"
    ? wrapInNikeShell
    : config.shell === "github"
      ? wrapInGithubShell
      : wrapInDocumentShell;

  let minRenderUs = Infinity;
  let html = "";
  for (let i = 0; i < RENDER_ITERS; i++) {
    const start = nowUs();
    const element = createElement(config.component, componentProps);
    const bodyHtml = renderToString(element);
    html = wrap(config.title, bodyHtml);
    const us = nowUs() - start;
    if (us < minRenderUs) minRenderUs = us;
  }
  return { html, renderUs: minRenderUs };
}

// POST /api/render — matches Proa's API format
app.post("/api/render", async (c) => {
  const e2eStart = nowUs();

  // Read raw body, then measure just JSON.parse (same as Proa measures serde_json::from_str)
  const rawBody = await c.req.text();
  const deserStart = nowUs();
  let body: { page: string; props?: unknown };
  try {
    body = JSON.parse(rawBody);
  } catch {
    return c.json({ error: "Invalid JSON" }, 400);
  }
  const deserUs = nowUs() - deserStart;

  try {
    const props = body.props ?? defaultProps[body.page];
    const { html, renderUs } = renderPage(body.page, props);
    const e2eUs = nowUs() - e2eStart;
    const htmlSize = Buffer.byteLength(html, "utf8");
    return new Response(html, {
      headers: {
        "content-type": "text/html; charset=utf-8",
        "x-render-us": renderUs.toFixed(3),
        "x-deser-us": deserUs.toFixed(3),
        "x-e2e-us": e2eUs.toFixed(3),
        "x-html-size": String(htmlSize),
      },
    });
  } catch (e: unknown) {
    return c.json({ error: (e as Error).message }, 400);
  }
});

// Load default props from the shared JSON (same data the Proa engine uses).
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const defaultProps: Record<string, unknown> = JSON.parse(
  readFileSync(join(__dirname, "..", "showcase-props.json"), "utf-8"),
);

// GET pages — full SSR with default props
function getHandler(page: string) {
  return (c: any) => {
    try {
      // The Nike components' built-in fixtures are the canonical documents
      // shared with the Proa showcase and the parity benchmark. Passing the
      // older JSON fixture here silently changed the live comparison page.
      const props = page.startsWith("nike-") ? undefined : defaultProps[page];
      const { html } = renderPage(page, props);
      return c.html(html);
    } catch (e: unknown) {
      console.error(`GET /${page} error:`, (e as Error).message);
      return c.text(`Internal Server Error: ${(e as Error).message}`, 500);
    }
  };
}

// Runtime detection (used in health check and server start)
const isBun = typeof (globalThis as { Bun?: unknown }).Bun !== "undefined";

// GET /health — health check
app.get("/health", (c) => c.json({ status: "ok", engine: isBun ? "bun" : "node" }));

app.get("/", getHandler("home"));
app.get("/search", getHandler("search"));
app.get("/listing", getHandler("listing"));
app.get("/nike", getHandler("nike-catalog"));
app.get("/nike/product", getHandler("nike-pdp"));
app.get("/github-pr-diff", getHandler("github-pr-diff"));

const port = parseInt(process.env.PORT || "5100", 10);
const runtime = isBun ? "Bun" : "Node.js";
console.log(`  Hono SSR server (${runtime}) running on http://localhost:${port}`);

if (isBun) {
  // Bun has a built-in server — export the fetch handler directly
  // @ts-expect-error Bun global
  Bun.serve({ fetch: app.fetch, port });
} else {
  // Node.js — use @hono/node-server
  const { serve } = await import("@hono/node-server");
  serve({ fetch: app.fetch, port });
}
