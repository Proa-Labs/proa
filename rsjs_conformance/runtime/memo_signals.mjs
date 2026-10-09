#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Memo (derived) signals: `rsjs::memo(|| price.get() * qty.get())`
// lowers to the runtime's `memo(fn, deps)` cell. Writing a source
// signal recomputes the derived value during the next microtask flush;
// bindings subscribed to the memo observe the new value one flush
// later (write-during-flush semantics, see the scheduler comment block
// in rsjs-runtime.js). The bound text must therefore show the derived
// value after two awaited microtasks, with one DOM write per flush.

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/signals/memo_signals.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const div = makeElement("div");
const addOne = makeElement("button", { "data-rsjs-handler": "1" });
const total = makeElement("span", { "data-rsjs-bind": "3" });
div.appendChild(addOne);
div.appendChild(total);
root.appendChild(div);
installDocument(root);

await importSnapshot(
  snapshotPath,
  "signals-memo-basic.mjs",
);

assert(
  total.textContent === "20",
  `hydration should seed the memo from the source signal initials (10 * 2), got ${JSON.stringify(total.textContent)}`,
);

// Write a source signal (qty: 2 -> 3) through the bound handler.
addOne.dispatchEvent({ type: "click", target: addOne });

// Flush 1: the memo's recompute effect runs (20 -> 30).
await Promise.resolve();
// Flush 2: subscribers of the memo (the text binding) observe it.
await Promise.resolve();

assert(
  total.textContent === "30",
  `the bound text should update to the derived value after two microtask flushes, got ${JSON.stringify(total.textContent)}`,
);

finish("rsjs conformance runtime memo signals");
