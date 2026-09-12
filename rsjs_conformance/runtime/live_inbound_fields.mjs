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

// Rust-authored `self.field` reads become live fragment inputs. The semantic
// linker owns the parent edge; this fixture proves the fragment ABI observes
// updated values and resolves callback props at invocation time.
const snapshotPath =
  "rsjs_conformance/snapshots/component/live_inbound_fields.fragment.js";
const fragmentAssetId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("section", { "data-rsjs-attr": "0" });
const label = makeElement("p", { "data-rsjs-bind": "1" });
const button = makeElement("button", {
  type: "button",
  "data-rsjs-cond-then": "0:0",
  "data-rsjs-handler": "2",
});
root.appendChild(label);
root.appendChild(button);

const { module, runtime } = await importSnapshotWithRuntime(
  snapshotPath,
  "live-inbound-fields-fragment.mjs",
);
const className = runtime.signal("panel-initial");
const liveLabel = runtime.signal("Initial");
const enabled = runtime.signal(true);
let staleCallbackCalls = 0;
let currentCallbackPayload;
root.__rsjs_callbacks = {
  __callback_0() {
    staleCallbackCalls += 1;
  },
};

const dispose = module[`mount_${fragmentAssetId}`](root, {
  signals: {
    0: className,
    1: liveLabel,
    2: enabled,
  },
});

assert(typeof dispose === "function", "live-inbound fragment mount should return a disposer");
assert(root.getAttribute("class") === "panel-initial", "class should hydrate from its inbound signal");
assert(label.textContent === "Initial", "text should hydrate from its inbound signal");
assert(button.style.display === "", "truthy inbound branch should start visible");

className.set("panel-updated");
liveLabel.set("Updated");
enabled.set(false);
await Promise.resolve();

assert(root.getAttribute("class") === "panel-updated", "class should observe the latest inbound value");
assert(label.textContent === "Updated", "text should observe the latest inbound value");
assert(button.style.display === "none", "branch should re-evaluate when its inbound value changes");

enabled.set(true);
await Promise.resolve();
root.__rsjs_callbacks = {
  __callback_0(payload) {
    currentCallbackPayload = payload;
  },
};
button.dispatchEvent({ type: "click", target: button });
await Promise.resolve();

assert(staleCallbackCalls === 0, "callback resolution must not capture the callback present at mount time");
assert(currentCallbackPayload === "Updated", "callback should receive the latest live payload");

dispose();
assert(
  (root._listeners.get("click") ?? []).length === 0,
  "fragment disposal should remove its delegated click listener",
);

finish("rsjs conformance runtime live inbound fields");
