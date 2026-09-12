#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, makeElement } from "./harness.mjs";

// The explicit-island cut and direct-child callback edge are final-linker
// concerns covered by proa_cli/tests/rsjs_explicit_island_static_scalar.rs and
// proa_cli/tests/rsjs_linked_site_codegen.rs. Here the reusable fragment proves
// that an explicit server boundary keeps its markup opaque while the linked
// owner can still supply a current callback.
const snapshotPath =
  "rsjs_conformance/snapshots/server_boundary/component_serverdecoratedbuttonsnapshot_8cfe6746.js";
const fragmentAssetId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("button", {
  "data-rsjs-handler": "0",
});
let callbackCalls = 0;
root.__rsjs_callbacks = {
  __callback_0(payload) {
    callbackCalls += 1;
    assert(payload === null, "unit callback payload should lower to null");
  },
};

const module = await importSnapshot(snapshotPath, "server-boundary-fragment.mjs");
const dispose = module[`mount_${fragmentAssetId}`](root);

assert(typeof dispose === "function", "server-boundary fragment mount should return a disposer");
assert((root._listeners.get("click") ?? []).length === 1, "click event should be delegated through one component-root listener");

root.dispatchEvent({ type: "click", target: root });
await Promise.resolve();

assert(callbackCalls === 1, `fragment should resolve its current callback once, got ${callbackCalls}`);
dispose();
assert(
  (root._listeners.get("click") ?? []).length === 0,
  "fragment disposal should remove its delegated click listener",
);

finish("rsjs conformance runtime server boundary");
