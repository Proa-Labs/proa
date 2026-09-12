#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// rsjs::navigator::language() is the first helper lowered through the
// declarative browser-helper registry's DeclaredRead path: the JS in
// the snapshot comes straight from the registry entry's template. This
// exercises both supported positions (a hydration-bound text slot and
// a handler payload writing into a signal) plus the template's
// SSR-safe guards (`?? ""` for a missing language, the `typeof` check
// for a missing navigator global).

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/browser_api/navigator_language.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const panel = makeElement("div");
const language = makeElement("span", { "data-rsjs-bind": "1" });
const button = makeElement("button", { "data-rsjs-handler": "2" });
const detected = makeElement("span", { "data-rsjs-bind": "4" });
panel.appendChild(language);
panel.appendChild(button);
panel.appendChild(detected);
root.appendChild(panel);
installDocument(root);

// Node defines `navigator` as a global getter, so shim it with
// defineProperty (matching browser_effects.mjs) before hydration runs.
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: { language: "fr-CA" },
});

await importSnapshot(
  snapshotPath,
  "navigator-language.mjs",
);

assert(
  language.textContent === "fr-CA",
  `hydration should bind navigator.language into the text slot, got ${JSON.stringify(language.textContent)}`,
);
assert(
  detected.textContent === "",
  `the detected signal should seed empty before any click, got ${JSON.stringify(detected.textContent)}`,
);

button.dispatchEvent({ type: "click", target: button });
await Promise.resolve();

assert(
  detected.textContent === "fr-CA",
  `the click handler should copy navigator.language into the signal, got ${JSON.stringify(detected.textContent)}`,
);

// A navigator without a language must coalesce to "".
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {},
});
button.dispatchEvent({ type: "click", target: button });
await Promise.resolve();

assert(
  detected.textContent === "",
  `a missing navigator.language must read as "", got ${JSON.stringify(detected.textContent)}`,
);

// No navigator global at all must hit the typeof guard, not throw.
delete globalThis.navigator;
button.dispatchEvent({ type: "click", target: button });
await Promise.resolve();

assert(
  detected.textContent === "",
  `an undefined navigator global must read as "", got ${JSON.stringify(detected.textContent)}`,
);

finish("rsjs conformance runtime navigator language");
