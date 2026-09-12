#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Numeric semantics: conditionals over i64 zero/non-zero and
// empty/non-empty strings.
//
// The island writes explicit Rust comparisons (count.get() == 0_i64,
// next == ""), which lower to strict JS comparisons (===), so the
// branch taken matches the Rust expression for the zero value, the
// non-zero value, the empty string, and the non-empty string. The
// paired compile-fail fixture proves that a bare numeric condition is
// rejected instead of falling back to JavaScript truthiness.

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/semantics/conditional_truthiness.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const div = makeElement("div");
const checkCount = makeElement("button", { "data-rsjs-handler": "1" });
const textInput = makeElement("input", { "data-rsjs-handler": "3" });
const countState = makeElement("span", { "data-rsjs-bind": "4" });
const textState = makeElement("span", { "data-rsjs-bind": "5" });
div.appendChild(checkCount);
div.appendChild(textInput);
div.appendChild(countState);
div.appendChild(textState);
root.appendChild(div);
installDocument(root);

await importSnapshot(
  snapshotPath,
  "semantics-conditional-truthiness.mjs",
);

assert(countState.textContent === "unset", `count state should hydrate to "unset", got ${JSON.stringify(countState.textContent)}`);
assert(textState.textContent === "unset", `text state should hydrate to "unset", got ${JSON.stringify(textState.textContent)}`);

// count == 0 takes the zero branch, then increments to 1.
checkCount.dispatchEvent({ type: "click", target: checkCount });
await Promise.resolve();
assert(
  countState.textContent === "zero",
  `count.get() == 0_i64 should take the zero branch like Rust, got ${JSON.stringify(countState.textContent)}`,
);

// count is now 1, so the comparison is false.
checkCount.dispatchEvent({ type: "click", target: checkCount });
await Promise.resolve();
assert(
  countState.textContent === "nonzero",
  `count.get() == 0_i64 should be false for 1 like Rust, got ${JSON.stringify(countState.textContent)}`,
);

// Empty string compares equal to "".
textInput.value = "";
textInput.dispatchEvent({ type: "input", target: textInput });
await Promise.resolve();
assert(
  textState.textContent === "empty",
  `next == "" should take the empty branch for "" like Rust, got ${JSON.stringify(textState.textContent)}`,
);

// Non-empty string takes the else branch.
textInput.value = "hello";
textInput.dispatchEvent({ type: "input", target: textInput });
await Promise.resolve();
assert(
  textState.textContent === "filled",
  `next == "" should be false for "hello" like Rust, got ${JSON.stringify(textState.textContent)}`,
);

finish("rsjs conformance runtime semantics conditional truthiness");
