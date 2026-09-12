# rsjs_web

`rsjs_web` is the typed authoring facade for browser operations compiled by
RSJS. Its DOM references are zero-sized Rust compiler handles and its event
types are compile-time interface witnesses, not server-side DOM objects or
WebAssembly bindings. An RSJS frontend must recognize and lower every operation
used from this crate into Client IR before the surrounding browser-only code
can execute.

This first slice deliberately covers a reviewed common DOM contract:

- one DOM-target hierarchy with element, `Document`, and `Window` markers;
- `ElementRef<E>` and `CurrentTarget<E>` compiler handles;
- an element-only `ElementTargetFor<Target>` relation for node refs and a
  separate `EventCurrentTargetFor<Target>` relation whose broad `Any` marker
  applies only to element-local handlers;
- capability-gated element, HTML element, form-control, form, and dialog
  operations;
- one generated operation registry shared by the Rust handles and compiler;
- one generated exact-tag registry shared by node-ref and event witnesses;
- generated event-interface markers and witness traits used by the current
  RSJS prototype.

It is not a general Rust DOM runtime and does not promise complete WebIDL
coverage. New operations must land together in the binding schema, generated
Rust, Client IR, and JavaScript lowering.

The rich `Event`, `MouseEvent`, and related authoring handles remain owned by
`rsjs`; defining them again here would create incompatible public Rust types
and strand their existing methods. `rsjs` implements this crate's `EventParam`
and `EventFor<I>` bridge traits for those canonical handles. This crate owns
the generated interface vocabulary and target-compatibility rules.

## Generated bindings

[`schema/bindings.txt`](schema/bindings.txt) is the small, reviewed source of
truth for this bootstrap surface. Its explicit schema version is also emitted
as `BINDING_SCHEMA_VERSION`. The generator has no third-party dependencies and
normalizes its output with the workspace Rust toolchain's `rustfmt`:

```bash
cargo run -p rsjs_web --bin rsjs-web-codegen -- --check
cargo run -p rsjs_web --bin rsjs-web-codegen -- --write
```

The schema is intentionally simple enough to review in a diff. A future Webref
ingestion step can produce the same normalized rows without changing the
generated public model.
