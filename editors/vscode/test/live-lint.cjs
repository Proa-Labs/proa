// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Run in an isolated VS Code extension host; see ../README.md.
const vscode = require('vscode');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

exports.run = async function run() {
  const root = process.env.PROA_EDITOR_LINT_WORKSPACE;
  assert(root, 'Set PROA_EDITOR_LINT_WORKSPACE to a disposable fixture');
  const htmlUri = vscode.Uri.file(path.join(root, 'src/html.rs'));
  const mdUri = vscode.Uri.file(path.join(root, 'src/md.rs'));
  const htmlBefore = fs.readFileSync(htmlUri.fsPath, 'utf8');
  const mdBefore = fs.readFileSync(mdUri.fsPath, 'utf8');
  const html = await vscode.workspace.openTextDocument(htmlUri);
  await vscode.window.showTextDocument(html);
  await vscode.extensions.getExtension('proa-labs.proa').activate();
  const coverage = () => vscode.languages.getDiagnostics(mdUri).filter(d => d.source === 'proa' && d.code === 'render/content-coverage');
  async function waitFor(predicate, message) {
    for (let attempt = 0; attempt < 200; attempt++) {
      if (predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert.fail(`${message}: ${JSON.stringify(vscode.languages.getDiagnostics(mdUri))}`);
  }
  async function replace(doc, text) {
    const edit = new vscode.WorkspaceEdit();
    edit.replace(doc.uri, new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length)), text);
    assert(await vscode.workspace.applyEdit(edit));
  }
  let md;
  try {
    assert(!vscode.workspace.textDocuments.some(doc => doc.uri.toString() === mdUri.toString()), 'Counterpart must start unopened');
    await replace(html, htmlBefore.replace('{self.title}', '{self.title} {Notice::new()}'));
    await waitFor(() => coverage().length === 1, 'Missing closed-counterpart diagnostic');
    assert.equal(coverage()[0].severity, vscode.DiagnosticSeverity.Error);
    assert(coverage()[0].relatedInformation.some(info => info.location.uri.toString() === htmlUri.toString()));
    assert(html.isDirty);

    md = await vscode.workspace.openTextDocument(mdUri);
    await vscode.window.showTextDocument(md);
    await replace(md, mdBefore.replace('{self.title}', '{self.title} {crate::product::Notice::new()}'));
    await waitFor(() => coverage().length === 0, 'Unsaved repair did not clear diagnostic');
    assert(md.isDirty);
    await replace(md, mdBefore);
    await waitFor(() => coverage().length === 1, 'Removing repair did not restore diagnostic');
    await replace(md, mdBefore.replace('md_sync!', 'proa_macros::omit!(crate::product::Notice); md_sync!'));
    await waitFor(() => coverage().length === 0, 'Intentional omission did not clear diagnostic');
    assert.equal(fs.readFileSync(htmlUri.fsPath, 'utf8'), htmlBefore);
    assert.equal(fs.readFileSync(mdUri.fsPath, 'utf8'), mdBefore);
  } finally {
    await replace(html, htmlBefore);
    if (md) await replace(md, mdBefore);
  }
  fs.writeFileSync(path.join(root, 'proa-live-lint-test-result.json'), JSON.stringify({ passed: true }));
  console.log('PASS: VS Code shows live Proa diagnostics for unsaved HTML and closed Markdown, clears unsaved repairs and intentional omissions, and leaves disk unchanged');
};
