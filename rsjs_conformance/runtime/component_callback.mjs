#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
  assert,
  assetIdFromSnapshot,
  finish,
  importSnapshotWithRuntime,
  makeElement,
} from "./harness.mjs";

// Parent/child callback wiring is a semantic-link decision and is exercised by
// proa_cli/tests/rsjs_linked_site_codegen.rs. This conformance fixture owns the
// reusable fragment contract: the linked owner supplies the current callback
// and live signal through the fragment ABI.
const snapshotPath =
  "rsjs_conformance/snapshots/component/component_callbacktextinputsnapshot_23897f3a.js";
const fragmentAssetId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("label");
const childValue = makeElement("span", { "data-rsjs-bind": "1" });
const input = makeElement("input", {
  "data-rsjs-attr": "2",
  "data-rsjs-handler": "2",
});
root.appendChild(childValue);
root.appendChild(input);

const { module, runtime } = await importSnapshotWithRuntime(
  snapshotPath,
  "component-callback-fragment.mjs",
);
const parentValue = runtime.signal("");
let callbackCalls = 0;
root.__rsjs_callbacks = {
  __callback_0(next) {
    callbackCalls += 1;
    parentValue.set(next);
  },
};
const dispose = module[`mount_${fragmentAssetId}`](root, {
  signals: { 0: parentValue },
});

assert(typeof dispose === "function", "component fragment mount should return a disposer");
assert((root._listeners.get("input") ?? []).length === 1, "input event should be delegated through one component-root listener");
assert(childValue.textContent === "", `child text should hydrate empty, got ${JSON.stringify(childValue.textContent)}`);
assert(input.getAttribute("value") === "", "input value attribute should hydrate empty");

const consoleLog = console.log;
const logs = [];
console.log = (...args) => {
  logs.push(args);
};
try {
  input.value = "typed";
  input.dispatchEvent({ type: "input", target: input });
  await Promise.resolve();
} finally {
  console.log = consoleLog;
}

assert(logs.length === 1, `child handler should log once, got ${logs.length}`);
assert(logs[0]?.[0] === "typed", `child handler should log input value, got ${JSON.stringify(logs[0]?.[0])}`);
assert(callbackCalls === 1, `fragment should resolve the current callback once, got ${callbackCalls}`);
assert(childValue.textContent === "typed", `child signal binding should update, got ${JSON.stringify(childValue.textContent)}`);
assert(input.getAttribute("value") === "typed", `input value attribute should update, got ${JSON.stringify(input.getAttribute("value"))}`);

dispose();
assert(
  (root._listeners.get("input") ?? []).length === 0,
  "fragment disposal should remove its delegated input listener",
);

finish("rsjs conformance runtime component callback");
