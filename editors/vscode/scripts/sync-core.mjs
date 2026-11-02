// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { copyFile, mkdir, readFile, writeFile, readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import { build } from 'esbuild';

// Keep the VSIX self-contained, with one maintained source for both adapters.
const destination = new URL('../src/core/', import.meta.url);
await mkdir(destination, { recursive: true });
for (const name of ['protocol.mjs', 'format.mjs', 'lint.mjs']) {
  await copyFile(new URL(`../../lsp/src/${name}`, import.meta.url), new URL(name, destination));
}

// Bundle the CommonMark parser into the VSIX. --no-dependencies packaging must
// work without pnpm's node_modules tree on the user's machine.
const root = fileURLToPath(new URL('../', import.meta.url));
const bundle = await build({
  absWorkingDir: root,
  metafile: true,
  stdin: {
    contents: await readFile(new URL('../../lsp/src/projection.mjs', import.meta.url), 'utf8'),
    resolveDir: fileURLToPath(new URL('../', import.meta.url)),
    sourcefile: 'projection.mjs',
  },
  outfile: fileURLToPath(new URL('projection.mjs', destination)),
  bundle: true,
  format: 'esm',
  platform: 'node',
  target: 'node18',
});

// Preserve the licenses for every package actually included in the bundle.
const packages = new Set(), licenses = [];
for (const input of Object.keys(bundle.metafile.inputs)) {
  if (!input.includes('node_modules/')) continue;
  let directory = dirname(resolve(root, input));
  let pkg;
  for (;;) {
    try { pkg = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8')); break; }
    catch (error) {
      if (error.code !== 'ENOENT' || directory === dirname(directory)) throw error;
      directory = dirname(directory);
    }
  }
  if (packages.has(directory)) continue;
  packages.add(directory);
  const names = (await readdir(directory)).filter(name => /^(?:licen[cs]e|notice)(?:[.-].*)?$/i.test(name));
  if (!names.length) throw new Error(`Missing bundled license for ${pkg.name}`);
  licenses.push(`${pkg.name}@${pkg.version}\n${(await Promise.all(names.sort().map(name => readFile(join(directory, name), 'utf8')))).join('\n')}`);
}
await writeFile(new URL('THIRD_PARTY_LICENSES.txt', destination), licenses.sort().join('\n'));
