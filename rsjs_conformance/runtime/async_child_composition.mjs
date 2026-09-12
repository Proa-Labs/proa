#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

// Parent/child owner fusion is covered by the final-link executable in
// proa_cli/tests/rsjs_linked_site_codegen.rs. This fixture exercises the
// reusable async child fragment and, critically, keeps its DOM authority
// (`article`) separate from the occurrence wrapper that owns query/mutation
// seed metadata.
const snapshotPath =
  "rsjs_conformance/snapshots/component/component_asyncchildcompositiondatachildsnapshot.js";
const fragmentAssetId = await assetIdFromSnapshot(snapshotPath);
const seedRoot = makeElement("rsjs-island", { id: fragmentAssetId });
const root = makeElement("article");
const price = makeElement("span", { "data-rsjs-bind": "1" });
const mutationStatus = makeElement("span", { "data-rsjs-bind": "2" });
const pending = makeElement("span", { "data-rsjs-attr": "3" });
const button = makeElement("button", {
  type: "button",
  "data-rsjs-handler": "4",
});
root.appendChild(price);
root.appendChild(mutationStatus);
root.appendChild(pending);
root.appendChild(button);
seedRoot.appendChild(root);

const querySeed = makeElement("script", {
  type: "application/json",
  "data-rsjs-island-queries": fragmentAssetId,
});
querySeed.textContent = JSON.stringify({
  0: {
    key: ["child_stock", "AAPL"],
    data: { value: 174.25, currency: "USD" },
    data_updated_at: 0,
  },
});
const mutationSeed = makeElement("script", {
  type: "application/json",
  "data-rsjs-island-mutations": fragmentAssetId,
});
mutationSeed.textContent = JSON.stringify({
  0: {
    key: ["child_update_cart", "AAPL"],
    invalidates: [["child_stock", "AAPL"]],
    options: { retry_count: 1, on_success_invalidate_strategy: "refetch" },
  },
});
seedRoot.nextElementSibling = querySeed;
querySeed.nextElementSibling = mutationSeed;
installDocument(seedRoot, [querySeed, mutationSeed]);

const calls = [];
let resolveMutation;
const queryBody = JSON.stringify({ value: 199.5, currency: "CAD" });
globalThis.fetch = async (url, init = {}) => {
  calls.push({ url, init });
  if (url === "/api/child-cart") {
    return await new Promise((resolve) => {
      resolveMutation = () => resolve(makeResponse(""));
    });
  }
  if (url === "/api/child-stocks/AAPL") {
    return makeResponse(queryBody);
  }
  return makeResponse("{}");
};

const module = await importSnapshot(snapshotPath, "async-child-composition-fragment.mjs");
const dispose = module[`mount_${fragmentAssetId}`](root, { seedRoot });

assert(typeof dispose === "function", "async child fragment mount should return a disposer");
assert(price.textContent === "174.25", `query seed should hydrate child price, got ${JSON.stringify(price.textContent)}`);
assert(price.getAttribute("data-currency") === "USD", `query seed should hydrate child currency attr, got ${JSON.stringify(price.getAttribute("data-currency"))}`);
assert(mutationStatus.textContent === "idle", `child mutation should start idle, got ${JSON.stringify(mutationStatus.textContent)}`);
assert(pending.hasAttribute("hidden"), "child mutation pending marker should start hidden");

button.dispatchEvent({ type: "click", target: button });
await waitFor(() => calls.length === 2 && resolveMutation, "child handler should start mutation and query refetch");
assert(calls[0].url === "/api/child-cart", `child mutation should use metadata-backed URL, got ${calls[0].url}`);
assert(calls[0].init.method === "PATCH", `child mutation should preserve PATCH method, got ${calls[0].init.method}`);
assert(headerValue(calls[0].init.headers, "X-Mutate") === "1", "child mutation should preserve static header");
assert(calls[0].init.body === '"AAPL"', `child mutation should encode run payload, got ${JSON.stringify(calls[0].init.body)}`);
assert(calls[1].url === "/api/child-stocks/AAPL", `child query refetch should use configured URL, got ${calls[1].url}`);
assert(!pending.hasAttribute("hidden"), "child mutation pending marker should show while in flight");
assert(mutationStatus.textContent === "pending", `child mutation status should be pending, got ${JSON.stringify(mutationStatus.textContent)}`);

resolveMutation();
await waitFor(
  () => calls.length === 3 && mutationStatus.textContent === "success",
  "child mutation metadata should invalidate the child query key",
);
assert(calls[2].url === "/api/child-stocks/AAPL", `child mutation invalidation should refetch child query, got ${calls[2].url}`);
assert(price.textContent === "199.5", `child query text should update after refetch, got ${JSON.stringify(price.textContent)}`);
assert(price.getAttribute("data-currency") === "CAD", `child query attr should update after refetch, got ${JSON.stringify(price.getAttribute("data-currency"))}`);
assert(pending.hasAttribute("hidden"), "child mutation pending marker should hide after success");

dispose();
assert(
  (root._listeners.get("click") ?? []).length === 0,
  "async child fragment disposal should remove its delegated click listener",
);

finish("rsjs conformance runtime async child composition");

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
