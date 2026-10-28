# Proa documentation clients

These are small, dependency-free clients for the public read-only documentation
API. They do not scaffold or build Proa applications; use the Rust Proa CLI for
that. JavaScript/TypeScript and Python expose equivalent search, list, get, and
pagination operations, plus a `proa-docs` terminal command.

| Language | Source | Public package |
| --- | --- | --- |
| JavaScript / TypeScript (Node 22+) | [javascript](javascript/README.md) | `@proa-labs/docs` on npm (publication pending) |
| Python (3.11+) | [python](python/README.md) | [proa-docs on PyPI](https://pypi.org/project/proa-docs/) |

Python version 0.1.0 is published and verified with a fresh installation. Install
the library with `pip install proa-docs` or run its CLI with uv:

```sh
uvx --from proa-docs proa-docs search streaming --limit 5
```

JavaScript publication is pending npm authentication. Its README's npm install
commands apply after publication; both CLIs can also run from source:

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
5. Verify both public package pages, fresh installations, and their links to
   `https://proa.so/docs/api` and the source repository. Replace the first-release
   notices in this file and the website's API guide with registry links and
   verified install commands only after both publications succeed.

For local-site smoke tests, set `--base-url http://localhost:5555`. Python's
global options come before the subcommand; JavaScript accepts either order.
The API has no test credentials or side effects: all operations read public
documentation. Do not include private source code or secrets in search terms.

## MCP registry metadata

The hosted MCP server is separate from these HTTP client packages. Its validated
[server manifest](../proa_marketing/server.json) publishes `https://proa.so/mcp`
under `so.proa/docs` and needs no npm/PyPI server package.

Use the official [registry publishing guide](https://modelcontextprotocol.io/registry/quickstart)
and [authentication guide](https://modelcontextprotocol.io/registry/authentication).
The `so.proa` namespace requires ownership verification for `proa.so` using DNS
or HTTP. Keep the private signing key outside the repository and deploy only
its public verification record. Then validate and publish:

```sh
mcp-publisher validate proa_marketing/server.json
mcp-publisher publish proa_marketing/server.json
```

If choosing the `io.github.Proa-Labs` namespace instead, update the manifest name
and use organization-capable authentication. Interactive GitHub login currently
only grants the personal namespace; the registry documents GitHub Actions OIDC
or a suitably scoped organization-owner token as alternatives. Never send an
existing broad repository token to a third party just to bypass this limitation.

After a listing is confirmed, add its exact URL to the website's API guide.
That reciprocal link helps directories verify the first-party relationship.
Do not advertise an unsubmitted or pending listing as published.
