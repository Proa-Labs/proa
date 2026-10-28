# Proa documentation SDK and CLI

Official, dependency-free Python client for the public
[Proa documentation API](https://proa.so/docs/api). Python 3.11+; no credentials
required. It reads documentation, while the separate
[Proa CLI](https://proa.so/docs/cli) scaffolds and builds applications.

```sh
uv add proa-docs
uvx proa-docs search streaming --limit 5
uvx proa-docs get /docs/core/html-rendering --markdown
```

```python
from proa_docs import ProaDocs

docs = ProaDocs()
matches = docs.search("streaming", limit=5)
if matches["results"]:
    page = docs.get(matches["results"][0]["path"])
    print(page["markdown"])
for page in docs.documents():
    print(page["path"])
```

`search` and `list` return typed dictionaries with `results`, `total`, and
`next_cursor`. Pass the returned cursor with the same query and restart after
a documentation deployment. `get` returns metadata and complete Markdown.

Configure `ProaDocs(base_url="http://localhost:5555", timeout=15)` for local
development. Timeouts are in seconds. `ProaApiError` exposes `status`, `problem`,
and `retry_after` seconds. There are no automatic retries: correct 400/404
inputs, respect 429 `retry_after`, and use bounded backoff for network or 5xx
failures. The server's 600 requests/minute budget is shared per instance.

Do not send secrets or private source code in search queries. CLI results are
JSON by default, errors go to stderr, and failures return a nonzero exit code.
Global CLI options (`--base-url`, `--timeout`) go before the command.

Development: `uv run pytest`, `uv run ruff check`, `uv run ty check`, `uv build`.
The contract is at <https://proa.so/openapi.json>; MCP clients can connect
directly to <https://proa.so/mcp>.
