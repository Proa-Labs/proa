#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/query/query_mutation_authoring.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const section = makeElement("section");
const input = makeElement("input", {
  "data-rsjs-attr": "1",
  "data-rsjs-handler": "1",
});
const price = makeElement("span", { "data-rsjs-bind": "2" });
const status = makeElement("span", { "data-rsjs-bind": "3" });
const loaded = makeElement("span", { "data-rsjs-bind": "4" });
const total = makeElement("span", { "data-rsjs-bind": "5" });
const percent = makeElement("span", { "data-rsjs-bind": "6" });
const fetchCount = makeElement("span", { "data-rsjs-bind": "7" });
const feed = makeElement("span", { "data-rsjs-bind": "8" });
const mutationStatus = makeElement("span", { "data-rsjs-bind": "9" });
const pending = makeElement("span", { "data-rsjs-attr": "10" });
const button = makeElement("button", {
  type: "button",
  "data-rsjs-handler": "11",
});
const refetchButton = makeElement("button", {
  type: "button",
  "data-rsjs-handler": "12",
});
const invalidateButton = makeElement("button", {
  type: "button",
  "data-rsjs-handler": "13",
});
section.appendChild(input);
section.appendChild(price);
section.appendChild(status);
section.appendChild(loaded);
section.appendChild(total);
section.appendChild(percent);
section.appendChild(fetchCount);
section.appendChild(feed);
section.appendChild(mutationStatus);
section.appendChild(pending);
section.appendChild(button);
section.appendChild(refetchButton);
section.appendChild(invalidateButton);
root.appendChild(section);

const querySeed = makeElement("script", {
  type: "application/json",
  "data-rsjs-island-queries": islandId,
});
querySeed.textContent = JSON.stringify({
  0: { data: { value: 172.5, currency: "USD" }, data_updated_at: 0 },
  1: { data: "ready", data_updated_at: 0 },
});
root.nextElementSibling = querySeed;
installDocument(root, [querySeed]);

const calls = [];
let resolveMutation;
const queryBody = JSON.stringify({ value: 200.25, currency: "USD" });
const feedBody = "streamed feed";
globalThis.fetch = async (url, init = {}) => {
  calls.push({ url, init });
  if (init.method === "PATCH") {
    return await new Promise((resolve) => {
      resolveMutation = () =>
        resolve(makeResponse(""));
    });
  }
  if (url === "/api/feed") {
    return makeResponse(feedBody);
  }
  return makeResponse(queryBody);
};

await importSnapshot(
  snapshotPath,
  "query-mutation.mjs",
);

assert(price.textContent === "172.5", `query seed should hydrate price text, got ${JSON.stringify(price.textContent)}`);
assert(status.textContent === "success", `query seed should hydrate success status, got ${JSON.stringify(status.textContent)}`);
assert(loaded.textContent === "0", `download loaded should start at 0, got ${JSON.stringify(loaded.textContent)}`);
assert(total.textContent === "", `download total should start empty, got ${JSON.stringify(total.textContent)}`);
assert(percent.textContent === "0", `download percent should start at 0, got ${JSON.stringify(percent.textContent)}`);
assert(fetchCount.textContent === "1", `fetch count should include query seed, got ${JSON.stringify(fetchCount.textContent)}`);
assert(feed.textContent === "ready", `stream text query should hydrate seed, got ${JSON.stringify(feed.textContent)}`);
assert(mutationStatus.textContent === "idle", `mutation status should start idle, got ${JSON.stringify(mutationStatus.textContent)}`);
assert(pending.hasAttribute("hidden"), "mutation pending marker should start hidden");

input.value = "buy";
input.dispatchEvent({ type: "input", target: input });
await Promise.resolve();
assert(input.getAttribute("value") === "buy", `input attr should track draft signal, got ${JSON.stringify(input.getAttribute("value"))}`);

button.dispatchEvent({ type: "click", target: button });
await waitFor(() => calls.length === 1 && resolveMutation, "mutation POST should start");
assert(calls[0].url === "/api/cart", `mutation should PATCH cart URL, got ${calls[0].url}`);
assert(calls[0].init.method === "PATCH", `mutation method should be PATCH, got ${calls[0].init.method}`);
assert(calls[0].init.credentials === "omit", `mutation credentials should be omit, got ${calls[0].init.credentials}`);
assert(headerValue(calls[0].init.headers, "X-Mutate") === "1", "mutation should preserve static headers");
assert(headerValue(calls[0].init.headers, "Content-Type") === "application/json", "mutation fallback JSON body should set content type");
assert(calls[0].init.body === '"buy"', `mutation body should encode the draft string, got ${JSON.stringify(calls[0].init.body)}`);
assert(!pending.hasAttribute("hidden"), "pending marker should show while mutation is in flight");
assert(mutationStatus.textContent === "pending", `mutation status should show pending, got ${JSON.stringify(mutationStatus.textContent)}`);

