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

The default features are `markdown`, `stream`, `framework`, and `rsjs`. They
provide Markdown macros, `proa::stream` loaders/components, the portable
`proa::framework` response API, and browser interactivity through `proa::rsjs`.
For example, use `proa::rsjs::{rsjs, signal, event_handler}` without a separate
RSJS dependency.

Enable `axum` for `proa::axum`, `actions` for `proa::actions`, or `react` for
`proa::react`. `rt-tokio` enables eager Tokio loads.

Use `default-features = false` for minimal HTML/XML templates, then enable only
the features you need. `html-to-markdown` enables the explicit conversion
adapter. Build-time font collectors remain separate dependencies.

The facade re-exports the implementation crates' existing types. It adds no
rendering wrappers or runtime dispatch. Advanced integrations can continue to
use the implementation crates directly.

See the [documentation](https://proa.so/docs),
[package and feature guide](https://github.com/Proa-Labs/proa/blob/main/docs/registry.md),
and [source](https://github.com/Proa-Labs/proa).
