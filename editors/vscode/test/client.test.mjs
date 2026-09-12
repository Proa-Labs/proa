// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import test from 'node:test';
import assert from 'node:assert/strict';
import { attachClient } from '../src/client.mjs';

const uri = 'file:///tmp/src/lib.rs';
const source = 'fn f() { md_sync! { r#"Hello {a.name}"# }; }';
function fakeClient(middleware = {}) {
  return { clientOptions: { middleware }, protocol2CodeConverter: { asWorkspaceEdit: value => value } };
}

test('composes existing middleware, supports both notification signatures, and restores hooks', async () => {
  let calls = 0;
  const previous = (type, next, params) => { calls++; return next(type, params); };
  const client = fakeClient({ sendNotification: previous });
  const originalConverter = client.protocol2CodeConverter.asWorkspaceEdit;
  const attached = attachClient(client);
  const params = { textDocument: { uri, text: source, version: 1 } };
  let sent;
  await client.clientOptions.middleware.sendNotification('textDocument/didOpen', (_, p) => { sent = p; }, params);
  assert.equal(calls, 1);
  assert.equal(params.textDocument.text, source);
  assert(!sent.textDocument.text.includes('Hello'));
  assert(sent.textDocument.text.includes('{a.name}'));
  attached.dispose();
  assert.equal(client.clientOptions.middleware.sendNotification, previous);
  assert.equal(client.protocol2CodeConverter.asWorkspaceEdit, originalConverter);
  const v10 = fakeClient();
  const other = attachClient(v10);
  await v10.clientOptions.middleware.sendNotification('textDocument/didOpen', params, (_, p) => { sent = p; });
  assert(!sent.textDocument.text.includes('Hello'));
  other.dispose();
});

test('blocks unsafe server workspace edits and cleans up cancelled requests', async () => {
  const client = fakeClient();
  const attached = attachClient(client, () => source);
  assert.throws(() => client.protocol2CodeConverter.asWorkspaceEdit({ changes: { [uri]: [{
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: source.length } }, newText: '',
  }] } }), /overwrite Markdown/);
  const token = { isCancellationRequested: false };
  await assert.rejects(client.clientOptions.middleware.sendRequest('textDocument/definition', {
    textDocument: { uri }, position: { line: 0, character: source.indexOf('name') },
  }, token, (_, __, actualToken) => {
    assert.equal(actualToken, token);
    throw new Error('cancelled');
  }), /cancelled/);
  assert.equal(attached.documents.requests.size, 0);
  attached.dispose();
});
