// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Lean Marko 6 SSR server (port 5502).
//
// Mirrors the proa-lean-server shape: serves a lean HTML page with N
// `<button>` toggles plus a Marko hydrate bundle. Install this package's
// compiler/esbuild dependencies with pnpm before starting the server.
//
// Routes:
//   GET /            → lean page with N toggles
//   GET /health      → {"status":"ok","engine":"marko-lean"}
//   GET /lean-hydrate.js → bundled hydrate JS

import { compileFile } from "@marko/compiler";
import { build as esbuild } from "esbuild";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";

const __dirname = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

const PORT = Number(process.env.PORT || 5502);
const sourcePath = join(__dirname, "lean.marko");

// 1) HTML output — server render.
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
const htmlCompiledFile = join(__dirname, "lean.compiled.mjs");
writeFileSync(htmlCompiledFile, htmlCompiled.code);
const Template = (await import(pathToFileURL(htmlCompiledFile).href)).default;

// 2) Hydrate output — bundle for the browser via esbuild.
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
const hydrateInputFile = join(__dirname, "lean.hydrate.input.mjs");
writeFileSync(hydrateInputFile, hydrateCompiled.code);

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
            return { contents: out.code, loader: "js", resolveDir: dirname(args.path) };
        });
    },
};

const hydrateBundleFile = join(__dirname, "lean.hydrate.bundle.mjs");
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

const PAGE_CSS =
    "*{box-sizing:border-box}body{margin:0;padding:16px;font:14px system-ui,sans-serif;background:#fafafa}.lean-bench{display:grid;grid-template-columns:repeat(auto-fill,minmax(72px,1fr));gap:8px;max-width:960px;margin:0 auto}.lean-bench>*{display:flex;align-items:center;justify-content:center;background:#fff;border:1px solid #e5e5e5;border-radius:6px;padding:12px;min-height:48px}button.lean-watch{background:transparent;border:0;cursor:pointer;font-size:24px;line-height:1;padding:4px;color:#c00}";

function buildPage(n) {
    const indices = Array.from({ length: n }, (_, i) => i);
    const inner = Template.render({ indices }).toString();
    return `<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">\
<meta name="viewport" content="width=device-width,initial-scale=1">\
<title>marko lean bench (n=${n})</title><style>${PAGE_CSS}</style></head>\
<body><main class="lean-bench">${inner}</main>\
<script type="module" src="/lean-hydrate.js"></script></body></html>`;
}

const server = createServer((req, res) => {
    const url = new URL(req.url, `http://localhost:${PORT}`);
    if (req.method === "GET" && url.pathname === "/health") {
        res.statusCode = 200;
        res.setHeader("content-type", "application/json");
        res.end(JSON.stringify({ status: "ok", engine: "marko-lean" }));
        return;
    }
    if (req.method === "GET" && url.pathname === "/lean-hydrate.js") {
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
    console.log(`marko-lean-server listening on :${PORT}, hydrate bundle = ${hydrateBundle.length} bytes`);
});
