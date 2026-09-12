#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from "./harness.mjs";

const snapshotPath = "rsjs_conformance/snapshots/browser_api/url_dynamic_attr.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);

const root = makeElement("rsjs-island", { id: islandId });
const wrapper = makeElement("div");
const link = makeElement("a", {
  "data-rsjs-attr": "1",
  href: "javascript:alert(1)",
});
const button = makeElement("button", {
  "data-rsjs-handler": "3",
});

wrapper.appendChild(link);
wrapper.appendChild(button);
root.appendChild(wrapper);
installDocument(root);

await importSnapshot(snapshotPath, "url-dynamic-attr.mjs");

assert(link.getAttribute("href") === "", `dangerous initial href should be cleared, got ${JSON.stringify(link.getAttribute("href"))}`);

button.dispatchEvent({ type: "click", target: button });
await Promise.resolve();

assert(
  link.getAttribute("href") === "https://example.com/safe",
  `safe href should be installed after signal update, got ${JSON.stringify(link.getAttribute("href"))}`,
);

finish("rsjs conformance runtime dynamic URL attr");
