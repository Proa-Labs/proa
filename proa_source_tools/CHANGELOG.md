# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/Proa-Labs/proa/compare/proa_source_tools-v0.1.0...proa_source_tools-v0.1.1) - 2026-11-27

### Added

- *(core)* catch managed-head structure mistakes at build and lint time ([#123](https://github.com/Proa-Labs/proa/pull/123))

### Fixed

- *(lint)* let documented RSJS islands pass the proa build check ([#124](https://github.com/Proa-Labs/proa/pull/124))

### Other

- Clear Proa CLI and workspace manifest lint warnings ([#120](https://github.com/Proa-Labs/proa/pull/120))

## [0.1.0](https://github.com/Proa-Labs/proa/releases/tag/proa_source_tools-v0.1.0) - 2026-11-24

### Other

- Upgrade Rust dependencies and retire React interop ([#107](https://github.com/Proa-Labs/proa/pull/107))
- Make the build output a complete, host-servable mirror of every asset URL ([#101](https://github.com/Proa-Labs/proa/pull/101))
- Improve proa ui CLI DX and extend asset-URL / missing-public-file lints ([#98](https://github.com/Proa-Labs/proa/pull/98))
- Improve P1 onboarding DX: persistent config cache, fast public/CSS dev reload, Markdown alt coverage, and scaffold cleanup ([#94](https://github.com/Proa-Labs/proa/pull/94))
- Generalize asset-URL linter across structs, constructors, Markdown, render_head, and public/ catalog ([#93](https://github.com/Proa-Labs/proa/pull/93))
- Add Rust project config and fingerprinted public assets ([#90](https://github.com/Proa-Labs/proa/pull/90))
- Validate UTF-8 at the write and stop zero-filling render pages ([#83](https://github.com/Proa-Labs/proa/pull/83))
- Add the proa facade and migrate application entrypoints ([#79](https://github.com/Proa-Labs/proa/pull/79))
- Retire unused framework features and redundant tests ([#77](https://github.com/Proa-Labs/proa/pull/77))
- Remove content_part and infer content coverage from inputs ([#66](https://github.com/Proa-Labs/proa/pull/66))
- Markdown templates: automatic block spacing, one string body, no heading offsets ([#63](https://github.com/Proa-Labs/proa/pull/63))
- Make Markdown template slots Rust at every depth ([#61](https://github.com/Proa-Labs/proa/pull/61))
- Fix Markdown control-flow indentation and nested block formatting
- Brand New Metadata API Design ([#60](https://github.com/Proa-Labs/proa/pull/60))
- Add builder-managed sitemaps and robots.txt endpoints ([#50](https://github.com/Proa-Labs/proa/pull/50))
- Show live Proa lint diagnostics in VS Code and LSP editors ([#45](https://github.com/Proa-Labs/proa/pull/45))
- Reduce workspace compile times and isolate tooling engines ([#33](https://github.com/Proa-Labs/proa/pull/33))
