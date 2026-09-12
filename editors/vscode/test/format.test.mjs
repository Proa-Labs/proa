// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import test from 'node:test';
import assert from 'node:assert/strict';
import { editionForFile, run } from '../src/format.mjs';
import { attachClient } from '../src/client.mjs';

test('uses the owning package and target editions from Cargo metadata', () => {
  const metadata = { packages: [
    { manifest_path: '/project/Cargo.toml', edition: '2021', targets: [] },
    { manifest_path: '/project/app/Cargo.toml', edition: '2024', targets: [
      { src_path: '/project/app/src/main.rs', edition: '2024' },
      { src_path: '/project/app/examples/demo.rs', edition: '2021' },
    ] },
  ] };
  assert.equal(editionForFile(metadata, '/project/app/src/nested/view.rs'), '2024');
  assert.equal(editionForFile(metadata, '/project/app/examples/demo.rs'), '2021');
  assert.equal(editionForFile(metadata, '/project/other.rs'), '2021');
  assert.throws(() => editionForFile(metadata, '/project2/other.rs'), /edition/);
});

test('formats original text and rejects a stale result when the user keeps typing', async () => {
  const uri = 'file:///project/src/lib.rs';
  let text = 'md! { r#"Hi {a.name}"# }';
  const client = { clientOptions: { middleware: {} }, protocol2CodeConverter: { asWorkspaceEdit: x => x } };
  let finish;
  const attached = attachClient(client, () => text, { format: document => {
    assert.equal(document.source, text);
    assert.notEqual(document.text, text);
    return new Promise(resolve => { finish = resolve; });
  } });
  const request = () => client.clientOptions.middleware.sendRequest('textDocument/formatting', {
    textDocument: { uri },
  }, {}, () => { throw new Error('must not format the projection'); });
  const fresh = request();
  finish([]);
  assert.deepEqual(await fresh, []);
  const stale = request();
  text = text.replace('name', 'note');
  finish([{ newText: 'obsolete' }]);
  assert.equal(await stale, null);
  attached.dispose();
});

test('formatter process preserves stdin and reports failures and cancellation', async () => {
  const input = '### 🦀 {field}\n';
  const result = await run(process.execPath, ['-e', 'process.stdin.pipe(process.stdout)'], { input });
  assert.equal(result, input);
  await assert.rejects(run(process.execPath, ['-e', 'process.stderr.write("bad syntax"); process.exit(1)']), /bad syntax/);
  assert.equal(await run(process.execPath, [], { token: { isCancellationRequested: true } }), null);
});
