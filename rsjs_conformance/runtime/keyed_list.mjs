#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Keyed lists: `{for item in items.get() { html_sync! { <li key={item}>{item}</li> } }}`
// lowers to `bind_list(container, () => __sig_0.get(), { key, create,
// adopt, deps })`. Hydration must adopt the server-rendered `<li>`s by
// `data-rsjs-key` (same DOM objects, no re-creation), and subsequent
// signal writes must insert, remove, and reorder children while
// preserving node identity for every surviving key.

import {
  assert,
  assetIdFromSnapshot,
  finish,
  importSnapshotWithRuntime,
  installDocument,
  makeElement,
} from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/template/list_keyed.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);

const root = makeElement("rsjs-island", { id: islandId });
const div = makeElement("div");
const shuffleButton = makeElement("button", { "data-rsjs-handler": "1" });
const removeButton = makeElement("button", { "data-rsjs-handler": "3" });
const list = makeElement("ul", { "data-rsjs-list": "5" });
const liAlpha = makeElement("li", { class: "todo", "data-rsjs-key": "Alpha" });
liAlpha.textContent = "Alpha";
const liBeta = makeElement("li", { class: "todo", "data-rsjs-key": "Beta" });
liBeta.textContent = "Beta";
list.appendChild(liAlpha);
list.appendChild(liBeta);
div.appendChild(shuffleButton);
div.appendChild(removeButton);
div.appendChild(list);
root.appendChild(div);

// The dehydrated state blob rides the wrapper's next sibling.
const host = makeElement("body");
host.appendChild(root);
const seed = makeElement("script", {
  type: "application/json",
  "data-rsjs-island": islandId,
});
seed.textContent = JSON.stringify({ signals: [["Alpha", "Beta"]] });
host.appendChild(seed);

installDocument(root);

const { runtime } = await importSnapshotWithRuntime(
  snapshotPath,
  "template-list-keyed.mjs",
);

const texts = () => list.children.map((child) => child.textContent).join(",");

// Hydration adopts the SSR children: same objects, same order, no
// re-created DOM.
assert(
  texts() === "Alpha,Beta",
  `hydration must keep the server-rendered items, got ${JSON.stringify(texts())}`,
);
assert(
  list.children[0] === liAlpha && list.children[1] === liBeta,
  "hydration must adopt the SSR <li> nodes by data-rsjs-key instead of re-creating them",
);

// Shuffle: ["Alpha","Beta"] -> ["Gamma","Beta","Alpha"]. Gamma is a
// new key (created), Beta and Alpha survive and reorder.
shuffleButton.dispatchEvent({ type: "click", target: shuffleButton });
await Promise.resolve();
await Promise.resolve();

assert(
  texts() === "Gamma,Beta,Alpha",
  `the shuffle write must insert and reorder, got ${JSON.stringify(texts())}`,
);
const liGamma = list.children[0];
assert(
  liGamma !== liAlpha && liGamma !== liBeta && liGamma.tagName === "LI",
  "the inserted key must get a freshly created node",
);
assert(
  liGamma.getAttribute("class") === "todo",
  "create() must re-apply the template's static attributes",
);
assert(
  list.children[1] === liBeta && list.children[2] === liAlpha,
  "reordered keys must keep their adopted SSR nodes (same objects, moved)",
);

// Remove: ["Gamma","Beta","Alpha"] -> ["Gamma","Alpha"]. Beta leaves
// the DOM; the survivors keep their identity.
removeButton.dispatchEvent({ type: "click", target: removeButton });
await Promise.resolve();
await Promise.resolve();

assert(
  texts() === "Gamma,Alpha",
  `the remove write must drop the missing key, got ${JSON.stringify(texts())}`,
);
assert(
  list.children[0] === liGamma && list.children[1] === liAlpha,
  "surviving keys must keep their nodes across a removal (created and adopted alike)",
);
assert(
  liBeta.parentElement === null,
  "the removed key's node must leave the container",
);

// Disposal stops updates but leaves the current DOM in place.
const disposed = runtime.dispose_hydrated(islandId);
assert(disposed === 1, `dispose_hydrated should tear down exactly the hydrated island, got ${disposed}`);

shuffleButton.dispatchEvent({ type: "click", target: shuffleButton });
await Promise.resolve();
await Promise.resolve();

assert(
  texts() === "Gamma,Alpha",
  `a disposed island must not reconcile further writes, got ${JSON.stringify(texts())}`,
);

finish("rsjs conformance runtime keyed list");
