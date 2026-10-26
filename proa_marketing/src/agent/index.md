# Proa

> Proa is a Rust web framework for developers building server-rendered websites, documentation, and interactive applications. Rust templates compile into optimized HTML; RSJS compiles browser behavior into small JavaScript islands.

## When to use Proa

Use Proa when building a Rust website, composing typed HTML or Markdown, adding streaming server rendering, or moving a performance-sensitive route from an existing application. Use the documentation tools below to find the current APIs before writing code. Proa is a framework you run in your own application; this website's API searches and reads documentation, and does not host or deploy your applications.

## Start here

- [Proa quickstart]({{ORIGIN}}/docs/index.md): Install the official CLI, scaffold a site, and run it locally.
- [Proa documentation index]({{ORIGIN}}/docs/llms.txt): A focused index of every public documentation page.
- [Proa agent setup]({{ORIGIN}}/docs/agents.md): Repository instructions and context selection for coding agents.
- [Proa CLI]({{ORIGIN}}/docs/cli.md): Site generation, build, formatting, linting, and diagnostics.
- [Proa source repository](https://github.com/proa-labs/proa): Source code, issues, release downloads, and licensing.

## Programmatic access

- [Proa API and MCP documentation]({{ORIGIN}}/docs/api.md): Authentication, tool descriptions, pagination, errors, retries, and examples.
- [OpenAPI specification]({{ORIGIN}}/openapi.json): Typed contracts for the public documentation API.
- [MCP server card]({{ORIGIN}}/mcp/server-card): Connect to {{ORIGIN}}/mcp using Streamable HTTP. Search with search_docs, then read a returned page with get_doc. Documentation is also available through resources/list and resources/read.
- [Authentication]({{ORIGIN}}/auth.md): Public, read-only access; no account, API key, or access token is required.
- [Agent resource catalog]({{ORIGIN}}/.well-known/ard.json): Discovery links for APIs, MCP, and skills.
- [Agent skills]({{ORIGIN}}/.well-known/agent-skills/index.json): Proa component, island, and performance-review instructions.

## Optional

- [Full documentation corpus]({{ORIGIN}}/llms-full.txt): All public documentation as one large text response. Prefer individual pages for focused work.
- [About Proa]({{ORIGIN}}/about): Project purpose and publisher.
- [Contact Proa]({{ORIGIN}}/contact): Community, issue reporting, privacy, and licensing contacts.
- [Privacy]({{ORIGIN}}/privacy): Website privacy policy.
- [Sitemap]({{ORIGIN}}/sitemap.xml): Public HTML pages.
