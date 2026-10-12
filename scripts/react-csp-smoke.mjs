#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { randomBytes } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";

const require = createRequire(import.meta.url);
const workspaceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const debug = process.argv.includes("--debug");

async function loadPlaywright() {
  try {
    return await import("playwright");
  } catch {
    return await import(pathToFileURL(require.resolve("playwright")).href);
  }
}

function packageFile(packageName, relativePath) {
  // Scheduler belongs to react-dom's dependency graph. Resolve it there so
  // strict package managers do not need to hoist a transitive dependency.
  const packageRequire = packageName === "scheduler"
    ? createRequire(require.resolve("react-dom/package.json", { paths: [workspaceRoot] }))
    : require;
  const packageJson = packageRequire.resolve(`${packageName}/package.json`);
  return resolve(dirname(packageJson), relativePath);
}

function renderFixture(nonce) {
  const result = spawnSync(
    "cargo",
    [
      "run",
      "--quiet",
      "-p",
      "proa_react",
      "--example",
      "react_csp_fixture",
      "--",
      nonce,
    ],
    {
      cwd: workspaceRoot,
      encoding: "utf8",
      env: {
        ...process.env,
        CARGO_TARGET_DIR:
          process.env.CARGO_TARGET_DIR ??
          resolve(tmpdir(), "proa-react-csp-smoke-target"),
      },
    },
  );

  if (result.status !== 0) {
    throw new Error(
      `failed to render React CSP fixture:\n${result.stderr || result.stdout}`,
    );
  }
  return result.stdout;
}

function wrapCommonJs(source, imports = "", namedExports = []) {
  const exports = namedExports
    .map((name) => `export const ${name} = cjsExports.${name};`)
    .join("\n");
  return `${imports}
const cjsExports = {};
const exports = cjsExports;
${source}
export default cjsExports;
${exports}
`;
}

function csp(nonce) {
  return [
    "default-src 'none'",
    `script-src 'nonce-${nonce}' 'strict-dynamic'`,
    "style-src 'unsafe-inline'",
    "img-src 'none'",
    "connect-src 'none'",
    "font-src 'none'",
    "object-src 'none'",
    "base-uri 'none'",
    "frame-ancestors 'none'",
  ].join("; ");
}

function send(response, status, contentType, body, headers = {}) {
  response.writeHead(status, {
    "content-type": contentType,
    "cache-control": "no-store",
    ...headers,
  });
  response.end(body);
}

async function buildAssets() {
  const [react, scheduler, reactDom, reactDomClient] = await Promise.all([
    readFile(packageFile("react", "cjs/react.production.js"), "utf8"),
    readFile(packageFile("scheduler", "cjs/scheduler.production.js"), "utf8"),
    readFile(packageFile("react-dom", "cjs/react-dom.production.js"), "utf8"),
    readFile(
      packageFile("react-dom", "cjs/react-dom-client.production.js"),
      "utf8",
    ),
  ]);

  return new Map([
    ["/static/react.js", wrapCommonJs(react)],
    ["/static/scheduler.js", wrapCommonJs(scheduler)],
    [
      "/static/react-dom.js",
      wrapCommonJs(
        reactDom,
        `import ReactModule from "/static/react.js";
const require = (name) => {
  if (name === "react") return ReactModule;
  throw new Error(\`unexpected ReactDOM dependency: \${name}\`);
};`,
      ),
    ],
    [
      "/static/react-dom-client.js",
      wrapCommonJs(
        reactDomClient,
        `import SchedulerModule from "/static/scheduler.js";
import ReactModule from "/static/react.js";
import ReactDOMModule from "/static/react-dom.js";
const require = (name) => {
  if (name === "scheduler") return SchedulerModule;
  if (name === "react") return ReactModule;
  if (name === "react-dom") return ReactDOMModule;
  throw new Error(\`unexpected ReactDOM client dependency: \${name}\`);
};`,
        ["createRoot", "hydrateRoot", "version"],
      ),
    ],
    [
      "/static/counter.js",
      `import React from "react";
window.__proaReactVersion = React.version;
export default function Counter({ count }) {
  const [value, setValue] = React.useState(count);
  return React.createElement(
    "button",
    {
      id: "counter-button",
      type: "button",
      onClick: () => setValue((current) => current + 1),
    },
    \`Count: \${value}\`,
  );
}`,
    ],
  ]);
}

function inspectCsp(policy, nonce, scripts, violations, label) {
  const scriptDirective = policy
    .split(";")
    .map((directive) => directive.trim())
    .find((directive) => directive.startsWith("script-src "));
  if (!scriptDirective?.includes(`'nonce-${nonce}'`)) {
    throw new Error(`${label} CSP does not contain its response nonce: ${policy}`);
  }
  if (!scriptDirective.includes("'strict-dynamic'")) {
    throw new Error(`${label} CSP is missing 'strict-dynamic': ${policy}`);
  }
  const mismatched = scripts.filter((script) => script.nonce !== nonce);
  if (mismatched.length > 0) {
    throw new Error(
      `${label} contains scripts without the response nonce:\n${JSON.stringify(
        mismatched,
        null,
        2,
      )}`,
    );
  }
  if (violations.length > 0) {
    throw new Error(
      `${label} reported CSP violations:\n${JSON.stringify(violations, null, 2)}`,
    );
  }
}

