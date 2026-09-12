// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Marko 6 SSR server for the proa_demo playground.
//
// Two compilations of the same template:
//   - `output: "html"`     — server-side render, streaming HTML
//   - `output: "hydrate"`  — client-side hydration JS (bundled with esbuild
//                             so the browser can `<script type="module">` it)
//
// Renders the eBay SRP listings template; matches the Hono Node/Bun
// servers' /api/render contract so the playground can swap engines.
//
// Env: PORT (default 5300), SHELL_DIR or the legacy MARKETPLACE_DATA_DIR
// (path to shared eBay fixture data).

import { compileFile, compile as compileSrc } from "@marko/compiler";
import { build as esbuild } from "esbuild";
import { existsSync } from "node:fs";
import { createServer } from "node:http";
import { writeFileSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";

const __dirname = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

const PORT = Number(process.env.PORT || 5300);
const RUNTIME = process.env.RUNTIME || (typeof Bun !== "undefined" ? "bun" : "node");
const SHELL_DIR = process.env.SHELL_DIR
  || process.env.MARKETPLACE_DATA_DIR
  || join(__dirname, "..", "..", "proa_showcase", "src", "ebay", "data");

const sourcePath = join(__dirname, "page_srp_real.marko");

// 1) HTML output → load as ESM, used for server render.
const htmlCompiled = await compileFile(sourcePath, {
  output: "html",
  optimize: true,
  modules: "esm",
  writeVersionComment: false,
  translator: require.resolve("marko/translator"),
  resolveVirtualDependency(_filename, dep) {
    return dep.virtualPath;
  },
});
const htmlCompiledFile = join(__dirname, "page_srp_real.compiled.mjs");
writeFileSync(htmlCompiledFile, htmlCompiled.code);
const Template = (await import(pathToFileURL(htmlCompiledFile).href)).default;

// 2) Hydrate output → bundle for the browser. esbuild resolves `marko/dom`
// and produces one self-contained ES module the browser can load directly.
const hydrateCompiled = await compileFile(sourcePath, {
  output: "hydrate",
  optimize: true,
  modules: "esm",
  writeVersionComment: false,
  translator: require.resolve("marko/translator"),
  resolveVirtualDependency(_filename, dep) {
    return dep.virtualPath;
  },
});
const hydrateInputFile = join(__dirname, "page_srp_real.hydrate.input.mjs");
writeFileSync(hydrateInputFile, hydrateCompiled.code);

// esbuild plugin: compile `.marko` imports with output:"dom" so the
// browser bundle contains the template's client-side render code, then
// hand the result back to esbuild as plain JS for further bundling.
const markoEsbuildPlugin = {
  name: "marko",
  setup(build) {
    build.onResolve({ filter: /\.marko$/ }, (args) => {
      const abs = args.path.startsWith("/")
        ? args.path
        : join(args.resolveDir, args.path);
      return { path: abs, namespace: "marko" };
    });
    build.onLoad({ filter: /.*/, namespace: "marko" }, async (args) => {
      if (!existsSync(args.path)) {
        return { errors: [{ text: `marko plugin: ${args.path} not found` }] };
      }
      const out = await compileFile(args.path, {
        output: "dom",
        optimize: true,
        modules: "esm",
        writeVersionComment: false,
        translator: require.resolve("marko/translator"),
        resolveVirtualDependency: (_f, dep) => dep.virtualPath,
      });
      return {
        contents: out.code,
        loader: "js",
        resolveDir: dirname(args.path),
      };
    });
  },
};

const hydrateBundleFile = join(__dirname, "page_srp_real.hydrate.bundle.mjs");
await esbuild({
  entryPoints: [hydrateInputFile],
  outfile: hydrateBundleFile,
  bundle: true,
  format: "esm",
  platform: "browser",
  minify: true,
  treeShaking: true,
  legalComments: "none",
  target: ["es2020"],
  plugins: [markoEsbuildPlugin],
});
const hydrateBundle = readFileSync(hydrateBundleFile, "utf8");

// Page styles: the captured page's own stylesheet (extracted by
// scripts/gen_srp_chrome.py) plus the chips toolbar stylesheet. The hydrate
// <script> tag lives inside the template, which renders the full document
// per request (no pre-rendered shell strings).
const STYLE_MAIN = readFileSync(join(SHELL_DIR, "srp_real_style.css"), "utf8");
const STYLE_CHIPS = readFileSync(join(SHELL_DIR, "srp_chips_style.css"), "utf8");

import { buildListings as defaultBuildListings, DEFAULT_QUERY, fmtCount } from "./helpers_srp_real.mjs";

function applyOverrides(listings, overrides) {
  if (!Array.isArray(overrides)) return listings;
  for (let i = 0; i < overrides.length && i < listings.length; i++) {
    const ov = overrides[i] || {};
    const l = listings[i];
    if (typeof ov.title === "string") l.title = ov.title;
    if (typeof ov.subtitle === "string") {
      l.subtitleParts = ov.subtitle.split(/[·|]/).map((s) => s.trim()).filter(Boolean);
    }
    if (typeof ov.imageHash === "string") l.imageHash = ov.imageHash;
    if (typeof ov.priceCents === "number") l.priceCents = ov.priceCents;
    if (Object.prototype.hasOwnProperty.call(ov, "originalPriceCents")) {
      l.originalPriceCents = ov.originalPriceCents == null ? null : Number(ov.originalPriceCents);
    }
    if (typeof ov.freeDelivery === "boolean" && ov.freeDelivery) l.deliveryCents = 0;
    if (typeof ov.deliveryCents === "number") l.deliveryCents = ov.deliveryCents;
    if (typeof ov.location === "string") l.location = ov.location;
    if (typeof ov.watcherCount === "number") l.watcherCount = ov.watcherCount;
    if (Object.prototype.hasOwnProperty.call(ov, "couponPct")) {
      l.couponPct = ov.couponPct == null ? null : Number(ov.couponPct);
    }
    if (typeof ov.starsFull === "number") l.starsFull = Math.min(5, ov.starsFull);
    if (typeof ov.hasHalfStar === "boolean") l.hasHalfStar = ov.hasHalfStar;
    if (typeof ov.reviewCount === "number") l.reviewCount = ov.reviewCount;
    if (typeof ov.format === "string") l.format = ov.format;
    if (typeof ov.caption === "string") l.caption = ov.caption === "None" ? null : ov.caption;
    if (typeof ov.sellerName === "string") l.sellerName = ov.sellerName;
    if (typeof ov.sellerFeedbackPctX10 === "number") l.sellerFeedbackPctX10 = ov.sellerFeedbackPctX10;
    if (typeof ov.sellerFeedbackCount === "number") l.sellerFeedbackCount = ov.sellerFeedbackCount;
  }
  return listings;
}

function buildListingsFromProps(props) {
  const count = Math.max(1, Math.min(1000, Number(props.count) || 60));
  const arr = defaultBuildListings(count);
  if (Array.isArray(props.listings)) applyOverrides(arr, props.listings);
  return arr;
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    req.on("error", reject);
  });
}

