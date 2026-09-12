#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Numeric semantics: i32 arithmetic at the i32::MAX boundary.
//
// The island declares signal(2147483647_i32) and adds 1_i32. Checked
// fixed-width lowering rejects the out-of-domain result before the signal
// mutates, so behavior is deterministic across Rust build profiles and JS.

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/semantics/numeric_i32_overflow.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const div = makeElement("div");
const overflow = makeElement("button", { "data-rsjs-handler": "1" });
const value = makeElement("span", { "data-rsjs-bind": "3" });
div.appendChild(overflow);
div.appendChild(value);
root.appendChild(div);
installDocument(root);

await importSnapshot(
  snapshotPath,
  "semantics-numeric-i32-overflow.mjs",
);

assert(
  value.textContent === "2147483647",
  `initial value should hydrate to i32::MAX, got ${JSON.stringify(value.textContent)}`,
);

let overflowRejected = false;
try {
  overflow.dispatchEvent({ type: "click", target: overflow });
} catch (error) {
  overflowRejected =
    error instanceof RangeError &&
    String(error.message).includes("checked i32 add overflow");
}
assert(overflowRejected, "i32::MAX + 1 must throw the checked i32 overflow error");
await Promise.resolve();
assert(
  value.textContent === "2147483647",
  `rejected overflow must leave the signal at i32::MAX, got ${JSON.stringify(value.textContent)}`,
);

finish("rsjs conformance runtime semantics numeric i32 overflow");
