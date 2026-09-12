# Proa UI component registry protocol v1

`proa ui add <slug>` resolves source components and blocks through the registry templates
in `proa.config.json`. A bare slug checks `@proa` first and checks `@proa-pro`
only when the public registry returns `404 Not Found`. Callers can select a
registry explicitly with `@proa/button`, `@proa-pro/calendar`, or
`--registry`.

The installer is implemented independently of the hosted registry. Local
registry fixtures can use an absolute path, a `file://` URL, or a path relative
to the project root. Hosted registries must use HTTPS; HTTP is accepted only
for localhost development.

The hosted Free artifact publication pipeline and the authenticated Pro
artifact service are separate deployment prerequisites. A missing endpoint,
missing component, or authentication failure never creates project files.

## Manifest

A registry template such as `https://ui.proa.so/r/{name}.json` returns one
strict JSON document:

```json
{
  "$schema": "https://proa.so/schema/ui-component-manifest-v1.json",
  "schemaVersion": 1,
  "name": "button",
  "version": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "registry": "@proa",
  "kind": "component",
  "tier": "free",
  "license": "MIT OR Apache-2.0",
  "dependencies": ["@proa/foundation", "spinner"],
  "files": [
    {
      "source": "artifacts/sha256/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/button/button/button.rs",
      "target": "@components/button/button.rs",
      "sha256": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }
  ],
  "cargoDependencies": []
}
```

The optional `rustModules` operation is available for registries that need a
managed module index:

```json
{
  "rustModules": [
    {
      "target": "@components/mod.rs",
      "modules": ["button"]
    }
  ]
}
```

The first-party Free registry publishes a hidden `@proa/foundation` manifest
that owns shared support modules, reviewed runtime files, legal notices, and
the host crate dependencies those modules need. Component and block manifests
contain only their direct production source and depend on that foundation.
They never create a nested Cargo package and never install source tests.

### Contract rules

- `schemaVersion` is exactly `1`; unknown manifest fields are rejected.
- `name` is a lowercase component or block slug and must match the requested slug.
- `kind` is `component` or `block`; omitted legacy values default to `component`.
- Free components come only from `@proa` and declare
  `MIT OR Apache-2.0`. Pro components come only from `@proa-pro`.
- `source` is a relative artifact reference. The installer resolves it against
  the manifest URL and does not accept an arbitrary absolute artifact URL.
- `target` and dependency `path` use the built-in `@components` or `@assets`
  alias, backed by `paths.components` and `paths.assets`. Absolute paths, `..`,
  symlinks, and unknown aliases are rejected.
- Every artifact carries an exact SHA-256 digest. Digests, all manifests, and
  the complete dependency graph are verified before the transaction writes.
- A parent manifest lists immediate component dependencies. Dependency
  component directories should be delivered by their own manifests rather
  than duplicated in the parent artifact set.
- A `cargoDependencies` entry may add either a version/registry dependency or
  a local path dependency and merge sorted, deduplicated features into a
  compatible existing declaration. It never replaces Git, workspace, or
  otherwise incompatible dependency sources.
- A managed Rust module block preserves content outside its explicit markers
  and is updated only when its current contents match `proa.lock.json`.

## Transaction and lock behavior

Resolution, dependency-cycle checks, download limits, target collision checks,
hash verification, Cargo edits, and managed-module edits complete before the
first publish operation. Outputs are staged below the project root. Existing
files are backed up before atomic same-filesystem renames, and any later
failure restores those backups.

Immutable artifact targets are idempotent when byte-identical and are never
overwritten when locally different. `proa ui add` also refuses to silently
upgrade a lock entry whose version or source hash changed; a future explicit
upgrade command can own local-edit reconciliation.

Shared glue may appear in both Free and Pro manifests only when its bytes are
identical; the second install then records it as already current. A registry
must not try to replace shared glue with different bytes during a Free-to-Pro
install, or mix an immutable artifact target with `rustModules`. The installer
rejects both cases instead of guessing whether a differing file is generated
or user-owned.

`proa.lock.json` records each resolved item, registry, version, source hash,
installed files, dependencies, managed modules, and Cargo dependency options.

## Pro authentication

The public `@proa` registry never receives an authentication header. Requests
to `@proa-pro` use `CARGO_REGISTRIES_PROA_TOKEN` as a bearer token when it is
set. Otherwise, the client reads the `registries.proa.token` value written to
`$CARGO_HOME/credentials.toml` by `cargo login --registry proa` (including
Cargo's legacy extensionless `credentials` file precedence).

The default Pro manifest endpoint is
`https://console.proa.so/r/{name}.json`. Authentication failures do not create
or modify project files. A bare slug that is absent from the public registry
falls back to the Pro registry; its error reports both lookups and explicitly
notes that Free registry access does not require a bearer token.
