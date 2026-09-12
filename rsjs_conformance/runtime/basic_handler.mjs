#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importRuntime, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/handler/basic_event_signal.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const input = makeElement("input", {
  "data-rsjs-attr": "0",
  "data-rsjs-handler": "0",
});
root.appendChild(input);
installDocument(root);

await importSnapshot(
  snapshotPath,
  "basic-handler.mjs",
);

assert(input.value === "", `initial value should hydrate to empty string, got ${JSON.stringify(input.value)}`);
assert(input.getAttribute("value") === "", "initial value attribute should be present as an empty string");
assert((root._listeners.get("input") ?? []).length === 1, "input event should be delegated through one root listener");

input.value = "typed";
input.dispatchEvent({ type: "input", target: input });
await Promise.resolve();

assert(input.value === "typed", `input value should remain synced after event, got ${JSON.stringify(input.value)}`);
assert(input.getAttribute("value") === "typed", "value attribute should update after signal write");

const runtime = await importRuntime();

const textNode = makeElement("span");
const textSignal = runtime.signal("one");
const disposeText = runtime.bind_text(textNode, () => textSignal.get(), [textSignal]);
assert(textNode.textContent === "one", "bind_text should apply its initial value");
textSignal.set("two");
await Promise.resolve();
assert(textNode.textContent === "two", "bind_text should react before disposal");
disposeText();
textSignal.set("three");
await Promise.resolve();
assert(textNode.textContent === "two", "bind_text disposer should unsubscribe from signals");

const button = makeElement("button");
let clicks = 0;
const disposeEvent = runtime.bind_event(button, "click", () => {
  clicks += 1;
});
button.dispatchEvent({ type: "click", target: button });
disposeEvent();
button.dispatchEvent({ type: "click", target: button });
assert(clicks === 1, "bind_event disposer should remove the listener");

const boundInput = makeElement("input");
const inputSignal = runtime.signal("");
const disposeInput = runtime.bind_input(boundInput, "value", inputSignal);
boundInput.value = "before-dispose";
boundInput.dispatchEvent({ type: "input", target: boundInput });
assert(inputSignal.peek() === "before-dispose", "bind_input should publish user edits before disposal");
disposeInput();
boundInput.value = "after-dispose";
boundInput.dispatchEvent({ type: "input", target: boundInput });
assert(inputSignal.peek() === "before-dispose", "bind_input disposer should remove DOM listeners");
inputSignal.set("signal-after-dispose");
await Promise.resolve();
assert(boundInput.value === "after-dispose", "bind_input disposer should unsubscribe from signal writes");

finish("rsjs conformance runtime basic handler");
