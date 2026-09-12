#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/handler/control_flow_if_else.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const section = makeElement("section");
const button = makeElement("button", { "data-rsjs-handler": "1" });
const selected = makeElement("span", { "data-rsjs-bind": "3" });
section.appendChild(button);
section.appendChild(selected);
root.appendChild(section);
installDocument(root);

await importSnapshot(
  snapshotPath,
  "control-flow-if-else.mjs",
);

assert((root._listeners.get("click") ?? []).length === 1, "click event should be delegated through one root listener");
assert(selected.textContent === "plain", `initial text should be "plain", got ${JSON.stringify(selected.textContent)}`);

button.dispatchEvent({ type: "click", target: button, shiftKey: true, ctrlKey: false });
await Promise.resolve();
assert(selected.textContent === "shift", `shift-click should set "shift", got ${JSON.stringify(selected.textContent)}`);

button.dispatchEvent({ type: "click", target: button, shiftKey: true, ctrlKey: true });
await Promise.resolve();
assert(selected.textContent === "both", `shift-ctrl-click should set "both", got ${JSON.stringify(selected.textContent)}`);

button.dispatchEvent({ type: "click", target: button, shiftKey: false, ctrlKey: true });
await Promise.resolve();
assert(selected.textContent === "plain", `non-shift click should set "plain", got ${JSON.stringify(selected.textContent)}`);

finish("rsjs conformance runtime control-flow if/else");
