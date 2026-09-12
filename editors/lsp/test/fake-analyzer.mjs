// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// A deterministic subprocess for testing transport and lifecycle contracts.
import { Decoder, encode } from '../src/transport.mjs';
const documents = new Map();
const send = message => process.stdout.write(encode({ jsonrpc: '2.0', ...message }));
const decoder = new Decoder(message => {
  const { method, params, id } = message;
  if (method === 'initialize') send({ id, result: { capabilities: { positionEncoding: params.capabilities.general.positionEncodings[0] } } });
  else if (method === 'textDocument/didOpen') documents.set(params.textDocument.uri, params.textDocument.text);
  else if (method === 'textDocument/didChange') documents.set(params.textDocument.uri, params.contentChanges[0].text);
  else if (method === 'test/document') send({ id, result: { text: documents.get(params.uri), pid: process.pid } });
  else if (method === 'test/crash') process.exit(23);
  else if (method === 'shutdown') send({ id, result: null });
  else if (method === 'exit') process.exit(0);
  else if (id !== undefined) send({ id, result: params ?? null });
});
process.stdin.on('data', chunk => decoder.push(chunk));
process.stdin.on('end', () => process.exit(0));
