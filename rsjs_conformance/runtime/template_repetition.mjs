#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, makeElement } from "./harness.mjs";

const root = makeElement("div");

const todayRow = makeElement("div", {
  "data-rsjs-payload-p0": JSON.stringify("Today"),
  "data-rsjs-payload-p1": JSON.stringify("Today"),
});
const todayInput = makeElement("input", { "data-rsjs-input-value": "3" });
const todayThen = makeElement("strong", { "data-rsjs-cond-then": "2:0" });
const todayElse = makeElement("span", { "data-rsjs-cond-else": "2:0" });
todayElse.style.display = "none";
const todayButton = makeElement("button", { "data-rsjs-handler": "8" });
todayRow.appendChild(todayInput);
todayRow.appendChild(todayThen);
todayRow.appendChild(todayElse);
todayRow.appendChild(todayButton);

const tomorrowRow = makeElement("div", {
  "data-rsjs-payload-p0": JSON.stringify("Tomorrow"),
  "data-rsjs-payload-p1": JSON.stringify("Tomorrow"),
});
const tomorrowInput = makeElement("input", { "data-rsjs-input-value": "3" });
const tomorrowThen = makeElement("strong", { "data-rsjs-cond-then": "2:0" });
tomorrowThen.style.display = "none";
const tomorrowElse = makeElement("span", { "data-rsjs-cond-else": "2:0" });
const tomorrowButton = makeElement("button", { "data-rsjs-handler": "8" });
tomorrowRow.appendChild(tomorrowInput);
tomorrowRow.appendChild(tomorrowThen);
tomorrowRow.appendChild(tomorrowElse);
tomorrowRow.appendChild(tomorrowButton);

root.appendChild(todayRow);
root.appendChild(tomorrowRow);

let callbackPayload;
root.__rsjs_callbacks = {
  __callback_0(payload) {
    callbackPayload = payload;
  },
};

const snapshotPath = "rsjs_conformance/snapshots/template/repetition.fragment.js";
const fragmentAssetId = await assetIdFromSnapshot(snapshotPath);
const mod = await importSnapshot(snapshotPath, "template-repetition.mjs");
const dispose = mod[`mount_${fragmentAssetId}`](root);

assert(typeof dispose === "function", "fragment mount should return a disposer");
assert((root._listeners.get("click") ?? []).length === 1, "click event should be delegated through one root listener");
assert(todayInput.value === "Today", `first repeated input should hydrate from the signal, got ${JSON.stringify(todayInput.value)}`);
assert(tomorrowInput.value === "Today", `second repeated input should hydrate from the signal, got ${JSON.stringify(tomorrowInput.value)}`);
assert(todayThen.style.display === "", "today then branch should be visible initially");
assert(todayElse.style.display === "none", "today else branch should be hidden initially");
assert(tomorrowThen.style.display === "none", "tomorrow then branch should be hidden initially");
assert(tomorrowElse.style.display === "", "tomorrow else branch should be visible initially");

tomorrowInput.value = "Tomorrow";
tomorrowInput.dispatchEvent({ type: "input", target: tomorrowInput });
await Promise.resolve();

assert(todayInput.value === "Tomorrow", `first repeated input should mirror edits, got ${JSON.stringify(todayInput.value)}`);
assert(tomorrowInput.value === "Tomorrow", `second repeated input should keep edited value, got ${JSON.stringify(tomorrowInput.value)}`);
assert(todayThen.style.display === "none", "today then branch should hide after editing the shared signal");
assert(todayElse.style.display === "", "today else branch should show after editing the shared signal");
assert(tomorrowThen.style.display === "", "tomorrow then branch should show after editing the shared signal");
assert(tomorrowElse.style.display === "none", "tomorrow else branch should hide after editing the shared signal");

const consoleLog = console.log;
const logs = [];
console.log = (...args) => {
  logs.push(args);
};
try {
  tomorrowButton.dispatchEvent({ type: "click", target: tomorrowButton });
  await Promise.resolve();
} finally {
  console.log = consoleLog;
}

assert(callbackPayload === "Tomorrow", `component callback should receive decoded payload, got ${JSON.stringify(callbackPayload)}`);
assert(logs.length === 1, `handler should log once, got ${logs.length}`);
assert(logs[0]?.[0] === "Tomorrow", `handler should log decoded payload, got ${JSON.stringify(logs[0]?.[0])}`);

finish("rsjs conformance runtime template repetition");
