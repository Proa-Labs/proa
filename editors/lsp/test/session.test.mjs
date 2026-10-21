// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import test from 'node:test';
import assert from 'node:assert/strict';
import { setImmediate } from 'node:timers/promises';
import { Session } from '../src/session.mjs';

const uri = 'file:///project/src/lib.rs';
const source = 'fn f() { md_sync! { r###"🦀 **{a.name}**"### }; }';
const request = (id, method, params) => ({ jsonrpc: '2.0', id, method, params });
const range = (start, end = start) => ({ start: { line: 0, character: start }, end: { line: 0, character: end } });
function setup(options = {}) {
  const client = [], server = [];
  const session = new Session({ toClient: message => client.push(message), toServer: message => server.push(message), ...options });
  session.fromClient({ jsonrpc: '2.0', method: 'textDocument/didOpen', params: { textDocument: { uri, text: source, version: 1, languageId: 'rust' } } });
  return { session, client, server };
}
function change(session, text) {
  session.fromClient({ jsonrpc: '2.0', method: 'textDocument/didChange', params: { textDocument: { uri, version: 2 }, contentChanges: [{ text }] } });
}

test('live lints see original Markdown and merge with analyzer diagnostics without replacing either', async () => {
  const lintDiagnostic = { code: 'render/content-coverage', severity: 'error', message: 'Missing child', labels: [{
    primary: true, range: { path: '/project/src/lib.rs', line: 1, column: 1, end_line: 1, end_column: 3 },
    source: [{ number: 1, text: source }],
  }] };
  let result = [lintDiagnostic];
  const { session, client } = setup({ lint: { watchFiles: false, delay: 60000, worker: {
    analyze: async overlays => { assert.equal(overlays[0].source, source); return result; }, dispose() {},
  } } });
  const rustDiagnostic = { source: 'rust-analyzer', message: 'Rust error' };
  const native = diagnostics => session.fromServer({ jsonrpc: '2.0', method: 'textDocument/publishDiagnostics', params: { uri, diagnostics } });
  native([rustDiagnostic]);
  await session.liveLint.flush();
  assert.deepEqual(client.at(-1).params.diagnostics.map(d => d.source), ['rust-analyzer', 'proa']);
  native([]);
  assert.equal(client.at(-1).params.diagnostics[0].code, 'render/content-coverage');
  native([rustDiagnostic]);
  result = [];
  await session.liveLint.flush();
  assert.deepEqual(client.at(-1).params.diagnostics, [rustDiagnostic]);
  session.dispose();
});

test('negotiates UTF-16 on both sides while retaining analyzer capabilities and settings', () => {
  const { session, client, server } = setup();
  const settings = { cargo: { features: ['some-feature'] } };
  session.fromClient(request(1, 'initialize', { capabilities: {
    general: { positionEncodings: ['utf-8', 'utf-16', 'utf-32'] }, workspace: { configuration: true },
  }, initializationOptions: settings }));
  assert.deepEqual(server.at(-1).params.capabilities.general.positionEncodings, ['utf-16']);
  assert.deepEqual(server.at(-1).params.initializationOptions, settings);
  session.fromServer({ jsonrpc: '2.0', id: 1, result: { capabilities: { hoverProvider: true, positionEncoding: 'utf-16' } } });
  assert.equal(client.at(-1).result.capabilities.hoverProvider, true);
  assert.equal(client.at(-1).result.capabilities.positionEncoding, 'utf-16');
  assert.equal(client.at(-1).result.offsetEncoding, 'utf-16');
  session.dispose();
});

test('rejects incompatible encoding instead of returning wrong Unicode positions', () => {
  const { session, client, server } = setup();
  session.fromClient(request(1, 'initialize', { capabilities: { general: { positionEncodings: ['utf-8'] } } }));
  assert.equal(client.at(-1).error.code, -32602);
  assert.equal(server.length, 1);
  session.fromClient(request(2, 'initialize', { capabilities: {} }));
  session.fromServer({ jsonrpc: '2.0', id: 2, result: { capabilities: { positionEncoding: 'utf-8' } } });
  assert.equal(client.at(-1).error.code, -32603);
  session.dispose();
});

