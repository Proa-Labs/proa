#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { parseArgs } from "node:util";
import { ProaDocs } from "./index.mjs";

const HELP = `Proa documentation CLI

Usage:
  proa-docs search <query> [--limit 10] [--cursor CURSOR]
  proa-docs list [--limit 10] [--cursor CURSOR]
  proa-docs get <path> [--markdown]

Options:
  --base-url URL   API origin (default: https://proa.so)
  --timeout MS     Request timeout (default: 15000)
  --markdown      Print a retrieved document's Markdown
  --help          Show this help
  --version       Show the package version

Results are JSON by default. No account or API key is required.
Errors go to stderr with exit status 1; rate limits include retryAfter seconds.
This reads documentation and does not scaffold or build applications.
`;

try {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      limit: { type: "string" }, cursor: { type: "string" },
      "base-url": { type: "string" }, timeout: { type: "string" },
      markdown: { type: "boolean" }, help: { type: "boolean", short: "h" },
      version: { type: "boolean" },
    },
  });
  if (values.help) process.stdout.write(HELP);
  else if (values.version) process.stdout.write("0.1.0\n");
  else {
    const [command, argument, ...extra] = positionals;
    if (!(["search", "get"].includes(command) && argument && extra.length === 0) && !(command === "list" && positionals.length === 1)) {
      throw new Error("Expected search <query>, list, or get <path>. Run proa-docs --help.");
    }
    if (values.markdown && command !== "get") throw new Error("--markdown requires get");
    if (command === "get" && (values.limit !== undefined || values.cursor !== undefined)) throw new Error("get does not accept --limit or --cursor");
    const limit = values.limit === undefined ? undefined : Number(values.limit);
    if (limit !== undefined && (!Number.isInteger(limit) || limit < 1 || limit > 50)) throw new Error("--limit must be an integer from 1 to 50");
    const client = new ProaDocs({ baseUrl: values["base-url"], timeoutMs: values.timeout === undefined ? undefined : Number(values.timeout) });
    const options = { limit, cursor: values.cursor };
    const result = await (command === "search" ? client.search(argument, options) : command === "list" ? client.list(options) : client.get(argument));
    process.stdout.write(values.markdown ? result.markdown + "\n" : JSON.stringify(result, null, 2) + "\n");
  }
} catch (error) {
  process.stderr.write(JSON.stringify({ error: error.message, status: error.status, retryAfter: error.retryAfter }) + "\n");
  process.exitCode = 1;
}
