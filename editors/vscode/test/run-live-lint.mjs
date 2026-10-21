// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const extension = fileURLToPath(new URL('../', import.meta.url));
const command = process.env.PROA_TEST_LINT || fileURLToPath(new URL(`../../../target/debug/proa${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url));
if (!existsSync(command)) throw new Error('Build the CLI first: cargo build -p proa-cli --bin proa');
const installed = join(homedir(), '.vscode/extensions');
const analyzer = process.env.PROA_RUST_ANALYZER_EXTENSION || readdirSync(installed)
  .filter(name => name.startsWith('rust-lang.rust-analyzer-'))
  .sort((a, b) => b.localeCompare(a, undefined, { numeric: true }))
  .map(name => join(installed, name))[0];
if (!analyzer) throw new Error('Install the official rust-analyzer extension or set PROA_RUST_ANALYZER_EXTENSION.');
const root = mkdtempSync(join(tmpdir(), 'proa-vscode-live-lint-'));
const project = join(root, 'project');
const extensions = join(root, 'extensions');
const data = join(root, 'user-data');
mkdirSync(join(project, 'src'), { recursive: true });
mkdirSync(extensions);
mkdirSync(join(data, 'User'), { recursive: true });
symlinkSync(resolve(analyzer), join(extensions, 'rust-analyzer'), process.platform === 'win32' ? 'junction' : 'dir');
writeFileSync(join(data, 'User/settings.json'), JSON.stringify({
  'proa.lint.path': command, 'rust-analyzer.checkOnSave': false,
  'rust-analyzer.cargo.buildScripts.enable': false, 'extensions.autoUpdate': false,
  'window.restoreWindows': 'none',
}));
for (const [path, source] of Object.entries({
  'Cargo.toml': "[package]\nname='editor_live_fixture'\nversion='0.1.0'\nedition='2024'\n",
  'src/lib.rs': 'mod product; mod html; mod md;\n',
  'src/product.rs': 'pub struct Product {pub title:String} pub struct Notice;\n',
  'src/html.rs': 'use crate::product::{Product,Notice}; use proa_core::WebRenderSync; use proa_macros::html_sync; impl WebRenderSync for Product {fn render(self,cx:&mut Cx)->Result {html_sync! {{self.title}}.render(cx)}}\n',
  'src/md.rs': 'use crate::product::Product; use proa_core::MdRenderSync; use proa_macros::md_sync; impl MdRenderSync for Product {fn render_md(self,cx:&mut Cx)->Result {md_sync! {r###"{self.title}"###}.render_md(cx)}}\n',
})) writeFileSync(join(project, path), source);
console.log(`VS Code live lint fixture: ${root}`);
const child = spawn(process.env.CODE || 'code', ['--new-window', '--wait', '--disable-workspace-trust',
  '--skip-release-notes', '--skip-welcome', '--user-data-dir', data, '--extensions-dir', extensions,
  `--extensionDevelopmentPath=${extension}`, `--extensionTestsPath=${fileURLToPath(new URL('./live-lint.cjs', import.meta.url))}`, project,
], { stdio: 'inherit', env: { ...process.env, PROA_EDITOR_LINT_WORKSPACE: project } });
let code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', resolve); });
if (code === 0 && !existsSync(join(project, 'proa-live-lint-test-result.json'))) {
  console.error('VS Code exited without running the live lint assertions.');
  code = 1;
}
if (code === 0) {
  // Only the disposable project and VS Code profile are removed; installed
  // extensions and the user's active VS Code session are never changed.
  rmSync(root, { recursive: true, force: true });
  console.log('PASS: real VS Code live lint diagnostics, unsaved repairs, intentional omissions, and unchanged source files.');
} else console.error(`Retained VS Code logs and fixture at ${root}`);
process.exitCode = code ?? 1;
