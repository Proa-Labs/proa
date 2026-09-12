// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { Bridge } from './protocol.mjs';

const methodName = type => typeof type === 'string' ? type : type.method;

// Attach to the LanguageClient exported by the official rust-analyzer extension.
// These are its public middleware/converter APIs; no server or transport is created.
export function attachClient(client, readDocument, { format } = {}) {
  const encoding = client.initializeResult?.capabilities?.positionEncoding;
  if (encoding && encoding !== 'utf-16') throw new Error('Proa Markdown requires UTF-16 editor positions.');
  const documents = new Bridge(readDocument);
  const middleware = client.clientOptions.middleware ??= {};
  const converter = client.protocol2CodeConverter;
  const previousNotification = middleware.sendNotification;
  const previousRequest = middleware.sendRequest;
  const previousWorkspaceEdit = converter.asWorkspaceEdit;
  let sequence = 0;

  function notification(...args) {
    // vscode-languageclient 9 uses (type, next, params); 10 uses (type, params, next).
    const nextIndex = typeof args[1] === 'function' ? 1 : 2;
    const paramsIndex = nextIndex === 1 ? 2 : 1;
    const result = documents.fromClient({
      method: methodName(args[0]), params: structuredClone(args[paramsIndex]),
    });
    args[paramsIndex] = result.forward.params;
    return previousNotification
      ? previousNotification(...args)
      : args[nextIndex](args[0], args[paramsIndex]);
  }

  async function request(type, params, token, next) {
    const uri = params?.textDocument?.uri;
    const original = uri && documents.document(uri);
    if (format && methodName(type) === 'textDocument/formatting' && original?.templates.length) {
      const result = await format(original, uri, token);
      // Do not replace an edit made while the formatter was running.
      return documents.document(uri)?.source === original.source ? result : null;
    }
    const id = ++sequence;
    const outgoing = documents.fromClient({ id, method: methodName(type), params: structuredClone(params) });
    if (outgoing.reply) return outgoing.reply.result;
    try {
      const result = await (previousRequest
        ? previousRequest(type, outgoing.forward.params, token, next)
        : next(type, outgoing.forward.params, token));
      const incoming = documents.fromServer({ id, result });
      if (incoming.forward.error) throw new Error(incoming.forward.error.message);
      return incoming.forward.result;
    } finally { documents.requests.delete(id); }
  }

  function workspaceEdit(edit, ...args) {
    // Includes server-initiated workspace/applyEdit, which bypasses request middleware.
    if (!documents.safeWorkspaceEdit(edit)) throw new Error('This Rust edit would overwrite Markdown template text.');
    return previousWorkspaceEdit.call(this, edit, ...args);
  }

  middleware.sendNotification = notification;
  middleware.sendRequest = request;
  converter.asWorkspaceEdit = workspaceEdit;

  return {
    documents,
    dispose() {
      if (middleware.sendNotification === notification) middleware.sendNotification = previousNotification;
      if (middleware.sendRequest === request) middleware.sendRequest = previousRequest;
      if (converter.asWorkspaceEdit === workspaceEdit) converter.asWorkspaceEdit = previousWorkspaceEdit;
    },
  };
}
