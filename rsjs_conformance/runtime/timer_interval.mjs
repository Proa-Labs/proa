#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Interval timers: a prelude `rsjs::interval(20, || tick.update(|t| t + 1))`
// lowers to `__rsjs_disposers.push(interval(20, () => { ... }));`. The
// timer is real (setInterval): hydration seeds the bound text from the
// signal initial, a few awaited tick periods advance it, and disposing
// the island clears the interval so no further tick ever lands.

import {
  assert,
  assetIdFromSnapshot,
  finish,
  importSnapshotWithRuntime,
  installDocument,
  makeElement,
} from "./harness.mjs";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const snapshotPath = "rsjs_conformance/snapshots/timers/timer_interval.island.js";
const islandId = await assetIdFromSnapshot(snapshotPath);
const root = makeElement("rsjs-island", { id: islandId });
const div = makeElement("div");
const tick = makeElement("span", { "data-rsjs-bind": "1" });
div.appendChild(tick);
root.appendChild(div);
installDocument(root);

const { runtime } = await importSnapshotWithRuntime(
  snapshotPath,
  "timers-interval-basic.mjs",
);

assert(
  tick.textContent === "0",
  `hydration should seed the tick text from the signal initial, got ${JSON.stringify(tick.textContent)}`,
);

// Let roughly three 20ms tick periods elapse (plus scheduler flushes).
await sleep(90);
await Promise.resolve();
await Promise.resolve();

const ticked = Number(tick.textContent);
assert(
  Number.isInteger(ticked) && ticked >= 2,
  `the interval should have advanced the bound text by ~3 ticks after 90ms, got ${JSON.stringify(tick.textContent)}`,
);

// Island teardown must clear the interval: no tick may land afterwards.
const disposed = runtime.dispose_hydrated(islandId);
assert(disposed === 1, `dispose_hydrated should tear down exactly the hydrated island, got ${disposed}`);

const frozen = tick.textContent;
await sleep(70);
await Promise.resolve();
await Promise.resolve();

assert(
  tick.textContent === frozen,
  `a disposed island must never observe another tick, got ${JSON.stringify(tick.textContent)} after ${JSON.stringify(frozen)}`,
);

finish("rsjs conformance runtime interval timer");
