// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import test from 'node:test';
import assert from 'node:assert/strict';
import { Decoder, encode } from '../src/transport.mjs';

test('decodes fragmented headers, split Unicode bytes, and several messages in one chunk', () => {
  const messages = [
    { jsonrpc: '2.0', id: 1, method: 'initialize', params: {} },
    { jsonrpc: '2.0', method: 'textDocument/didOpen', params: { text: '🦀 café 名\r\n' } },
    { jsonrpc: '2.0', id: 1, result: null },
  ];
  for (const size of [1, 7, 1024]) {
    const received = [];
    const decoder = new Decoder(message => received.push(message));
    const bytes = Buffer.concat(messages.map(encode));
    for (let i = 0; i < bytes.length; i += size) decoder.push(bytes.subarray(i, i + size));
    decoder.end();
    assert.deepEqual(received, messages);
  }
});

test('rejects malformed framing and truncated messages', () => {
  for (const header of ['Content-Length: -1', 'Content-Length: 999999999', 'X-Length: 1', 'Content-Length: 1\r\nContent-Length: 2']) {
    assert.throws(() => new Decoder(() => {}).push(Buffer.from(header + '\r\n\r\n')));
  }
  const decoder = new Decoder(() => {});
  decoder.push(encode({ jsonrpc: '2.0', id: 1, result: 'hello' }).subarray(0, -1));
  assert.throws(() => decoder.end(), /Incomplete/);
});
