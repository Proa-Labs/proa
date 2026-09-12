// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { copyFile, mkdir } from 'node:fs/promises';

// Keep the VSIX self-contained, with one maintained source for both adapters.
const destination = new URL('../src/core/', import.meta.url);
await mkdir(destination, { recursive: true });
for (const name of ['projection.mjs', 'protocol.mjs', 'format.mjs']) {
  await copyFile(new URL(`../../lsp/src/${name}`, import.meta.url), new URL(name, destination));
}
