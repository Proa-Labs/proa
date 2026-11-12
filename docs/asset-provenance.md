# Asset provenance and trademark policy

Prefer Proa-authored markup, synthetic data, and first-party visual fixtures
for new showcases. Historical reference benchmarks are the exception: the
manifest below records the public-page captures that are intentionally retained
to reproduce the named Airbnb, Nike, eBay, GitHub, and ESPN workloads.
Adding or replacing a third-party capture requires an explicit repository-owner
review and a provenance entry with the source or capture procedure.

Product names and visual material remain the property of their respective
owners. Their use identifies the reference workload and does not imply
affiliation or endorsement. The reference pages must remain clearly framed as
unofficial Proa technology demos.

## Retained asset manifest

Each path pattern below covers only files with the same provenance. CI requires
new binary assets and captured-fixture-shaped files to match a row here.

| Path pattern | Provenance | Use |
|---|---|---|
| `assets/logo.svg` | Proa-authored | Repository logo |
| `assets/discord/proa-icon-white.png` | Generated from the Proa favicon with an opaque white background | Discord community icon |
| `proa_marketing/public/favicon.svg` | Proa-authored | Proa site identity |
| `proa_marketing/public/favicon.ico` | Generated from the Proa-authored favicon | Browser icon |
| `proa_marketing/public/apple-touch-icon.png` | Rendered from the Proa-authored favicon with an opaque background in commit `ab6e5cd` | iOS home-screen icon |
| `proa_marketing/public/og.jpg` | Compressed from the supplied Proa site artwork | Social preview |
| `proa_marketing/public/images/team/andrew-hariri.jpg` | Andrew Hariri's public GitHub profile portrait, downloaded from `https://avatars.githubusercontent.com/u/31301117?v=4&s=512` for his requested team profile | Proa founder portrait |
| `proa_marketing/public/images/blog/*.svg` | Proa-authored | Explanatory diagrams |
| `assets/benchmarks/*.svg` | Derived from Proa Labs' Apache-2.0 framework-benchmarks captures; exact revision and derivation in [Benchmark charts](#benchmark-charts) | Dated README performance figures |
| `proa_marketing/public/images/benchmarks/*.svg` | Byte-identical copies of the corresponding README figures, with the same [provenance](#benchmark-charts) | Shared website HTML, Markdown, and chart metadata |
| `proa_marketing/public/images/synthetic/*.svg` | Proa-authored geometric illustrations | Demo, benchmark, and marketing fixture imagery |
| `proa_cli/src/site_scaffold/templates/origin/public/fonts/*.woff2` | Vendored from Google Fonts by `proa_fonts_build`; Instrument Sans and Azeret Mono are SIL OFL-1.1, whose text sits beside the files | Scaffold reference-project self-hosted webfonts |
| `proa_marketing/public/images/airbnb/**` | Captured from public Airbnb pages used by the retained home, search, and listing fixtures | Airbnb reference showcase imagery |
| `proa_marketing/public/images/airbnb-preview-*` | Derived from the retained Airbnb captures | Airbnb marketing previews |
| `proa_marketing/public/images/airbnb-showcase-home*` | Derived from the retained Airbnb captures | Airbnb marketing previews and image fallbacks |
| `proa_marketing/public/images/nike/**` | Captured from public Nike catalog and Alphafly 3 product pages | Nike catalog and PDP reference imagery |
| `proa_showcase/src/espn/data/game.json` | Derived from the retained ESPN event API snapshot for event `401695777` | ESPN Gamecast serialization benchmark |
| `proa_showcase/src/espn/data/summary-401695777.json` | Captured from ESPN event API `401695777` | Source snapshot for the Gamecast fixture |
| `proa_showcase/src/espn/data/images/**` | Captured from ESPN CDN event, team, venue, and news endpoints | ESPN Gamecast reference imagery |
| `proa_showcase/src/ebay/data/*.css` | Extracted from the retained eBay search-results capture | Cross-engine page-shape benchmark |
| `proa_showcase/src/ebay/srp_chrome_real.rs` | Generated from the retained eBay capture | Proa eBay document chrome |
| `proa_demo/marko-server/page_srp_real.marko` | Authored from the retained eBay capture | Marko comparison page |
| `proa_demo/react-ssr/chrome.generated.tsx` | Generated from the retained eBay capture | React comparison page chrome |
| `proa_demo/src/github_pr_diff_fixtures.rs` | Derived from the public `oven-sh/bun` PR 31136 diff | Proa GitHub PR reference fixture |
| `proa_demo/react-ssr-shared/src/github-pr-diff-fixtures.ts` | Derived from the public `oven-sh/bun` PR 31136 diff | React GitHub PR reference fixture |
| `proa_demo/src/github_pr_diff.rs` | References GitHub's public default avatar asset | Proa GitHub PR reference page |
| `proa_demo/react-ssr-shared/src/github-pr-diff.tsx` | References GitHub's public default avatar asset | React GitHub PR reference page |
| `proa_demo/showcase-props.json` | Shared snapshot of the retained reference-page datasets | Showcase and FFI fixture |

The synthetic illustrations deliberately avoid third-party logos, product
designs, photos, and trade dress. When adding an asset, update this manifest
with its author or source and the capture or generation procedure when
applicable.

Run `python3 scripts/check_asset_provenance.py` before committing asset or
fixture changes.

## Benchmark charts

Source: [Proa-Labs/framework-benchmarks](https://github.com/Proa-Labs/framework-benchmarks), Apache-2.0.
Benchmark source revision: `04c702ba2f959ef711551604c3f83a3123d1b1e8`
Measured Proa revision: `bbbc9684335109949257eea8b4c98bb7bc8572ae`.

| Local filename under `assets/benchmarks/` | Upstream source | Treatment |
| --- | --- | --- |
| [01-canonical-scoreboard.svg](../assets/benchmarks/01-canonical-scoreboard.svg) | `charts/svg/01-canonical-scoreboard.svg` | Exact copy |
| [21-canonical-pages-http.svg](../assets/benchmarks/21-canonical-pages-http.svg) | `charts/svg/08-canonical-pages-http.svg` | Exact copy |
| [22-cold-start.svg](../assets/benchmarks/22-cold-start.svg) | `charts/svg/09-cold-start.svg` | Exact copy |
| [20-suspense-load.svg](../assets/benchmarks/20-suspense-load.svg) | `charts/figures/suspense.mjs`, using `sveltekit.md` | Matched-host Proa/SvelteKit rows; two-column latency layout |

Proa was refreshed November 11, 2026 on Apple M4 Max; peer rows retain October
21 captures. The render-only lane includes fresh buffer allocation and excludes
String conversion. Cold-start captures use warm filesystem caches on a shared
host. The derived streaming chart excludes the upstream Xeon Marko/React rows,
adjusts the legend and layout, and corrects the same-host caption. Retained
measurements are unchanged. SvelteKit's response end delivers data; Proa's
response end delivers resolved HTML. Browser completion is reported separately.

Website copies under `proa_marketing/public/images/benchmarks/` match the render,
HTTP, and streaming figures byte for byte. Local filenames preserve existing
public URLs. To refresh from a clean checkout of the recorded source revision:

```sh
node scripts/sync_benchmark_assets.mjs /path/to/framework-benchmarks
node scripts/sync_benchmark_assets.mjs /path/to/framework-benchmarks --check
```

When selecting a newer revision, review the README, website captions, and
performance guide against the raw captures as well as regenerating the SVGs.

### Landing-page paired comparisons

The GitHub, Sentry, and Airbnb hero metrics use a separate [paired GCP CI capture
and methodology](https://github.com/Proa-Labs/framework-benchmarks/blob/65474b57b5c02f1f5cd881c886ef4148f2b6959e/showcase/README.md#gcp-ci-capture-november-12-2026-utc),
recorded November 12, 2026 UTC on `c4-standard-16-lssd` (Intel Xeon 6985P-C,
Granite Rapids), the same machine type and image as Proa CI. Timed processes
were pinned to CPU 0. The capture uses Proa `bbbc968`, Rust 1.95.0, and production
React 19.2.4 on Node 26.13.0. Each figure is the median of five independent
process P50s.

The selected figures explicitly prime Proa's allocator before render warmups.
Both engines return fresh complete Strings, including allocation and document
assembly; UTF-8 Buffer encoding, output destruction, HTTP, and browser work
are excluded. The same-host default-allocator comparison is retained alongside
the selected capture because allocator state materially affects these timings.
Full DOM parity and changed-output negative controls passed for every fixture.
Raw samples, process ranges, the exact native priming source, and image/host
provenance are retained. These paired results have a different host and output
boundary from the canonical scoreboard above; the figures are not mixed.

## Historical performance diagrams

| Path | Source / license | Purpose |
|---|---|---|
| `docs/compile-time/diagrams/*.png` | Generated from local compiler measurements by Proa Labs, Inc. | Historical build performance reports |
