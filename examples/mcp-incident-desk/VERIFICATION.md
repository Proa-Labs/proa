# Verification record

## Native Codex MCP and browser: passed

Verified 2026-09-10 on macOS 26.5.2 with Codex 0.153.4 and `rmcp` 3.2.0.

- Registered `https://localhost:5570/mcp` with Codex using dynamic client registration. Completed Alice's password login and Aster Commerce consent in the browser. Codex performed PKCE authorization and stored its own OAuth credentials. No token was pasted or minted for the client; TLS validation remained enabled.
- Invoked `whoami`, `get_issue_brief`, `get_issue_data`, and `resolve_issue` through the installed Codex engine's documented `mcpServer/tool/call` API. Codex reported OAuth authentication and discovered all five tools. The check used an ephemeral in-memory context without starting a model turn or saving a separate task.
- `whoami` returned Alice, Aster Commerce, maintainer, and `issues:read issues:write`. The brief returned composed Markdown; the JSON projection returned the same issue.
- Resolved `AST-1042` from open, revision 7, to resolved, revision 8. Audit ID: `_v7o4AAheZc54YQHs9Lmblr2nhRqLorrE8kBjTVzrxs`. Note: “Verified recovery end-to-end from native Codex MCP using Alice's OAuth grant.” The audit attributes the update to Alice and the dynamically registered Codex client.
- Completed a separate website OAuth session and opened `https://localhost:5570/issues/AST-1042` in the Codex in-app browser. The rendered page showed Resolved, Revision 8, Alice, and that exact activity note.
- Repeated the website sign-in in the user's Chrome tab that was still displaying an earlier permission error. Fresh password login, workspace consent, and callback succeeded; that same Chrome tab now shows `AST-1042`, resolved at revision 8, with the Codex activity note.

The live demo database intentionally retains this verified resolution. Use another open seeded issue for the next walkthrough; do not reset the developer database to rerun a check.

## Fixes found during real-client verification

- Codex can send repeated identical OAuth `resource` parameters. The issuer accepts identical repeats while rejecting conflicting audiences and duplicate security parameters.
- `Referrer-Policy: no-referrer` turns native form POST origins into `null`. Form pages now use `same-origin`; Origin and CSRF checks remain enforced.
- Expired form transactions are checked before expired cookies so they report a clear restart message rather than a misleading permission failure. No credential values are included in rejection logs.

The five HTTPS integration tests pass, including missing-cookie rejection, expiration without creating sessions or grants, repeated-resource handling, OAuth cryptography, rotation/replay, workspace authorization, shared HTML/Markdown/JSON data, and transactional mutation behavior. The application binary builds successfully.

## Native Codex incident creation: passed

Added `create_issue` with tenant-scoped reference validation, authenticated ownership, input limits, atomic audit/event insertion, and idempotent retries. Both MCP scope enforcement and the reusable domain command require a current write grant and maintainer membership.

On 2026-09-10, Codex's native MCP engine discovered all six tools and reused Alice's stored OAuth connection to create ten sample incidents. Each was read back through `get_issue_data`; the first was also retrieved as composed Markdown. Repeating its creation with the same idempotency key returned the original result. MCP search and the user's Chrome incident list both showed 16 Aster Commerce incidents, including all ten new records.

Examples: `INC-T0Mq_bTLLAYU` (Checkout database pool exhausted), `INC-AfJdwzkETnqb` (Inventory reservation lock contention), and `INC-RRetfy6W33Zx` (Payment session writes timing out). The live records carry individual notes identifying the user's requested Codex MCP sample batch. All ten package tests passed, including concurrent creation retries, missing/foreign references, invalid inputs, viewer/read-only restrictions, revoked grants, HTML escaping, and Markdown reuse. The application binary built successfully and the changed backend files passed the Proa performance lint.

## Remaining desktop acceptance

This verifies the real Codex MCP engine and the browser UI. It does **not** claim a model-driven tool conversation in the existing Codex Desktop task: that task's current tool catalog has not loaded the newly registered server. Claude Desktop's conversation has not been verified. The stricter two-desktop conversation checks in `DESIGN.md` remain open.

Protocol reference: [Codex app-server](https://developers.openai.com/codex/app-server). Browser header behavior: [Fetch Standard: Origin header](https://fetch.spec.whatwg.org/#origin-header).

## Presentation refresh — 2026-09-09 (PDT)

- Replaced starter UI stubs with the live `ui.proa.so` registry versions of
  Button, Badge, Card, Table, Input, and Textarea, including foundation/spinner.
  The source manifests remain pinned in `proa.lock.json`; Rust dependencies
  use this workspace's Proa crates. Imported sources were formatted locally.
- Refined list/detail, shared navigation, sign-in, connection, and lab styling.
  Checked the list, resolved issue, and open issue in the existing Chrome tab;
  completed normal browser OAuth using the installed Input/Button components.
  Checked the detail at a 390 × 844 viewport and restored the original size.
- HTML and Markdown now share ActivityLog as well as the other report children.
  Markdown preserves event bytes inside adaptive fences, uses one ordered
  recovery list, provides a canonical link, and omits mutation arguments for
  read-only/resolved projections. Component spacing is checked explicitly.
- All 12 tests pass across the full suite and the focused presentation rerun.
  `proa build --profile dev --bin mcp-incident-desk`, Proa source checks,
  verified formatting, Cargo formatting, and performance lint passed. The
  performance lint scanned all 77 application Rust files without findings.
- Renewed the existing native Codex OAuth login after its saved refresh grant
  was rejected, then retrieved fresh `get_issue_brief` responses through the
  native Codex app-server MCP engine for AST-1042 and INC-T0Mq_bTLLAYU.
  Confirmed resolved/open action differences and clean Markdown spacing after
  the final app restart. This remains a native engine verification, not a
  direct model-tool call from this chat or a Claude Desktop conversation.
