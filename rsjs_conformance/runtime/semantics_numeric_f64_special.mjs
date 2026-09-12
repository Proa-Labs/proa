#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Numeric semantics: f64 NaN, Infinity, and addition precision.
//
// f64 arithmetic lowers to JS Number arithmetic, which is the same
// IEEE-754 binary64 arithmetic Rust uses, so the produced values
// match Rust exactly: 0.0 / 0.0 is NaN, 1.0 / 0.0 is +Infinity, and
// 0.1 + 0.2 is 0.30000000000000004. For expressions the compiler
// proves are f32/f64, bind_text also preserves Rust's non-finite
// Display spellings (`NaN`, `inf`, and `-inf`).

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/semantics/numeric_f64_special.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const div = makeElement("div");
const nanButton = makeElement("button", { "data-rsjs-handler": "1" });
const infButton = makeElement("button", { "data-rsjs-handler": "3" });
const sumButton = makeElement("button", { "data-rsjs-handler": "5" });
const ratio = makeElement("span", { "data-rsjs-bind": "7" });
const sum = makeElement("span", { "data-rsjs-bind": "8" });
div.appendChild(nanButton);
div.appendChild(infButton);
div.appendChild(sumButton);
div.appendChild(ratio);
div.appendChild(sum);
root.appendChild(div);
installDocument(root);

await importSnapshot(
  snapshotPath,
  "semantics-numeric-f64-special.mjs",
);

assert(ratio.textContent === "0", `initial ratio should be "0", got ${JSON.stringify(ratio.textContent)}`);

nanButton.dispatchEvent({ type: "click", target: nanButton });
await Promise.resolve();
assert(
  ratio.textContent === "NaN" && Number.isNaN(Number(ratio.textContent)),
  `0.0 / 0.0 should render NaN like Rust's f64 Display, got ${JSON.stringify(ratio.textContent)}`,
);

infButton.dispatchEvent({ type: "click", target: infButton });
await Promise.resolve();
assert(
  ratio.textContent === "inf",
  `1.0 / 0.0 should render with Rust's f64 Display spelling "inf", got ${JSON.stringify(ratio.textContent)}`,
);

sumButton.dispatchEvent({ type: "click", target: sumButton });
await Promise.resolve();
assert(
  sum.textContent === "0.30000000000000004" && Number(sum.textContent) === 0.1 + 0.2,
  `0.1 + 0.2 should match Rust's binary64 result 0.30000000000000004, got ${JSON.stringify(sum.textContent)}`,
);

finish("rsjs conformance runtime semantics numeric f64 special values");
