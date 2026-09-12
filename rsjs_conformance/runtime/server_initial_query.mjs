#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/query/server_initial_query.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const section = makeElement("section");
const price = makeElement("span", { "data-rsjs-bind": "1" });
const button = makeElement("button", {
  type: "button",
  "data-rsjs-handler": "2",
});
section.appendChild(price);
section.appendChild(button);
root.appendChild(section);

const querySeed = makeElement("script", {
  type: "application/json",
  "data-rsjs-island-queries": islandId,
});
querySeed.textContent = JSON.stringify({
  0: {
    key: ["stock", "AAPL"],
  },
});
root.nextElementSibling = querySeed;

const pageSeed = makeElement("script", {
  id: "rsjs-query-seed-table",
  type: "application/json",
});
pageSeed.textContent = JSON.stringify({
  queries: [
    {
      key: ["stock", "AAPL"],
      state: {
        key: ["stock", "AAPL"],
        data: { value: 174.25, currency: "USD" },
        data_updated_at: 0,
      },
    },
  ],
});
querySeed.nextElementSibling = pageSeed;
installDocument(root, [querySeed, pageSeed]);

const calls = [];
globalThis.fetch = async (url, init = {}) => {
  calls.push({ url, init });
  return makeResponse(JSON.stringify({ value: 180.5, currency: "CAD" }));
};

await importSnapshot(
  snapshotPath,
  "server-initial-query.mjs",
);

assert(calls.length === 0, `seeded query should not fetch on hydrate, got ${calls.length} calls`);
assert(price.textContent === "174.25", `query seed should hydrate price text, got ${JSON.stringify(price.textContent)}`);
assert(price.getAttribute("data-currency") === "USD", `query seed should hydrate currency attr, got ${JSON.stringify(price.getAttribute("data-currency"))}`);
assert(price.getAttribute("data-status") === "success", `query seed should hydrate status attr, got ${JSON.stringify(price.getAttribute("data-status"))}`);

const consoleLog = console.log;
const logs = [];
console.log = (...args) => {
  logs.push(args);
};
try {
  button.dispatchEvent({ type: "click", target: button });
  await waitFor(() => calls.length === 1 && price.textContent === "180.5", "manual refetch should update server-initial query data");
} finally {
  console.log = consoleLog;
}

assert(logs.length === 1 && logs[0]?.[0] === "refresh", `handler should log before refetch, got ${JSON.stringify(logs)}`);
assert(calls[0].url === "/api/stocks/AAPL", `manual refetch should use configured URL, got ${calls[0].url}`);
assert(calls[0].init.method === "GET", `manual refetch method should default GET, got ${calls[0].init.method}`);
assert(calls[0].init.credentials === "same-origin", `manual refetch credentials should default same-origin, got ${calls[0].init.credentials}`);
assert(price.getAttribute("data-currency") === "CAD", `refetch should update currency attr, got ${JSON.stringify(price.getAttribute("data-currency"))}`);
assert(price.getAttribute("data-status") === "success", `refetch should keep status attr successful, got ${JSON.stringify(price.getAttribute("data-status"))}`);

finish("rsjs conformance runtime server-initial query");

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