function fullPage(listings, query) {
  const doc = Template.render({
    listings,
    query,
    totalFmt: fmtCount(117_406 + listings.length * 7),
    styleMain: STYLE_MAIN,
    styleChips: STYLE_CHIPS,
  }).toString();
  return `<!DOCTYPE html>${doc}`;
}

const server = createServer(async (req, res) => {
  const e2eStart = process.hrtime.bigint();

  if (req.method === "GET" && req.url === "/health") {
    res.statusCode = 200;
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify({ status: "ok", engine: "marko", runtime: RUNTIME }));
    return;
  }

  // Serve the bundled hydrate JS.
  if (req.method === "GET" && req.url === "/marko-hydrate.js") {
    res.statusCode = 200;
    res.setHeader("content-type", "application/javascript; charset=utf-8");
    res.end(hydrateBundle);
    return;
  }

  // Full HTML page (the GET equivalent of /api/render with a default props
  // payload). Used for browser smoke tests + bundle-size measurement.
  // `?n=` mirrors eBay's items-per-page options; `?q=` seeds the query echo.
  if (req.method === "GET" && req.url.startsWith("/showcase/ebay/marko")) {
    const params = new URL(req.url, "http://localhost").searchParams;
    const n = Math.max(1, Math.min(1000, Number(params.get("n")) || 60));
    const query = params.get("q") || DEFAULT_QUERY;
    const listings = buildListingsFromProps({ count: n });
    res.statusCode = 200;
    res.setHeader("content-type", "text/html; charset=utf-8");
    res.end(fullPage(listings, query));
    return;
  }

  if (req.method !== "POST" || !req.url.startsWith("/api/render")) {
    res.statusCode = 404;
    res.end("not found");
    return;
  }
  const body = await readBody(req);
  let payload;
  try {
    payload = JSON.parse(body);
  } catch {
    res.statusCode = 400;
    res.end("bad json");
    return;
  }
  if (payload.page !== "ebay-srp" && payload.page !== "ebay-srp-rsjs") {
    res.statusCode = 400;
    res.end("unknown page");
    return;
  }

  const props = (payload && payload.props) || {};
  const listings = buildListingsFromProps(props);

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
  res.setHeader("x-deser-us", "0");
  res.setHeader("x-e2e-us", e2eUs.toFixed(2));
  res.setHeader("x-html-size", String(htmlSize));
  res.end(html);
});

server.listen(PORT, () => {
  console.log(`marko (${RUNTIME}) listening on :${PORT}, hydrate bundle = ${hydrateBundle.length} bytes`);
});
