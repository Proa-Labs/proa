#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

function makeStorage(seed) {
  const values = new Map(Object.entries(seed));
  const calls = [];
  return {
    calls,
    getItem(key) {
      calls.push(["getItem", key]);
      return values.has(key) ? values.get(key) : null;
    },
    setItem(key, value) {
      calls.push(["setItem", key, value]);
      values.set(key, String(value));
    },
    removeItem(key) {
      calls.push(["removeItem", key]);
      values.delete(key);
    },
  };
}

const snapshotPath = "rsjs_conformance/snapshots/browser_api/browser_effects.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const panel = makeElement("div", { "data-rsjs-attr": "0" });
const search = makeElement("span", { "data-rsjs-bind": "1" });
const button = makeElement("button", { "data-rsjs-handler": "2" });
panel.appendChild(search);
panel.appendChild(button);
root.appendChild(panel);
installDocument(root);

const events = [];
globalThis.document.dispatchEvent = (event) => {
  events.push(event);
  return true;
};
globalThis.document.title = "";

globalThis.CustomEvent = class CustomEvent {
  constructor(type, options = {}) {
    this.type = type;
    this.detail = options.detail;
  }
};

if (typeof globalThis.btoa !== "function") {
  globalThis.btoa = (value) => Buffer.from(value, "binary").toString("base64");
}

const localStorage = makeStorage({
  "": "filters=closed",
  enabled: "true",
  count: "7",
});
const sessionStorage = makeStorage({
  "": "session-value",
  ratio: "1.5",
});
const location = {
  href: "https://example.test/catalog?stale=1",
  search: "?stale=1",
  pathname: "/catalog",
  hash: "",
  assign(url) {
    calls.push(["location.assign", url]);
  },
  replace(url) {
    calls.push(["location.replace", url]);
  },
  reload() {
    calls.push(["location.reload"]);
  },
};
const calls = [];
const history = {
  pushState(state, title, url) {
    calls.push(["history.pushState", state, title, url]);
  },
  replaceState(state, title, url) {
    calls.push(["history.replaceState", state, title, url]);
  },
  back() {
    calls.push(["history.back"]);
  },
  forward() {
    calls.push(["history.forward"]);
  },
  go(delta) {
    calls.push(["history.go", delta]);
  },
};
globalThis.window = {
  localStorage,
  sessionStorage,
  location,
  history,
  scrollTo(x, y) {
    calls.push(["window.scrollTo", x, y]);
  },
  scrollBy(x, y) {
    calls.push(["window.scrollBy", x, y]);
  },
  print() {
    calls.push(["window.print"]);
  },
};

const clipboardWrites = [];
const credentialCalls = [];
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {
    clipboard: {
      writeText(value) {
        clipboardWrites.push(value);
        return Promise.resolve();
      },
    },
    credentials: {
      create(options) {
        credentialCalls.push(["create", options]);
        return Promise.resolve({ toJSON: () => ({ id: "created" }) });
      },
      get(options) {
        credentialCalls.push(["get", options]);
        return Promise.resolve({ toJSON: () => ({ id: "asserted" }) });
      },
    },
  },
});
globalThis.window.navigator = globalThis.navigator;
globalThis.PublicKeyCredential = {
  parseCreationOptionsFromJSON(options) {
    credentialCalls.push(["parseCreation", options]);
    return options;
  },
  parseRequestOptionsFromJSON(options) {
    credentialCalls.push(["parseRequest", options]);
    return options;
  },
};

await importSnapshot(
  snapshotPath,
  "browser-effects.mjs",
);

assert(search.textContent === "?stale=1", `location search should bind initially, got ${JSON.stringify(search.textContent)}`);
assert(panel.getAttribute("data-url") === location.href, "location href should bind to data-url initially");
assert((root._listeners.get("click") ?? []).length === 1, "click event should be delegated through one root listener");

const consoleLog = console.log;
const logs = [];
console.log = (...args) => {
  logs.push(args);
};
try {
  button.dispatchEvent({ type: "click", target: button });
  await Promise.resolve();
  await Promise.resolve();
} finally {
  console.log = consoleLog;
}

assert(location.hash === "filters=closed", `hash should be updated from localStorage, got ${JSON.stringify(location.hash)}`);
assert(globalThis.document.title === "filters=closed", `document title should update, got ${JSON.stringify(globalThis.document.title)}`);
assert(clipboardWrites[0] === "filters=closed", `clipboard should receive filter text, got ${JSON.stringify(clipboardWrites[0])}`);
assert(events.length === 2, `expected two document events, got ${events.length}`);
assert(events[0]?.type === "filters:changed", `first event type mismatch: ${JSON.stringify(events[0]?.type)}`);
assert(events[1]?.detail === "filters=closed", `second event detail mismatch: ${JSON.stringify(events[1]?.detail)}`);
assert(localStorage.calls.some((call) => call.join(":") === "setItem:filters:filters=closed"), "localStorage filters value should be written");
assert(localStorage.calls.some((call) => call.join(":") === "removeItem:filters:stale"), "localStorage stale value should be removed");
assert(sessionStorage.calls.some((call) => call.join(":") === "setItem:filters:filters=closed"), "sessionStorage filters value should be written");
assert(sessionStorage.calls.some((call) => call.join(":") === "removeItem:filters:stale"), "sessionStorage stale value should be removed");
assert(calls.some((call) => call.join(":") === "window.scrollTo:0:100"), "window.scrollTo should be called");
assert(calls.some((call) => call.join(":") === "window.scrollBy:0:20"), "window.scrollBy should be called");
assert(calls.some((call) => call.join(":") === "window.print"), "window.print should be called");
assert(calls.some((call) => call.join(":") === "location.assign:/catalog/filters"), "location.assign should be called");
assert(calls.some((call) => call.join(":") === "location.replace:filters=closed"), "location.replace should be called");
assert(calls.some((call) => call.join(":") === "location.reload"), "location.reload should be called");
assert(calls.some((call) => call.join(":") === "history.back"), "history.back should be called");
assert(calls.some((call) => call.join(":") === "history.forward"), "history.forward should be called");
assert(calls.some((call) => call.join(":") === "history.go:1"), "history.go should be called");
assert(credentialCalls.some(([name]) => name === "parseCreation"), "WebAuthn creation options should parse");
assert(credentialCalls.some(([name]) => name === "create"), "WebAuthn create should be called");
assert(credentialCalls.some(([name]) => name === "parseRequest"), "WebAuthn request options should parse");
assert(credentialCalls.some(([name]) => name === "get"), "WebAuthn get should be called");
assert(logs[0]?.[0] === "session-value", `session storage log mismatch: ${JSON.stringify(logs[0])}`);
assert(logs[1]?.[0] === true, `bool storage log mismatch: ${JSON.stringify(logs[1]?.[0])}`);
assert(
  logs[1]?.[1] === 7n,
  `integer storage log mismatch: ${String(logs[1]?.[1])} (${typeof logs[1]?.[1]})`,
);
assert(logs[1]?.[2] === 1.5, `float storage log mismatch: ${JSON.stringify(logs[1]?.[2])}`);
assert(logs[2]?.[0] === "ZmlsdGVycz1jbG9zZWQ", `base64url log mismatch: ${JSON.stringify(logs[2]?.[0])}`);

finish("rsjs conformance runtime browser effects");
