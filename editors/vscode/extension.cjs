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
  const { LiveLint } = await import('./src/core/lint.mjs');
  const api = await vscode.extensions.getExtension('rust-lang.rust-analyzer').activate();
  const output = vscode.window.createOutputChannel('Proa');
  context.subscriptions.push(output);
  let current, attachment, busy = false, stopped = false;
  let liveLint;
  const diagnostics = vscode.languages.createDiagnosticCollection('proa');
  const rustFile = doc => doc.languageId === 'rust' && doc.uri.scheme === 'file';
  const openForLint = doc => {
    if (rustFile(doc)) liveLint?.fromClient({ method: 'textDocument/didOpen', params: { textDocument: {
      uri: doc.uri.toString(), text: doc.getText(), version: doc.version,
    } } });
  };
  const range = value => new vscode.Range(value.start.line, value.start.character, value.end.line, value.end.character);
  function startLint() {
    liveLint?.dispose();
    liveLint = undefined;
    diagnostics.clear();
    const config = vscode.workspace.getConfiguration('proa.lint');
    if (!config.get('enable', true)) return;
    liveLint = new LiveLint({
      command: config.get('path', 'proa'),
      publish: params => {
        const doc = vscode.workspace.textDocuments.find(doc => doc.uri.toString() === params.uri);
        if (params.version !== undefined && doc && params.version !== doc.version) return;
        diagnostics.set(vscode.Uri.parse(params.uri), params.diagnostics.map(item => {
          const diagnostic = new vscode.Diagnostic(range(item.range), item.message, item.severity - 1);
          diagnostic.source = item.source;
          diagnostic.code = item.code;
          diagnostic.relatedInformation = item.relatedInformation?.map(info =>
            new vscode.DiagnosticRelatedInformation(new vscode.Location(vscode.Uri.parse(info.location.uri), range(info.location.range)), info.message));
          return diagnostic;
        }));
      },
      onError: error => {
        output.appendLine(error.message);
        void vscode.window.showWarningMessage(`Proa live lint: ${error.message}`);
      },
    });
    for (const doc of vscode.workspace.textDocuments) openForLint(doc);
  }
  startLint();
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument(openForLint),
    vscode.workspace.onDidChangeTextDocument(event => {
      if (rustFile(event.document)) liveLint?.fromClient({ method: 'textDocument/didChange', params: {
        textDocument: { uri: event.document.uri.toString(), version: event.document.version }, contentChanges: event.contentChanges,
      } });
    }),
    vscode.workspace.onDidCloseTextDocument(doc => liveLint?.fromClient({ method: 'textDocument/didClose', params: { textDocument: { uri: doc.uri.toString() } } })),
    vscode.workspace.onDidSaveTextDocument(doc => liveLint?.fromClient({ method: 'textDocument/didSave', params: { textDocument: { uri: doc.uri.toString() } } })),
    vscode.workspace.onDidChangeConfiguration(event => { if (event.affectsConfiguration('proa.lint')) startLint(); }),
    { dispose: () => { liveLint?.dispose(); diagnostics.dispose(); } },
  );

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
    liveLint?.dispose();
    clearInterval(timer);
    const selected = new Set(attachment?.documents.documents.keys());
    attachment?.dispose();
    if (current?.isRunning()) await resync(current, selected);
  };
  return { get client() { return current; }, get documents() { return attachment?.documents; } };
};

exports.deactivate = async () => { await stop?.(); stop = undefined; };
