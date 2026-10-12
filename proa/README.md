# Proa

Proa compiles Rust-authored HTML and Markdown templates into optimized rendering
code. Components can share typed data and child relationships across views.

```toml
[dependencies]
proa = "0.3.0"
```

```rust
use proa::{html_sync, WebContext, WebRenderSync};

let name = "Rust & agents";
let mut cx = WebContext::new();
html_sync! { <p>{name}</p> }.render(&mut cx)?;
let html = cx.into_output().into_string();
```

The default `macros` feature exports HTML, Markdown, and component macros.
Enable `html-to-markdown` for runtime HTML conversion, or `axum` for the framework
adapter at `proa::axum`. Individual `proa_core`, `proa_macros`, and framework
crates remain available for applications that prefer explicit dependencies.

Documentation: <https://proa.so>.
