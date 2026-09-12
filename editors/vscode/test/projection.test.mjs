// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import test from 'node:test';
import assert from 'node:assert/strict';
import { project, applyChanges, safeRange } from '../src/projection.mjs';
import { Bridge } from '../src/protocol.mjs';

const source = 'fn show() { md_sync! { r###"**{a.user_name.as_str()}** · via {client}"### }; }';
const uri = 'file:///project/src/lib.rs';
function range(start, end = start) { return { start: { line: 0, character: start }, end: { line: 0, character: end } }; }
function bridgeFor(text = source) {
  const bridge = new Bridge();
  bridge.fromClient({ method: 'textDocument/didOpen', params: { textDocument: { uri, text, version: 1 } } });
  return bridge;
}

test('exposes expressions as token composition at their original positions', () => {
  const result = project(source);
  assert.equal(result.text, source.replace('r###"**', '       ').replace('** · via ', '         ').replace('"###', '    '));
  assert.equal(result.text.indexOf('a.user_name'), source.indexOf('a.user_name'));
  assert.equal(result.templates.length, 1);
  assert.equal(result.templates[0].expressions.length, 2);
});

test('preserves UTF-16 positions, CRLF, and surrounding Rust', () => {
  const text = 'fn show() {\r\n  proa_macros::md! [r#"🦀 café · {data.名}\r\n{next}"#];\r\n}\r\n';
  const result = project(text);
  assert.equal(result.text.length, text.length);
  assert.deepEqual([...result.text.matchAll(/\r?\n/g)].map(x => x.index), [...text.matchAll(/\r?\n/g)].map(x => x.index));
  assert.equal(result.text.indexOf('data.名'), text.indexOf('data.名'));
  assert.ok(result.text.startsWith('fn show() {\r\n  proa_macros::md! ['));
});

test('ignores Markdown fences, inline code, and escaped braces', () => {
  const text = 'md_sync! { r###"\n```rust\n{not_rust}\n```\n~~~\n{also_literal}\n~~~\n`{literal}` `` `{other}` `` {{escaped}} {a.name}\n"### }';
  const result = project(text);
  assert.equal(result.templates[0].expressions.length, 1);
  assert.ok(result.text.includes('{a.name}'));
  assert.ok(!result.text.includes('literal'));
  assert.ok(!result.text.includes('escaped'));
});

test('balances nested Rust blocks, raw strings, chars, and nested comments', () => {
  const text = 'md! { r###"{foo({ let x = r#"}"#; /* } /* { */ */ x }, \'}\')} {&\'a_ref}"### }';
  const result = project(text);
  assert.equal(result.templates[0].expressions.length, 2);
  assert.ok(result.text.includes('foo({ let x = r#"}"#; /* } /* { */ */ x }, \'}\')'));
});

test('leaves comments, unrelated literals/macros, and token composition alone', () => {
  for (const text of [
    '// md_sync! { r#"{x}"# }', '/* outer /* md! { r#"{x}"# } */ */',
    'let s = r###"md! { r#"{x}"# }"###;', 'other! { r#"{x}"# }',
    'md! { "prefix" {x} }', 'md! { "{x}" }', 'md! { r#"{name}"#, name = a.name }',
    'my_md! { r#"{x}"# }',
  ]) assert.equal(project(text).text, text, text);
});

test('projects nested control flow with the original bindings and expression positions', () => {
  const text = 'md! { r#"{for x in xs { - {x.name} {if x.ready { Yes {x.value} } else if flag { {other} } else { No }} }}"# }';
  const result = project(text);
  assert.equal(result.text.length, text.length);
  for (const expr of ['for x in xs', 'x.name', 'if x.ready', 'x.value', 'else if flag', 'other']) {
    assert.equal(result.text.indexOf(expr), text.indexOf(expr));
    assert.ok(safeRange(result, range(text.indexOf(expr), text.indexOf(expr) + expr.length)));
  }
  assert.ok(result.text.includes(' x.name;'));
  assert.ok(!result.text.includes('Yes'));
  assert.ok(!safeRange(result, range(text.indexOf('Yes'))));
  assert.equal(project('md! { r#"{unfinished"# }').text, 'md! { r#"{unfinished"# }');
});

