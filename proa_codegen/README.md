# Proa TypeScript code generation

`proa_codegen` turns a JSON component-metadata manifest into React components
and class-variance-authority recipes. It is deliberately independent of any
particular component library or workspace layout.

## Command line

Pass an array of `ComponentDef` values by file:

```bash
cargo run -p proa_codegen -- \
  --input component-manifest.json \
  --output generated \
  --react 19
```

Or pipe the same JSON through standard input:

```bash
component-metadata-exporter |
  cargo run -p proa_codegen -- --input - --output generated
```

The output directory contains:

```text
generated/
├── components/
│   ├── <component>/
│   │   ├── <Component>.tsx
│   │   ├── recipe.ts
│   │   └── index.ts
│   └── index.ts
├── lib/utils.ts
├── metadata.json
├── manifest.ts
└── index.ts
```

The producing component library owns metadata discovery. This crate does not
run a named package, search neighboring repositories, or choose an output path
on the caller's behalf.

## Library API

Applications can deserialize [`ComponentDef`](src/types.rs) and call
[`TsGenerator`](src/ts_generator.rs) directly when they need a custom file
layout. [`CodegenConfig`](src/config.rs) selects React 18 or React 19 output
and controls `data-slot` generation.
