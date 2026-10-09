#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Numeric semantics: i64 values past 2^53.
//
// The signal is declared as signal(9007199254740993_i64), which is
// 2^53 + 1, the first i64 a JS Number cannot represent exactly. The
// i64 is always BigInt-backed, so the island hydrates the signal through
// signal_initial_integer (decoding and range-checking the canonical decimal
// JSON-string wire form) and the handler's `+ 1_i64` literal emits as `1n`.
// The result matches Rust exactly at every magnitude.

import {
  assert,
  assetIdFromSnapshot,
  finish,
  importSnapshotWithRuntime,
  installDocument,
  makeElement,
} from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/semantics/numeric_i64_precision.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const div = makeElement("div");
const increment = makeElement("button", { "data-rsjs-handler": "1" });
const incrementViaUpdate = makeElement("button", { "data-rsjs-handler": "3" });
const incrementViaLocal = makeElement("button", { "data-rsjs-handler": "5" });
const incrementViaMemo = makeElement("button", { "data-rsjs-handler": "7" });
const value = makeElement("span", { "data-rsjs-bind": "9" });
const successor = makeElement("span", { "data-rsjs-bind": "10" });
div.appendChild(increment);
div.appendChild(incrementViaUpdate);
div.appendChild(incrementViaLocal);
div.appendChild(incrementViaMemo);
div.appendChild(value);
div.appendChild(successor);
root.appendChild(div);

// The dehydrated state blob rides the wrapper's next sibling. Wide-int
// seeds use the ClientEncode JSON-string convention: a bare JSON
// number would already have rounded to 9007199254740992 in JSON.parse.
const host = makeElement("body");
host.appendChild(root);
const seed = makeElement("script", {
  type: "application/json",
  "data-rsjs-island": islandId,
});
// The derived `successor` cell occupies index 1 with a null
// placeholder (derived values are recomputed, never seeded).
seed.textContent = '{"signals":["9007199254740993",null]}';
host.appendChild(seed);

installDocument(root);

const { runtime } = await importSnapshotWithRuntime(
  snapshotPath,
  "semantics-numeric-i64-precision.mjs",
);

assert(
  value.textContent === "9007199254740993",
  `the 2^53 + 1 seed must hydrate exactly (Rust renders 9007199254740993), got ${JSON.stringify(value.textContent)}`,
);

increment.dispatchEvent({ type: "click", target: increment });
await Promise.resolve();
assert(
  value.textContent === "9007199254740994",
  `+ 1_i64 past 2^53 must increment exactly like Rust (expected 9007199254740994), got ${JSON.stringify(value.textContent)}`,
);

increment.dispatchEvent({ type: "click", target: increment });
await Promise.resolve();
assert(
  value.textContent === "9007199254740995",
  `repeated + 1_i64 must keep exact BigInt arithmetic (expected 9007199254740995), got ${JSON.stringify(value.textContent)}`,
);

// Wide proofs through the whole-island typing pass: the update
// closure parameter, a handler local holding big.get(), and a read
// of the derived `successor` cell are all BigInt-backed, so their
// `+ 1_i64` literals emit as `1n` and arithmetic stays exact.
//
// Bindings subscribed to a memo observe the new value one
// microtask flush after the source signal (write-during-flush
// semantics), hence the extra await before successor assertions.
await Promise.resolve();
assert(
  successor.textContent === "9007199254740996",
  `the derived successor cell must recompute exactly (expected 9007199254740996), got ${JSON.stringify(successor.textContent)}`,
);

incrementViaUpdate.dispatchEvent({ type: "click", target: incrementViaUpdate });
await Promise.resolve();
assert(
  value.textContent === "9007199254740996",
  `update(|b| b + 1_i64) must increment exactly through the closure parameter (expected 9007199254740996), got ${JSON.stringify(value.textContent)}`,
);

incrementViaLocal.dispatchEvent({ type: "click", target: incrementViaLocal });
await Promise.resolve();
assert(
  value.textContent === "9007199254740997",
  `a BigInt-backed handler local + 1_i64 must increment exactly (expected 9007199254740997), got ${JSON.stringify(value.textContent)}`,
);

incrementViaMemo.dispatchEvent({ type: "click", target: incrementViaMemo });
await Promise.resolve();
assert(
  value.textContent === "9007199254740999",
  `successor.get() + 1_i64 through the memo read must stay exact (expected 9007199254740999), got ${JSON.stringify(value.textContent)}`,
);
await Promise.resolve();
assert(
  successor.textContent === "9007199254741000",
  `the derived successor cell must track the new value exactly (expected 9007199254741000), got ${JSON.stringify(successor.textContent)}`,
);

// Strict fixed-width hydration coverage: canonical decimal strings decode
// exactly, while a malformed/cross-representation seed is rejected.
const { signal_initial_integer, signal_initial_typed } = runtime;
assert(
  signal_initial_integer(["9007199254740993"], 0, 0n, "i64") === 9007199254740993n,
  "JSON-string seed must decode to the exact BigInt",
);
assert(
  signal_initial_typed(
    ["9007199254740993"],
    0,
    null,
    { kind: "option", inner: { kind: "i64" } },
  ) === 9007199254740993n,
  "Option<i64>::Some seed must recursively decode to exact BigInt",
);
assert(
  signal_initial_integer([], 0, 9007199254740993n, "i64") === 9007199254740993n,
  "missing seed must fall back to the compiled BigInt literal",
);
for (const badSeed of [42, "01", "-0", "not a number"]) {
  let rejected = false;
  try {
    signal_initial_integer([badSeed], 0, 7n, "i64");
  } catch (error) {
    rejected = error instanceof TypeError;
  }
  assert(rejected, `malformed i64 seed ${JSON.stringify(badSeed)} must be rejected`);
}

finish("rsjs conformance runtime semantics numeric i64 precision");
