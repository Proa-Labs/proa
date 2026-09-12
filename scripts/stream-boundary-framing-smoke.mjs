#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(import.meta.url);
const workspaceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const nonce = "c3RyZWFtLWJvdW5kYXJ5LWZyYW1pbmc=";

async function loadPlaywright() {
  try {
    return await import("playwright");
  } catch {
    return await import(pathToFileURL(require.resolve("playwright")).href);
  }
}

function chromeExecutablePath() {
  const candidates = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
  ];
  return candidates.find((candidate) => existsSync(candidate));
}

function renderFixture() {
  const result = spawnSync(
    "cargo",
    [
      "run",
      "--quiet",
      "-p",
      "proa_stream",
      "--example",
      "boundary_framing_fixture",
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
          resolve(tmpdir(), "proa-stream-boundary-smoke-target"),
      },
    },
  );

  if (result.status !== 0) {
    throw new Error(
      `failed to render boundary framing fixture:\n${result.stderr || result.stdout}`,
    );
  }

  let runtime = null;
  const chunks = new Map();
  for (const line of result.stdout.trimEnd().split("\n")) {
    const [kind, id, ...rest] = line.split("\t");
    if (kind === "R") {
      runtime = [id, ...rest].join("\t");
    } else if (kind === "C") {
      chunks.set(Number.parseInt(id, 10), rest.join("\t"));
    } else {
      throw new Error(`unknown fixture record: ${line}`);
    }
  }
  if (runtime == null || chunks.size !== 5) {
    throw new Error(
      `incomplete fixture output: runtime=${runtime != null}, chunks=${chunks.size}`,
    );
  }
  return { runtime, chunks };
}

function csp() {
  return [
    "default-src 'none'",
    `script-src 'nonce-${nonce}'`,
    "style-src 'none'",
    "img-src 'none'",
    "connect-src 'none'",
    "font-src 'none'",
    "object-src 'none'",
    "base-uri 'none'",
    "frame-ancestors 'none'",
  ].join("; ");
}

function boundarySlot(id) {
  return `<div id="slot-${id}"><!--proa:start:${id}--><p data-fallback="${id}">loading ${id}</p><!--proa:end:${id}--></div>`;
}

function shell(runtime) {
  return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Proa boundary framing smoke</title>
<script nonce="${nonce}">
globalThis.__payloadScriptExecutions = 0;
globalThis.__boundaryReady = [];
globalThis.__islandSeed = null;
document.addEventListener("proa:boundary-ready", (event) => {
  globalThis.__boundaryReady.push(event.detail.id);
  if (event.detail.id === 4) {
    const seed = document.querySelector(
      'script[type="application/json"][data-rsjs-island="fixture_island"]',
    );
    globalThis.__islandSeed = seed == null ? null : JSON.parse(seed.textContent);
  }
});
</script>
${runtime}
</head>
<body>
<main id="boundary-root">
${[1, 2, 3, 4, 5].map(boundarySlot).join("\n")}
<div id="slot-99"><!--proa:start:99--><p id="wrong-encoding-fallback">wrong encoding fallback</p><!--proa:end:99--></div>
<div id="outside-sentinel">outside remains stable</div>
</main>
<template id="proa-tpl-99" data-proa-encoding="base64-v0"><div id="wrong-encoding-live">must remain inert</div></template>
<script nonce="${nonce}">$proa(99);</script>
`;
}

async function startFixtureServer(fixture) {
  const order = [2, 1, 4, 3, 5];
  const server = createServer((request, response) => {
    if (request.url !== "/") {
      response.writeHead(404).end("not found");
      return;
    }

    response.writeHead(200, {
      "content-type": "text/html; charset=utf-8",
      "content-security-policy": csp(),
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
    });
    response.write(shell(fixture.runtime));

    let index = 0;
    const writeNext = () => {
      if (index === order.length) {
        response.end("</body></html>");
        return;
      }
      const id = order[index];
      index += 1;
      response.write(fixture.chunks.get(id));
      setTimeout(writeNext, 20);
    };
    setTimeout(writeNext, 150);
  });

  await new Promise((resolveListen, rejectListen) => {
    server.once("error", rejectListen);
    server.listen(0, "127.0.0.1", resolveListen);
  });
  const address = server.address();
  if (address == null || typeof address === "string") {
    throw new Error("fixture server did not expose a TCP address");
  }
  return { server, url: `http://127.0.0.1:${address.port}/` };
}

function assert(condition, message, details) {
  if (!condition) {
    throw new Error(
      `${message}${details === undefined ? "" : `:\n${JSON.stringify(details, null, 2)}`}`,
    );
  }
}

