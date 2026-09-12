# Generated And Captured Assets

This repository contains a mix of source files, generated outputs, benchmark
fixtures, and public/static assets. Before public launch, each generated or
captured artifact should have a clear owner and regeneration path.

## Generated Outputs

- `proa_marketing/public/styles.css` is generated from
  `proa_marketing/styles/input.css`. Regenerate it from the repository root
  with:

  ```bash
  PROA_MARKETING_BUILD_CSS_ONLY=1 scripts/watch-marketing.sh
  ```

- `proa_demo/react-ssr/client.bundle.js` and Marko hydrate bundles are ignored
  and regenerated locally.

`proa_showcase_web/public/assets/airbnb-showcase.js` is source-owned browser
code, not a generated bundle. Edit it directly and validate it with
`node --check proa_showcase_web/public/assets/airbnb-showcase.js`.
The legacy travel listing still consumes the checked-in
`DatePickerIsland.js`, `BookingCardIsland.js`, and their one imported Calendar
chunk. Do not hand-edit those generated modules; remove them when the listing's
`build_islands` path is replaced.
Rust-authored `rsjs` island assets are generated from the checked-in Rust
sources at compile time and served through the island manifest; they are not
checked into `public/assets`.

## Captured Fixtures

Showcase and benchmark fixtures in this repository use Proa-authored markup,
fictional data, and synthetic visuals. Named reference comparisons may retain
the broad page shape needed to explain the workload, but they must not include
captured HTML, API responses, logos, photographs, or remote asset dependencies.
The retained files and their provenance are listed in
[`docs/asset-provenance.md`](docs/asset-provenance.md).

## Policy

When adding or updating generated assets:

- document the source input and command that regenerates the file
- avoid checking in local-only build outputs
- do not check in third-party captures without an explicit redistribution grant
- keep benchmark fixtures separate from product documentation claims
