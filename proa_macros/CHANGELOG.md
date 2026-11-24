# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/Proa-Labs/proa/releases/tag/proa_macros-v0.4.0) - 2026-11-24

### Other

- Secure registry credentials, client responses, and renderer output ([#106](https://github.com/Proa-Labs/proa/pull/106))
- Upgrade Rust dependencies and retire React interop ([#107](https://github.com/Proa-Labs/proa/pull/107))
- Make the build output a complete, host-servable mirror of every asset URL ([#101](https://github.com/Proa-Labs/proa/pull/101))
- Fix P0 onboarding DX: shared Cargo targets, zero-flag build->start, md URL slots, and clean scaffold ([#92](https://github.com/Proa-Labs/proa/pull/92))
- Validate UTF-8 at the write and stop zero-filling render pages ([#83](https://github.com/Proa-Labs/proa/pull/83))
- Add the proa facade and migrate application entrypoints ([#79](https://github.com/Proa-Labs/proa/pull/79))
- Retire unused framework features and redundant tests ([#77](https://github.com/Proa-Labs/proa/pull/77))
- Remove content_part and infer content coverage from inputs ([#66](https://github.com/Proa-Labs/proa/pull/66))
- Remove obsolete code and repair standalone site scaffolds ([#67](https://github.com/Proa-Labs/proa/pull/67))
- Markdown templates: automatic block spacing, one string body, no heading offsets ([#63](https://github.com/Proa-Labs/proa/pull/63))
- Make Markdown template slots Rust at every depth ([#61](https://github.com/Proa-Labs/proa/pull/61))
- Fix Markdown control-flow indentation and nested block formatting
- Brand New Metadata API Design ([#60](https://github.com/Proa-Labs/proa/pull/60))
- Add builder-managed sitemaps and robots.txt endpoints ([#50](https://github.com/Proa-Labs/proa/pull/50))
- Fix Windows linked builds and add portability coverage ([#46](https://github.com/Proa-Labs/proa/pull/46))
- Fix Open Graph meta property validation ([#44](https://github.com/Proa-Labs/proa/pull/44))
- Reduce workspace compile times and isolate tooling engines ([#33](https://github.com/Proa-Labs/proa/pull/33))
- Reduce warm rebuild latency with incremental descriptors and per-unit plan reuse ([#13](https://github.com/Proa-Labs/proa/pull/13))
