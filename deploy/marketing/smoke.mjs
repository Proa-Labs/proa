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

const fingerprintedImages = new Set();
const renderedPages = new Map();
for (const [path, image] of [
  ["/", "airbnb-preview-exterior-720"],
  ["/", "01-canonical-scoreboard"],
  ["/", "og"],
  ["/team", "andrew-hariri"],
  ["/ecosystem/apps", "ecosystem-travel"],
  ["/docs/framework/routing", "nested-layouts"],
  ["/demos/quantity-stepper", "atlas-jacket-front"],
]) {
  if (!renderedPages.has(path)) renderedPages.set(path, await (await request(path)).text());
  const html = renderedPages.get(path);
  const imagePaths = [...html.matchAll(/\/_proa\/immutable\/[^\s"'<>?,]+\.[a-f0-9]{16}\.(?:svg|webp|jpg|png|ico)/g)]
    .map((match) => match[0]);
  assert.ok(imagePaths.some((asset) => asset.split("/").at(-1).startsWith(`${image}.`)),
    `${path}: missing fingerprinted ${image}`);
  for (const asset of imagePaths) fingerprintedImages.add(asset);
}
for (const asset of fingerprintedImages) {
  const response = await request(asset);
  assert.match(response.headers.get("content-type") || "", /^image\//, asset);
  assert.match(response.headers.get("cache-control") || "", /\bimmutable\b/, asset);
  assert.ok((await response.arrayBuffer()).byteLength > 0, `${asset}: empty image`);
}
// Files that keep their names are revalidated on every load.
for (const path of ["/search.mjs", "/search-index.mjs", "/vendor/orama.mjs"]) {
  const asset = await request(`/_proa/mutable${path}`);
  assert.equal(asset.headers.get("cache-control"), "public, no-cache", path);
  assert.ok(asset.headers.has("etag"), `${path}: missing catalog validator`);
}
// The stylesheet is named after its bytes and cached like an image.
{
  const home = renderedPages.get("/") ?? await (await request("/")).text();
  const stylesheet = home.match(/\/_proa\/immutable\/styles\.[a-f0-9]{16}\.css/);
  assert.ok(stylesheet, "home page links no hashed stylesheet");
  const asset = await request(stylesheet[0]);
  assert.match(asset.headers.get("content-type") || "", /^text\/css/);
  assert.match(asset.headers.get("cache-control") || "", /\bimmutable\b/);
}
for (const path of ["/static/images/benchmarks/01-canonical-scoreboard.svg", "/images/airbnb/bedroom.jpg"]) {
  const alias = await fetch(new URL(path, base), { redirect: "manual", signal: AbortSignal.timeout(30_000) });
  assert.equal(alias.status, 307, path);
  assert.equal(alias.headers.get("cache-control"), "no-cache", path);
  const image = await request(alias.headers.get("location"));
  assert.match(image.headers.get("content-type") || "", /^image\//, path);
  assert.match(image.headers.get("cache-control") || "", /\bimmutable\b/, path);
}
const espnImage = await request("/images/espn/dodger-stadium.jpg");
assert.match(espnImage.headers.get("content-type") || "", /^image\//);

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
console.log(`Release image passed: health, HTML routes, ${assets.size} scripts/styles, ${fingerprintedImages.size} fingerprinted images, Proa/Node/Bun rendering.`);
