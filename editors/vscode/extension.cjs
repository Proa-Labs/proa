// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

const vscode = require('vscode');
const fs = require('node:fs');
const { fileURLToPath } = require('node:url');
let stop;

exports.activate = async function activate(context) {
  if (!vscode.workspace.isTrusted) return;
  const { attachClient } = await import('./src/client.mjs');
  const { formatDocument } = await import('./src/format.mjs');
  const api = await vscode.extensions.getExtension('rust-lang.rust-analyzer').activate();
  const output = vscode.window.createOutputChannel('Proa');
  context.subscriptions.push(output);
  let current, attachment, busy = false, stopped = false;

  const readDocument = uri => {
    const document = vscode.workspace.textDocuments.find(doc => doc.uri.toString() === uri);
    if (document) return document.getText();
    try { return uri?.startsWith('file:') ? fs.readFileSync(fileURLToPath(uri), 'utf8') : undefined; }
    catch { return undefined; }
  };

  async function resync(client, selected) {
    for (const doc of vscode.workspace.textDocuments) {
      if (doc.languageId !== 'rust' || doc.uri.scheme !== 'file') continue;
      const uri = doc.uri.toString();
      if (selected && !selected.has(uri)) continue;
      await client.sendNotification('textDocument/didClose', { textDocument: { uri } });
      await client.sendNotification('textDocument/didOpen', { textDocument: {
        uri, languageId: doc.languageId, version: doc.version, text: doc.getText(),
      } });
    }
  }

  async function refresh() {
    if (busy || stopped || !api.client?.isRunning() || current === api.client) return;
    busy = true;
    try {
      attachment?.dispose();
      current = undefined;
      attachment = attachClient(api.client, readDocument, {
        format: (document, uri, token) => {
          const config = vscode.workspace.getConfiguration('rust-analyzer', vscode.Uri.parse(uri));
          return formatDocument(document, uri, token, {
            overrideCommand: config.get('rustfmt.overrideCommand'),
            extraArgs: config.get('rustfmt.extraArgs'),
          });
        },
      });
      current = api.client;
      await resync(current);
      output.appendLine('Attached to the existing rust-analyzer client.');
    } finally { busy = false; }
  }

  await refresh();
  // rust-analyzer recreates its client on restart and exposes it through a getter.
  const timer = setInterval(() => {
    void refresh().catch(error => output.appendLine(error.stack ?? String(error)));
  }, 1000);
  stop = async () => {
    stopped = true;
    clearInterval(timer);
    const selected = new Set(attachment?.documents.documents.keys());
    attachment?.dispose();
    if (current?.isRunning()) await resync(current, selected);
  };
  return { get client() { return current; }, get documents() { return attachment?.documents; } };
};

exports.deactivate = async () => { await stop?.(); stop = undefined; };
