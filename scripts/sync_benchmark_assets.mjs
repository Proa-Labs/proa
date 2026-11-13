#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { readFileSync, writeFileSync, unlinkSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
const destination = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = process.argv[2] && resolve(process.argv[2]);
const check = process.argv.includes('--check');
if (!source) throw new Error('Usage: node scripts/sync_benchmark_assets.mjs <benchmark-checkout> [--check]');
const manifest = readFileSync(join(destination, 'docs/asset-provenance.md'), 'utf8');
const expected = /Benchmark source revision: `([a-f0-9]{40})`/.exec(manifest)?.[1];
const actual = execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], {encoding: 'utf8'}).trim();
if (!expected || actual !== expected) throw new Error('Source checkout must match the recorded benchmark revision');
if (execFileSync('git', ['-C', source, 'status', '--porcelain', '--untracked-files=no'], {encoding: 'utf8'}).trim()) {
  throw new Error('Source checkout has tracked changes; use a clean benchmark checkout');
}
let count = 0;
function emit(target, content) {
  const path = join(destination, target);
  if (check) {
    if (!readFileSync(path).equals(Buffer.from(content))) throw new Error(`Stale benchmark asset: ${target}`);
  } else {
    writeFileSync(path, content);
  }
  count++;
}
for (const [local, upstream] of [
  ['01-canonical-scoreboard.svg', '01-canonical-scoreboard.svg'],
  ['21-canonical-pages-http.svg', '08-canonical-pages-http.svg'],
  ['22-cold-start.svg', '09-cold-start.svg'],
]) {
  const content = readFileSync(join(source, 'charts/svg', upstream));
  emit(`assets/benchmarks/${local}`, content);
  if (local !== '22-cold-start.svg') emit(`proa_marketing/public/images/benchmarks/${local}`, content);
}
// The upstream figure mixes Xeon Marko/React with Mac Proa/SvelteKit rows.
// Keep only the matched-host table; leave every retained measurement unchanged.
const figure = join(source, 'charts/figures/suspense.mjs');
let code = readFileSync(figure, 'utf8')
  .replace('const COLORS = { Proa: C.proa, Marko: C.peer, React: C.alt, SvelteKit: C.kit };', 'const COLORS = { Proa: C.proa, SvelteKit: C.kit };')
  .replace("table.objs.filter(row => row.Engine !== 'Proa')", 'table.objs.filter(() => false)')
  .replace('const latW = (COL - latGap * 3) / 4;', 'const latW = (COL - latGap) / 2;')
  .replace(' Marko/React: ${meta.streamPeerDate ?? meta.suspenseDate}. Same host and offered rates.', ' Apple M4 Max; same offered rates.')
  .replace('SvelteKit response end delivers data; the others deliver resolved HTML. Browser completion: sveltekit/browser.svg.', 'SvelteKit response end delivers data; Proa delivers resolved HTML. See methodology for browser completion.')
  .replace('Proa, Marko, React and SvelteKit streaming', 'Proa and SvelteKit streaming')
  .replace('the other streams deliver resolved HTML.', 'Proa delivers resolved HTML.')
  .replace('; Marko and React retained ${meta.streamPeerDate ?? meta.suspenseDate}. One worker thread or process per engine.', '. Apple M4 Max; one worker thread or process per engine.');
const temporary = join(source, `charts/figures/.proa-matched-streaming-${process.pid}.mjs`);
writeFileSync(temporary, code, {flag: 'wx'});
try {
  const {readSources, parseTables, readMeta} = await import(pathToFileURL(join(source, 'charts/lib/tables.mjs')));
  const {suspenseLoad} = await import(pathToFileURL(temporary));
  const md = readSources(source);
  const svg = suspenseLoad(parseTables(md), readMeta(md));
  if (/Marko|React|Same host and offered rates/.test(svg)) throw new Error('unmatched-host series remain');
  for (const target of ['assets/benchmarks/20-suspense-load.svg', 'proa_marketing/public/images/benchmarks/20-suspense-load.svg']) {
    emit(target, svg);
  }
} finally {
  unlinkSync(temporary);
}

console.log(`${check ? "Verified" : "Updated"} ${count} benchmark assets from ${actual}.`);
