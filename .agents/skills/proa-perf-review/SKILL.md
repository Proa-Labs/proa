---
name: proa-perf-review
description: Review Proa Rust changes for render-path allocation regressions and zero-allocation SSR violations. Use after editing Proa components, rsjs islands, html_sync/html templates, recipes, catalog pages, or when the user asks about performance, allocations, ClassList, tw_join, optimized Proa code, or interpreting Proa CLI lint/check diagnostics.
---

# Proa Perf Review

For authored HTML, Markdown, XML, plain-text, or JSON documents, read
[`docs/agent/document-rendering.md`](../../../docs/agent/document-rendering.md).
Use the matching macro or typed serializer in handlers and cached documents as
well as components. Give relevant page types an `MdRenderSync` implementation;
do not substitute parallel Markdown files, string replacement, or hand-built
markup. Keep runtime JSON typed; use `const_json!` for literal JSON.

## Overview

Use Proa CLI lint/check as the first reviewer and the focused perf lint as the
render-path backstop. Treat findings as bugs unless the allocation is
intentionally outside the SSR hot path.

## Quick Start

Run the CLI check on changed files when possible:

```bash
cargo proa check --changed
cargo proa lint --changed --deny warnings
```

Run targeted checks when you know the touched paths:

```bash
cargo proa fmt --check --verify <paths>
cargo proa lint <paths> --deny warnings
cargo proa chunks <paths>
```

Use `cargo proa fmt --verify <paths>` to rewrite Proa macro bodies only after
reviewing that formatting is render-preserving.

Then run the older focused render allocation lint:

```bash
python3 scripts/proa_perf_lint.py
```

By default this scans changed and untracked Rust files. Use targeted or broader
scopes when needed:

```bash
python3 scripts/proa_perf_lint.py proa_demo/src/ebay_srp.rs
python3 scripts/proa_perf_lint.py --staged
python3 scripts/proa_perf_lint.py --changed --base origin/main
python3 scripts/proa_perf_lint.py --all
```

`--all` is useful for audits, but this repository still has legacy hotspots, so
do not use full-repo results as a merge gate until those are cleaned up.

## Fix Strategy

- Read the diagnostic code and help text before editing. The Proa CLI is
  intended to teach the better render shape.
- Replace `format!` in text with adjacent static/dynamic template nodes:
  `"$"{price}" / night"`.
- Replace class string assembly with static constants and `ClassList<N>`.
- Replace `.to_string()` on numeric/text values by rendering the value directly.
- Replace `Vec`/`.collect()` with direct iteration or stack storage.
- Move required serialization or path construction to data construction time,
  outside `render`.
- Replace owned view models built inside `render` with borrowed props,
  precomputed data, or child components.
- Replace unsupported rsjs slot logic with a named handler, a pure helper
  annotated once with `#[rsjs::client]`, `rsjs::server_only(...)`, or supported
  reactive expressions. `#[rsjs::client_module]` is optional grouping, not
  helper eligibility.
- If `proa lint` reports an async render without await, switch to
  `WebRenderSync` and `html_sync!`.
- Keep tests simple; the lint skips `#[cfg(test)] mod tests` by default.

## Suppressions

Use a suppression only when a finding is deliberate and not part of the SSR hot
path:

```rust
// proa-perf-ok: serialization happens once for static seed data
let json = serde_json::to_string(seed).unwrap_or_default();
```

For item-wide intentional cases, use a scope suppression directly above the
item:

```rust
// proa-perf-ok(scope): generated demo data, never in request render path
fn build_fixture() -> Vec<Row> {
    /* ... */
}
```

Every suppression needs a short reason. Do not suppress just to make the lint
pass.
