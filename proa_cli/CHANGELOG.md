# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Proa-Labs/proa/compare/proa-cli-v0.1.3...proa-cli-v0.2.0) - 2026-11-10

### Added

- *(framework)* default the island manifest and add RouteResponse::render_to_bytes ([#76](https://github.com/Proa-Labs/proa/pull/76))

### Fixed

- *(cli)* wait for dev readiness before opening browser ([#28](https://github.com/Proa-Labs/proa/pull/28))

### Other

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