async function main() {
  const fixture = renderFixture();
  const { server, url } = await startFixtureServer(fixture);
  let browser = null;

  try {
    const playwright = await loadPlaywright();
    const { chromium } = playwright.default ?? playwright;
    const executablePath = chromeExecutablePath();
    browser = await chromium.launch({
      headless: true,
      ...(executablePath ? { executablePath } : {}),
    });
    const page = await browser.newPage();
    const consoleErrors = [];
    page.on("console", (message) => {
      if (message.type() === "error") {
        consoleErrors.push(message.text());
      }
    });
    await page.addInitScript(() => {
      globalThis.__cspViolations = [];
      document.addEventListener("securitypolicyviolation", (event) => {
        globalThis.__cspViolations.push({
          blockedURI: event.blockedURI,
          effectiveDirective: event.effectiveDirective,
        });
      });
    });

    const response = await page.goto(url, { waitUntil: "commit" });
    assert(response != null, "streaming navigation returned no response");
    await page.locator('[data-fallback="1"]').waitFor({
      state: "visible",
      timeout: 1_000,
    });
    const observedFallback = await page.locator('[data-fallback="1"]').textContent();

    await page.waitForFunction(
      () => globalThis.__boundaryReady?.length === 5,
      undefined,
      { timeout: 5_000 },
    );
    await page.waitForLoadState("load");

    const result = await page.evaluate(() => {
      const inSlot = (selector, slot) =>
        document.querySelector(selector)?.closest(`#slot-${slot}`)?.id === `slot-${slot}`;
      const innerTemplate = document.querySelector("#inner-template");
      return {
        ready: globalThis.__boundaryReady,
        payloadScriptExecutions: globalThis.__payloadScriptExecutions,
        islandSeed: globalThis.__islandSeed,
        fallbacks: document.querySelectorAll("[data-fallback]").length,
        unicode: document.querySelector("#unicode-copy")?.textContent,
        nestedTemplateText: innerTemplate?.content.textContent,
        afterBalancedInSlot: inSlot("#after-balanced-template", 1),
        rawLowerInSlot: inSlot("#raw-after-lower", 2),
        rawMixedInSlot: inSlot("#raw-after-mixed", 2),
        rawTabInSlot: inSlot("#raw-after-tab", 2),
        rawNewlineInSlot: inSlot("#raw-after-newline", 2),
        sanitizedInSlot: inSlot("#sanitized-cms", 3),
        islandInSlot: inSlot('[data-rsjs-island-root="fixture_island"]', 4),
        escapeAttemptInSlot: inSlot("#escape-attempt", 5),
        afterEscapeInSlot: inSlot("#after-escape-attempt", 5),
        escapeAttemptCount: document.querySelectorAll("#escape-attempt").length,
        outsideSentinel: document.querySelector("#outside-sentinel")?.textContent,
        wrongEncodingFallback:
          document.querySelector("#wrong-encoding-fallback")?.textContent,
        wrongEncodingLive: document.querySelector("#wrong-encoding-live") != null,
        remainingTransportIds: Array.from(
          document.querySelectorAll('template[id^="proa-tpl-"]'),
          (node) => node.id,
        ),
        scriptNonces: Array.from(document.scripts, (script) => ({
          type: script.type,
          nonce: script.nonce,
          payload: script.textContent.includes("__payloadScriptExecutions"),
        })),
        cspViolations: globalThis.__cspViolations,
      };
    });

    assert(observedFallback?.trim() === "loading 1", "shell fallback was not observed");
    assert(
      JSON.stringify(result.ready) === JSON.stringify([2, 1, 4, 3, 5]),
      "multiple boundaries did not preserve streamed ready order",
      result,
    );
    assert(result.fallbacks === 0, "resolved fallbacks remain in the DOM", result);
    assert(
      result.payloadScriptExecutions === 0,
      "a script parsed from the inert payload unexpectedly executed",
      result,
    );
    assert(
      result.islandSeed?.message === "streamed seed" &&
        result.islandSeed?.emoji === "🧭",
      "island state was unavailable when boundary-ready fired",
      result,
    );
    assert(
      result.unicode === "Grüße, 東京 🌊",
      "Unicode payload changed during framing",
      result,
    );
    assert(
      result.nestedTemplateText === "nested template content" &&
        result.afterBalancedInSlot,
      "balanced nested template content did not survive the swap",
      result,
    );
    assert(
      result.rawLowerInSlot &&
        result.rawMixedInSlot &&
        result.rawTabInSlot &&
        result.rawNewlineInSlot,
      "an unmatched closing-template variant escaped the raw CMS boundary",
      result,
    );
    assert(
      result.sanitizedInSlot && result.islandInSlot,
      "sanitized CMS or island markup was not inserted in its boundary",
      result,
    );
    assert(
      result.escapeAttemptInSlot &&
        result.afterEscapeInSlot &&
        result.escapeAttemptCount === 1 &&
        result.outsideSentinel === "outside remains stable",
      "adversarial closing-tag payload escaped the intended replacement",
      result,
    );
    assert(
      result.wrongEncodingFallback === "wrong encoding fallback" &&
        result.wrongEncodingLive === false &&
        JSON.stringify(result.remainingTransportIds) === JSON.stringify(["proa-tpl-99"]),
      "the runtime accepted an unversioned transport or exposed its payload",
      result,
    );
    assert(
      result.scriptNonces.every((script) => script.nonce === nonce),
      "strict-CSP document contains a script without the response nonce",
      result,
    );
    assert(
      result.cspViolations.length === 0 && consoleErrors.length === 0,
      "browser reported CSP or console errors",
      { ...result, consoleErrors },
    );

    console.log(
      JSON.stringify(
        {
          ok: true,
          observedFallback: observedFallback.trim(),
          ready: result.ready,
          unicode: result.unicode,
          payloadScriptExecutions: result.payloadScriptExecutions,
          islandSeed: result.islandSeed,
          remainingGuardedTransport: result.remainingTransportIds,
        },
        null,
        2,
      ),
    );
  } finally {
    await browser?.close();
    await new Promise((resolveClose) => server.close(resolveClose));
  }
}

main().catch((error) => {
  console.error(error?.stack || error);
  process.exit(1);
});
