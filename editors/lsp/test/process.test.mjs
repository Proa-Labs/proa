// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { Decoder, encode } from '../src/transport.mjs';

const cli = fileURLToPath(new URL('../bin/proa-lsp.mjs', import.meta.url));
const analyzer = fileURLToPath(new URL('./fake-analyzer.mjs', import.meta.url));
function launch(t, args = ['--rust-analyzer', process.execPath, '--', analyzer]) {
  const child = spawn(process.execPath, [cli, ...args], { stdio: ['pipe', 'pipe', 'pipe'] });
  let stderr = '';
  const replies = new Map();
  const decoder = new Decoder(message => {
    const pending = replies.get(message.id);
    if (pending) { replies.delete(message.id); clearTimeout(pending.timer); pending.resolve(message); }
  });
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', chunk => { stderr += chunk; });
  child.stdout.on('data', chunk => decoder.push(chunk));
  const closed = once(child, 'close');
  t.after(() => { child.kill(); for (const pending of replies.values()) clearTimeout(pending.timer); });
  let next = 0;
  return { child, closed, stderr: () => stderr,
    notify(method, params) { child.stdin.write(encode({ jsonrpc: '2.0', method, params })); },
    request(method, params) {
      const id = ++next;
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`No response to ${method}: ${stderr}`)), 5000);
        replies.set(id, { resolve, timer });
        // Exercise framing through actual OS pipes, not just an in-memory parser.
        const frame = encode({ jsonrpc: '2.0', id, method, params });
        child.stdin.write(frame.subarray(0, 11));
        child.stdin.write(frame.subarray(11));
      });
    },
  };
}

test('CLI projects unsaved documents over stdio and shuts down its analyzer', { timeout: 10000 }, async t => {
  const peer = launch(t);
  const init = await peer.request('initialize', { capabilities: { general: { positionEncodings: ['utf-8', 'utf-16'] } } });
  assert.equal(init.result.capabilities.positionEncoding, 'utf-16');
  const uri = 'file:///project/src/lib.rs';
  const source = 'fn main() { md_sync! { r###"🦀 Hello {a.name}"### }; }';
  peer.notify('textDocument/didOpen', { textDocument: { uri, text: source, version: 1, languageId: 'rust' } });
  const opened = (await peer.request('test/document', { uri })).result;
  assert.equal(opened.text.length, source.length);
  assert.equal(opened.text.indexOf('a.name'), source.indexOf('a.name'));
  assert(!opened.text.includes('Hello'));
  peer.notify('textDocument/didChange', { textDocument: { uri, version: 2 }, contentChanges: [{
    range: { start: { line: 0, character: source.indexOf('name') }, end: { line: 0, character: source.indexOf('name') + 4 } }, text: 'note',
  }] });
  assert((await peer.request('test/document', { uri })).result.text.includes('{a.note}'));
  assert.equal((await peer.request('shutdown')).result, null);
  peer.notify('exit');
  assert.equal((await peer.closed)[0], 0, peer.stderr());
  assert.throws(() => process.kill(opened.pid, 0), { code: 'ESRCH' });
});

test('CLI closes the analyzer on editor EOF', { timeout: 10000 }, async t => {
  const peer = launch(t);
  await peer.request('initialize', { capabilities: {} });
  const { pid } = (await peer.request('test/document', { uri: 'file:///none' })).result;
  peer.child.stdin.end();
  assert.equal((await peer.closed)[0], 0);
  assert.throws(() => process.kill(pid, 0), { code: 'ESRCH' });
});

test('CLI reports analyzer crashes and spawn failures through stderr', { timeout: 10000 }, async t => {
  const peer = launch(t);
  await peer.request('initialize', { capabilities: {} });
  peer.notify('test/crash');
  assert.equal((await peer.closed)[0], 23);
  assert.match(peer.stderr(), /Restart the editor/);
  const missing = launch(t, ['--rust-analyzer', '/nonexistent/proa-test-rust-analyzer']);
  assert.equal((await missing.closed)[0], 1);
  assert.match(missing.stderr(), /Could not start/);
});
