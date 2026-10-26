# Proa — Rust web framework

Proa is a batteries-included Rust web framework built around composable, multi-target rendering. It is for developers building server-rendered websites, documentation, and interactive applications who want typed Rust components and efficient rendering.

## What Proa provides

- Compiled HTML templates with static consolidation and allocation-conscious rendering.
- Synchronous and asynchronous components, request-scoped data loading, and streaming SSR.
- RSJS islands: Rust-authored signals and event handlers compiled into small JavaScript modules.
- Typed HTML and Markdown output, with documentation available without running JavaScript.
- Routing, metadata, static assets, forms, and response control in the Rust framework.
- Compile-time accessibility checks and automatic escaping.

## Get started

Read the [quickstart]({{ORIGIN}}/docs/index.md) to install the official Proa CLI, create a site, and run it locally. Explore the [playground]({{ORIGIN}}/playground), [showcase]({{ORIGIN}}/showcase), and [ecosystem]({{ORIGIN}}/ecosystem) for examples and integrations. Source and licensing information are in the [official repository](https://github.com/proa-labs/proa).

## For agents

Start with [llms.txt]({{ORIGIN}}/llms.txt) and the [documentation index]({{ORIGIN}}/docs/llms.txt). Search the public documentation with `GET /api/v1/search?q=streaming`, then retrieve a page with `GET /api/v1/docs/page?path=/docs/core/streaming-ssr` or its `.md` URL.

The [OpenAPI specification]({{ORIGIN}}/openapi.json) defines the read-only API. The [MCP server]({{ORIGIN}}/mcp) exposes `search_docs`, `get_doc`, and documentation resources over Streamable HTTP. Browser agents can use the same search and retrieval actions through WebMCP when supported.

No authentication, account, or payment is required to read documentation or use these tools. See [API documentation]({{ORIGIN}}/docs/api.md) for limits, pagination, error handling, and safe retries, and [auth.md]({{ORIGIN}}/auth.md) for the access policy.

## Publisher and support

Proa is published by Proa Labs, Inc. Read [About]({{ORIGIN}}/about), [Contact]({{ORIGIN}}/contact), [Privacy]({{ORIGIN}}/privacy), and [Terms]({{ORIGIN}}/terms).
