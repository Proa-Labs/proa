# Proa + Wellformed + RSJS + WebMCP

This is a complete pizza-order application showing one native operation across
four layers:

- **Proa** renders the complete HTML document and the form.
- **Wellformed** generates typed Rust values, field metadata, form state, and
  authoritative server validation from one JSON IR.
- **RSJS** binds the visible controls to a live pizza preview and ships only the
  browser logic reachable from that component.
- **WebMCP** discovers the same native form through `toolname`,
  `tooldescription`, and `toolparamdescription` annotations.

The bridge also registers an imperative `configure_pizza_order` adapter for
agent clients that currently discover imperative tools but omit declarative
form tools. It prefers to resolve and invoke `create_pizza_order` through the
browser. If the client also omits declarative tools from the page's own
`getTools()` result, the adapter performs the equivalent visible native-form
operation itself. Neither path submits the order.

## Run it

From the Proa workspace root:

```bash
cargo proa run --root . -p proa_webmcp_pizza --bin proa_webmcp_pizza
```

Then open <http://127.0.0.1:5560/>.

Run its integration suite through the linked RSJS pipeline:

```bash
cargo proa test --root . -p proa_webmcp_pizza --test e2e
```

WebMCP is experimental. Use a browser build with WebMCP enabled. The form still
works as an ordinary progressively enhanced HTML form in every other browser.

## Source map

```text
schemas/pizza-order.json  Wellformed input contract
src/forms.rs              Generated form facade + operation identity
src/components.rs         Proa SSR components and RSJS live projection
src/webmcp.rs             Prototype composable WebMcp trait and registry
src/lib.rs                Axum routes, Wellformed boundary, document finalizer
src/assets.rs             Embedded CSS/bridge/RSJS asset endpoints
assets/webmcp-bridge.js   WebMCP-to-RSJS reconciliation hook
assets/styles.css         Demo styling
src/main.rs               Server entry point
tests/e2e.rs              Linked-pipeline HTTP integration tests
```

## Why the bridge exists

Declarative WebMCP fills actual native controls and then emits
`toolactivated`. The current draft does not promise that browser-owned writes
also emit the ordinary `input` and `change` events consumed by controlled UI
systems. The bridge scopes itself to the activated form and replays those
events, allowing RSJS `bind_value` signals to observe the values. It also
provides the temporary imperative-to-declarative compatibility adapter
described above.

This is intentionally explicit rather than hidden inside the example. The
fallback demonstrates an interoperability seam in current experimental
clients, not a claim that imperative and declarative registration are the same
thing. A first-class Proa integration could generate both adapters from a typed
operation/form component.

The form omits `toolautosubmit`: the agent can configure the order, while the
person sees the same live preview and confirms the mutation.

## Trait-composition prototype

The example also contains a local `WebMcp` trait experiment. `PizzaForm`
opens one `create_pizza_order` operation scope, and Wellformed-backed parameter
descriptors contribute fields inside that scope. `PizzaPage` exposes the
form's complete tool by delegating to its child with the same registry context.

This intentionally separates two composition rules:

- sibling component tools form a page-level set;
- child parameters form the input object of one enclosing operation.

The prototype produces metadata for tests and design exploration. The actual
experimental browser API remains the declarative HTML form, because WebMCP
does not define a server-emitted manifest format for this Rust registry.
