#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Score the docs search without a browser.
//
// Loads the search documents the docs build emits, indexes them with the same
// module the site ships (proa_marketing/public/search-index.mjs, which pulls
// the vendored Orama bundle), and runs every query in
// scripts/search-battery.json, reporting where the expected page landed.
//
//   node scripts/search_battery.mjs                 # local server on :5555
//   node scripts/search_battery.mjs --source prod   # https://proa.so
//   node scripts/search_battery.mjs --source ./documents.json
//   node scripts/search_battery.mjs --config tuning.json --verbose
//
// Exit status is 1 when the top-1 or top-3 hit counts fall below the
// thresholds in the fixture (override with --min-top1 / --min-top3).

import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');

const SOURCES = {
  local: 'http://127.0.0.1:5555/docs/_search/documents.json',
  prod: 'https://proa.so/docs/_search/documents.json',
};

const args = parseArgs(process.argv.slice(2));
if (args.help) {
  console.log(usage());
  process.exit(0);
}

const fixture = JSON.parse(await readFile(resolve(here, 'search-battery.json'), 'utf8'));
const indexModule = await import(
  pathToFileURL(resolve(root, 'proa_marketing/public/search-index.mjs')).href
);
const { buildIndex, searchIndex, DEFAULT_CONFIG } = indexModule;

let config = DEFAULT_CONFIG;
if (args.config) {
  const override = JSON.parse(await readFile(resolve(args.config), 'utf8'));
  config = {
    ...DEFAULT_CONFIG,
    ...override,
    boost: { ...DEFAULT_CONFIG.boost, ...(override.boost || {}) },
    relevance: { ...DEFAULT_CONFIG.relevance, ...(override.relevance || {}) },
  };
}

const source = args.source || 'local';
const documents = await loadDocuments(source);
const withDescription = documents.filter((doc) => doc.description).length;

const buildStart = performance.now();
const db = await buildIndex(documents, config);
const buildMs = performance.now() - buildStart;

const rows = [];
let queryMs = 0;
for (const entry of fixture.queries) {
  const start = performance.now();
  const outcome = await searchIndex(db, entry.query, config);
  queryMs += performance.now() - start;
  const urls = outcome.hits.map((hit) => hit.url);
  const accepted = Array.isArray(entry.expect) ? entry.expect : [entry.expect];
  const ranks = accepted.map((url) => urls.indexOf(url) + 1).filter((rank) => rank > 0);
  const rank = ranks.length > 0 ? Math.min(...ranks) : 0;
  rows.push({
    query: entry.query,
    expect: accepted,
    rank,
    pass: outcome.pass,
    count: outcome.count,
    top: outcome.hits.slice(0, 3).map((hit) => hit.href),
  });
}

const total = rows.length;
const top1 = rows.filter((row) => row.rank === 1).length;
const top3 = rows.filter((row) => row.rank >= 1 && row.rank <= 3).length;
const topN = rows.filter((row) => row.rank >= 1).length;
const misses = rows.filter((row) => row.rank !== 1);

if (args.json) {
  const { writeFile } = await import('node:fs/promises');
  await writeFile(
    resolve(args.json),
    JSON.stringify({ source, documents: documents.length, config, top1, top3, topN, total, rows }, null, 2)
  );
}

console.log(`source: ${source} (${documents.length} documents, ${withDescription} with a description)`);
console.log(`index: built in ${buildMs.toFixed(0)} ms, ${(queryMs / total).toFixed(2)} ms per query`);
console.log('');
console.log(`top-1: ${top1}/${total}   top-3: ${top3}/${total}   top-${config.limit}: ${topN}/${total}`);
console.log('');

const shown = args.verbose ? rows : misses;
if (shown.length > 0) {
  console.log(args.verbose ? 'all queries:' : 'queries whose expected page is not first:');
  for (const row of shown) {
    const where = row.rank === 0 ? 'absent' : `#${row.rank}`;
    console.log(`  ${pad(row.query, 28)} ${pad(where, 7)} ${pad(row.pass, 7)} ${row.top.join('  ')}`);
  }
  console.log('');
}

const minTop1 = args.minTop1 ?? fixture.thresholds?.top1 ?? 0;
const minTop3 = args.minTop3 ?? fixture.thresholds?.top3 ?? 0;
if (top1 < minTop1 || top3 < minTop3) {
  console.error(`FAIL: need top-1 >= ${minTop1} and top-3 >= ${minTop3}`);
  process.exit(1);
}
console.log(`PASS: top-1 >= ${minTop1} and top-3 >= ${minTop3}`);

async function loadDocuments(name) {
  const target = SOURCES[name] || name;
  let text;
  if (/^https?:\/\//.test(target)) {
    const response = await fetch(target);
    if (!response.ok) throw new Error(`${response.status} fetching ${target}`);
    text = await response.text();
  } else {
    text = await readFile(resolve(target), 'utf8');
  }
  const payload = JSON.parse(text);
  return Array.isArray(payload) ? payload : payload.documents;
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => argv[++i];
    switch (arg) {
      case '--source':
        out.source = next();
        break;
      case '--config':
        out.config = next();
        break;
      case '--json':
        out.json = next();
        break;
      case '--min-top1':
        out.minTop1 = Number(next());
        break;
      case '--min-top3':
        out.minTop3 = Number(next());
        break;
      case '--verbose':
      case '-v':
        out.verbose = true;
        break;
      case '--help':
      case '-h':
        out.help = true;
        break;
      default:
        throw new Error(`unknown argument ${arg}\n${usage()}`);
    }
  }
  return out;
}

function usage() {
  return [
    'usage: node scripts/search_battery.mjs [--source local|prod|<url|file>] [--config file.json]',
    '                                       [--json out.json] [--min-top1 N] [--min-top3 N] [--verbose]',
  ].join('\n');
}

function pad(value, width) {
  const text = String(value);
  return text.length >= width ? text : text + ' '.repeat(width - text.length);
}
