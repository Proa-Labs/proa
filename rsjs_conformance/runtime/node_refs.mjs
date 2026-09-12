#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const calls = [];
const snapshotPath = "rsjs_conformance/snapshots/browser_api/node_refs.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const wrapper = makeElement("div");
const input = makeElement("input", {
  "data-kind": "search",
  "data-rsjs-ref": "1",
  hidden: "",
});
const form = makeElement("form", { "data-rsjs-ref": "2" });
const dialog = makeElement("dialog", { "data-rsjs-ref": "3" });
const popover = makeElement("div", {
  "data-rsjs-ref": "5",
  popover: "",
});
const button = makeElement("button", { "data-rsjs-handler": "7" });
const current = makeElement("span", { "data-rsjs-bind": "9" });

input.value = "before";
for (const name of ["focus", "click", "select"]) {
  input[name] = () => calls.push(`input.${name}`);
}
input.scrollTo = (x, y) => calls.push(`input.scrollTo:${x},${y}`);
input.scrollBy = (x, y) => calls.push(`input.scrollBy:${x},${y}`);
input.setSelectionRange = (start, end) => calls.push(`input.setSelectionRange:${start},${end}`);
input.style = {
  setProperty(name, value) {
    calls.push(`input.style.setProperty:${name}=${value}`);
  },
};
form.requestSubmit = () => calls.push("form.requestSubmit");
form.reset = () => calls.push("form.reset");
dialog.showModal = () => calls.push("dialog.showModal");
dialog.close = () => calls.push("dialog.close");
popover.showPopover = () => calls.push("popover.showPopover");
popover.hidePopover = () => calls.push("popover.hidePopover");
popover.togglePopover = () => calls.push("popover.togglePopover");

wrapper.appendChild(input);
wrapper.appendChild(form);
wrapper.appendChild(dialog);
wrapper.appendChild(popover);
wrapper.appendChild(button);
wrapper.appendChild(current);
root.appendChild(wrapper);
installDocument(root);

await importSnapshot(
  snapshotPath,
  "node-refs.mjs",
);

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

for (const expected of [
  "input.focus",
  "input.click",
  "form.requestSubmit",
  "form.reset",
  "dialog.showModal",
  "dialog.close",
  "popover.showPopover",
  "popover.hidePopover",
  "popover.togglePopover",
  "input.scrollTo:0,32",
  "input.scrollBy:0,8",
  "input.select",
  "input.setSelectionRange:0,0",
  "input.style.setProperty:outline-color=transparent",
]) {
  assert(calls.includes(expected), `expected ${expected} call`);
}
assert(input.value === "", `input value should be cleared, got ${JSON.stringify(input.value)}`);
assert(input.getAttribute("aria-expanded") === "false", "aria-expanded should be set through the ref");
assert(!input.hasAttribute("hidden"), "hidden attribute should be removed through the ref");
assert(logs.length === 1, `handler should log once, got ${logs.length}`);
assert(logs[0]?.[0] === "search", `logged data-kind should be "search", got ${JSON.stringify(logs[0]?.[0])}`);
assert(logs[0]?.[1] === false, `logged hidden state should be false, got ${JSON.stringify(logs[0]?.[1])}`);
assert(current.textContent === "", `current value binding should remain empty after clear, got ${JSON.stringify(current.textContent)}`);

finish("rsjs conformance runtime node refs");
