# Agent-readiness rollout

Snapshot: 2026-10-28. The [Ora report](https://ora.ai/score/proa.so) showed
**86/100**, based on its October 27 scan. Local changes do not change that score
until they are deployed and scanned again. A perfect score is not guaranteed:
some checks depend on indexing, third-party adoption, or whether Ora treats
read-only APIs as exempt from write/job features.

## Changes ready for review

| Report finding | Implementation |
| --- | --- |
| Partial structured response coverage | `/openapi.json` now describes the four typed JSON GET operations. The full ten-operation website/legacy contract remains at `/openapi/site.json`; all existing routes remain available. |
| No SDKs or detected CLI | Dependency-free JavaScript/TypeScript and Python clients with `proa-docs` CLIs, typed responses, pagination, bounded timeouts, and structured errors. Python 0.1.0 is published; npm publication is pending. These read docs and do not replace the Rust build CLI. |
| Missing section indexes | Framework, RSJS, and ecosystem `llms.txt` indexes, linked from the homepage HTML and Markdown. |
| Limited schema breadth | Visible homepage FAQ shared with Markdown and FAQPage JSON-LD; per-document TechArticle/APIReference and BreadcrumbList metadata. |
| Limited identity links | Verified Proa YouTube channel added alongside GitHub in `sameAs`. |
| Three authoring skills | Added documentation API and typed Markdown skills, bringing the published index to five after deployment. |
| No registry listing | Validated remote-server manifest in `proa_marketing/server.json`, pointing directly to `https://proa.so/mcp`. Publication remains pending. |

The SDKs have local HTTP contract/CLI tests and are included in CI. See the
[SDK release guide](../sdk/README.md) for packaging, publishing, and verification.
The Rust marketing tests cover the new schemas, section boundaries, metadata,
and consistent HTML/Markdown discovery links.

## External integration status

| Service | Observed state | Remaining action |
| --- | --- | --- |
| npm | Signed in as `proa_rs`; created the free `proa-labs` organization and verified its ownership. CLI authorization is still pending after a security-key error. | Complete CLI authentication, publish `@proa-labs/docs@0.1.0`, and verify a fresh installation. |
| PyPI | Published [proa-docs 0.1.0](https://pypi.org/project/proa-docs/0.1.0/) as `proa`, with wheel and source distribution. A fresh registry installation successfully searched the live API. | Complete the password-confirmed revocation of the temporary upload token. |
| Smithery | Created the Proa Labs workspace (`proa-labs`); attempted `proa-docs` registration. The final step returned 403: "Updating servers has been removed." No listing was confirmed. | Resume when Smithery supports publication, or resolve through its support process. |
| Glama | Submitted “Proa documentation” with hosted endpoint `https://proa.so/mcp`. Glama's email receipt confirms it is pending review. | Wait for approval, verify the public listing, then add its exact URL to the API guide. |
| Official MCP Registry | Manifest validates; its version now matches the running server's `1.0.0`, with a regression test. `so.proa/docs` needs domain ownership verification. | Complete domain verification or authenticate the Proa GitHub publishing identity, publish, and add the confirmed listing URL to the API guide. |
| PulseMCP | Site states that new submissions are paused. | Resume when submissions reopen; no submission was made. |
| MCP.so | Submission flow requires a $39 one-time payment. | Optional paid submission needs a spending decision; no payment or submission was made. |
| Google Search Console | Accessible Proa accounts have no verified access to the `proa.so` domain property. | Verify ownership, submit `https://proa.so/sitemap.xml`, and request indexing for the API/agent guides. |

The browser publishing pages are waiting for authentication or the secure upload
step. Credentials and 2FA codes must be entered directly into the service or its
secure publishing prompt. No secrets belong in this document or source control.

## Deployment and rescan

1. Review the local changes. Follow the repository's authorization policy for
   committing, pushing, and opening a pull request.
2. Publish and verify SDK packages using the release guide; replace first-release
   notices with the confirmed package links and installation commands.
3. Deploy the reviewed marketing changes through the normal release workflow.
4. Verify production's new indexes, five-skill catalog, FAQ/article metadata,
   and both OpenAPI documents. Check that API reads and MCP tools still work.
5. Complete at least one registry listing and add its exact backlink to the
   API guide. Confirm both the registry and Proa website identify each other.
6. Submit the sitemap and allow search engines and skills.sh to reindex. The
   reported skills.sh maximum also requires 200+ genuine installs; adding two
   skills alone cannot satisfy that adoption threshold.
7. Request a fresh Ora scan, compare individual checks, and investigate any
   remaining failures using the deployed responses rather than assumed points.

## Read-only service constraints

The documentation API, including legacy `POST /api/search`, only reads public
compiled content. It has no state-changing operation requiring an idempotency
key, no long-running operation requiring job polling, and no paid/live account
state requiring a separate sandbox. The API guide and canonical OpenAPI
description now state this explicitly. Creating artificial writes, jobs, or
credentials would make the API worse. If Ora still deducts those points, its
applicability assessment needs review.

The report's generated integration narrative also describes API-key
authentication, despite its own successful zero-auth probe. Recheck that claim
against `/auth.md`, the OpenAPI security definition, and a live unauthenticated
API request after deployment. The verified YouTube identity is legitimate, but
Ora's authority-profile allowlist may not count it; do not add an unrelated
Wikipedia, Wikidata, or LinkedIn entity just to satisfy the check.

The full repository boundary check currently also reports three pre-existing
developer-machine references in untracked `output/` research artifacts. Those
files were left unchanged and are outside this work.
