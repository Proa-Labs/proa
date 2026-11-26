# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1](https://github.com/Proa-Labs/proa/compare/proa-cli-v0.2.0...proa-cli-v0.2.1) - 2026-11-26

### Fixed

- *(cli)* ship Intel macOS release archives ([#117](https://github.com/Proa-Labs/proa/pull/117))

### Other

- Add Linux arm64 & Windows CLI releases, honor --to on source fallback, and speed up proa ui add ([#116](https://github.com/Proa-Labs/proa/pull/116))
- Speed up proa dev HMR (3.5s -> 1.7s) and cut proa-rsjs disk usage by 69% ([#114](https://github.com/Proa-Labs/proa/pull/114))
- Polish 0.4.1 onboarding: macOS arm64 CLI release, quieter installer/UI DX, and code-first docs fixes ([#112](https://github.com/Proa-Labs/proa/pull/112))

## [0.2.0](https://github.com/Proa-Labs/proa/compare/proa-cli-v0.1.3...proa-cli-v0.2.0) - 2026-11-24

### Added

- *(framework)* serve linked RSJS assets by default in FrameworkBuilder ([#104](https://github.com/Proa-Labs/proa/pull/104))
- *(framework)* default the island manifest and add RouteResponse::render_to_bytes ([#76](https://github.com/Proa-Labs/proa/pull/76))

### Fixed

- Generated pages, components, and layouts pass the formatting check during `proa build`, including Markdown components with children.
- *(axum)* follow up on the Axum 0.8 upgrade ([#108](https://github.com/Proa-Labs/proa/pull/108))
- generate application modules through the Proa facade ([#82](https://github.com/Proa-Labs/proa/pull/82))
- *(cli)* wait for dev readiness before opening browser ([#28](https://github.com/Proa-Labs/proa/pull/28))

### Other

- Secure registry credentials, client responses, and renderer output ([#106](https://github.com/Proa-Labs/proa/pull/106))
- Upgrade Rust dependencies and retire React interop ([#107](https://github.com/Proa-Labs/proa/pull/107))
- Make the build output a complete, host-servable mirror of every asset URL ([#101](https://github.com/Proa-Labs/proa/pull/101))
- Bundle the public asset catalog beside the exported executable ([#100](https://github.com/Proa-Labs/proa/pull/100))
- Improve proa ui CLI DX and extend asset-URL / missing-public-file lints ([#98](https://github.com/Proa-Labs/proa/pull/98))
- Polish P2 DX: expression-anchored macro hints, idiomatic home Markdown, single asset mount, and no rust-toolchain pin ([#96](https://github.com/Proa-Labs/proa/pull/96))
- DX improvements: metadata cx.url helpers, env PORT/HOST, live public assets, primitive renders, and docs search ([#97](https://github.com/Proa-Labs/proa/pull/97))
- Improve P1 onboarding DX: persistent config cache, fast public/CSS dev reload, Markdown alt coverage, and scaffold cleanup ([#94](https://github.com/Proa-Labs/proa/pull/94))
- Fix P0 onboarding DX: shared Cargo targets, zero-flag build->start, md URL slots, and clean scaffold ([#92](https://github.com/Proa-Labs/proa/pull/92))
- Add Rust project config and fingerprinted public assets ([#90](https://github.com/Proa-Labs/proa/pull/90))
- Validate UTF-8 at the write and stop zero-filling render pages ([#83](https://github.com/Proa-Labs/proa/pull/83))
- Add the proa facade and migrate application entrypoints ([#79](https://github.com/Proa-Labs/proa/pull/79))
- Retire unused framework features and redundant tests ([#77](https://github.com/Proa-Labs/proa/pull/77))
- Cache Rust validation and standardize crates.io releases ([#68](https://github.com/Proa-Labs/proa/pull/68))
- Make RSJS URL mounting explicit and support cx.url() ([#69](https://github.com/Proa-Labs/proa/pull/69))
- Remove content_part and infer content coverage from inputs ([#66](https://github.com/Proa-Labs/proa/pull/66))
- Remove obsolete code and repair standalone site scaffolds ([#67](https://github.com/Proa-Labs/proa/pull/67))
- Add team page and footer navigation ([#65](https://github.com/Proa-Labs/proa/pull/65))
- Markdown templates: automatic block spacing, one string body, no heading offsets ([#63](https://github.com/Proa-Labs/proa/pull/63))
- Make Markdown template slots Rust at every depth ([#61](https://github.com/Proa-Labs/proa/pull/61))
- Brand New Metadata API Design ([#60](https://github.com/Proa-Labs/proa/pull/60))
- Add installation-aware proa update with versioned standalone bundles ([#49](https://github.com/Proa-Labs/proa/pull/49))
- Add builder-managed sitemaps and robots.txt endpoints ([#50](https://github.com/Proa-Labs/proa/pull/50))
- Fix Windows linked builds and add portability coverage ([#46](https://github.com/Proa-Labs/proa/pull/46))
- Show live Proa lint diagnostics in VS Code and LSP editors ([#45](https://github.com/Proa-Labs/proa/pull/45))
- Reduce workspace compile times and isolate tooling engines ([#33](https://github.com/Proa-Labs/proa/pull/33))
- Reduce warm rebuild latency with incremental descriptors and per-unit plan reuse ([#13](https://github.com/Proa-Labs/proa/pull/13))
- Support runtime base paths for application and island URLs ([#38](https://github.com/Proa-Labs/proa/pull/38))
- Align website and docs with Proa's multi-target interface positioning ([#31](https://github.com/Proa-Labs/proa/pull/31))
