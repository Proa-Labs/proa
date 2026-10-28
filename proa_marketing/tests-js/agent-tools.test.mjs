// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { registerAgentTools } from "../public/agent-tools.mjs";

test("browser tools use fixed public routes, encode inputs, and propagate cancellation and errors", async t => {
  const tools = [];
  await registerAgentTools({ registerTool: tool => tools.push(tool) });
  assert.deepEqual(tools.map(tool => tool.name), ["search_docs", "get_doc"]);
  assert.ok(tools.every(tool => tool.annotations.readOnlyHint));
  const signal = new AbortController().signal;
  const calls = [];
  t.mock.method(globalThis, "fetch", async (url, options) => {
    calls.push({ url, options });
    return { ok: true, json: async () => ({ results: [], next_cursor: null }) };
  });
  const result = JSON.parse(await tools[0].execute({ q: "&limit=500", limit: 2 }, { signal }));
  assert.deepEqual(result.results, []);
  const origin = "https://preview.example.test";
  const url = new URL(calls[0].url, origin);
  assert.equal(url.origin, origin);
  assert.equal(url.pathname, "/api/v1/search");
  assert.equal(url.searchParams.get("q"), "&limit=500");
  assert.equal(url.searchParams.get("limit"), "2");
  assert.equal(calls[0].options.signal, signal);
  assert.equal(calls[0].options.credentials, "omit");
  assert.equal(calls[0].options.redirect, "error");
  await tools[1].execute({ path: "https://example.org/private" });
  assert.ok(calls[1].url.startsWith("/api/v1/docs/page?"));
  globalThis.fetch.mock.mockImplementation(async () => ({ ok: false, status: 429, json: async () => ({ detail: "Retry after 60 seconds." }) }));
  await assert.rejects(tools[0].execute({ q: "render" }), /Retry after 60 seconds/);
});

test("browsers without WebMCP and denied registrations do not break the page", async () => {
  await registerAgentTools(undefined);
  await registerAgentTools({});
  await registerAgentTools({ registerTool() { throw new Error("Permission denied"); } });
});

test("browser tools retain HTTP failures for non-JSON bodies and preserve cancellation", async t => {
  const tools = [];
  await registerAgentTools({ registerTool: tool => tools.push(tool) });
  t.mock.method(globalThis, "fetch", async () => ({
    ok: false,
    status: 503,
    json: async () => { throw new SyntaxError("Gateway returned HTML"); },
  }));
  await assert.rejects(tools[0].execute({ q: "render" }), /\(503\)/);
  globalThis.fetch.mock.mockImplementation(async () => ({
    ok: false, status: 429, json: async () => null,
  }));
  await assert.rejects(tools[0].execute({ q: "render" }), /\(429\)/);

  const controller = new AbortController();
  const cancelled = new Error("Tool execution cancelled");
  globalThis.fetch.mock.mockImplementation(async () => ({
    ok: false,
    status: 503,
    json: async () => {
      controller.abort(cancelled);
      throw cancelled;
    },
  }));
  await assert.rejects(
    tools[0].execute({ q: "render" }, { signal: controller.signal }),
    error => error === cancelled,
  );
});
