// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { LiveLint, diagnosticGroups } from '../src/lint.mjs';

const uri = 'file:///project/src/html.rs';
const markdown = 'file:///project/src/md.rs';
const original = 'fn f() { md_sync! { r###"Hello {self.title}"### }; }';
const open = (uri, text = original, version = 1) => ({ method: 'textDocument/didOpen', params: { textDocument: { uri, text, version } } });
const close = uri => ({ method: 'textDocument/didClose', params: { textDocument: { uri } } });
const change = (uri, text, version = 2) => ({ method: 'textDocument/didChange', params: { textDocument: { uri, version }, contentChanges: [{ text }] } });
function missing() {
  return { severity: 'error', code: 'render/content-coverage', message: 'Missing Notice', notes: ['HTML renders the child'], help: 'Render the child in Markdown', labels: [
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

test('converts exact scalar snapshot ranges to UTF-16, retaining counterpart evidence', () => {
  const diagnostics = diagnosticGroups([missing()]).get(markdown);
  assert.equal(diagnostics[0].range.start.character, 3); // astral character adds a UTF-16 unit
  assert.equal(diagnostics[0].range.end.character, 7);
  assert.equal(diagnostics[0].relatedInformation[0].location.uri, uri);
  assert.equal(diagnostics[0].severity, 1);
  assert(diagnostics[0].message.includes('Render the child in Markdown'));
});

test('sends original unsaved source, publishes unopened counterparts, and clears repaired warnings', async t => {
  const snapshots = [];
  const { live, published } = setup(t, async overlays => {
    snapshots.push(overlays);
    return snapshots.length === 1 ? [missing()] : [];
  });
  live.fromClient(open(uri));
  await live.flush();
  assert.equal(snapshots[0][0].source, original);
  assert.equal(published[0].uri, markdown);
  assert.equal(published[0].version, undefined); // the counterpart is unopened
  live.fromClient(open(markdown, 'unsaved repair', 4));
  await live.flush();
  assert.equal(snapshots[1][1].source, 'unsaved repair');
  assert.deepEqual(published.at(-1), { uri: markdown, version: 4, diagnostics: [] });
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

test('a transient snapshot read failure can recover on the next edit', async t => {
  let calls = 0;
  const { live, published, errors } = setup(t, async () => {
    if (++calls === 1) throw Object.assign(new Error('File changed during discovery'), { fatal: false });
    return [missing()];
  });
  live.fromClient(open(uri));
  await live.flush();
  assert.equal(errors.length, 1);
  live.fromClient(change(uri, 'repaired'));
  await live.flush();
  assert.equal(published[0].diagnostics[0].code, 'render/content-coverage');
});

test('filesystem watcher refreshes a disk counterpart while its view stays closed', { timeout: 5000 }, async t => {
  const root = mkdtempSync(join(tmpdir(), 'proa-lint-watch-'));
  mkdirSync(join(root, 'src'));
  writeFileSync(join(root, 'Cargo.toml'), "[package]\nname='watch_fixture'\nversion='0.1.0'\n");
  writeFileSync(join(root, 'src/lib.rs'), '');
  const md = join(root, 'src/md.rs');
  writeFileSync(md, 'before');
  let notify;
  const changed = new Promise(resolve => { notify = resolve; });
  const live = new LiveLint({ publish() {}, delay: 20, worker: {
    analyze: async () => { notify(); return []; }, dispose() {},
  } });
  t.after(() => { live.dispose(); rmSync(root, { recursive: true, force: true }); });
  live.fromClient(open(pathToFileURL(join(root, 'src/lib.rs')).href));
  await live.flush();
  let notifyDisk;
  const diskChanged = new Promise(resolve => { notifyDisk = resolve; });
  notify = notifyDisk;
  writeFileSync(md, 'after');
  await diskChanged;
  await changed;
  assert.equal(live.documents.size, 1);
});
