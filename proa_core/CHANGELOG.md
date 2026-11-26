# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1](https://github.com/Proa-Labs/proa/compare/proa_core-v0.4.0...proa_core-v0.4.1) - 2026-11-26

### Other

- Defer render context work on Axum HTTP routes ([#111](https://github.com/Proa-Labs/proa/pull/111))

## [0.4.0](https://github.com/Proa-Labs/proa/releases/tag/proa_core-v0.4.0) - 2026-11-24

### Other

- Secure registry credentials, client responses, and renderer output ([#106](https://github.com/Proa-Labs/proa/pull/106))
- Upgrade Rust dependencies and retire React interop ([#107](https://github.com/Proa-Labs/proa/pull/107))
- Make the build output a complete, host-servable mirror of every asset URL ([#101](https://github.com/Proa-Labs/proa/pull/101))
- DX improvements: metadata cx.url helpers, env PORT/HOST, live public assets, primitive renders, and docs search ([#97](https://github.com/Proa-Labs/proa/pull/97))
- Fix P0 onboarding DX: shared Cargo targets, zero-flag build->start, md URL slots, and clean scaffold ([#92](https://github.com/Proa-Labs/proa/pull/92))
- Add Rust project config and fingerprinted public assets ([#90](https://github.com/Proa-Labs/proa/pull/90))
- Keep string conversion unchecked and check every write window
- Document write window limit and test UTF-8 rejection
- Harden render buffer UTF-8 conversion and write bounds
- Escape dynamic values in one pass ([#88](https://github.com/Proa-Labs/proa/pull/88))
- Check short escaped values a byte at a time before scanning with memchr ([#86](https://github.com/Proa-Labs/proa/pull/86))
- Copy short runtime-length text inline instead of calling memcpy ([#84](https://github.com/Proa-Labs/proa/pull/84))
- Validate UTF-8 at the write and stop zero-filling render pages ([#83](https://github.com/Proa-Labs/proa/pull/83))
- Retire unused framework features and redundant tests ([#77](https://github.com/Proa-Labs/proa/pull/77))
- Remove obsolete code and repair standalone site scaffolds ([#67](https://github.com/Proa-Labs/proa/pull/67))
- Markdown templates: automatic block spacing, one string body, no heading offsets ([#63](https://github.com/Proa-Labs/proa/pull/63))
- Make Markdown template slots Rust at every depth ([#61](https://github.com/Proa-Labs/proa/pull/61))
- Brand New Metadata API Design ([#60](https://github.com/Proa-Labs/proa/pull/60))
- record why components render directly into buffers ([#57](https://github.com/Proa-Labs/proa/pull/57))
- Reduce workspace compile times and isolate tooling engines ([#33](https://github.com/Proa-Labs/proa/pull/33))
- Reduce warm rebuild latency with incremental descriptors and per-unit plan reuse ([#13](https://github.com/Proa-Labs/proa/pull/13))
- Support runtime base paths for application and island URLs ([#38](https://github.com/Proa-Labs/proa/pull/38))
- Publish Proa libraries and CLI to crates.io ([#24](https://github.com/Proa-Labs/proa/pull/24))
