// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, renameSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { setImmediate } from 'node:timers/promises';
import { LiveLint, LintWorker, SourceWatcher, diagnosticGroups } from '../src/lint.mjs';

const uri = 'file:///project/src/html.rs';
const markdown = 'file:///project/src/md.rs';
const original = 'fn f() { md_sync! { r###"Hello {self.title}"### }; }';
const open = (uri, text = original, version = 1) => ({ method: 'textDocument/didOpen', params: { textDocument: { uri, text, version } } });
const close = uri => ({ method: 'textDocument/didClose', params: { textDocument: { uri } } });
const change = (uri, text, version = 2) => ({ method: 'textDocument/didChange', params: { textDocument: { uri, version }, contentChanges: [{ text }] } });
function missing() {
  return { severity: 'error', code: 'render/content-coverage', message: 'Missing Notice', labels: [
    { primary: true, range: { path: '/project/src/md.rs', line: 1, column: 3, end_line: 1, end_column: 7 }, source: [{ number: 1, text: '🦀 title' }] },
    { primary: false, range: { path: '/project/src/html.rs', line: 2, column: 1, end_line: 2, end_column: 7 }, source: [{ number: 2, text: 'Notice' }], message: 'Rendered in HTML' },
  ] };
}
function setup(t, analyze) {
  const published = [], errors = [];
  const live = new LiveLint({ publish: params => published.push(params), onError: error => errors.push(error),
    worker: { analyze, dispose() {} }, delay: 60000, watchFiles: false });
  t.after(() => live.dispose());
  return { live, published, errors };
}

test('converts scalar snapshot ranges to UTF-16', () => {
  const diagnostics = diagnosticGroups([missing()]).get(markdown);
  assert.equal(diagnostics[0].range.start.character, 3); // astral character adds a UTF-16 unit
  assert.equal(diagnostics[0].range.end.character, 7);
});

test('an edit in either view invalidates all pending diagnostics and coalesces requests', async t => {
  let resolve, calls = 0;
  const { live, published } = setup(t, overlays => {
    calls++;
    if (calls === 1) return new Promise(done => { resolve = done; });
    assert.equal(overlays[0].source, 'new HTML');
    assert.equal(overlays[1].source, 'new Markdown');
    return Promise.resolve([]);
  });
  live.fromClient(open(uri));
  const pending = live.flush();
  live.fromClient(change(uri, 'new HTML'));
  live.fromClient(open(markdown, 'new Markdown'));
  await live.flush(); // does not queue another worker request
  assert.equal(calls, 1);
  resolve([missing()]);
  await pending;
  assert.deepEqual(published, []);
  await live.flush();
  assert.equal(calls, 2);
});

test('incremental Unicode edits and closing unsaved buffers update the next snapshot', async t => {
  const snapshots = [];
  const { live, published } = setup(t, async overlays => { snapshots.push(overlays); return [missing()]; });
  live.fromClient(open(uri, '🦀 old'));
  live.fromClient({ method: 'textDocument/didChange', params: { textDocument: { uri, version: 2 }, contentChanges: [{
    range: { start: { line: 0, character: 3 }, end: { line: 0, character: 6 } }, text: 'new',
  }] } });
  live.fromClient(open(markdown));
  await live.flush();
  assert.equal(snapshots[0][0].source, '🦀 new');
  live.fromClient(close(uri));
  await live.flush();
  assert.deepEqual(snapshots[1].map(item => item.path), ['/project/src/md.rs']);
  live.fromClient(close(markdown));
  assert.deepEqual(published.at(-1), { uri: markdown, diagnostics: [] });
});

test('closed or disposed sessions never publish an in-flight result', async t => {
  for (const dispose of [false, true]) {
    let resolve;
    const { live, published } = setup(t, () => new Promise(done => { resolve = done; }));
    live.fromClient(open(uri));
    const pending = live.flush();
    if (dispose) live.dispose(); else live.fromClient(close(uri));
    resolve([missing()]);
    await pending;
    assert.deepEqual(published, []);
  }
});

