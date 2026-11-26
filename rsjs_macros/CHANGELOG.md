# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/Proa-Labs/proa/compare/rsjs_macros-v0.3.0...rsjs_macros-v0.3.1) - 2026-11-26

### Other

- updated the following local packages: proa_core, proa_macros

## [0.3.0](https://github.com/Proa-Labs/proa/releases/tag/rsjs_macros-v0.3.0) - 2026-11-24

### Added

- expose RSJS through Proa and expand default features

### Fixed

- *(rsjs)* resolve browser-read placeholders through the facade and keep rewritten path spans
- *(rsjs)* compile linked lookups for unused components ([#41](https://github.com/Proa-Labs/proa/pull/41))

### Other

- Secure registry credentials, client responses, and renderer output ([#106](https://github.com/Proa-Labs/proa/pull/106))
- Upgrade Rust dependencies and retire React interop ([#107](https://github.com/Proa-Labs/proa/pull/107))
- DX improvements: metadata cx.url helpers, env PORT/HOST, live public assets, primitive renders, and docs search ([#97](https://github.com/Proa-Labs/proa/pull/97))
- Add Rust project config and fingerprinted public assets ([#90](https://github.com/Proa-Labs/proa/pull/90))
- Add the proa facade and migrate application entrypoints ([#79](https://github.com/Proa-Labs/proa/pull/79))
- Retire unused framework features and redundant tests ([#77](https://github.com/Proa-Labs/proa/pull/77))
- Clarify RSJS URL opt-in and request diagnostics ([#70](https://github.com/Proa-Labs/proa/pull/70))
- Make RSJS URL mounting explicit and support cx.url() ([#69](https://github.com/Proa-Labs/proa/pull/69))
- Remove obsolete code and repair standalone site scaffolds ([#67](https://github.com/Proa-Labs/proa/pull/67))
- Brand New Metadata API Design ([#60](https://github.com/Proa-Labs/proa/pull/60))
- Add builder-managed sitemaps and robots.txt endpoints ([#50](https://github.com/Proa-Labs/proa/pull/50))
- Reduce workspace compile times and isolate tooling engines ([#33](https://github.com/Proa-Labs/proa/pull/33))
- Reduce warm rebuild latency with incremental descriptors and per-unit plan reuse ([#13](https://github.com/Proa-Labs/proa/pull/13))
- Support runtime base paths for application and island URLs ([#38](https://github.com/Proa-Labs/proa/pull/38))
- Publish Proa libraries and CLI to crates.io ([#24](https://github.com/Proa-Labs/proa/pull/24))
