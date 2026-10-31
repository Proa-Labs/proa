# Typed head metadata expansion

The bulk `HeadMetadata` API now covers document text, Open Graph, Twitter
summary cards, page robots/Googlebot policies, icons, alternate representations,
manifest links, authors, application details, keywords, referrer policy, and
verification tokens. `OpenGraph::article` adds publication/modification/expiration
times, author profile URLs, sections, and tags.

## Patch semantics

Every bulk field defaults to `HeadValue::Inherit`. `.into()` sets a value;
`HeadValue::Clear` removes it. A title accepts ordinary text or
`HeadTitle::absolute(...)` to bypass the inherited template. A template still
applies only to descendant hooks. Existing individual setters remain available.
Bulk literals using `Some(...)` must migrate to the new field representation.

Lists replace inherited lists, including an empty replacement. Open Graph and
Twitter builders patch individual properties. Clearing the outer social field
removes its entire object. Twitter defaults resolve at emission from explicit
Open Graph text, then final document text; its image falls back to the first
final Open Graph image. A cleared Twitter property suppresses its fallback.
Only summary and summary-large-image cards are supported in this change.

Validation prepares all changed inputs before mutation under the existing head
lock. URL policy, text limits, aggregate named/resource/extended slot budgets,
and the existing publication byte limit remain enforced. Article properties
share the graph's list budget and emit only for a final `article` type.
Timestamp strings are bounded and escaped, not parsed as dates.

Named and typed metadata share replacement semantics. Named overrides suppress
matching typed tags rather than duplicating them; typed setters can replace
those overrides. A named Twitter image override suppresses its old typed alt
text. Author and verification groups support repeated tags and whole-list
replacement. Slot accounting considers suppressed typed groups, so replacing
128 inherited author tags with one named tag is allowed. Verification names
cannot impersonate the reserved document/Twitter names.

`FrameworkBuilder::with_robots(RobotsTxt)` still serves `/robots.txt` and controls
crawl access. `HeadMetadata::robots` and `HeadWriter::robots` emit a fetched
page's HTML indexing/preview directives. A robots file does not communicate a
page's `noindex`; crawlers must be allowed to fetch the page to read its tag.

## Storage and allocation checks

Optional metadata families use an optional shared extension on `HeadFields`.
Lists are immutable shared slices. Twitter's resolved dynamic text uses shared
`HeadText`, so changing one property does not copy inherited title/description
strings. Existing title/description/canonical patches do not allocate extension
state or clone the complete field collection for validation.

The comparison baseline is PR commit `c6ac23e5`, including the latest Axum,
response, documentation, and CI fixes. Native probes link the same source to
baseline and candidate release libraries. All four storage outputs match
exactly: ordinary local/Send components, async ErrorBoundary, and local/Send
Suspense retain their allocation counts, allocated bytes, future/context sizes,
poll counts, and output bytes. See
[raw storage evidence](evidence/head-metadata-expansion-storage.json).

Complete-request probes use the retained `managed_requests/request.rs`, frozen
lockfile, identical release/LTO configuration, and the counting allocator in
both builds. Raw and endpoint tiny responses and a dynamic endpoint retain
identical allocations and bytes. The existing managed fixture retains 60
allocations and uses 48 additional allocated bytes (19,241 → 19,289). The new
bulk form uses 59 allocations and 19,278 bytes for the same 240-byte response.
New optional fields naturally pay for their data during head preparation; these
checks do not claim that adding icons or a Twitter card costs no work.

## Request timings and remaining cost

Fifteen alternating process pairs/triplets per case ran 100,000 complete warmed
requests per process, with compiler jobs idle. The binaries use the same frozen
request lockfile and release/LTO settings. Every output hash matched. No CPU
pinning was used; these are local regression measurements, not capacity claims.

| Complete request | Baseline median | Candidate median | Median change |
| --- | --- | --- | --- |
| Raw tiny response | 910.51 ns | 898.73 ns | -1.29% |
| Endpoint tiny response | 1,197.08 ns | 1,178.99 ns | -1.51% |
| Dynamic endpoint | 1,981.73 ns | 1,980.44 ns | -0.06% |
| Managed document, individual setters | 2,895.90 ns | 2,829.95 ns | -2.28% |

The candidate bulk form measured 2,892.36 ns in that run. The paired 95% bootstrap interval
for bulk versus the baseline individual setters includes zero. The ordinary
request intervals either include zero or favor the candidate; the individual-setter
managed interval favored the candidate. Matching allocation counts do not prove zero latency cost.

A separate direct old-object/new-object control used the baseline's original
bulk source (only the constructor syntax differs). Its medians were
2,911.66 → 2,995.11 ns: **+2.87%**, or 83.45 ns per complete request. The paired
median change was 3.54%, with a 95% bootstrap interval of
1.11% to 3.83%. This small, measurable cost remains; the expanded
object is not presented as a zero-overhead replacement for the smaller one.
Compare medians within each run, since host frequency and load vary.

An initial implementation measured about +4.6% in the direct object control.
Preparation now borrows its input through the internal boundaries, separates
optional-family validation from common fields, skips unchanged slot validation,
and permits inlining across the lifecycle check so unused literal fields can
be eliminated. Lifecycle validation itself still runs even for an empty patch.

`HeadFields` grew from 432 to 456 bytes; `OpenGraph` grew from 272 to 288 bytes.
The complete managed fixture's allocation count stayed constant, with 48 extra
allocated bytes across its request lifecycle. Existing ordinary request paths
retain their previous allocated bytes. New optional families pay for their own
payload during preparation. No separate latency claim is made for adding those
families, cold startup, full ORIGIN requests, browser hydration, or maximum HTTP
throughput. The earlier canonical benchmark review remains separate.

See [all request samples and provenance](evidence/head-metadata-expansion-requests.json)
for binary/source/lockfile hashes, allocation results, output hashes, paired
intervals, and the direct object control. The harness is
[`managed_requests`](../../proa_bench/probes/managed_requests/README.md); the direct
old-object control compiles `request.rs` from `c6ac23e5` with its `bulk` feature.

## Validation

- 779 affected library/integration tests passed across core, stream, framework
  core, and Axum. The final core-only run also passed all 285 tests after the
  shared Twitter-text refinement.
- The HTTP regression serves `/robots.txt` and page robots metadata together,
  through buffered and streamed responses. It checks dynamic route titles,
  inherited social defaults, clearing, URL resolution, and head-before-body
  publication.
- Workspace all-target checking, formatting, Proa source lint, and render
  performance lint passed. All 16 active core doctests passed; 5 remain ignored.
- The ORIGIN example/template parity check and its 5 library tests passed.
- All 33 implemented-API metadata-reference Rust snippets compiled. The 25
  independent examples with generated HTML were executed in fresh managed
  contexts, and their exact emitted tags matched the displayed output (ignoring
  display-only newlines). The guide's builder-installed app-wide defaults example
  also compiled. The separately labeled proposed `cx.url(...).segment(...)`
  examples are design illustrations and are excluded from that validation.

The [metadata reference](../../proa_marketing/src/docs/framework/metadata-reference.mdx)
shows each mutation immediately above its generated tags. Agent authoring
references, the ORIGIN scaffold/example, the changelog, and PR summary use the
new bulk field representation.
