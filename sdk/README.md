# Proa documentation clients

These are small, dependency-free clients for the public read-only documentation
API. They do not scaffold or build Proa applications; use the Rust Proa CLI for
that. JavaScript/TypeScript and Python expose equivalent search, list, get, and
pagination operations, plus a `proa-docs` terminal command.

| Language | Source | Public package |
| --- | --- | --- |
| JavaScript / TypeScript (Node 22+) | [javascript](javascript/README.md) | [@proa-labs/docs on npm](https://www.npmjs.com/package/@proa-labs/docs) |
| Python (3.11+) | [python](python/README.md) | [proa-docs on PyPI](https://pypi.org/project/proa-docs/) |

Python version 0.1.1 is published and verified with a fresh installation. Install
the library with `pip install proa-docs` or run its CLI with uv:

```sh
uvx --from proa-docs proa-docs search streaming --limit 5
```

JavaScript version 0.1.0 is also published and verified. Install the library with
`npm install @proa-labs/docs`, or run the CLI with npx:

```sh
npx --yes --package @proa-labs/docs proa-docs search streaming --limit 5
```

Both CLIs can also run from source:

```sh
node sdk/javascript/cli.mjs search streaming --limit 5
uv run --directory sdk/python proa-docs search streaming --limit 5
```

## Validation

Run from the repository root:

```sh
node --test sdk/javascript/test/*.test.mjs
uv run --directory sdk/python --locked pytest
uv run --directory sdk/python --locked ruff check
uv run --directory sdk/python --locked ruff format --check
uv run --directory sdk/python --locked ty check src
```

CI runs these checks with the marketing application's API tests. Both SDK test
suites use local HTTP servers and exercise encoded requests, cursor handling,
structured failures, and CLI behavior. No live credentials are needed.

## Releasing

1. Review the code and choose a version. Keep JavaScript's `package.json` and
   CLI `--version` aligned. Keep Python's `pyproject.toml` and `__version__`
   aligned. Follow the repository's commit and release authorization policy.
2. Run validation, then build archives into a directory outside the checkout:

   ```sh
   npm pack ./sdk/javascript --pack-destination /tmp/proa-docs-dist
   uv build --directory sdk/python --out-dir /tmp/proa-docs-dist
   ```

   Create the destination directory first. Install the exact archives in an
   isolated environment and smoke-test `proa-docs search`, `get --markdown`,
   and a missing-document failure against the live API.
3. Sign in with an npm account authorized for the `@proa-labs` scope. Publish
   the reviewed JavaScript tarball with `npm publish <tarball> --access public`.
   Complete npm's login and 2FA prompts in the browser.
4. Publish the reviewed Python wheel and source distribution with
   `uv publish <wheel> <sdist>`. Use the organization's PyPI publishing identity
   or configured trusted publisher. Enter credentials directly in the tool's
   secure prompt; never put them in source, release notes, or chat.
5. Verify the public package pages, fresh installations, and their links to
   `https://proa.so/docs/api` and the source repository. Update the release
   versions here only after publication and verification succeed. Revoke any
   temporary upload token after the release.

For local-site smoke tests, set `--base-url http://localhost:5555`. Python's
global options come before the subcommand; JavaScript accepts either order.
The API has no test credentials or side effects: all operations read public
documentation. Do not include private source code or secrets in search terms.

## MCP registry metadata

The hosted MCP server is separate from these HTTP client packages. Its validated
[server manifest](../proa_marketing/server.json) publishes `https://proa.so/mcp`
under `io.github.proa-web/docs` and needs no npm/PyPI server package. The
[official listing](https://registry.modelcontextprotocol.io/v0.1/servers?search=io.github.proa-web%2Fdocs)
is active at version 1.0.0. The registry distribution name and the runtime MCP
implementation name (`so.proa/docs`) are separate identities.

Use the official [registry publishing guide](https://modelcontextprotocol.io/registry/quickstart)
and [authentication guide](https://modelcontextprotocol.io/registry/authentication).
Sign in with the `proa-web` GitHub account, then validate and publish. Keep
publisher credentials outside the repository:

```sh
mcp-publisher login github
mcp-publisher validate proa_marketing/server.json
mcp-publisher publish proa_marketing/server.json
```

If migrating to `so.proa/docs`, verify ownership of `proa.so` through DNS or
HTTP first. Keep its private signing key outside the repository; publish only
the public verification record.

If choosing the `io.github.Proa-Labs` namespace instead, update the manifest name
and use organization-capable authentication. Interactive GitHub login currently
only grants the personal namespace; the registry documents GitHub Actions OIDC
or a suitably scoped organization-owner token as alternatives. Never send an
existing broad repository token to a third party just to bypass this limitation.

After a listing is confirmed, add its exact URL to the website's API guide.
That reciprocal link helps directories verify the first-party relationship.
Do not advertise an unsubmitted or pending listing as published.
