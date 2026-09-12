// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { Bridge } from './protocol.mjs';
import { formatDocument } from './format.mjs';

function readDocument(uri) {
  try { return uri?.startsWith('file:') ? readFileSync(fileURLToPath(uri), 'utf8') : undefined; }
  catch { return undefined; }
}

function cancellation() {
  const listeners = new Set();
  return {
    isCancellationRequested: false,
    onCancellationRequested(listener) {
      listeners.add(listener);
      return { dispose: () => listeners.delete(listener) };
    },
    cancel() {
      this.isCancellationRequested = true;
      for (const listener of listeners) listener();
      listeners.clear();
    },
  };
}

export class Session {
  bridge;
  initializeId;
  supportsConfiguration = false;
  settings = {};
  sequence = 0;
  pending = new Map();
  formatting = new Map();
  disposed = false;

  constructor({ toClient, toServer, format = formatDocument, read = readDocument }) {
    this.toClient = toClient;
    this.toServer = toServer;
    this.format = format;
    this.bridge = new Bridge(read);
  }

  fromClient(message) {
    if (this.disposed) return;
    const { method, params, id } = message;
    if (!method) {
      const pending = this.pending.get(id);
      if (pending) {
        this.pending.delete(id);
        if (pending.resolve) pending.resolve(message.error ? undefined : message.result);
        else this.toServer({ ...message, id: pending.serverId });
      }
      return;
    }
    if (method === 'initialize') {
      const capabilities = params.capabilities ??= {};
      const encodings = capabilities.general?.positionEncodings;
      if (encodings && !encodings.includes('utf-16')) {
        this.toClient({ jsonrpc: '2.0', id, error: {
          code: -32602, message: 'Proa requires UTF-16 positions. Include utf-16 in general.positionEncodings.',
        } });
        return;
      }
      this.initializeId = id;
      this.supportsConfiguration = capabilities.workspace?.configuration === true;
      this.settings = params.initializationOptions ?? {};
      capabilities.general ??= {};
      capabilities.general.positionEncodings = ['utf-16'];
      if (capabilities.offsetEncoding) capabilities.offsetEncoding = ['utf-16'];
      // rust-analyzer should follow this bridge's lifetime.
      params.processId = process.pid;
    } else if (method === 'workspace/didChangeConfiguration') {
      this.settings = params.settings?.['rust-analyzer'] ?? params.settings ?? {};
    } else if (method === '$/cancelRequest') {
      const token = this.formatting.get(params.id);
      if (token) { token.cancel(); return; }
      // Keep request context until the response, so even a late response to a
      // cancelled rename/completion still passes the edit guards.
    }
    if (method === 'textDocument/formatting' && params.textDocument.uri.startsWith('file:')) {
      const original = this.bridge.document(params.textDocument.uri);
      if (original?.templates.length) {
        void this.formatOriginal(id, params.textDocument.uri, original);
        return;
      }
    }
    const result = this.bridge.fromClient(message);
    if (result.reply) this.toClient(result.reply);
    else this.toServer(result.forward);
  }

  fromServer(message) {
    if (this.disposed) return;
    if (message.method === '$/cancelRequest') {
      const entry = [...this.pending].find(([, pending]) => !pending.resolve && pending.serverId === message.params.id);
      if (entry) this.toClient({ ...message, params: { ...message.params, id: entry[0] } });
      return;
    }
    if (!message.method && message.id === this.initializeId && message.result) {
      const capabilities = message.result.capabilities;
      if (capabilities.positionEncoding && capabilities.positionEncoding !== 'utf-16') {
        this.toClient({ jsonrpc: '2.0', id: message.id, error: {
          code: -32603, message: 'rust-analyzer selected an unsupported position encoding; UTF-16 is required.',
        } });
        this.bridge.requests.delete(message.id);
        return;
      }
      capabilities.positionEncoding = 'utf-16';
      // Older clients use this extension instead of the standardized field.
      message.result.offsetEncoding = 'utf-16';
      this.initializeId = undefined;
    }
    const result = this.bridge.fromServer(message);
    if (result.reply) { this.toServer(result.reply); return; }
    if (message.method && message.id !== undefined) {
      // Give all server-to-client requests our own IDs so formatting's
      // workspace/configuration requests cannot collide with analyzer IDs.
      const id = `proa:${++this.sequence}`;
      this.pending.set(id, { serverId: message.id });
      result.forward.id = id;
    }
    this.toClient(result.forward);
  }

  configuration(uri, token) {
    if (!this.supportsConfiguration) return Promise.resolve(undefined);
    return new Promise(resolve => {
      const id = `proa:${++this.sequence}`;
      let done = false;
      const finish = result => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        subscription.dispose();
        this.pending.delete(id);
        resolve(result?.[0]);
      };
      const timer = setTimeout(() => finish(undefined), 5000);
      const subscription = token.onCancellationRequested(() => finish(undefined));
      this.pending.set(id, { resolve: finish });
      this.toClient({ jsonrpc: '2.0', id, method: 'workspace/configuration', params: {
        items: [{ scopeUri: uri, section: 'rust-analyzer.rustfmt' }],
      } });
    });
  }

  async formatOriginal(id, uri, original) {
    const token = cancellation();
    this.formatting.set(id, token);
    try {
      const config = await this.configuration(uri, token);
      const settings = config ?? this.settings.rustfmt ?? {};
      const edits = token.isCancellationRequested ? null : await this.format(original, uri, token, settings);
      if (!this.disposed) {
        if (token.isCancellationRequested) this.toClient({ jsonrpc: '2.0', id, error: { code: -32800, message: 'Formatting cancelled.' } });
        else this.toClient({ jsonrpc: '2.0', id, result:
          this.bridge.documents.get(uri) === original ? edits : null });
      }
    } catch (error) {
      if (!this.disposed) this.toClient({ jsonrpc: '2.0', id, error: { code: -32603, message: error.message } });
    } finally { this.formatting.delete(id); }
  }

  dispose() {
    this.disposed = true;
    for (const token of this.formatting.values()) token.cancel();
    this.formatting.clear();
    this.pending.clear();
    this.bridge.requests.clear();
    this.bridge.documents.clear();
    this.bridge.resolveContexts.clear();
  }
}
