#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/handler/named_event_handler_refs.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const section = makeElement("section");
const button = makeElement("button", {
  "data-date": "sample-day",
  "data-rsjs-handler": "1",
});
const picked = makeElement("span", { "data-rsjs-bind": "3" });
const logged = makeElement("span", { "data-rsjs-bind": "4" });
section.appendChild(button);
section.appendChild(picked);
section.appendChild(logged);
root.appendChild(section);
installDocument(root);

await importSnapshot(snapshotPath, "named-event-handler.mjs");

assert((root._listeners.get("click") ?? []).length === 1, "click event should be delegated through one root listener");

const consoleLog = console.log;
const logs = [];
console.log = (...args) => {
  logs.push(args);
};
try {
  button.dispatchEvent({ type: "click", target: button });
  await Promise.resolve();
} finally {
  console.log = consoleLog;
}

assert(logs.length === 1, `handler should log once, got ${logs.length}`);
assert(logs[0]?.[0] === "today", `handler payload should log "today", got ${JSON.stringify(logs[0]?.[0])}`);
assert(picked.textContent === "sample-day", `closest_attr should set picked date, got ${JSON.stringify(picked.textContent)}`);
assert(logged.textContent === "today", `payload should set logged label, got ${JSON.stringify(logged.textContent)}`);

finish("rsjs conformance runtime named event handler");
