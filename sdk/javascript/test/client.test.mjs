// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import test from "node:test";
import { ProaDocs, ProaApiError } from "../index.mjs";

const run = promisify(execFile);
const cli = fileURLToPath(new URL("../cli.mjs", import.meta.url));
const summary = { title: "Example", path: "/docs/example", markdown_url: "/docs/example.md", description: "Example document" };

async function server(t, handler) {
  const app = createServer(handler);
  await new Promise(resolve => app.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise(resolve => { app.closeAllConnections(); app.close(resolve); }));
  return `http://127.0.0.1:${app.address().port}`;
}

test("search encodes parameters and CLI returns the same typed result", async t => {
  const received = [];
  const result = { results: [summary], total: 1, next_cursor: null };
  const baseUrl = await server(t, (request, response) => {
    received.push({ url: new URL(request.url, "http://localhost"), headers: request.headers, method: request.method });
    response.setHeader("Content-Type", "application/json");
    response.end(JSON.stringify(result));
  });
  assert.deepEqual(await new ProaDocs({ baseUrl }).search("A & B/C?", { limit: 5, cursor: "0" }), result);
  const { stdout, stderr } = await run(process.execPath, [cli, "search", "A & B/C?", "--base-url", baseUrl, "--limit", "5"]);
  assert.deepEqual(JSON.parse(stdout), result);
  assert.equal(stderr, "");
  for (const request of received) {
    assert.equal(request.url.pathname, "/api/v1/search");
    assert.equal(request.url.searchParams.get("q"), "A & B/C?");
    assert.equal(request.url.searchParams.get("limit"), "5");
    assert.equal(request.method, "GET");
    assert.equal(request.headers.authorization, undefined);
  }
});

test("get preserves document paths and CLI can print Markdown", async t => {
  const baseUrl = await server(t, (request, response) => {
    assert.equal(new URL(request.url, "http://localhost").searchParams.get("path"), "/docs/a b&c.md");
    response.end(JSON.stringify({ ...summary, markdown: "# Sample\n\n<literal>" }));
  });
  const result = await new ProaDocs({ baseUrl }).get("/docs/a b&c.md");
  assert.equal(result.markdown, "# Sample\n\n<literal>");
  const { stdout } = await run(process.execPath, [cli, "get", "/docs/a b&c.md", "--base-url", baseUrl, "--markdown"]);
  assert.equal(stdout, result.markdown + "\n");
});

test("pagination follows opaque cursors and terminates at null", async t => {
  const cursors = [];
  const baseUrl = await server(t, (request, response) => {
    const cursor = new URL(request.url, "http://localhost").searchParams.get("cursor");
    cursors.push(cursor);
    response.end(JSON.stringify({ results: [{ ...summary, path: cursor === null ? "/first" : "/second" }], total: 2, next_cursor: cursor === null ? "next +/&" : null }));
  });
  const paths = [];
  for await (const page of new ProaDocs({ baseUrl }).documents()) paths.push(page.path);
  assert.deepEqual(paths, ["/first", "/second"]);
  assert.deepEqual(cursors, [null, "next +/&"]);
});

test("repeating cursors fail instead of looping", async () => {
  const client = new ProaDocs({ fetch: async () => Response.json({ results: [], total: 0, next_cursor: "repeat" }) });
  await assert.rejects(async () => { for await (const page of client.documents()) void page; }, /repeating/);
});

test("rate limits preserve problem details and retry delay without retrying", async t => {
  let calls = 0;
  const problem = { status: 429, code: "rate_limited", detail: "Wait for the shared budget" };
  const baseUrl = await server(t, (_request, response) => {
    calls++;
    response.writeHead(429, { "Content-Type": "application/problem+json", "Retry-After": "12" });
    response.end(JSON.stringify(problem));
  });
  await assert.rejects(new ProaDocs({ baseUrl }).list(), error => {
    assert.ok(error instanceof ProaApiError);
    assert.equal(error.status, 429);
    assert.equal(error.retryAfter, 12);
    assert.deepEqual(error.problem, problem);
    return true;
  });
  await assert.rejects(run(process.execPath, [cli, "list", "--base-url", baseUrl]), error => {
    assert.equal(error.code, 1);
    assert.equal(error.stdout, "");
    assert.equal(JSON.parse(error.stderr).retryAfter, 12);
    return true;
  });
  assert.equal(calls, 2);
});

test("non-JSON failures retain HTTP status", async () => {
  const client = new ProaDocs({ fetch: async () => new Response("unavailable", { status: 503 }) });
  await assert.rejects(client.list(), error => error.status === 503 && error.problem === null);
});

test("timeouts and caller cancellation abort HTTP reads", async t => {
  const baseUrl = await server(t, () => {});
  await assert.rejects(new ProaDocs({ baseUrl, timeoutMs: 20 }).list(), { name: "TimeoutError" });
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(new ProaDocs({ baseUrl }).list({ signal: controller.signal }), { name: "AbortError" });
});

test("invalid configuration and CLI arguments fail before a request", async () => {
  for (const baseUrl of ["file:///etc/passwd", "https://user:secret@example.com", "https://example.com/?q=x"]) assert.throws(() => new ProaDocs({ baseUrl }), TypeError);
  for (const timeoutMs of [0, -1, NaN, Infinity]) assert.throws(() => new ProaDocs({ timeoutMs }), TypeError);
  await assert.rejects(run(process.execPath, [cli, "search", "rust", "--limit", "0"]), error => error.code === 1 && /--limit/.test(error.stderr));
});
