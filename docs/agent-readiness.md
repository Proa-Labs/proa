# Agent-readiness rollout

Snapshot: 2026-10-28. A fresh [Ora scan](https://ora.ai/score/proa.so) scored
**89/100**, up from 86/100 on October 27, before this PR was deployed. Site
changes cannot affect the score until deployment and another scan.
A perfect score is not guaranteed:
some checks depend on indexing, third-party adoption, or whether Ora treats
read-only APIs as exempt from write/job features.

## Changes ready for review

| Report finding | Implementation |
| --- | --- |
| Partial structured response coverage | `/openapi.json` now describes the four typed JSON GET operations. The full ten-operation website/legacy contract remains at `/openapi/site.json`; all existing routes remain available. |
| No SDKs or detected CLI | Published dependency-free JavaScript/TypeScript and Python clients with `proa-docs` CLIs, typed responses, pagination, bounded timeouts, and structured errors. These read docs and do not replace the Rust build CLI. |
| Missing section indexes | Framework, RSJS, and ecosystem `llms.txt` indexes, linked from the homepage HTML and Markdown. |
| Limited schema breadth | Visible homepage FAQ shared with Markdown and FAQPage JSON-LD; per-document TechArticle/APIReference and BreadcrumbList metadata. |
| Limited identity links | Verified Proa YouTube channel added alongside GitHub in `sameAs`. |
| Three authoring skills | Added documentation API and typed Markdown skills, bringing the published index to five after deployment. |
| No registry listing | Published the validated remote-server manifest as `io.github.proa-web/docs`, pointing directly to `https://proa.so/mcp`; the API guide includes its verified backlink. |

The SDKs have local HTTP contract/CLI tests and are included in CI. See the
[SDK release guide](../sdk/README.md) for packaging, publishing, and verification.
The Rust marketing tests cover the new schemas, section boundaries, metadata,
and consistent HTML/Markdown discovery links.

## External integration status

| Service | Observed state | Remaining action |
| --- | --- | --- |
| npm | Created the free `proa-labs` organization and published [@proa-labs/docs 0.1.0](https://www.npmjs.com/package/@proa-labs/docs). A fresh registry installation successfully searched the live API. | Complete. |
| PyPI | Published [proa-docs 0.1.1](https://pypi.org/project/proa-docs/0.1.1/) as `proa`, with wheel and source distribution. A fresh registry installation searched the live API. Both temporary release tokens were revoked and deletion confirmed by PyPI. | Complete. |
| Smithery | Created the Proa Labs workspace (`proa-labs`); attempted `proa-docs` registration. The final step returned 403: "Updating servers has been removed." No listing was confirmed. | Resume when Smithery supports publication, or resolve through its support process. |
| Glama | Submitted “Proa documentation” with hosted endpoint `https://proa.so/mcp`. Glama's email receipt confirms it is pending review. | Wait for approval, verify the public listing, then add its exact URL to the API guide. |
| Official MCP Registry | Published [io.github.proa-web/docs 1.0.0](https://registry.modelcontextprotocol.io/v0.1/servers?search=io.github.proa-web%2Fdocs) using the Proa GitHub identity. The registry reports it active. | Deploy the API guide's reciprocal link with this PR. |
| PulseMCP | Site states that new submissions are paused. | Resume when submissions reopen; no submission was made. |
| MCP.so | Submission flow requires a $39 one-time payment. | Optional paid submission needs a spending decision; no payment or submission was made. |
| Google Search Console | Verified the Proa account's domain ownership using an additional Cloudflare TXT record. Google processed `https://proa.so/sitemap.xml` successfully and discovered 125 pages. | Allow indexing and recheck the report after deployment. |

Publisher credentials stay outside the repository. Temporary PyPI release
tokens are revoked after publication; no secrets belong in this document or
source control.

The October 28 scan recognizes the npm SDK (1/1) and CLI (3/3). It still
detects only npm for multi-language SDKs (1/3), despite the verified PyPI
publication, and has not recognized the official MCP listing (0/1). The
deployed API guide needs the package and registry links from this PR before
retesting discovery. Search indexing and skills.sh adoption remain external
dependencies.

## Deployment and rescan

1. Review [PR #56](https://github.com/Proa-Labs/proa/pull/56). The current
   authorization covers committing, pushing, and opening the PR only; merging
   and production deployment remain separate actions.
2. Deploy the reviewed marketing changes through the normal release workflow
   after merge/deployment authorization.
3. Verify production's new indexes, five-skill catalog, FAQ/article metadata,
   and both OpenAPI documents. Check that API reads and MCP tools still work.
4. Confirm the published MCP listing and deployed API guide identify each other.
5. Allow search engines and skills.sh to reindex after deployment. The
   reported skills.sh maximum also requires 200+ genuine installs; adding two
   skills alone cannot satisfy that adoption threshold.
6. Request a fresh Ora scan, compare individual checks, and investigate any
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
