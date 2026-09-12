// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const editor = process.env.NVIM || 'nvim';
const analyzer = process.env.PROA_RUST_ANALYZER || 'rust-analyzer';
for (const command of [editor, analyzer]) {
  const version = spawnSync(command, ['--version'], { encoding: 'utf8' });
  if (version.error || version.status !== 0) {
    console.error(`Cannot run ${command}: ${version.error?.message ?? version.stderr}`);
    console.error('Install Neovim 0.11+ and rust-analyzer, or set NVIM / PROA_RUST_ANALYZER.');
    process.exit(1);
  }
  console.log(version.stdout.split('\n')[0]);
}
const fixture = fileURLToPath(new URL('../../vscode/test/fixture/src/lib.rs', import.meta.url));
const before = readFileSync(fixture);
const directory = mkdtempSync(join(tmpdir(), 'proa-neovim-'));
console.log(`Neovim test state: ${directory}`);
const result = spawnSync(editor, ['--headless', '-u', 'NONE', '--noplugin', '-i', 'NONE', '-l', fileURLToPath(new URL('integration.lua', import.meta.url))], {
  stdio: 'inherit', timeout: 300_000,
  env: { ...process.env,
    XDG_CONFIG_HOME: join(directory, 'config'), XDG_DATA_HOME: join(directory, 'data'),
    XDG_STATE_HOME: join(directory, 'state'), XDG_CACHE_HOME: join(directory, 'cache'),
    PROA_TEST_NODE: process.execPath, PROA_RUST_ANALYZER: analyzer,
    PROA_TEST_FIXTURE: fixture, PROA_TEST_EDITORS: fileURLToPath(new URL('../../', import.meta.url)),
  },
});
assert.deepEqual(readFileSync(fixture), before, 'Integration tests must never modify the fixture on disk');
if (result.status === 0) rmSync(directory, { recursive: true, force: true });
else console.error(`Neovim failed; logs retained in ${directory}. ${result.error?.message ?? ''}`);
process.exitCode = result.status ?? 1;
