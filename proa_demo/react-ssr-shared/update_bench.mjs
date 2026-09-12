// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Update-latency microbenchmark: React (VDOM re-render + reconcile) vs
// fine-grained signals (direct DOM write, the mechanism rsjs compiles to).
// N rows, M single-row updates each, forced synchronous so we time the real
// "change one value -> DOM reflects it" cost.
import React from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";

const N = 1000; // rows in the list
const M = 2000; // single-row updates

function Row({ i, v }) {
  return React.createElement("div", { className: "row" }, `Row ${i}: ${v}`);
}
let setVals;
function List() {
  const [vals, set] = React.useState(() => Array.from({ length: N }, (_, i) => i));
  setVals = set;
  return React.createElement(
    React.Fragment,
    null,
    vals.map((v, i) => React.createElement(Row, { key: i, i, v }))
  );
}

function runReact() {
  const root = createRoot(document.getElementById("react-root"));
  flushSync(() => root.render(React.createElement(List)));
  const upd = (k) => flushSync(() => setVals((p) => { const c = p.slice(); c[k % N] = k; return c; }));
  for (let k = 0; k < 200; k++) upd(k); // warmup
  const t0 = performance.now();
  for (let k = 0; k < M; k++) upd(k);
  const t1 = performance.now();
  return { totalMs: +(t1 - t0).toFixed(2), perUpdateUs: +((t1 - t0) * 1000 / M).toFixed(2) };
}

function runSignal() {
  const mount = document.getElementById("signal-root");
  const nodes = [];
  for (let i = 0; i < N; i++) {
    const d = document.createElement("div");
    d.className = "row";
    d.textContent = `Row ${i}: ${i}`;
    mount.appendChild(d);
    nodes.push(d);
  }
  const upd = (k) => { nodes[k % N].textContent = `Row ${k % N}: ${k}`; };
  for (let k = 0; k < 200; k++) upd(k); // warmup
  const t0 = performance.now();
  for (let k = 0; k < M; k++) upd(k);
  const t1 = performance.now();
  return { totalMs: +(t1 - t0).toFixed(2), perUpdateUs: +((t1 - t0) * 1000 / M).toFixed(2) };
}

const react = runReact();
const signal = runSignal();
const out = { N, M, react, signal, ratio: +(react.perUpdateUs / signal.perUpdateUs).toFixed(1) };
document.getElementById("results").textContent = JSON.stringify(out, null, 2);
console.log("BENCH_RESULT " + JSON.stringify(out));
