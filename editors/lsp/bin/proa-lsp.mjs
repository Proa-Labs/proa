#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { readFileSync } from 'node:fs';
import { runProxy } from '../src/proxy.mjs';

const argv = process.argv.slice(2);
let command = process.env.PROA_RUST_ANALYZER || 'rust-analyzer';
let args = [];
for (let index = 0; index < argv.length; index++) {
  const arg = argv[index];
  if (arg === '--help' || arg === '-h') {
    console.log(`Usage: proa-lsp [--stdio] [--rust-analyzer PATH] [-- ANALYZER_ARGS...]

Proa Markdown template LSP bridge. Requires Node.js 22+, rust-analyzer,
and the project's Rust toolchain. Communicates over stdin/stdout;
logs go to stderr. Source files are never rewritten for analysis.

  --rust-analyzer PATH  Analyzer executable (default: PROA_RUST_ANALYZER or PATH)
  --stdio              Explicitly select the default stdio transport
  --version            Print the bridge version
  --                   Pass remaining arguments to rust-analyzer`);
    process.exit(0);
  } else if (arg === '--version' || arg === '-V') {
    console.log(`proa-lsp ${JSON.parse(readFileSync(new URL('../package.json', import.meta.url))).version}`);
    process.exit(0);
  } else if (arg === '--stdio') continue;
  else if (arg === '--rust-analyzer' && argv[index + 1]) command = argv[++index];
  else if (arg === '--') { args = argv.slice(index + 1); break; }
  else {
    console.error(`proa-lsp: Unknown or incomplete argument: ${arg}. See --help.`);
    process.exit(1);
  }
}
process.exitCode = await runProxy({ command, args });