test('guards server edits and preserves IDs for bidirectional requests', () => {
  const { session, client, server } = setup();
  const position = range(source.indexOf('name')).start;
  session.fromClient(request(1, 'textDocument/definition', { textDocument: { uri }, position }));
  session.fromServer(request(1, 'workspace/configuration', { items: [{ section: 'rust-analyzer' }] }));
  const id = client.at(-1).id;
  assert.notEqual(id, 1);
  session.fromServer({ jsonrpc: '2.0', method: '$/cancelRequest', params: { id: 1 } });
  assert.equal(client.at(-1).params.id, id);
  session.fromClient({ jsonrpc: '2.0', id, result: [{ cargo: {} }] });
  assert.deepEqual(server.at(-1), { jsonrpc: '2.0', id: 1, result: [{ cargo: {} }] });
  session.fromServer({ jsonrpc: '2.0', id: 1, result: [{ uri, range: range(3, 5) }] });
  assert.equal(client.at(-1).id, 1);
  session.fromServer(request(2, 'workspace/applyEdit', { edit: { changes: { [uri]: [{ range: range(0, source.length), newText: '' }] } } }));
  assert.equal(server.at(-1).id, 2);
  assert.equal(server.at(-1).result.applied, false);
  assert.equal(session.pending.size, 0);
  session.dispose();
});

test('late responses after cancellation still pass edit guards', () => {
  const { session, client, server } = setup();
  session.fromClient(request(1, 'textDocument/rename', { textDocument: { uri }, position: range(source.indexOf('name')).start }));
  session.fromClient({ jsonrpc: '2.0', method: '$/cancelRequest', params: { id: 1 } });
  assert.equal(server.at(-1).method, '$/cancelRequest');
  session.fromServer({ jsonrpc: '2.0', id: 1, result: { changes: { [uri]: [{ range: range(0, source.length), newText: '' }] } } });
  assert.equal(client.at(-1).error.code, -32803);
  assert.equal(session.bridge.requests.size, 0);
  session.dispose();
});

test('formats original text with scoped client settings and without request-ID collisions', async () => {
  const settings = { extraArgs: ['--config', 'hard_tabs=true'] };
  const { session, client, server } = setup({ format: async (document, actualUri, token, config) => {
    assert.equal(document.source, source);
    assert.notEqual(document.text, source);
    assert.equal(actualUri, uri);
    assert.deepEqual(config, settings);
    return [{ range: range(0, source.length), newText: source + '\n' }];
  } });
  session.supportsConfiguration = true;
  session.fromServer(request('proa:1', 'workspace/configuration', { items: [] }));
  const upstreamId = client.at(-1).id;
  session.fromClient(request(1, 'textDocument/formatting', { textDocument: { uri } }));
  const config = client.at(-1);
  assert.equal(config.params.items[0].scopeUri, uri);
  assert.equal(config.params.items[0].section, 'rust-analyzer.rustfmt');
  assert.notEqual(config.id, upstreamId);
  session.fromClient({ jsonrpc: '2.0', id: config.id, result: [settings] });
  session.fromClient({ jsonrpc: '2.0', id: upstreamId, result: [] });
  await setImmediate();
  assert.equal(client.at(-1).result[0].newText, source + '\n');
  assert.equal(server.at(-1).id, 'proa:1');
  assert(!server.some(message => message.method === 'textDocument/formatting'));
  session.dispose();
});

test('discards formatting when text changes or the document closes', async () => {
  for (const action of ['change', 'close']) {
    let finish;
    const { session, client } = setup({ format: () => new Promise(resolve => { finish = resolve; }) });
    session.fromClient(request(1, 'textDocument/formatting', { textDocument: { uri } }));
    await setImmediate();
    if (action === 'change') change(session, source.replace('name', 'note'));
    else session.fromClient({ jsonrpc: '2.0', method: 'textDocument/didClose', params: { textDocument: { uri } } });
    finish([{ newText: 'stale' }]);
    await setImmediate();
    assert.equal(client.at(-1).result, null);
    session.dispose();
  }
});

test('cancels local formatting even while awaiting configuration and releases state', async () => {
  const { session, client, server } = setup({ format: () => { throw new Error('must not run'); } });
  session.supportsConfiguration = true;
  session.fromClient(request(1, 'textDocument/formatting', { textDocument: { uri } }));
  session.fromClient({ jsonrpc: '2.0', method: '$/cancelRequest', params: { id: 1 } });
  await setImmediate();
  assert.equal(client.at(-1).id, 1);
  assert.equal(client.at(-1).error.code, -32800);
  assert.equal(session.pending.size, 0);
  assert.equal(session.formatting.size, 0);
  assert(!server.some(message => message.method === '$/cancelRequest'));
  session.dispose();
});

test('uses initialization/configuration settings without workspace/configuration support', async () => {
  const options = { overrideCommand: ['custom-rustfmt', '--stdin'] };
  const { session, client } = setup({ format: async (_, __, ___, settings) => {
    assert.deepEqual(settings, options);
    throw new Error('formatter failed');
  } });
  session.fromClient({ jsonrpc: '2.0', method: 'workspace/didChangeConfiguration', params: { settings: { 'rust-analyzer': { rustfmt: options } } } });
  session.fromClient(request(1, 'textDocument/formatting', { textDocument: { uri } }));
  await setImmediate();
  assert.match(client.at(-1).error.message, /formatter failed/);
  session.dispose();
});