test('worker failure clears stale diagnostics, reports once, and stops retrying each keystroke', async t => {
  let calls = 0;
  const { live, published, errors } = setup(t, async () => {
    if (++calls === 1) return [missing()];
    throw new Error('CLI unavailable');
  });
  live.fromClient(open(uri));
  await live.flush();
  live.fromClient(change(uri, 'changed'));
  await live.flush();
  assert.deepEqual(published.at(-1), { uri: markdown, diagnostics: [] });
  assert.equal(errors.length, 1);
  live.fromClient(change(uri, 'again', 3));
  await live.flush();
  assert.equal(calls, 2);
});

test('a timed-out worker is stopped and the next edit starts a fresh worker', { timeout: 5000 }, async t => {
  const children = [], errors = [];
  const worker = new LintWorker('proa', { timeout: 30, spawnProcess: () => {
    const child = Object.assign(new EventEmitter(), {
      stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), exitCode: null,
      kill() {
        this.exitCode = 0;
        this.stdout.end();
        queueMicrotask(() => this.emit('close', 0));
      },
    });
    children.push(child);
    if (children.length > 1) child.stdin.on('data', line => {
      child.stdout.write(JSON.stringify({ id: JSON.parse(line).id, diagnostics: [] }) + '\n');
    });
    return child;
  } });
  const live = new LiveLint({ publish() {}, onError: error => errors.push(error), worker, watchFiles: false, delay: 60000 });
  t.after(() => live.dispose());
  live.fromClient(open(uri));
  await live.flush();
  assert.match(errors[0].message, /timed out/);
  assert.equal(errors[0].fatal, false);
  assert.equal(children[0].exitCode, 0);
  assert.equal(live.failed, false);
  live.fromClient(change(uri, 'next edit'));
  await live.flush();
  assert.equal(children.length, 2);
  assert.equal(errors.length, 1);
  assert.equal(worker.pending.size, 0);
});

test('source watchers exclude build trees before traversal and track new or removed directories', { timeout: 5000 }, async t => {
  const root = mkdtempSync(join(tmpdir(), 'proa-source-watches-'));
  for (const directory of ['src/components', 'target/debug/deps', 'node_modules/dependency', 'src/node_modules/nested', '.git/objects', 'dist/assets']) {
    mkdirSync(join(root, directory), { recursive: true });
  }
  let changes = 0;
  const errors = [];
  const watcher = new SourceWatcher(root, { changed: () => changes++, onError: error => errors.push(error) });
  t.after(() => { watcher.dispose(); rmSync(root, { recursive: true, force: true }); });
  assert.deepEqual([...watcher.watchers.keys()].sort(), [root, join(root, 'src'), join(root, 'src/components')].sort());
  async function waitFor(predicate) {
    for (let attempt = 0; attempt < 100; attempt++) {
      if (predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    assert.fail('Source watchers did not refresh');
  }
  const added = join(root, 'src/new-components/nested');
  mkdirSync(added, { recursive: true });
  await waitFor(() => watcher.watchers.has(added));
  const before = changes;
  writeFileSync(join(added, 'view.rs'), 'fn new_view() {}');
  await waitFor(() => changes > before);
  rmSync(join(root, 'src/new-components'), { recursive: true });
  await waitFor(() => !watcher.watchers.has(added));
  assert.deepEqual(errors, []);
});

test('replacing a directory at the same path reinstalls its watch and observes later edits', { timeout: 5000 }, async t => {
  const root = mkdtempSync(join(tmpdir(), 'proa-replaced-watch-'));
  const directory = join(root, 'src/components');
  mkdirSync(directory, { recursive: true });
  const changes = [], errors = [];
  const watcher = new SourceWatcher(root, { changed: path => changes.push(path), onError: error => errors.push(error) });
  t.after(() => { watcher.dispose(); rmSync(root, { recursive: true, force: true }); });
  const original = watcher.watchers.get(directory);
  // Replace within the scan debounce while retaining the original inode.
  renameSync(directory, join(root, 'src/retired'));
  mkdirSync(directory);
  watcher.sync();
  assert.notEqual(watcher.watchers.get(directory).identity, original.identity);
  async function waitFor(predicate) {
    for (let attempt = 0; attempt < 100; attempt++) {
      if (predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    assert.fail('Replacement source watcher did not refresh');
  }
  await setImmediate();
  const file = join(directory, 'later.rs');
  writeFileSync(file, 'fn replacement() {}');
  await waitFor(() => changes.includes(file));
  assert.deepEqual(errors, []);
});
