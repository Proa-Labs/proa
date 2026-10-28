# Proa documentation SDK and CLI

Official, dependency-free JavaScript/TypeScript client for the public
[Proa documentation API](https://proa.so/docs/api). Requires Node.js 22+.
No credentials are needed. It searches and reads documentation; the separate
[Proa CLI](https://proa.so/docs/cli) builds and scaffolds applications.

```sh
npm install @proa-labs/docs
npx --package @proa-labs/docs proa-docs search streaming --limit 5
npx --package @proa-labs/docs proa-docs get /docs/core/html-rendering --markdown
```

```ts
import { ProaDocs, ProaApiError } from "@proa-labs/docs";

const docs = new ProaDocs();
const matches = await docs.search("streaming", { limit: 5 });
if (matches.results.length) {
  const page = await docs.get(matches.results[0].path);
  console.log(page.markdown);
}
for await (const page of docs.documents()) console.log(page.path);
```

`search` and `list` return `results`, `total`, and `next_cursor`. Pass the cursor
with the same query; restart pagination after a documentation deployment. `get`
accepts a path returned by the API and includes the complete `markdown` field.

Requests time out after 15 seconds. Configure `timeoutMs`, `baseUrl` (for local
development), or an alternative `fetch` in the constructor. All methods accept
an optional `signal`. A `ProaApiError` exposes `status`, `problem`, and
`retryAfter` seconds. Calls do not retry automatically: correct 400/404 inputs,
respect 429 `retryAfter`, and use bounded backoff for transient failures.

Do not send secrets or private source code in search queries. The server's
600 requests/minute budget is shared across clients per server instance.

Local development: `node --test test/*.test.mjs`; run `node cli.mjs --help`.
The HTTP contract is at <https://proa.so/openapi.json>, and MCP clients can
connect directly to <https://proa.so/mcp>.
