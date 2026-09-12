# Proa application instructions

- Use `WebRenderSync<L>` unless a component awaits; then use `WebRender<L>` and `html!`.
- Put request-shared keyed reads behind `cx.load_typed(...)` and create one `DataResolver::request_scope(...)` per HTML response.
- Keep `RootDocument` HTML-only. Put semantic non-browser output on the page with `MdRenderSync`.
- Static classes stay in constants and hot render paths must not allocate.
- Every component root has a stable `data-*` marker.
- Run `cargo fmt`, `cargo test`, `proa check`, and the Proa performance lint before merging render changes.
