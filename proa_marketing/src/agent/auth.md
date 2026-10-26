# Proa public documentation authentication

## Discover

The documentation API on {{ORIGIN}} and the Streamable HTTP MCP endpoint at {{ORIGIN}}/mcp are public and read-only. Their specification is at [openapi.json]({{ORIGIN}}/openapi.json), and usage examples are in the [API guide]({{ORIGIN}}/docs/api.md).

## Choose a method

Use ordinary HTTPS without an Authorization header. No registration, API key, OAuth grant, identity assertion, access token, or cookie is needed. These interfaces only read published documentation. They do not access accounts, change projects, deploy applications, or charge money.

## Make a request

```bash
curl '{{ORIGIN}}/api/v1/search?q=streaming&limit=5'
curl '{{ORIGIN}}/api/v1/docs/page?path=/docs/core/streaming-ssr'
curl -H 'Accept: text/markdown' '{{ORIGIN}}/'
```

For MCP, configure a Streamable HTTP connection to {{ORIGIN}}/mcp without credentials. Initialize the connection, list tools, and call search_docs or get_doc. Browser clients must originate from this deployment; non-browser clients may omit the Origin header.

## Errors and retries

The REST API returns application/problem+json errors with status, code, detail, and a link to the API guide. A 400 means the query, cursor, limit, or path is invalid; a 404 means the requested public document or route is absent. A 429 includes Retry-After in seconds. Respect the advertised shared per-instance request budget and retry after that delay. GET requests and the documentation tools are read-only and safe to retry.

## Revocation

There are no public API credentials to revoke. Remove the MCP connection from your client to stop using it. Authentication for applications you build with Proa is controlled by those applications, not by this documentation website. Never send application secrets or personal data as documentation search queries.
