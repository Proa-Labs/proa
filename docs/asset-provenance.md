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
| `deploy/proa-preview-app-icon.png` | Generated from Proa-authored brand artwork | Preview deployment icon |
| `proa_marketing/public/favicon.svg` | Proa-authored | Proa site identity |
| `proa_marketing/public/favicon.ico` | Generated from the Proa-authored favicon | Browser icon |
| `proa_marketing/public/apple-touch-icon.png` | Rendered from the Proa-authored favicon with an opaque background in commit `ab6e5cd` | iOS home-screen icon |
| `proa_marketing/public/og.jpg` | Compressed from the supplied Proa site artwork | Social preview |
| `proa_marketing/public/images/blog/*.svg` | Proa-authored | Explanatory diagrams |
| `proa_marketing/public/images/synthetic/*.svg` | Proa-authored geometric illustrations | Demo, benchmark, and marketing fixture imagery |
| `examples/origin-routing-atlas/public/fonts/*.woff2` | Vendored from Google Fonts by `proa_fonts_build`; Instrument Sans and Azeret Mono are SIL OFL-1.1, whose text sits beside the files | Example self-hosted webfonts |
| `proa_cli/src/site_scaffold/templates/origin/public/fonts/*.woff2` | Vendored from Google Fonts by `proa_fonts_build`; Instrument Sans and Azeret Mono are SIL OFL-1.1, whose text sits beside the files | Scaffold reference-project self-hosted webfonts |
| `proa_marketing/public/images/airbnb/**` | Captured from public Airbnb pages used by the retained home, search, and listing fixtures | Airbnb reference showcase imagery |
| `proa_marketing/public/images/airbnb-preview-*` | Derived from the retained Airbnb captures | Airbnb marketing previews |
| `proa_marketing/public/images/airbnb-showcase-home*` | Derived from the retained Airbnb captures | Airbnb marketing previews and image fallbacks |
| `proa_marketing/public/images/nike/**` | Captured from public Nike catalog and Alphafly 3 product pages | Nike catalog and PDP reference imagery |
| `proa_showcase/src/espn/data/game.json` | Derived from the retained ESPN event API snapshot for event `401695777` | ESPN Gamecast serialization benchmark |
| `proa_showcase/src/espn/data/summary-401695777.json` | Captured from ESPN event API `401695777` | Source snapshot for the Gamecast fixture |
| `proa_showcase/src/espn/data/images/**` | Captured from ESPN CDN event, team, venue, and news endpoints | ESPN Gamecast reference imagery |
| `proa_showcase/src/ebay/data/*.html` | Captured from a public eBay search-results page | Cross-engine page-shape benchmark |
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
