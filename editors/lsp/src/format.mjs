// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, relative, isAbsolute } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';

function executable(name) {
  const local = join(homedir(), '.cargo', 'bin', process.platform === 'win32' ? `${name}.exe` : name);
  return existsSync(local) ? local : name;
}

function configFile(directory) {
  for (;;) {
    for (const name of ['rustfmt.toml', '.rustfmt.toml']) {
      const path = join(directory, name);
      if (existsSync(path)) return path;
    }
    const parent = dirname(directory);
    if (parent === directory) return null;
    directory = parent;
  }
}

export function run(command, args, { cwd, input = '', token, env = process.env } = {}) {
  return new Promise((resolve, reject) => {
    if (token?.isCancellationRequested) { resolve(null); return; }
    const child = spawn(command, args, { cwd, env, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    let stdout = '', stderr = '', cancelled = false;
    const timer = setTimeout(() => { child.kill(); reject(new Error(`${command} timed out.`)); }, 30_000);
    const cancellation = token?.onCancellationRequested?.(() => { cancelled = true; child.kill(); });
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.stdin.on('error', () => {}); // Early exit (including parse errors).
    child.once('error', reject);
    child.once('close', code => {
      clearTimeout(timer);
      cancellation?.dispose();
      if (cancelled) resolve(null);
      else if (code === 0) resolve(stdout);
      else reject(new Error(stderr.trim() || `${command} exited with code ${code}.`));
    });
    child.stdin.end(input);
  });
}

export function editionForFile(metadata, file) {
  const contains = (directory, path) => {
    const rel = relative(directory, path);
    return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
  };
  const packages = metadata.packages.filter(pkg => contains(dirname(pkg.manifest_path), file))
    .sort((a, b) => b.manifest_path.length - a.manifest_path.length);
  const pkg = packages[0];
  if (!pkg) throw new Error('Could not determine the Rust edition for this file.');
  const target = pkg.targets.filter(target => contains(dirname(target.src_path), file))
    .sort((a, b) => b.src_path.length - a.src_path.length)[0];
  return target?.edition ?? pkg.edition;
}

// Format the real, unsaved document through rustfmt's stdin interface. The
// analyzer's projection never reaches a formatter or a source file.
export async function formatDocument(document, uri, token, settings = {}) {
  const file = fileURLToPath(uri);
  const cwd = dirname(file);
  const metadataText = await run(executable('cargo'), ['metadata', '--no-deps', '--format-version', '1'], { cwd, token });
  if (metadataText === null) return null;
  const edition = editionForFile(JSON.parse(metadataText), file);
  const override = settings.overrideCommand;
  const command = override?.[0] ?? executable('rustfmt');
  const config = configFile(cwd);
  const args = override?.length ? override.slice(1) : [
    '--edition', edition, '--emit', 'stdout', ...(config ? ['--config-path', config] : []), ...(settings.extraArgs ?? []),
  ];
  const text = await run(command, args, { cwd, input: document.source, token });
  if (text === null) return null;
  if (text === document.source) return [];
  const lines = document.source.split('\n');
  return [{ range: {
    start: { line: 0, character: 0 },
    end: { line: lines.length - 1, character: lines.at(-1).length },
  }, newText: text }];
}
