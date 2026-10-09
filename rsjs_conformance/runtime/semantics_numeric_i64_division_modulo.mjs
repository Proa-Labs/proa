#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Numeric semantics: i64 division and remainder with negative operands.
//
// Rust truncates integer division toward zero (-7 / 3 == -2) and `%`
// keeps the sign of the dividend (-7 % 3 == -1, 7 % -3 == 1). The
// emitted JS keeps every i64 BigInt-backed and routes `/` and `%` through
// checked i64 operations. BigInt division already truncates toward zero;
// the domain check also covers zero division and signed overflow.

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/semantics/numeric_i64_division_modulo.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const div = makeElement("div");
const negDividend = makeElement("button", { "data-rsjs-handler": "1" });
const negDivisor = makeElement("button", { "data-rsjs-handler": "3" });
const localDivision = makeElement("button", { "data-rsjs-handler": "5" });
const quotient = makeElement("span", { "data-rsjs-bind": "7" });
const remainder = makeElement("span", { "data-rsjs-bind": "8" });
div.appendChild(negDividend);
div.appendChild(negDivisor);
div.appendChild(localDivision);
div.appendChild(quotient);
div.appendChild(remainder);
root.appendChild(div);
installDocument(root);

await importSnapshot(
  snapshotPath,
  "semantics-numeric-i64-division-modulo.mjs",
);

assert(quotient.textContent === "0", `initial quotient should be "0", got ${JSON.stringify(quotient.textContent)}`);
assert(remainder.textContent === "0", `initial remainder should be "0", got ${JSON.stringify(remainder.textContent)}`);

// -7 / 3 and -7 % 3. Rust: quotient -2, remainder -1.
negDividend.dispatchEvent({ type: "click", target: negDividend });
await Promise.resolve();
assert(
  remainder.textContent === "-1",
  `-7 % 3 should be -1 (matches Rust truncated remainder), got ${JSON.stringify(remainder.textContent)}`,
);
assert(
  quotient.textContent === "-2",
  `-7 / 3 should be -2 (matches Rust truncation toward zero), got ${JSON.stringify(quotient.textContent)}`,
);

// 7 / -3 and 7 % -3. Rust: quotient -2, remainder 1.
negDivisor.dispatchEvent({ type: "click", target: negDivisor });
await Promise.resolve();
assert(
  remainder.textContent === "1",
  `7 % -3 should be 1 (matches Rust truncated remainder), got ${JSON.stringify(remainder.textContent)}`,
);
assert(
  quotient.textContent === "-2",
  `7 / -3 should be -2 (matches Rust truncation toward zero), got ${JSON.stringify(quotient.textContent)}`,
);

// Division proofs through the whole-island typing pass: a handler local
// (`dividend`), compound `/=` on a local, and a memo-cell read all
// retain the exact i64 domain. Rust: -7 / 3 == -2, -2 / 2 == -1,
// and doubled == -14, so -14 / 4 == -3 (JS float division would give
// -3.5 under floating-point division).
localDivision.dispatchEvent({ type: "click", target: localDivision });
await Promise.resolve();
assert(
  quotient.textContent === "-1",
  `(-7 / 3) /= 2 over handler locals should be -1 (matches Rust truncation), got ${JSON.stringify(quotient.textContent)}`,
);
assert(
  remainder.textContent === "-3",
  `-14 / 4 over a memo read should be -3 (matches Rust truncation), got ${JSON.stringify(remainder.textContent)}`,
);

finish("rsjs conformance runtime semantics numeric i64 division/modulo");
