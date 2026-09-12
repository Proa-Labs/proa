// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Lean React 19 SSR server (port 5503). Renders `<App n=N />` once per
// request, ships the React client bundle for hydration, and nothing
// else. Mirrors proa-lean-server / marko-lean-server.

import { build as esbuild } from "esbuild";
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { renderToString } from "react-dom/server";
import { App } from "./page.tsx";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PORT = Number(process.env.PORT || 5503);

const hydrateOut = join(__dirname, "client.bundle.js");
await esbuild({
    entryPoints: [join(__dirname, "client.tsx")],
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

const PAGE_CSS =
    "*{box-sizing:border-box}body{margin:0;padding:16px;font:14px system-ui,sans-serif;background:#fafafa}.lean-bench{display:grid;grid-template-columns:repeat(auto-fill,minmax(72px,1fr));gap:8px;max-width:960px;margin:0 auto}.lean-bench>*{display:flex;align-items:center;justify-content:center;background:#fff;border:1px solid #e5e5e5;border-radius:6px;padding:12px;min-height:48px}button.lean-watch{background:transparent;border:0;cursor:pointer;font-size:24px;line-height:1;padding:4px;color:#c00}";

function buildPage(n: number): string {
    const inner = renderToString(<App n={n} />);
    return `<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">\
<meta name="viewport" content="width=device-width,initial-scale=1">\
<title>react lean bench (n=${n})</title><style>${PAGE_CSS}</style></head>\
<body><main id="__lean_root__" class="lean-bench">${inner}</main>\
<script type="application/json" id="__lean_data__">${n}</script>\
<script type="module" src="/client.bundle.js"></script></body></html>`;
}

const server = createServer(async (req, res) => {
    if (!req.url) {
        res.statusCode = 400;
        res.end("bad request");
        return;
    }
    const url = new URL(req.url, `http://localhost:${PORT}`);
    res.setHeader("Access-Control-Allow-Origin", "*");
    if (req.method === "GET" && url.pathname === "/health") {
        res.statusCode = 200;
        res.setHeader("content-type", "application/json");
        res.end(JSON.stringify({ status: "ok", engine: "react-lean" }));
        return;
    }
    if (req.method === "GET" && url.pathname === "/client.bundle.js") {
        res.statusCode = 200;
        res.setHeader("content-type", "application/javascript; charset=utf-8");
        res.end(hydrateBundle);
        return;
    }
    if (req.method === "GET" && url.pathname === "/") {
        const n = Math.min(1000, Math.max(1, Number(url.searchParams.get("n")) || 60));
        const html = buildPage(n);
        res.statusCode = 200;
        res.setHeader("content-type", "text/html; charset=utf-8");
        res.end(html);
        return;
    }
    res.statusCode = 404;
    res.end("not found");
});

server.listen(PORT, () => {
    console.log(`react-lean-server listening on :${PORT}, hydrate bundle = ${hydrateBundle.length} bytes`);
});
