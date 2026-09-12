// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Exercise real HTTP rendering in the packaged companion, including components
// that are never reached by /health or the Nike parity check.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const origin = process.argv[2] ?? "http://127.0.0.1:5100";
const props = JSON.parse(
  readFileSync(new URL("../showcase-props.json", import.meta.url), "utf8"),
);
const pages = [
  ["home", "/"],
  ["search", "/search"],
  ["listing", "/listing"],
  ["nike-catalog", "/nike"],
  ["nike-pdp", "/nike/product"],
  ["github-pr-diff", "/github-pr-diff"],
];

async function render(path, body) {
  const response = await fetch(new URL(path, origin), {
    method: body ? "POST" : "GET",
    headers: body ? { "content-type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(10_000),
  });
  const html = await response.text();
  const label = body ? `POST ${body.page}` : `GET ${path}`;
  assert.equal(response.status, 200, `${label}: ${html.slice(0, 300)}`);
  assert.match(response.headers.get("content-type") ?? "", /text\/html/);
  assert.match(html, /<!doctype html>/i);
  assert.match(html, /<body[\s>]/);
  if (body) {
    for (const name of ["x-render-us", "x-deser-us", "x-e2e-us"]) {
      const value = Number(response.headers.get(name));
      assert.ok(Number.isFinite(value) && value > 0, `${label}: missing ${name}`);
    }
    assert.equal(Number(response.headers.get("x-html-size")), Buffer.byteLength(html));
  }
  console.log(`companion smoke: ${label} -> 200 (${Buffer.byteLength(html)} bytes)`);
  return html;
}

for (const [page, path] of pages) {
  await render(path);
  await render("/api/render", { page });
  await render("/api/render", { page, props: props[page] });
}

// The playground sends editable snake_case props. Verify they reach the React
// tree and remain escaped, rather than silently rendering the default fixture.
const editedHome = structuredClone(props.home);
editedHome.homes_sections[0].title = 'Edited home <script>alert("probe")</script>';
const html = await render("/api/render", { page: "home", props: editedHome });
assert.ok(html.includes("Edited home &lt;script&gt;"));
assert.ok(!html.includes(editedHome.homes_sections[0].title));
