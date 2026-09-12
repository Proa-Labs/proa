// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

const vscode = require('vscode');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

exports.run = async function run() {
  const file = process.env.PROA_EDITOR_TEST_FILE;
  assert(file, 'Set PROA_EDITOR_TEST_FILE to a Rust fixture or issue_report.rs');
  const uri = vscode.Uri.file(file);
  const before = fs.readFileSync(file, 'utf8');
  const doc = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(doc);
  const extension = await vscode.extensions.getExtension('proa-labs.proa').activate();
  assert.equal(vscode.workspace.getConfiguration('rust-analyzer').get('server.path'), null);
  const sourceOffset = before.indexOf('**{a.user_name.as_str()}');
  assert(sourceOffset >= 0);
  const fieldStart = doc.positionAt(before.indexOf('user_name', sourceOffset));
  const position = fieldStart.translate(0, 2);
  const expectedFile = file.endsWith('issue_report.rs')
    ? path.resolve(path.dirname(file), '../../domain/mod.rs') : file;
  const targetSource = fs.readFileSync(expectedFile, 'utf8');
  async function definitionFor(pos, expectedName) {
    let definitions;
    for (let attempt = 0; attempt < 180; attempt++) {
      definitions = await vscode.commands.executeCommand('vscode.executeDefinitionProvider', uri, pos);
      if (definitions?.length) break;
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
    assert.equal(definitions?.length, 1, JSON.stringify(definitions));
    const definition = definitions[0];
    assert.equal((definition.targetUri ?? definition.uri).fsPath, expectedFile);
    const target = definition.targetSelectionRange ?? definition.range;
    const targetLine = targetSource.split('\n')[target.start.line];
    assert.equal(targetLine.slice(target.start.character, target.end.character), expectedName);
    return target;
  }
  const target = await definitionFor(position, 'user_name');
  const legend = extension.client.initializeResult.capabilities.semanticTokensProvider.legend.tokenTypes;
  const tokens = await extension.client.sendRequest('textDocument/semanticTokens/full', { textDocument: { uri: uri.toString() } });
  let line = 0, column = 0;
  const seen = {};
  for (let index = 0; index < tokens.data.length; index += 5) {
    const [dl, dc, length, kind] = tokens.data.slice(index, index + 5);
    line += dl; column = dl ? dc : column + dc;
    if (line === fieldStart.line) seen[doc.lineAt(line).text.slice(column, column + length)] = legend[kind];
  }
  assert.equal(seen.user_name, 'property');
  assert.equal(seen.as_str, 'method');
  assert(['parameter', 'variable'].includes(seen.a));
  assert(extension.documents.documents.get(uri.toString()).templates.length);
  const formatted = await extension.client.sendRequest('textDocument/formatting', {
    textDocument: { uri: uri.toString() }, options: { tabSize: 4, insertSpaces: true },
  });
  assert(Array.isArray(formatted));
  if (formatted.length) {
    assert(formatted[0].newText.includes('**{a.user_name.as_str()}**'));
    assert(formatted[0].newText.includes('r###"'));
  }
  const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, position);
  assert(hover?.length, 'Rust hover is missing');
  const rename = await vscode.commands.executeCommand('vscode.executeDocumentRenameProvider', uri, position, 'display_name');
  assert(rename?.size, 'Rust rename is missing');
  assert([...rename.entries()].flatMap(([, edits]) => edits).some(edit => edit.newText === 'display_name'));
  for (const text of ['row.user_name', 'row.note.is_empty', 'person.user_name']) {
    const offset = before.indexOf(text, sourceOffset);
    if (offset >= 0) await definitionFor(doc.positionAt(offset + text.indexOf('.') + 2), text.includes('.note') ? 'note' : 'user_name');
  }
  if (process.env.PROA_EDITOR_TEST_MUTATION === '1') {
    try {
      const edit = new vscode.WorkspaceEdit();
      edit.replace(uri, new vscode.Range(fieldStart, fieldStart.translate(0, 9)), 'note');
      assert(await vscode.workspace.applyEdit(edit));
      await definitionFor(position, 'note');
    } finally {
      const restore = new vscode.WorkspaceEdit();
      restore.replace(uri, new vscode.Range(new vscode.Position(0, 0), doc.positionAt(doc.getText().length)), before);
      assert(await vscode.workspace.applyEdit(restore));
    }
    // Completion must work during an incomplete field access, not only after
    // the template is valid Rust again. No source is saved by this test.
    try {
      const start = before.indexOf('a.user_name.as_str()', sourceOffset);
      const edit = new vscode.WorkspaceEdit();
      edit.replace(uri, new vscode.Range(doc.positionAt(start), doc.positionAt(start + 'a.user_name.as_str()'.length)), 'a.');
      assert(await vscode.workspace.applyEdit(edit));
      const at = doc.positionAt(start + 2);
      const completions = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', uri, at, '.');
      assert(completions?.items.some(item => (item.label.label ?? item.label) === 'user_name'), 'field completion is missing');
      const rustCompletions = await extension.client.sendRequest('textDocument/completion', {
        textDocument: { uri: uri.toString() }, position: { line: at.line, character: at.character },
        context: { triggerKind: 2, triggerCharacter: '.' },
      });
      assert((rustCompletions?.items ?? rustCompletions)?.some(item => item.label === 'user_name'), 'rust-analyzer field completion is missing');
    } finally {
      const restore = new vscode.WorkspaceEdit();
      restore.replace(uri, new vscode.Range(new vscode.Position(0, 0), doc.positionAt(doc.getText().length)), before);
      assert(await vscode.workspace.applyEdit(restore));
    }
    const previousClient = extension.client;
    await vscode.commands.executeCommand('rust-analyzer.restartServer');
    for (let attempt = 0; attempt < 60 && extension.client === previousClient; attempt++) {
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
    assert.notEqual(extension.client, previousClient);
    await definitionFor(position, 'user_name');
  }
  assert.equal(doc.getText(), before);
  assert.equal(fs.readFileSync(file, 'utf8'), before);
  const result = { file, targetFile: expectedFile, target: target.start, highlights: seen,
    sourceUnchanged: true, serverPathOverride: null, editsAndRestart: process.env.PROA_EDITOR_TEST_MUTATION === '1' };
  if (process.env.PROA_EDITOR_TEST_RESULT) fs.writeFileSync(process.env.PROA_EDITOR_TEST_RESULT, JSON.stringify(result, null, 2));
  console.log('PASS: real VS Code navigation and highlighting through existing rust-analyzer; source unchanged');
};
