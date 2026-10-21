// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Deliberately separate from the fast test suite: requires a built Proa CLI.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { once } from 'node:events';
import { spawn } from 'node:child_process';
import { LintWorker } from '../src/lint.mjs';
import { Decoder, encode } from '../src/transport.mjs';

const command = process.env.PROA_TEST_LINT || fileURLToPath(new URL(`../../../target/debug/proa${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url));
const html = 'use crate::product::{Product,Notice};use proa_core::WebRenderSync;use proa_macros::html_sync;impl WebRenderSync for Product {fn render(self,cx:&mut Cx)->Result {html_sync! {{self.title}}.render(cx)}}';
const md = 'use crate::product::Product;use proa_core::MdRenderSync;use proa_macros::md_sync;impl MdRenderSync for Product {fn render_md(self,cx:&mut Cx)->Result {md_sync! {r###"{self.title}"###}.render_md(cx)}}';
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'proa-live-integration-'));
  mkdirSync(join(root, 'src'));
  for (const [path, source] of Object.entries({ 'Cargo.toml': "[package]\nname='live_fixture'\nversion='0.1.0'\n", 'src/lib.rs': 'mod product;mod html;mod md;',
    'src/product.rs': 'pub struct Product {pub title:String}pub struct Notice;', 'src/html.rs': html, 'src/md.rs': md })) writeFileSync(join(root, path), source);
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}
const open = (uri, text, version = 1) => ({ method: 'textDocument/didOpen', params: { textDocument: { uri, text, version, languageId: 'rust' } } });

test('standalone LSP diagnoses unsaved changes, clears repairs, and watches closed counterparts', { timeout: 10000 }, async t => {
  const root = fixture(t);
  const bridge = fileURLToPath(new URL('../bin/proa-lsp.mjs', import.meta.url));
  const analyzer = fileURLToPath(new URL('./fake-analyzer.mjs', import.meta.url));
  const child = spawn(process.execPath, [bridge, '--proa', command, '--rust-analyzer', process.execPath, '--', analyzer]);
  const closed = once(child, 'close');
  t.after(() => child.kill());
  let stderr = '';
  child.stderr.on('data', data => { stderr += data; });
  const replies = new Map();
  const waiting = new Set();
  const diagnostics = predicate => new Promise(resolve => { waiting.add({ predicate, resolve }); });
  const decoder = new Decoder(message => {
    if (replies.has(message.id)) { replies.get(message.id)(message); replies.delete(message.id); }
    if (message.method === 'textDocument/publishDiagnostics') {
      for (const pending of waiting) if (pending.predicate(message.params)) {
        waiting.delete(pending);
        pending.resolve(message.params);
      }
    }
  });
  child.stdout.on('data', data => decoder.push(data));
  const send = message => child.stdin.write(encode({ jsonrpc: '2.0', ...message }));
  const request = (id, method) => new Promise(resolve => { replies.set(id, resolve); send({ id, method, params: { capabilities: {} } }); });
  const htmlUri = pathToFileURL(join(root, 'src/html.rs')).href;
  const mdUri = pathToFileURL(join(root, 'src/md.rs')).href;
  const hasCoverage = params => params.diagnostics.some(d => d.code === 'render/content-coverage');
  await request(1, 'initialize');
  const missing = diagnostics(params => params.uri === mdUri && hasCoverage(params));
  send(open(htmlUri, html.replace('{self.title}', '{self.title} {Notice::new()}')));
  const result = await missing;
  const omission = result.diagnostics.find(d => d.code === 'render/content-coverage');
  assert.equal(omission.severity, 1);
  assert(omission.relatedInformation.some(info => info.location.uri === htmlUri));

  const repaired = md.replace('{self.title}', '{self.title} {crate::product::Notice::new()}');
  const cleared = diagnostics(params => params.uri === mdUri && params.version === 2 && !hasCoverage(params));
  send(open(mdUri, md));
  send({ method: 'textDocument/didChange', params: { textDocument: { uri: mdUri, version: 2 }, contentChanges: [{ text: repaired }] } });
  await cleared;
  assert.equal(readFileSync(join(root, 'src/md.rs'), 'utf8'), md);

  const discarded = diagnostics(params => params.uri === mdUri && hasCoverage(params));
  send({ method: 'textDocument/didClose', params: { textDocument: { uri: mdUri } } });
  await discarded;
  const diskRepair = diagnostics(params => params.uri === mdUri && !hasCoverage(params));
  writeFileSync(join(root, 'src/md.rs'), repaired);
  await diskRepair;
  assert.equal(readFileSync(join(root, 'src/html.rs'), 'utf8'), html);
  assert.equal(readFileSync(join(root, 'src/md.rs'), 'utf8'), repaired);
  await request(2, 'shutdown');
  send({ method: 'exit' });
  assert.equal((await closed)[0], 0, stderr);
});

test('missing CLI gives an actionable failure without hanging', { timeout: 5000 }, async () => {
  const worker = new LintWorker('/nonexistent/proa-test-lint');
  try { await assert.rejects(worker.analyze([]), /Install an updated Proa CLI/); }
  finally { worker.dispose(); }
});
