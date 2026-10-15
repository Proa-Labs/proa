# Proa HTML policy

Allocation-free URL security policy shared by Proa's template compiler and
runtime renderer. This crate has no dependencies and supports `no_std`.

```rust
use proa_html_policy::has_dangerous_url_scheme;

assert!(has_dangerous_url_scheme("java&#9;script:alert(1)"));
assert!(has_dangerous_url_scheme("data:text/html,<script>example</script>"));
assert!(!has_dangerous_url_scheme("https://example.com"));
```

The validator normalizes ASCII controls and numeric HTML entities before
checking executable URL schemes and data MIME types. Proa's renderer re-exports
this same function at `proa_core::shared::attr::has_dangerous_url_scheme`.
Keeping the policy separate lets template compilers validate URLs without
compiling or depending on the renderer.