test('keeps if-let bindings and incomplete Rust expressions visible', () => {
  const text = 'md_sync! { r#"{if let Some(a) = value { **{a.}** } else { None }}"# }';
  const result = project(text);
  assert.ok(result.text.includes('if let Some(a) = value'));
  assert.ok(result.text.includes(' a.;'));
  assert.equal(result.text.indexOf('a.;'), text.indexOf('a.}'));
});

test('uses a position-preserving Rust block for incomplete fields until the macro can expand', () => {
  for (const macro of ['md_sync!', 'proa_macros::md!', '::proa_macros::md_sync!']) {
    const text = `fn f() { ${macro}[r###"🦀 {a.} {for row in rows { {row.name} }}"###]; }`;
    const projection = project(text);
    assert.equal(projection.text.length, text.length);
    assert(!projection.text.includes('md'));
    assert.equal(projection.text.indexOf('a.;'), text.indexOf('a.}'));
    assert.equal(projection.text.indexOf('row.name;'), text.indexOf('row.name}'));
    assert(projection.text.includes('for row in rows'));
    assert(project(text.replace('{a.}', '{a.name}')).text.includes(macro));
  }
  assert(project('return ::proa_macros::md_sync! { r#"{a.}"# };').text.startsWith('return '));
});

test('URLs and punctuation inside control-flow prose do not hide Rust slots', () => {
  const text = 'md_sync! { r#"{for item in items { Read [docs](https://example.com) — don\'t forget {item.name} }}"# }';
  const result = project(text);
  assert.ok(result.templates.length);
  assert.ok(result.text.includes(' item.name;'));
  assert.ok(!result.text.includes('https'));
  assert.equal(result.text.indexOf('item.name'), text.indexOf('item.name'));
});

test('recognizes fences after template indentation and ignores backticks in invalid fence info', () => {
  const text = 'md! { r#"\n    ```rust\n    {literal}\n    ```\n    {a.name}\n    "# }';
  assert.equal(project(text).templates[0].expressions.length, 1);
  const invalid = 'md! { r#"\n```rust `info`\n{x.name}\n"# }';
  assert.ok(project(invalid).text.includes('{x.name}'));
});

test('retains the source document across incremental edits and removes stale projections', () => {
  const bridge = bridgeFor();
  const start = source.indexOf('user_name');
  const change = { range: range(start, start + 9), text: 'note' };
  const expected = applyChanges(source, [change]);
  const result = bridge.fromClient({ method: 'textDocument/didChange', params: { textDocument: { uri, version: 2 }, contentChanges: [change] } });
  assert.equal(bridge.documents.get(uri).source, expected);
  assert.equal(result.forward.params.contentChanges[0].text, project(expected).text);
  bridge.fromClient({ method: 'textDocument/didChange', params: { textDocument: { uri, version: 3 }, contentChanges: [{ text: 'fn main() {}' }] } });
  assert.equal(bridge.documents.get(uri).templates.length, 0);
  bridge.fromClient({ method: 'textDocument/didClose', params: { textDocument: { uri } } });
  assert.equal(bridge.documents.size, 0);
});

test('forwards definition requests and results at the original cursor position', () => {
  const bridge = bridgeFor();
  const request = { id: 1, method: 'textDocument/definition', params: { textDocument: { uri }, position: range(source.indexOf('user_name') + 2).start } };
  assert.deepEqual(bridge.fromClient(structuredClone(request)).forward, request);
  const response = { id: 1, result: [{ targetUri: uri, targetRange: range(3, 5) }] };
  assert.deepEqual(bridge.fromServer(response).forward, response);
});

test('blocks formatting and Rust requests inside literal Markdown', () => {
  const bridge = bridgeFor();
  for (const method of ['textDocument/formatting', 'textDocument/rangeFormatting', 'textDocument/onTypeFormatting', 'textDocument/willSaveWaitUntil']) {
    assert.equal(bridge.fromClient({ id: 1, method, params: { textDocument: { uri } } }).reply.result, null);
  }
  assert.equal(bridge.fromClient({ id: 2, method: 'textDocument/completion', params: { textDocument: { uri }, position: range(source.indexOf('via')).start } }).reply.result, null);
});

