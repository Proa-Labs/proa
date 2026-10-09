#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Debounced handlers: `on_input={rsjs::debounce(|event| ..., 30)}`
// lowers to `const __handler_0 = debounce((event) => { ... }, 30);`
// with the wrapper's `_dispose` in the island disposer list and the
// delegation table referencing the wrapped function. A burst of input
// events must collapse to exactly one trailing call carrying the last
// value: the island counts handler invocations in a second signal.

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const snapshotPath = "rsjs_conformance/snapshots/timers/debounced_handler.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const div = makeElement("div");
const input = makeElement("input", { "data-rsjs-handler": "1" });
const query = makeElement("span", { "data-rsjs-bind": "2" });
const calls = makeElement("span", { "data-rsjs-bind": "3" });
div.appendChild(input);
div.appendChild(query);
div.appendChild(calls);
root.appendChild(div);
installDocument(root);

await importSnapshot(
  snapshotPath,
  "timers-debounce-basic.mjs",
);

assert(
  query.textContent === "" && calls.textContent === "0",
  `hydration should seed the bound text from the signal initials, got query=${JSON.stringify(query.textContent)} calls=${JSON.stringify(calls.textContent)}`,
);

// Dispatch a burst of input events well inside the 30ms window.
for (const value of ["a", "ab", "abc"]) {
  input.value = value;
  input.dispatchEvent({ type: "input", target: input });
}

// Inside the window nothing may have fired yet (trailing edge only).
await Promise.resolve();
await Promise.resolve();
assert(
  query.textContent === "" && calls.textContent === "0",
  `no handler call may land before the debounce window elapses, got query=${JSON.stringify(query.textContent)} calls=${JSON.stringify(calls.textContent)}`,
);

// After the window (plus scheduler flushes), exactly one trailing call
// must have run, carrying the burst's last value.
await sleep(60);
await Promise.resolve();
await Promise.resolve();

assert(
  query.textContent === "abc",
  `the trailing call should observe the last value of the burst, got ${JSON.stringify(query.textContent)}`,
);
assert(
  calls.textContent === "1",
  `the burst should collapse to exactly one handler call, got ${JSON.stringify(calls.textContent)}`,
);

finish("rsjs conformance runtime debounced handler");
