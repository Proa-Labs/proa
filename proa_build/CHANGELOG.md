# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Proa-Labs/proa/compare/proa_build-v0.1.0...proa_build-v0.2.0) - 2026-11-26

### Other

- Speed up proa dev HMR (3.5s -> 1.7s) and cut proa-rsjs disk usage by 69% ([#114](https://github.com/Proa-Labs/proa/pull/114))

## [0.1.0](https://github.com/Proa-Labs/proa/releases/tag/proa_build-v0.1.0) - 2026-11-24

### Fixed

- *(deploy)* prune marketing Cloud Build cache to prevent ENOSPC ([#105](https://github.com/Proa-Labs/proa/pull/105))

### Other

- Upgrade Rust dependencies and retire React interop ([#107](https://github.com/Proa-Labs/proa/pull/107))
- Improve P1 onboarding DX: persistent config cache, fast public/CSS dev reload, Markdown alt coverage, and scaffold cleanup ([#94](https://github.com/Proa-Labs/proa/pull/94))
- Fix P0 onboarding DX: shared Cargo targets, zero-flag build->start, md URL slots, and clean scaffold ([#92](https://github.com/Proa-Labs/proa/pull/92))
- Add Rust project config and fingerprinted public assets ([#90](https://github.com/Proa-Labs/proa/pull/90))
- Remove obsolete code and repair standalone site scaffolds ([#67](https://github.com/Proa-Labs/proa/pull/67))
- Fix Windows linked builds and add portability coverage ([#46](https://github.com/Proa-Labs/proa/pull/46))
- Reduce workspace compile times and isolate tooling engines ([#33](https://github.com/Proa-Labs/proa/pull/33))
