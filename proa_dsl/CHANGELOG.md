# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Proa-Labs/proa/releases/tag/proa_dsl-v0.2.0) - 2026-11-24

### Added

- expose RSJS through Proa and expand default features

### Other

- Upgrade Rust dependencies and retire React interop ([#107](https://github.com/Proa-Labs/proa/pull/107))
- Polish P2 DX: expression-anchored macro hints, idiomatic home Markdown, single asset mount, and no rust-toolchain pin ([#96](https://github.com/Proa-Labs/proa/pull/96))
- Generalize asset-URL linter across structs, constructors, Markdown, render_head, and public/ catalog ([#93](https://github.com/Proa-Labs/proa/pull/93))
- Fix P0 onboarding DX: shared Cargo targets, zero-flag build->start, md URL slots, and clean scaffold ([#92](https://github.com/Proa-Labs/proa/pull/92))
- Add the proa facade and migrate application entrypoints ([#79](https://github.com/Proa-Labs/proa/pull/79))
- Remove content_part and infer content coverage from inputs ([#66](https://github.com/Proa-Labs/proa/pull/66))
- Remove obsolete code and repair standalone site scaffolds ([#67](https://github.com/Proa-Labs/proa/pull/67))
- Markdown templates: automatic block spacing, one string body, no heading offsets ([#63](https://github.com/Proa-Labs/proa/pull/63))
- Make Markdown template slots Rust at every depth ([#61](https://github.com/Proa-Labs/proa/pull/61))
- Fix Markdown control-flow indentation and nested block formatting
- Add builder-managed sitemaps and robots.txt endpoints ([#50](https://github.com/Proa-Labs/proa/pull/50))
- Fix Windows linked builds and add portability coverage ([#46](https://github.com/Proa-Labs/proa/pull/46))
- Fix Open Graph meta property validation ([#44](https://github.com/Proa-Labs/proa/pull/44))
- Reduce workspace compile times and isolate tooling engines ([#33](https://github.com/Proa-Labs/proa/pull/33))
- Reduce warm rebuild latency with incremental descriptors and per-unit plan reuse ([#13](https://github.com/Proa-Labs/proa/pull/13))
