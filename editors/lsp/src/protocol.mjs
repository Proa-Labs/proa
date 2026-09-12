// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { applyChanges, project, safeRange } from './projection.mjs';

export class Bridge {
  documents = new Map();
  requests = new Map();
  resolveContexts = new Map();
  constructor(readDocument = () => undefined) { this.readDocument = readDocument; }

  document(uri) {
    if (this.documents.has(uri)) return this.documents.get(uri);
    const source = this.readDocument(uri);
    return typeof source === 'string' ? project(source) : undefined;
  }

  safeEdits(uri, edits) {
    const document = this.document(uri);
    return !document?.templates.length || edits.every(edit => {
      const ranges = edit.range ? [edit.range] : [edit.insert, edit.replace].filter(Boolean);
      return ranges.length > 0 && ranges.every(range => safeRange(document, range));
    });
  }

  safeWorkspaceEdit(edit) {
    if (!edit) return true;
    for (const [uri, edits] of Object.entries(edit.changes ?? {})) {
      if (!this.safeEdits(uri, edits)) return false;
    }
    for (const change of edit.documentChanges ?? []) {
      if (change.textDocument && !this.safeEdits(change.textDocument.uri, change.edits)) return false;
    }
    return true;
  }

  // Return a replacement message for the server, or an immediate client reply.
  fromClient(message) {
    const { method, params, id } = message;
    const uri = params?.textDocument?.uri ?? this.resolveContexts.get(JSON.stringify(params?.data));
    if (method === 'textDocument/didOpen') {
      const document = project(params.textDocument.text);
      this.documents.set(uri, document);
      params.textDocument.text = document.text;
    } else if (method === 'textDocument/didChange') {
      const previous = this.documents.get(uri);
      if (previous) {
        const document = project(applyChanges(previous.source, params.contentChanges));
        this.documents.set(uri, document);
        params.contentChanges = [{ text: document.text }];
      }
    } else if (method === 'textDocument/didClose') {
      this.documents.delete(uri);
    } else if (method === 'textDocument/didSave' && typeof params.text === 'string') {
      params.text = project(params.text).text;
    }
    if (id !== undefined && method) {
      const document = uri ? this.document(uri) : undefined;
      if (document?.templates.length) {
        // rustfmt would format the projection, losing literal Markdown.
        if (['textDocument/formatting', 'textDocument/rangeFormatting', 'textDocument/onTypeFormatting', 'textDocument/willSaveWaitUntil'].includes(method)) {
          return { reply: { jsonrpc: '2.0', id, result: null } };
        }
        if (params.position && !safeRange(document, { start: params.position, end: params.position })) {
          return { reply: { jsonrpc: '2.0', id, result: null } };
        }
      }
      this.requests.set(id, { method, uri });
    }
    return { forward: message };
  }

  remember(item, uri) {
    if (uri && item.data !== undefined) {
      this.resolveContexts.set(JSON.stringify(item.data), uri);
      if (this.resolveContexts.size > 512) this.resolveContexts.delete(this.resolveContexts.keys().next().value);
    }
  }

  fromServer(message) {
    if (message.method === 'workspace/applyEdit' && !this.safeWorkspaceEdit(message.params.edit)) {
      return { reply: { jsonrpc: '2.0', id: message.id, result: {
        applied: false, failureReason: 'This Rust edit would overwrite Markdown template text. Edit the template directly instead.',
      } } };
    }
    if (message.method || !this.requests.has(message.id)) return { forward: message };
    const { method, uri } = this.requests.get(message.id);
    this.requests.delete(message.id);
    const result = message.result;
    const projected = Boolean(uri && this.document(uri)?.templates.length);
    if (message.error || result === null || result === undefined) return { forward: message };
    if (['textDocument/rename', 'workspace/willCreateFiles', 'workspace/willRenameFiles', 'workspace/willDeleteFiles'].includes(method) && !this.safeWorkspaceEdit(result)) {
      return { forward: { jsonrpc: '2.0', id: message.id, error: {
        code: -32803, message: 'This workspace edit would overwrite Markdown template text; no edits were applied.',
      } } };
    } else if (method === 'textDocument/codeAction') {
      message.result = result.filter(action => (!projected || !action.command) && this.safeWorkspaceEdit(action.edit));
      for (const action of message.result) this.remember(action, uri);
    } else if (method === 'codeAction/resolve') {
      if ((projected && result.command) || !this.safeWorkspaceEdit(result.edit)) message.result = { title: result.title, disabled: { reason: 'This action cannot safely edit a projected Markdown template.' } };
    } else if (method === 'textDocument/completion') {
      const defaults = result.itemDefaults;
      const defaultEdit = defaults?.editRange;
      const safeItem = item => {
        const edit = item.textEdit ?? (defaultEdit && (defaultEdit.start ? { range: defaultEdit } : defaultEdit));
        return this.safeEdits(uri, [...(edit ? [edit] : []), ...(item.additionalTextEdits ?? [])]);
      };
      if (Array.isArray(result)) message.result = result.filter(safeItem);
      else result.items = result.items.filter(safeItem);
      for (const item of Array.isArray(message.result) ? message.result : result.items) {
        this.remember(item.data === undefined && defaults?.data !== undefined ? { data: defaults.data } : item, uri);
      }
    } else if (method === 'completionItem/resolve') {
      const edits = [...(result.textEdit ? [result.textEdit] : []), ...(result.additionalTextEdits ?? [])];
      if (!uri && edits.length && [...this.documents.values()].some(document => document.templates.length)) message.result = null;
      else if (uri && !this.safeEdits(uri, edits)) message.result = null;
    }
    return { forward: message };
  }
}