resolveMutation();
await waitFor(() => calls.length === 3 && price.textContent === "200.25" && feed.textContent === feedBody, "mutation invalidation should refetch query data and stream text");
assert(calls[1].url === "/api/stocks/AAPL", `query should refetch stock URL, got ${calls[1].url}`);
assert(calls[1].init.method === "POST", `query refetch method should be POST, got ${calls[1].init.method}`);
assert(calls[1].init.credentials === "include", `query credentials should be include, got ${calls[1].init.credentials}`);
assert(headerValue(calls[1].init.headers, "Accept") === "application/json", "query should preserve Accept header");
assert(headerValue(calls[1].init.headers, "X-RSJS") === "1", "query should preserve custom header");
assert(headerValue(calls[1].init.headers, "Content-Type") === "application/json", "query JSON request body should set content type");
assert(calls[1].init.body === '{"symbol":"AAPL"}', `query refetch should use configured body, got ${JSON.stringify(calls[1].init.body)}`);
assert(status.textContent === "success", `query status should return to success, got ${JSON.stringify(status.textContent)}`);
assert(loaded.textContent === String(byteLength(queryBody)), `download loaded should track response bytes, got ${JSON.stringify(loaded.textContent)}`);
assert(total.textContent === String(byteLength(queryBody)), `download total should track response bytes, got ${JSON.stringify(total.textContent)}`);
assert(percent.textContent === "1", `download percent should complete, got ${JSON.stringify(percent.textContent)}`);
assert(fetchCount.textContent === "2", `fetch count should increment after refetch, got ${JSON.stringify(fetchCount.textContent)}`);
assert(calls[2].url === "/api/feed", `stream query should refetch feed URL, got ${calls[2].url}`);
assert(calls[2].init.method === "GET", `stream query method should be GET, got ${calls[2].init.method}`);
assert(calls[2].init.credentials === "same-origin", `stream query credentials should default same-origin, got ${calls[2].init.credentials}`);
assert(headerValue(calls[2].init.headers, "Accept") === "text/plain", "stream query should preserve Accept header");
assert(feed.textContent === feedBody, `stream text query should update text, got ${JSON.stringify(feed.textContent)}`);
assert(mutationStatus.textContent === "success", `mutation status should finish success, got ${JSON.stringify(mutationStatus.textContent)}`);
assert(pending.hasAttribute("hidden"), "pending marker should hide after mutation settles");

refetchButton.dispatchEvent({ type: "click", target: refetchButton });
await waitFor(() => calls.length === 4 && fetchCount.textContent === "3", "manual query refetch should refetch stock data");
assert(calls[3].url === "/api/stocks/AAPL", `manual refetch should hit stock URL, got ${calls[3].url}`);

invalidateButton.dispatchEvent({ type: "click", target: invalidateButton });
await waitFor(() => calls.length === 5 && fetchCount.textContent === "4", "manual query invalidate should refetch observed stock data");
assert(calls[4].url === "/api/stocks/AAPL", `manual invalidate should refetch stock URL, got ${calls[4].url}`);

finish("rsjs conformance runtime query mutation");

async function waitFor(predicate, message) {
  for (let i = 0; i < 30; i += 1) {
    if (predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  assert(false, message);
}

function makeResponse(body) {
  const bytes = new TextEncoder().encode(body);
  return {
    ok: true,
    status: 200,
    headers: {
      get(name) {
        return String(name).toLowerCase() === "content-length" ? String(bytes.byteLength) : null;
      },
    },
    async text() {
      return body;
    },
    async arrayBuffer() {
      return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
    },
  };
}

function byteLength(value) {
  return new TextEncoder().encode(value).byteLength;
}

function headerValue(headers, name) {
  const needle = String(name).toLowerCase();
  if (Array.isArray(headers)) {
    const found = headers.find((entry) => String(entry[0]).toLowerCase() === needle);
    return found?.[1] ?? null;
  }
  for (const [key, value] of Object.entries(headers ?? {})) {
    if (key.toLowerCase() === needle) {
      return value;
    }
  }
  return null;
}
