# Proa

Proa compiles Rust-authored HTML and Markdown templates into efficient rendering
code. Most applications use this crate for rendering traits, buffers, metadata,
and template macros.

```toml
[dependencies]
proa = "0.4"
```

```rust
use proa::{html_sync, render_to_string};

let name = "<Ada>";
let html = render_to_string(html_sync! { <p>{name}</p> });
assert_eq!(html, "<p>&lt;Ada&gt;</p>");
```

Enable `axum` for `proa::axum`, `actions` for `proa::actions`, or `react` for
`proa::react`. The default `markdown` and `stream` features provide Markdown
macros and `proa::stream` loaders/components. `rt-tokio` enables eager Tokio
loads; `framework` exposes the portable response API without Axum.

Use `default-features = false` for HTML/XML templates without Markdown compiler
or streaming dependencies. `html-to-markdown` enables the explicit conversion
adapter. RSJS and build-time font collectors remain separate dependencies.

The facade re-exports the implementation crates' existing types. It adds no
rendering wrappers or runtime dispatch. Advanced integrations can continue to
use the implementation crates directly.

See the [documentation](https://proa.so/docs),
[package and feature guide](https://github.com/Proa-Labs/proa/blob/main/docs/registry.md),
and [source](https://github.com/Proa-Labs/proa).