test('accepts field edits but rejects edits that include Markdown or delimiters', () => {
  const document = project(source);
  const start = source.indexOf('user_name');
  assert.ok(safeRange(document, range(start, start + 9)));
  assert.ok(safeRange(document, range(0)));
  assert.ok(!safeRange(document, range(source.indexOf('via'), source.indexOf('via') + 3)));
  assert.ok(!safeRange(document, range(0, source.length)));
  assert.ok(!safeRange(document, range(source.indexOf('{a'), source.indexOf('{a') + 1)));
});

test('rejects an entire rename/applyEdit rather than partially applying unsafe edits', () => {
  const bridge = bridgeFor();
  const edits = { changes: { [uri]: [{ range: range(source.indexOf('user_name'), source.indexOf('user_name') + 9), newText: 'name' }, { range: range(0, source.length), newText: 'lost Markdown' }] } };
  bridge.fromClient({ id: 1, method: 'textDocument/rename', params: { textDocument: { uri }, position: range(source.indexOf('user_name')).start } });
  assert.ok(bridge.fromServer({ id: 1, result: edits }).forward.error);
  assert.equal(bridge.fromServer({ id: 2, method: 'workspace/applyEdit', params: { edit: edits } }).reply.result.applied, false);
});

test('guards completion resolve using the original document URI', () => {
  const bridge = bridgeFor();
  const position = range(source.indexOf('user_name')).start;
  bridge.fromClient({ id: 1, method: 'textDocument/completion', params: { textDocument: { uri }, position } });
  const item = { label: 'name', data: { resolve: 1 } };
  bridge.fromServer({ id: 1, result: { items: [item] } });
  bridge.fromClient({ id: 2, method: 'completionItem/resolve', params: item });
  assert.equal(bridge.fromServer({ id: 2, result: { ...item, additionalTextEdits: [{ range: range(0, source.length), newText: 'bad' }] } }).forward.result, null);
});

test('preserves ordinary Rust formatting and command actions', () => {
  const bridge = bridgeFor('fn main() {}');
  const request = { id: 1, method: 'textDocument/formatting', params: { textDocument: { uri } } };
  assert.deepEqual(bridge.fromClient(request).forward, request);
  bridge.fromClient({ id: 2, method: 'textDocument/codeAction', params: { textDocument: { uri } } });
  const action = { title: 'Run', command: { title: 'Run', command: 'rust-analyzer.runSingle' } };
  assert.deepEqual(bridge.fromServer({ id: 2, result: [action] }).forward.result, [action]);
});

test('guards completion-list default edit ranges and inherited resolve data', () => {
  const bridge = bridgeFor();
  const params = { textDocument: { uri }, position: range(source.indexOf('user_name')).start };
  bridge.fromClient({ id: 1, method: 'textDocument/completion', params });
  const unsafe = bridge.fromServer({ id: 1, result: {
    itemDefaults: { editRange: range(0, source.length) }, items: [{ label: 'name', textEditText: 'name' }],
  } });
  assert.deepEqual(unsafe.forward.result.items, []);
  const start = source.indexOf('user_name');
  const defaults = { editRange: { insert: range(start), replace: range(start, start + 9) }, data: { resolve: 'default' } };
  bridge.fromClient({ id: 2, method: 'textDocument/completion', params });
  const safe = bridge.fromServer({ id: 2, result: { itemDefaults: defaults, items: [{ label: 'name', textEditText: 'name' }] } });
  assert.equal(safe.forward.result.items.length, 1);
  bridge.fromClient({ id: 3, method: 'completionItem/resolve', params: { label: 'name', data: defaults.data } });
  assert.equal(bridge.fromServer({ id: 3, result: { additionalTextEdits: [{ range: range(0, source.length), newText: '' }] } }).forward.result, null);
});

test('guards workspace file-operation edits in unopened template files', () => {
  const bridge = new Bridge(() => source);
  for (const method of ['workspace/willRenameFiles', 'workspace/willCreateFiles', 'workspace/willDeleteFiles']) {
    bridge.fromClient({ id: 1, method, params: { files: [] } });
    const response = bridge.fromServer({ id: 1, result: { changes: { [uri]: [{ range: range(0, source.length), newText: '' }] } } });
    assert.equal(response.forward.error.code, -32803);
  }
});
