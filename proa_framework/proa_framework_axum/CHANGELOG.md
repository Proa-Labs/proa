# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1](https://github.com/Proa-Labs/proa/compare/proa_framework_axum-v0.4.0...proa_framework_axum-v0.4.1) - 2026-11-26

### Other

- Drop reqwest and rustls from proa_framework_axum runtime dependencies ([#113](https://github.com/Proa-Labs/proa/pull/113))
- Defer render context work on Axum HTTP routes ([#111](https://github.com/Proa-Labs/proa/pull/111))

## [0.4.0](https://github.com/Proa-Labs/proa/releases/tag/proa_framework_axum-v0.4.0) - 2026-11-24

### Added

- *(framework)* serve linked RSJS assets by default in FrameworkBuilder ([#104](https://github.com/Proa-Labs/proa/pull/104))
- *(framework)* default the island manifest and add RouteResponse::render_to_bytes ([#76](https://github.com/Proa-Labs/proa/pull/76))

### Fixed

- *(axum)* follow up on the Axum 0.8 upgrade ([#108](https://github.com/Proa-Labs/proa/pull/108))

### Other

- Secure registry credentials, client responses, and renderer output ([#106](https://github.com/Proa-Labs/proa/pull/106))
- Upgrade Rust dependencies and retire React interop ([#107](https://github.com/Proa-Labs/proa/pull/107))
- Make the build output a complete, host-servable mirror of every asset URL ([#101](https://github.com/Proa-Labs/proa/pull/101))
- Bundle the public asset catalog beside the exported executable ([#100](https://github.com/Proa-Labs/proa/pull/100))
- DX improvements: metadata cx.url helpers, env PORT/HOST, live public assets, primitive renders, and docs search ([#97](https://github.com/Proa-Labs/proa/pull/97))
- Add Rust project config and fingerprinted public assets ([#90](https://github.com/Proa-Labs/proa/pull/90))
- Validate UTF-8 at the write and stop zero-filling render pages ([#83](https://github.com/Proa-Labs/proa/pull/83))
- Retire unused framework features and redundant tests ([#77](https://github.com/Proa-Labs/proa/pull/77))
- Remove obsolete code and repair standalone site scaffolds ([#67](https://github.com/Proa-Labs/proa/pull/67))
- Markdown templates: automatic block spacing, one string body, no heading offsets ([#63](https://github.com/Proa-Labs/proa/pull/63))
- Brand New Metadata API Design ([#60](https://github.com/Proa-Labs/proa/pull/60))
- Add builder-managed sitemaps and robots.txt endpoints ([#50](https://github.com/Proa-Labs/proa/pull/50))
- Reduce workspace compile times and isolate tooling engines ([#33](https://github.com/Proa-Labs/proa/pull/33))
- Support runtime base paths for application and island URLs ([#38](https://github.com/Proa-Labs/proa/pull/38))
