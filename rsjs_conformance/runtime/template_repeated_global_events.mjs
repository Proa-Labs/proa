#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, makeElement } from "./harness.mjs";

function makeEventTarget() {
  return {
    _listeners: new Map(),
    addEventListener(name, handler) {
      if (!this._listeners.has(name)) {
        this._listeners.set(name, []);
      }
      this._listeners.get(name).push(handler);
    },
    removeEventListener(name, handler) {
      const listeners = this._listeners.get(name);
      if (!listeners) {
        return;
      }
      this._listeners.set(
        name,
        listeners.filter((listener) => listener !== handler),
      );
    },
    dispatchEvent(event) {
      if (!event.target) {
        event.target = this;
      }
      for (const handler of this._listeners.get(event.type) ?? []) {
        handler(event);
      }
    },
  };
}

function attachDocument(node, doc) {
  node.ownerDocument = doc;
  for (const child of node.children ?? []) {
    attachDocument(child, doc);
  }
}

function repeatedRow(label) {
  return makeElement("li", {
    "data-rsjs-handler": "2",
    "data-rsjs-payload-p0": JSON.stringify(label),
    "data-rsjs-payload-p1": JSON.stringify(label),
  });
}

const snapshotPath = "rsjs_conformance/snapshots/template/repeated_global_events.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const ul = makeElement("ul");
const first = repeatedRow("A");
const second = repeatedRow("B");
ul.appendChild(first);
ul.appendChild(second);
root.appendChild(ul);

const win = makeEventTarget();
const doc = makeEventTarget();
doc.readyState = "complete";
doc.defaultView = win;
doc.querySelectorAll = (selector) => {
  if (selector === `rsjs-island[id="${islandId}"]`) {
    return [root];
  }
  if (selector === "rsjs-island[id]") {
    return [root];
  }
  // The runtime queries the marker attribute first and falls back to
  // the legacy id lookup when this returns nothing; this root only
  // carries an id, so marker selectors match nothing.
  if (/^rsjs-island\[data-rsjs-island-root(="[^"]*")?\]$/.test(selector)) {
    return [];
  }
  throw new Error(`unsupported selector in document shim: ${selector}`);
};
attachDocument(root, doc);
globalThis.document = doc;
globalThis.window = win;

const consoleLog = console.log;
const logs = [];
console.log = (...args) => {
  logs.push(args);
};
try {
  await importSnapshot(snapshotPath, "template-repeated-global-events.mjs");

  assert((doc._listeners.get("keydown") ?? []).length === 2, "document keydown should bind once per repeated owner");
  assert((win._listeners.get("resize") ?? []).length === 2, "window resize should bind once per repeated owner");

  doc.dispatchEvent({ type: "keydown", key: "Enter" });
  win.dispatchEvent({ type: "resize" });
} finally {
  console.log = consoleLog;
}

assert(logs.length === 4, `global handlers should log four calls, got ${logs.length}`);
assert(logs[0]?.[0] === "A" && logs[0]?.[1] === "Enter", `first document handler should decode A/key, got ${JSON.stringify(logs[0])}`);
assert(logs[1]?.[0] === "B" && logs[1]?.[1] === "Enter", `second document handler should decode B/key, got ${JSON.stringify(logs[1])}`);
assert(logs[2]?.[0] === "A" && logs[2]?.length === 1, `first window handler should decode A, got ${JSON.stringify(logs[2])}`);
assert(logs[3]?.[0] === "B" && logs[3]?.length === 1, `second window handler should decode B, got ${JSON.stringify(logs[3])}`);

finish("rsjs conformance runtime repeated global events");
