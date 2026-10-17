// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Bundle @orama/orama plus the English stemmer into one self-contained ESM
// file the marketing site serves from /static/vendor/orama.mjs. The .mjs
// extension keeps Node treating it as an ES module without a package.json
// `type` field, which would change how every other .js file in the repo parses.
//
// Run with `pnpm run search:vendor` after bumping the @orama/* devDependencies
// in package.json, then commit the regenerated bundle. The same file is what
// scripts/search_battery.mjs imports, so the battery exercises the exact
// bytes the browser loads.
import { build } from 'esbuild';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outfile = resolve(root, 'proa_marketing/public/vendor/orama.mjs');
const oramaVersion = await packageVersion('@orama/orama');
const stemmersVersion = await packageVersion('@orama/stemmers');

const entry = [
  'export * from "@orama/orama";',
  'export { stemmer as englishStemmer } from "@orama/stemmers/english";',
].join('\n');

async function packageVersion(name) {
  // Not every package exports its package.json, so read it from disk.
  const manifest = resolve(root, 'node_modules', name, 'package.json');
  return JSON.parse(await readFile(manifest, 'utf8')).version;
}

await mkdir(dirname(outfile), { recursive: true });
const result = await build({
  stdin: { contents: entry, resolveDir: root, loader: 'js' },
  bundle: true,
  format: 'esm',
  platform: 'browser',
  target: ['es2020'],
  minify: true,
  legalComments: 'none',
  write: false,
});
const banner = `// @orama/orama ${oramaVersion} + @orama/stemmers/english ${stemmersVersion}, bundled by scripts/vendor_orama.mjs. Do not edit; regenerate with \`pnpm run search:vendor\`.\n`;
const code = banner + result.outputFiles[0].text;
await writeFile(outfile, code);
const bytes = Buffer.byteLength(code);
console.log(`wrote ${outfile} (${bytes} bytes, orama ${oramaVersion})`);
