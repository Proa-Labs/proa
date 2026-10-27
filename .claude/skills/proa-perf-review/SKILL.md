---
name: proa-perf-review
description: Review Proa Rust changes for render-path allocation regressions and zero-allocation SSR violations. Use after editing Proa components, rsjs islands, html_sync/html templates, recipes, catalog pages, or when the user asks about performance, allocations, ClassList, tw_join, or optimized Proa code.
---

# Proa perf review

For authored HTML, Markdown, XML, plain-text, or JSON documents, read
[`docs/agent/document-rendering.md`](../../../docs/agent/document-rendering.md).
Use the matching macro or typed serializer in handlers and cached documents as
well as components. Give relevant page types an `MdRenderSync` implementation;
do not substitute parallel Markdown files, string replacement, or hand-built
markup. Keep runtime JSON typed; use `const_json!` for literal JSON.

Run the deterministic lint before finalizing Proa Rust changes:

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

Treat findings as bugs unless the allocation is intentionally outside the SSR
hot path. Fix by:

- Replacing `format!` in text with adjacent static/dynamic template nodes:
  `"$"{price}" / night"`.
- Replacing class string assembly with static constants and `ClassList<N>`.
- Rendering numbers and `&str` directly instead of using `.to_string()`.
- Replacing `Vec`/`.collect()` with direct iteration or stack storage.
- Moving required serialization or path construction outside `render`.

Suppress only deliberate non-hot-path cases:

```rust
// proa-perf-ok: serialization happens once for static seed data
let json = serde_json::to_string(seed).unwrap_or_default();
```

Every suppression needs a short reason. Do not suppress just to make the lint
pass.
