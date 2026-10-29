# Agent-readiness rollout

Snapshot: 2026-10-29 at 00:19 UTC. After PR #58 merged and production revision
`c5e1fa3434d6a7f59356127f150afca4f2274b6d` deployed, a forced
[Ora scan](https://ora.ai/score/proa.so) completed at **94/100**, up from
91/100 after PR #56. Public API documentation, schema breadth, response-schema
coverage, function-calling compatibility, and the official MCP Registry now
pass. Ora detects both npm and PyPI SDKs (2/3); a third ecosystem is needed
for the remaining SDK point. A perfect score is not guaranteed: some checks
depend on search samples or whether Ora treats read-only APIs as exempt from
write/job features.

Ora's [check catalog](https://ora.ai/api/checks) marks the skills-install check
as **emerging**, which is excluded from the canonical score. Its 200-install
target does not block 100/100. Tranco top-50,000 rank is a separate
[leaderboard eligibility criterion](https://ora.ai/leaderboard/criteria),
not a readiness-score deduction.

## Deployed changes

| Report finding | Implementation |
| --- | --- |
| Partial structured response coverage | `/openapi.json` now describes the four typed JSON GET operations. The full ten-operation website/legacy contract remains at `/openapi/site.json`; all existing routes remain available. |
| No SDKs or detected CLI | Published dependency-free JavaScript/TypeScript and Python clients with `proa-docs` CLIs, typed responses, pagination, bounded timeouts, and structured errors. These read docs and do not replace the Rust build CLI. |
| Missing section indexes | Framework, RSJS, and ecosystem `llms.txt` indexes, linked from the homepage HTML and Markdown. |
| Limited schema breadth | Visible homepage FAQ, videos, and benchmark charts share their content with FAQPage, VideoObject, ItemList, and ImageObject metadata. Documents expose TechArticle/APIReference and BreadcrumbList metadata. |
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
| Glama | The [Proa documentation connector](https://glama.ai/mcp/connectors/so.proa/proa-documentation) is publicly listed. Verified October 29: Healthy, Streamable HTTP, endpoint `https://proa.so/mcp`, and both read-only tools. | This follow-up adds the exact listing URL to the API guide and shared homepage HTML/Markdown footer; recheck Ora after deployment. |
| Official MCP Registry | Published [io.github.proa-web/docs 1.0.0](https://registry.modelcontextprotocol.io/v0.1/servers?search=io.github.proa-web%2Fdocs) using the Proa GitHub identity. Ora now verifies it through the first-party backlink (1/1). | Complete. |
| LinkedIn | The user created [Proa Labs](https://www.linkedin.com/company/proa-labs/). Verified its name, Rust tagline, and website link to `https://proa.so`. | This follow-up adds it to Organization `sameAs` and the shared HTML/Markdown homepage links. |
| PulseMCP | Site states that new submissions are paused. | Resume when submissions reopen; no submission was made. |
| MCP.so | Submission flow requires a $39 one-time payment. | Optional paid submission needs a spending decision; no payment or submission was made. |
| Google Search Console | Verified the Proa account's domain ownership using an additional Cloudflare TXT record. Google processed `https://proa.so/sitemap.xml` successfully and discovered 125 pages. Indexing requests for `/docs/api` and `/docs/agents` were accepted; the latter was already indexed. | Allow recrawling and recheck Ora's name-based search sample. |

Publisher credentials stay outside the repository. Temporary PyPI release
tokens are revoked after publication; no secrets belong in this document or
source control.

The October 29 scan recognizes the npm SDK (1/1), CLI (3/3), both SDK
ecosystems (2/3), and the official MCP listing (1/1). The production homepage
and API guide link both packages and the registry. Name-based developer
resource discovery remains 1/3 despite `/docs/api` appearing in a separate
web search; search providers and sampled queries can differ.

## Follow-up fixes

PR #58 merged on October 29 at 00:00 UTC and its normal production deployment
completed before the 00:19 scan. These homepage changes are live:

- Put the API guide before subsection indexes in the shared homepage footer.
  The deployed scan inspected only `/docs`, `/docs/llms.txt`, and
  `/docs/framework/llms.txt`, reducing public API documentation to 2/3.
- Link the published Python SDK, TypeScript SDK, and official MCP entry
  directly from homepage HTML and Markdown.
- Describe the homepage's existing videos and three benchmark charts with
  `VideoObject`, `ItemList`, and `ImageObject`, sharing their URLs and chart
  content with the visible page.

Ora's live `POST /api/scan/checks` against a temporary public preview passed
both changed checks under contract 1.37.0: public API documentation **3/3**
(`API/docs link found on homepage and resolves: /docs/api`) and schema breadth
**2/2** (`FAQPage, VideoObject, ItemList`). This selected check run did not
update the canonical production score. All 161 marketing tests, Proa format
and lint checks, and the render-allocation lint passed.

PR #59 adds a functional optional `Idempotency-Key` to a typed
JSON `POST /api/v1/search`. A successful request is retained for fifteen
minutes on the serving instance. Matching retries replay its JSON body;
changed parameters with an active key return 422. The bounded cache rejects
new keys with 503 when full and is not shared across replicas or restarts.
This adds one operation to each OpenAPI document: five JSON operations in the
canonical contract and eleven in the complete website contract. Existing GET,
MCP, SDK, and legacy search clients continue to work.

Ora's live selected checks against this follow-up's temporary public preview
under contract 1.38.0 awarded idempotency **3/3**, response-schema coverage
**2/2**, and function-calling compatibility **2/2**. A direct public HTTP
probe also confirmed the first keyed response reports
`Idempotency-Replayed: false` and the matching retry reports `true`.
All 169 marketing tests passed, including replay, expiry, capacity, concurrent
requests, header validation, body limits, and typed errors. This preview check
does not update the canonical production score.

After adding the verified LinkedIn company profile, a second preview check
under contract 1.38.0 awarded entity linking **2/2** and identified both
`github.com` and `linkedin.com`. The same URL constant supplies Organization
`sameAs` and the homepage's shared HTML/Markdown Explore links. The 169
marketing tests and formatting/performance checks still pass.

## Deployment and rescan

1. Review and merge the follow-up PR. The current authorization permits
   committing, pushing, and opening a PR; the user performs the merge.
2. Wait for the normal production release and verify `/readyz` reports the
   expected `x-proa-revision`.
3. Recheck homepage links and structured data, then request a fresh Ora scan.
   `POST /api/scan` with `force: true` bypasses scan freshness; forced scans are
   limited to six per day. Selected live checks use `POST /api/scan/checks`
   and do not update the canonical score without an account key.
4. Confirm remaining deductions against live evidence. Registry publication,
   correct schemas, and local tests do not themselves prove a score increase.

## Read-only service constraints

The documentation API, including both POST search interfaces, only reads
public compiled content. JSON POST search now offers optional response replay
with real retention and conflict handling. There is still no long-running
operation requiring job polling or paid/live account state requiring a
separate sandbox. The API guide and canonical OpenAPI description state these
limits. Artificial writes, jobs, or credentials would make the API worse;
remaining applicability deductions need review by Ora.

The report's generated integration narrative also describes API-key
authentication, despite its own successful zero-auth probe. Recheck that claim
against `/auth.md`, the OpenAPI security definition, and a live unauthenticated
API request after deployment. The verified YouTube identity is legitimate, but
Ora's authority-profile allowlist may not count it; do not add an unrelated
Wikipedia, Wikidata, or LinkedIn entity just to satisfy the check.

The full repository boundary check currently also reports three pre-existing
developer-machine references in untracked `output/` research artifacts. Those
files were left unchanged and are outside this work.