async function exercise(page, url, expectedNonce, label, errors) {
  await page.addInitScript(() => {
    window.__proaCspViolations = [];
    document.addEventListener("securitypolicyviolation", (event) => {
      window.__proaCspViolations.push({
        blockedURI: event.blockedURI,
        effectiveDirective: event.effectiveDirective,
        violatedDirective: event.violatedDirective,
      });
    });
  });

  const response = await page.goto(url, { waitUntil: "networkidle" });
  if (!response?.ok()) {
    throw new Error(`${label} returned ${response?.status() ?? "no response"}`);
  }
  try {
    await page.waitForSelector(
      '[data-island="Counter"][data-hydrated="true"]',
    );
  } catch (error) {
    const diagnostic = await page.evaluate(() => ({
      html: document.documentElement.outerHTML,
      reactVersion: window.__proaReactVersion,
      violations: window.__proaCspViolations,
    }));
    throw new Error(
      `${label} did not hydrate: ${error.message}\n${JSON.stringify(
        diagnostic,
        null,
        2,
      )}\n${errors.join("\n")}`,
    );
  }

  const button = page.locator("#counter-button");
  if ((await button.textContent()) !== "Count: 1") {
    throw new Error(`${label} did not preserve the server-rendered count`);
  }
  await button.click();
  await page.waitForFunction(
    () => document.querySelector("#counter-button")?.textContent === "Count: 2",
  );

  const browserState = await page.evaluate(() => ({
    reactVersion: window.__proaReactVersion,
    scripts: [...document.scripts].map((node) => ({
      type: node.type,
      id: node.id,
      nonce: node.nonce,
      src: node.src,
    })),
    violations: window.__proaCspViolations,
  }));
  const policy = response.headers()["content-security-policy"] ?? "";

  if (!browserState.reactVersion?.startsWith("19.")) {
    throw new Error(
      `${label} did not execute the installed React 19 runtime: ${browserState.reactVersion}`,
    );
  }
  inspectCsp(
    policy,
    expectedNonce,
    browserState.scripts,
    browserState.violations,
    label,
  );

  return { policy, nonce: expectedNonce, ...browserState };
}

async function main() {
  const assets = await buildAssets();
  const firstNonce = randomBytes(16).toString("base64url");
  const secondNonce = randomBytes(16).toString("base64url");
  const pages = new Map([
    ["/first", { nonce: firstNonce, html: renderFixture(firstNonce) }],
    ["/second", { nonce: secondNonce, html: renderFixture(secondNonce) }],
  ]);
  const requests = [];

  const server = createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://localhost");
    requests.push(url.pathname);
    const fixture = pages.get(url.pathname);
    if (fixture) {
      send(response, 200, "text/html; charset=utf-8", fixture.html, {
        "content-security-policy": csp(fixture.nonce),
      });
      return;
    }
    const asset = assets.get(url.pathname);
    if (asset) {
      send(response, 200, "text/javascript; charset=utf-8", asset);
      return;
    }
    send(response, 404, "text/plain; charset=utf-8", "not found");
  });

  await new Promise((resolveListen, rejectListen) => {
    server.once("error", rejectListen);
    server.listen(0, "127.0.0.1", resolveListen);
  });
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("failed to allocate the React CSP smoke port");
  }
  const origin = `http://127.0.0.1:${address.port}`;

  const playwright = await loadPlaywright();
  const { chromium } = playwright.default ?? playwright;
  const browser = await chromium.launch({ headless: !debug });
  const context = await browser.newContext();
  const page = await context.newPage();
  const errors = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(`console: ${message.text()}`);
  });
  page.on("pageerror", (error) => errors.push(`page: ${error.message}`));
  page.on("requestfailed", (request) =>
    errors.push(
      `request: ${request.url()} (${request.failure()?.errorText ?? "failed"})`,
    ),
  );

  try {
    const first = await exercise(
      page,
      `${origin}/first`,
      firstNonce,
      "first response",
      errors,
    );
    const second = await exercise(
      page,
      `${origin}/second`,
      secondNonce,
      "second response",
      errors,
    );

    if (first.nonce === second.nonce) {
      throw new Error("React CSP smoke reused a nonce across responses");
    }
    if (errors.length > 0) {
      throw new Error(`browser errors:\n${errors.join("\n")}`);
    }
    const unexpected = requests.filter(
      (pathname) => !pages.has(pathname) && !assets.has(pathname),
    );
    if (unexpected.length > 0) {
      throw new Error(`unexpected local requests: ${unexpected.join(", ")}`);
    }

    console.log(
      JSON.stringify(
        {
          ok: true,
          reactVersion: second.reactVersion,
          hydrated: true,
          clicked: true,
          strictDynamic: true,
          distinctResponseNonces: true,
          cspViolations: 0,
          externalRequests: 0,
          requests: [...new Set(requests)],
        },
        null,
        2,
      ),
    );
  } finally {
    await browser.close();
    await new Promise((resolveClose) => server.close(resolveClose));
  }
}

main().catch((error) => {
  console.error(error.stack ?? error);
  process.exitCode = 1;
});
