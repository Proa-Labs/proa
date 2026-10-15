#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { setTimeout } from "node:timers/promises";

const base = new URL(process.argv[2] || "http://127.0.0.1:5000");
async function request(path, options) {
  const response = await fetch(new URL(path, base), {
    ...options, signal: AbortSignal.timeout(30_000),
  });
  assert.equal(response.status, 200, `${path}: HTTP ${response.status}`);
  return response;
}

let healthy = false;
for (let attempt = 0; attempt < 120; attempt++) {
  try {
    await request("/healthz");
    healthy = true;
    break;
  } catch {
    await setTimeout(500);
  }
}
assert.ok(healthy, "release image must serve /healthz within 60 seconds");
if (process.argv[3]) {
  assert.match(process.argv[3], /^[0-9a-f]{40}$/);
  const ready = await request("/readyz");
  assert.equal(ready.headers.get("x-proa-revision"), process.argv[3], "readyz must identify the packaged commit");
  assert.equal(ready.headers.get("cache-control"), "no-store");
}

const assets = new Set();
for (const path of ["/", "/docs", "/showcase/nike/proa/product", "/showcase/airbnb/proa"]) {
  const response = await request(path);
  assert.match(response.headers.get("content-type"), /text\/html/);
  const html = await response.text();
  assert.match(html, /<!doctype html/i, `${path}: complete HTML document`);
  for (const match of html.matchAll(/(?:src|href)="(\/[^"?#]+\.(?:js|mjs|css)(?:\?[^"#]*)?)"/g)) {
    assets.add(match[1]);
  }
}
assert.ok(assets.size > 0, "release HTML must reference packaged browser assets");
for (const asset of assets) {
  const response = await request(asset);
  assert.ok((await response.arrayBuffer()).byteLength > 0, `${asset}: empty asset`);
  assert.doesNotMatch(response.headers.get("content-type") || "", /text\/html/, `${asset}: HTML fallback`);
}

for (const engine of ["", "-node", "-bun"]) {
  const deadline = Date.now() + 30_000;
  let failure;
  do {
    try {
      const response = await request(`/api/render${engine}`, {
        method: "POST", headers: { "content-type": "application/json", "x-proa-silent": "1" },
        body: JSON.stringify({ page: "nike-pdp" }),
      });
      assert.ok((await response.text()).length > 1000, `${engine || "proa"}: empty render`);
      failure = undefined;
      break;
    } catch (error) {
      failure = error;
      await setTimeout(250);
    }
  } while (Date.now() < deadline);
  if (failure) throw failure;
}
console.log(`Release image passed: health, four routes, ${assets.size} assets, Proa/Node/Bun rendering.`);
