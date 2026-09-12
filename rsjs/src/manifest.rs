// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Runtime island manifest.
//!
//! v0.1 of rsjs ships a single, simple way for the framework adapter
//! to discover compiled islands: a runtime manifest populated at link
//! time via `linkme::distributed_slice`. Each hydrated/runtime-emitting
//! `#[rsjs(component...)]` strategy result emits a
//! `&'static IslandManifestEntry` into
//! [`ISLAND_REGISTRY`]; `ServerOnly` results write analysis artifacts but no
//! runtime entry. Each `#[rsjs::client_module]`-annotated module emits a
//! `&'static ClientModuleManifestEntry` into [`CLIENT_MODULE_REGISTRY`]
//! (M7e wires that side once the macro lands); and each reusable rsjs
//! component emits a `&'static ComponentFragmentManifestEntry` into
//! [`COMPONENT_FRAGMENT_REGISTRY`]. Explicit `#[rsjs::client(class)]`
//! modules register through [`CLIENT_CLASS_REGISTRY`]. Calling [`manifest`]
//! from anywhere in the user crate (or a binary that links it) walks the
//! slices and returns a [`Manifest`] view.
//!
//! The manifest's primary consumers are:
//!
//! 1. The framework adapter that serves `/_rsjs/<asset_id>.js` from
//!    memory.
//! 2. The collector helper [`crate::manifest::write_manifest_json`] /
//!    [`crate::manifest::write_manifest_assets`] that materialises
//!    `manifest.json` plus per-asset `.js` files into a dist directory
//!    for static-CDN deployments.
//!
//! Manifest JSON writing is hand-written, mirroring the client codec exposed
//! through [`crate::ClientEncode`] and [`crate::ClientDecode`]. The
//! client-module transition path parses already-generated Client IR v3 helper
//! bodies through `rsjs_ir` before emitting bundle modules.

use crate::ClientHelperMeta;
use core::fmt::Write as _;
use rsjs_ir::abi::CLIENT_IR_SCHEMA_VERSION;
use rsjs_ir::{
    client_module_json, client_type_from_name, parse_client_body_json_str_with_options,
    prune_unreachable_client_functions, ClientFunction, ClientFunctionId, ClientFunctionRef,
    ClientJsonParseOptions, ClientModule, ClientModuleKind, ClientParam,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

/// Heuristic age threshold for reclaiming orphan `.tmp.*` staging
/// files left in the manifest output directory by a crashed run.
///
/// See [`write_manifest_assets`] for context. Files whose name
/// contains `.tmp.` and whose modification time is older than this
/// duration are treated as orphans and removed; younger files are
/// assumed to be in flight on a peer process or thread and left
/// alone.
const ORPHAN_TMP_MAX_AGE: Duration = Duration::from_secs(60);

/// Marker filename written at the top of an `out_dir` the first time
/// [`write_manifest_assets`] manages it.
///
/// Subsequent calls only run the cleanup pass when this file is
/// present, which prevents a misconfigured caller from accidentally
/// nuking unrelated `.js` files (a user-provided polyfill, an app
/// shell, etc.). Manually creating an empty file with this name in a
/// pre-populated directory opts that directory in to cleanup.
const MARKER_FILE: &str = ".rsjs-managed";

// Re-export linkme so generated code resolves the slice machinery
// through `::rsjs::linkme` without forcing user crates to declare
// `linkme` as a direct dependency.
pub use linkme;

/// Client-side trigger used by a registered rsjs island.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IslandHydrationStrategy {
    /// Hydrate as soon as the DOM is ready.
    Immediate,
    /// Hydrate when the island root first intersects the viewport.
    Visible,
    /// Hydrate during an idle callback.
    Idle,
    /// Hydrate on the first interaction with the island root.
    Interaction { event: Option<&'static str> },
    /// Hydrate once a media query matches.
    Media { query: &'static str },
}

/// One deferred island module URL for strategy-aware framework bootstraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeferredIslandAsset<'a> {
    /// Island asset id, matching the rendered
    /// `<rsjs-island data-rsjs-island-root="...">`.
    pub asset_id: &'a str,
    /// Browser URL for the island module.
    pub module_url: &'a str,
    /// Hydration strategy recorded in the rsjs manifest.
    pub strategy: IslandHydrationStrategy,
}

/// Return true when a framework adapter can defer fetching the island module
/// until the hydration trigger is reached.
///
/// Interaction remains eager: the loaded island module can hydrate during the
/// capture phase and still observe the first event. Loading the module only
/// after that event would miss the user action that triggered hydration.
pub fn should_defer_island_asset(strategy: &IslandHydrationStrategy) -> bool {
    matches!(
        strategy,
        IslandHydrationStrategy::Visible
            | IslandHydrationStrategy::Idle
            | IslandHydrationStrategy::Media { .. }
    )
}

/// One entry per hydrated `#[rsjs(component...)]` island in the linked binary.
///
/// All fields are `&'static`: the proc macro bakes them into the
/// crate's `.rodata` at compile time. The `js_source` is the raw,
/// unminified per-island JavaScript produced by `rsjs_ir::emit_js`;
/// minification and compression are the framework adapter's
/// responsibility.
#[derive(Debug)]
pub struct IslandManifestEntry {
    /// Stable identifier used for the asset URL and for cross-
    /// referencing the island in deploy tooling. Lowercased final
    /// segment of the user struct's path (`Counter` -> `"counter"`).
    pub asset_id: &'static str,

    /// Unqualified user struct name. Carried for diagnostics and the
    /// dehydrated state blob's `data-rsjs-island` attribute. Two
    /// islands may share `struct_name` legally as long as their
    /// `asset_id`s differ.
    pub struct_name: &'static str,

    /// Per-island JS source, baked into the binary as a `&'static str`
    /// at proc-macro expansion time.
    pub js_source: &'static str,

    /// `true` when the island's render path is async
    /// (`WebRender<L, B>`); `false` for the sync `WebRenderSync<L, B>`
    /// path. The framework can use this to decide whether to surface
    /// loading hints in the dehydrated blob.
    pub is_async: bool,

    /// Client-side trigger used by generated island modules.
    pub hydration_strategy: IslandHydrationStrategy,

    /// Canonical Rust paths of reachable `#[rsjs::client_module]` bundles
    /// this island imports through linked client helper calls. Unreachable
    /// helper bundles are not listed here.
    pub client_module_imports: &'static [&'static str],

    /// Asset ids of reachable descriptor-backed rsjs component fragments
    /// composed into this island. Server-only child descriptors are not listed.
    /// This is the asset-graph edge used by shared component fragment chunks:
    /// several parent islands may list the same child fragment asset id, and
    /// deploy tooling can ship that child chunk once.
    pub component_fragment_imports: &'static [&'static str],

    /// `true` when the island has reachable query work.
    /// The framework adapter uses this flag to decide whether to inject the
    /// query-runtime asset (`/rsjs-runtime-query.js`) on pages containing this
    /// island. Dead query declarations do not set this flag.
    pub uses_queries: bool,

    /// `true` when the island has reachable mutation work.
    /// Mutations and queries share a single ESM bundle
    /// (`/rsjs-runtime-query.js`) so a page that has either needs
    /// exactly one runtime import; the framework adapter logs the
    /// flag for devtools / tracing but does not need to load a
    /// separate asset. Dead mutation declarations do not set this flag.
    pub uses_mutations: bool,
}

/// One entry per `#[rsjs::client_module]` bundle in the linked binary.
///
/// M7e owns the producer side; this type is defined here so the
/// runtime crate is the single source of truth for the runtime entry
/// shape. The `module_path` is the canonical Rust path of the bundle
/// (e.g. `"crate::pricing"`) exactly as `Expr::ClientCall::module`
/// records it. `helpers` carries post-expansion per-helper metadata;
/// [`client_module_js_source`] emits the bundle JS from that Client IR
/// metadata.
#[derive(Debug)]
pub struct ClientModuleManifestEntry {
    /// Canonical Rust path of the bundle. Matches the strings the
    /// island IR records in `Expr::ClientCall::module` and the
    /// `client_module_imports` list of every island that imports
    /// this bundle.
    pub module_path: &'static str,

    /// Per-helper metadata in source order. Bundle JS is generated
    /// from this after all inner `#[rsjs::client]` attributes have
    /// expanded, avoiding duplicate helper-body lowering in the outer
    /// module macro.
    pub helpers: &'static [&'static ClientHelperMeta],
}

/// One ESM module generated by `#[rsjs::client(class)]`.
///
/// Class modules are deployment assets for application-owned JavaScript
/// interop. They are intentionally separate from island hydration imports:
/// the application imports them explicitly when it needs `new`, `instanceof`,
/// or a third-party JavaScript class boundary.
#[derive(Debug)]
pub struct ClientClassManifestEntry {
    /// Canonical Rust module path reported by `module_path!()`.
    pub module_path: &'static str,
    /// Public ESM class names exported by this module.
    pub export_names: &'static [&'static str],
    /// Fully lowered ESM source. Runtime imports use the canonical
    /// `/rsjs-runtime*.js` specifiers; custom asset layouts should rewrite
    /// them with [`rewrite_runtime_import_specifiers`].
    pub js_source: &'static str,
}

/// One reusable rsjs component fragment chunk in the linked binary.
///
/// A fragment chunk is not an island boundary by itself. It exports a mount
/// function that parent island modules can import once shared-fragment
/// hydration is enabled. The current producer side emits the artifact and
/// dependency graph first; parent islands still use descriptor inlining for
/// runtime semantics.
#[derive(Debug)]
pub struct ComponentFragmentManifestEntry {
    /// Component asset id. Matches the descriptor sidecar id and the
    /// `component_<asset_id>.js` filename written by [`write_manifest_assets`].
    pub asset_id: &'static str,

    /// Unqualified component struct name for diagnostics/tooling.
    pub struct_name: &'static str,

    /// Exported JS mount function name inside [`Self::js_source`].
    pub export_name: &'static str,

    /// Reusable component-fragment JS source.
    pub js_source: &'static str,
}

/// Linkme distributed slice for island registrations.
///
/// Each hydrated/runtime-emitting `#[rsjs(component...)]` strategy result emits a
/// `#[distributed_slice(::rsjs::ISLAND_REGISTRY)]` static; the linker gathers
/// them into a single contiguous slice. `ServerOnly` rsjs output records no
/// runtime entry. User crates do not touch this directly; they call
/// [`manifest`].
#[linkme::distributed_slice]
pub static ISLAND_REGISTRY: [&'static IslandManifestEntry] = [..];

/// Linkme distributed slice for client-module bundle registrations.
///
/// M7e wires the producer side: `#[rsjs::client_module]` emits a
/// `#[distributed_slice(::rsjs::CLIENT_MODULE_REGISTRY)]` static for
/// each bundle module. M7g exposes the slice so the runtime / the
/// collector helper can iterate it.
#[linkme::distributed_slice]
pub static CLIENT_MODULE_REGISTRY: [&'static ClientModuleManifestEntry] = [..];

/// Link-time registry for explicit Rust-authored JavaScript class modules.
#[linkme::distributed_slice]
pub static CLIENT_CLASS_REGISTRY: [&'static ClientClassManifestEntry] = [..];

/// Linkme distributed slice for reusable component fragment chunks.
#[linkme::distributed_slice]
pub static COMPONENT_FRAGMENT_REGISTRY: [&'static ComponentFragmentManifestEntry] = [..];

/// Materialised view of the linkme registries.
///
/// Constructed by [`manifest`]; entries are sorted by `asset_id` (for
/// islands) and `module_path` (for client modules) so the manifest is
/// byte-for-byte reproducible across compilations. This is required
/// for content-addressed asset URLs, CDN cache busting, and
/// deterministic CI artefacts.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    /// All islands compiled into the linked binary, sorted by `asset_id`.
    pub islands: Vec<&'static IslandManifestEntry>,
    /// All client-module bundles compiled into the linked binary,
    /// sorted by `module_path`.
    pub client_modules: Vec<&'static ClientModuleManifestEntry>,
    /// All explicit JavaScript class modules compiled into the binary.
    pub client_classes: Vec<&'static ClientClassManifestEntry>,
    /// All reusable component fragment chunks compiled into the linked binary,
    /// sorted by `asset_id`.
    pub component_fragments: Vec<&'static ComponentFragmentManifestEntry>,
}

impl PartialEq for IslandManifestEntry {
    fn eq(&self, other: &Self) -> bool {
        self.asset_id == other.asset_id
            && self.struct_name == other.struct_name
            && self.js_source == other.js_source
            && self.is_async == other.is_async
            && self.hydration_strategy == other.hydration_strategy
            && self.client_module_imports == other.client_module_imports
            && self.component_fragment_imports == other.component_fragment_imports
            && self.uses_queries == other.uses_queries
            && self.uses_mutations == other.uses_mutations
    }
}

impl PartialEq for ClientClassManifestEntry {
    fn eq(&self, other: &Self) -> bool {
        self.module_path == other.module_path
            && self.export_names == other.export_names
            && self.js_source == other.js_source
    }
}

impl Eq for ClientClassManifestEntry {}
impl Eq for IslandManifestEntry {}

impl PartialEq for ClientModuleManifestEntry {
    fn eq(&self, other: &Self) -> bool {
        self.module_path == other.module_path && helper_meta_slices_eq(self.helpers, other.helpers)
    }
}
impl Eq for ClientModuleManifestEntry {}

fn helper_meta_slices_eq(
    left: &[&'static ClientHelperMeta],
    right: &[&'static ClientHelperMeta],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.name == right.name
                && left.params == right.params
                && left.return_ty == right.return_ty
                && left.body_ir_version == right.body_ir_version
                && left.body_ir_json == right.body_ir_json
        })
}

impl PartialEq for ComponentFragmentManifestEntry {
    fn eq(&self, other: &Self) -> bool {
        self.asset_id == other.asset_id
            && self.struct_name == other.struct_name
            && self.export_name == other.export_name
            && self.js_source == other.js_source
    }
}
impl Eq for ComponentFragmentManifestEntry {}

/// Walk the linkme distributed slices and return an owned [`Manifest`].
///
/// Cheap: each entry is a single pointer copy; the only allocations
/// are the two `Vec`s and the in-place sort. Entries are sorted by
/// `asset_id` (islands) and `module_path` (client modules) so the
/// returned manifest is reproducible across compilations.
///
/// **Sort order note**: an `asset_id` is `<lowercase_struct>_<8-hex-hash>`
/// (e.g. `counter_3a1f0b2c`). Sorting by `asset_id` is deterministic
/// and content-addressable, but the 8-hex suffix dominates the
/// comparison whenever two struct names share a prefix: `Counter`
/// and `CounterPro` will not appear adjacent in the output, since
/// the hashes interleave them with unrelated islands. Consumers
/// that want human-alphabetical order (e.g. devtools listings)
/// should re-sort by `struct_name` themselves.
pub fn manifest() -> Manifest {
    let mut islands: Vec<&'static IslandManifestEntry> = ISLAND_REGISTRY.iter().copied().collect();
    islands.sort_by_key(|i| i.asset_id);
    let mut client_modules: Vec<&'static ClientModuleManifestEntry> =
        CLIENT_MODULE_REGISTRY.iter().copied().collect();
    client_modules.sort_by_key(|m| m.module_path);
    let mut client_classes: Vec<&'static ClientClassManifestEntry> =
        CLIENT_CLASS_REGISTRY.iter().copied().collect();
    client_classes.sort_by_key(|class| class.module_path);
    let mut component_fragments: Vec<&'static ComponentFragmentManifestEntry> =
        COMPONENT_FRAGMENT_REGISTRY.iter().copied().collect();
    component_fragments.sort_by_key(|f| f.asset_id);
    let manifest = Manifest {
        islands,
        client_modules,
        client_classes,
        component_fragments,
    };
    assert_manifest_keys_unique(&manifest);
    manifest
}

fn assert_manifest_keys_unique(manifest: &Manifest) {
    assert_unique_by_key(
        manifest.islands.iter().copied(),
        |entry| entry.asset_id,
        |entry| entry.struct_name,
        "island asset id",
    );
    assert_unique_by_key(
        manifest.component_fragments.iter().copied(),
        |entry| entry.asset_id,
        |entry| entry.struct_name,
        "component fragment asset id",
    );
    assert_unique_by_key(
        manifest.client_modules.iter().copied(),
        |entry| entry.module_path,
        |entry| entry.module_path,
        "client module path",
    );
    assert_unique_by_key(
        manifest.client_classes.iter().copied(),
        |entry| entry.module_path,
        |entry| entry.module_path,
        "client class module path",
    );

    let mut client_module_files: Vec<(&str, &str)> = manifest
        .client_modules
        .iter()
        .map(|entry| (entry.module_path, entry.module_path))
        .collect();
    client_module_files.sort_by_key(|(module_path, _)| client_module_filename(module_path));
    let mut previous: Option<(String, &str)> = None;
    for (module_path, label) in client_module_files {
        let filename = client_module_filename(module_path);
        if let Some((previous_filename, previous_label)) = &previous {
            if previous_filename == &filename {
                panic!(
                    "rsjs manifest collision: duplicate client module asset filename `{filename}` \
                     for `{previous_label}` and `{label}`. Rename one `#[rsjs::client_module]` \
                     module so generated JS files cannot overwrite each other."
                );
            }
        }
        previous = Some((filename, label));
    }

    let client_module_files: BTreeSet<String> = manifest
        .client_modules
        .iter()
        .map(|entry| client_module_filename(entry.module_path))
        .collect();
    for island in &manifest.islands {
        for module_path in island.client_module_imports {
            let filename = client_module_filename(module_path);
            if !client_module_files.contains(&filename) {
                panic!(
                    "rsjs manifest missing client module `{module_path}` imported by island `{}`. \
                     Ensure the module is annotated with #[rsjs::client_module] and linked into \
                     the binary.",
                    island.asset_id
                );
            }
        }
    }

    let component_fragments: BTreeSet<&str> = manifest
        .component_fragments
        .iter()
        .map(|entry| entry.asset_id)
        .collect();
    for island in &manifest.islands {
        for fragment_asset_id in island.component_fragment_imports {
            if !component_fragments.contains(fragment_asset_id) {
                panic!(
                    "rsjs manifest missing component fragment asset `{fragment_asset_id}` \
                     imported by island `{}`. Ensure the child rsjs component fragment is linked \
                     into the binary.",
                    island.asset_id
                );
            }
        }
    }
}

fn assert_unique_by_key<T, K, L, FK, FL>(
    entries: impl IntoIterator<Item = T>,
    key: FK,
    label: FL,
    kind: &str,
) where
    T: Copy,
    K: Ord + core::fmt::Display,
    L: core::fmt::Display,
    FK: Fn(T) -> K,
    FL: Fn(T) -> L,
{
    let mut entries: Vec<T> = entries.into_iter().collect();
    entries.sort_by_key(|entry| key(*entry));

    let mut previous: Option<(K, L)> = None;
    for entry in entries {
        let current_key = key(entry);
        let current_label = label(entry);
        if let Some((previous_key, previous_label)) = &previous {
            if previous_key == &current_key {
                panic!(
                    "rsjs manifest collision: duplicate {kind} `{current_key}` for \
                     `{previous_label}` and `{current_label}`. rsjs components and islands must \
                     have globally unique generated asset keys across linked crates."
                );
            }
        }
        previous = Some((current_key, current_label));
    }
}

/// Filename for a client-module bundle asset.
///
/// Island JS imports client modules by the final Rust path segment
/// (legacy `self::pricing` -> `./pricing.js`), matching
/// `rsjs_ir`'s import convention.
pub fn client_module_filename(module_path: &str) -> String {
    format!("{}.js", client_module_asset_stem(module_path))
}

/// Stable filename for an explicit client-class ESM module.
///
/// The readable final Rust path segment is paired with a hash of the full
/// module path, so same-named modules under different parents cannot collide.
pub fn client_class_filename(module_path: &str) -> String {
    let stem = module_path.rsplit("::").next().unwrap_or(module_path);
    let stem = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let stem = if stem.is_empty() { "class" } else { &stem };
    let mut hasher = Sha256::new();
    hasher.update(b"proa.rsjs.client-class-filename.v1\0");
    hasher.update(module_path.as_bytes());
    let digest = hasher.finalize();
    let mut suffix = String::with_capacity(8);
    for byte in digest.iter().take(4) {
        let _ = write!(suffix, "{byte:02x}");
    }
    format!("client_class_{stem}_{suffix}.js")
}

/// Build the JavaScript source for a client-module bundle from its
/// post-expansion helper metadata.
///
/// `#[rsjs::client_module]` output stores helper metadata in the manifest
/// entry. This function is the canonical way for framework adapters and
/// collectors to obtain the actual JS asset; helper bundles are emitted from
/// Client IR v3 metadata.
// proa-perf-ok(scope): trusted low-level JavaScript asset emitter, not SSR HTML rendering
pub fn client_module_js_source(bundle: &ClientModuleManifestEntry) -> String {
    let safe_name = client_module_asset_stem(bundle.module_path);
    let mut content = String::new();
    content.push_str("// Auto-generated by rsjs from #[rsjs::client_module] metadata.\n");
    content.push_str("// Each helper is exported at <module>__<helper>; consuming island JS\n");
    content.push_str("// imports them by that name.\n");
    content.push_str("// Module: ");
    content.push_str(bundle.module_path);
    content.push('\n');

    let module_js = client_module_js_source_v3(bundle, safe_name).unwrap_or_else(|err| {
        panic!(
            "failed to emit Client IR v3 for client module `{}`: {err}",
            bundle.module_path
        )
    });
    content.push_str(&module_js);
    content
}

/// Build the optimized Client IR v3 module graph JSON for a client-module bundle.
///
/// This is the structured counterpart to [`client_module_js_source`]. It lets
/// conformance snapshots pin the module graph before JavaScript emission, so
/// helper metadata, function ids, params, exports, and body IR all regress
/// together after module-level Client IR optimization.
pub fn client_module_ir_json_source(bundle: &ClientModuleManifestEntry) -> String {
    let safe_name = client_module_asset_stem(bundle.module_path);
    let module = optimized_client_module_ir_v3(bundle, safe_name).unwrap_or_else(|err| {
        panic!(
            "failed to build Client IR v3 module `{}`: {err}",
            bundle.module_path
        )
    });
    client_module_json(&module)
}

fn client_module_js_source_v3(
    bundle: &ClientModuleManifestEntry,
    safe_name: &str,
) -> Result<String, String> {
    let module = optimized_client_module_ir_v3(bundle, safe_name)?;
    rsjs_ir::client::emit_client_module_js(&module).map_err(|err| err.to_string())
}

fn optimized_client_module_ir_v3(
    bundle: &ClientModuleManifestEntry,
    safe_name: &str,
) -> Result<ClientModule, String> {
    let mut module = client_module_ir_v3(bundle, safe_name)?;
    prune_unreachable_client_functions(&mut module);
    Ok(module)
}

fn client_module_ir_v3(
    bundle: &ClientModuleManifestEntry,
    safe_name: &str,
) -> Result<ClientModule, String> {
    let mut helper_ids = BTreeMap::new();
    for (idx, helper) in bundle.helpers.iter().enumerate() {
        helper_ids.insert(helper.name, ClientFunctionId(idx as u32));
    }

    let mut module = ClientModule::new(safe_name, ClientModuleKind::ClientModule);
    for (idx, helper) in bundle.helpers.iter().enumerate() {
        if helper.body_ir_version != CLIENT_IR_SCHEMA_VERSION {
            return Err(format!(
                "unsupported helper body_ir_version {}",
                helper.body_ir_version
            ));
        }

        let function_id = ClientFunctionId(idx as u32);
        let resolve = |path: &str| {
            let helper = path.rsplit("::").next().unwrap_or(path);
            if let Some(id) = helper_ids.get(helper) {
                return Some(ClientFunctionRef::Local(*id));
            }
            Some(ClientFunctionRef::Runtime(
                crate::client_helper_export_name(path, Some(safe_name)),
            ))
        };
        let options = ClientJsonParseOptions::new().with_unresolved_function_ref(&resolve);
        let body = parse_client_body_json_str_with_options(helper.body_ir_json, &options)
            .map_err(String::from)?;
        let mut function = ClientFunction::new(function_id, helper.name)
            .with_return_ty(client_type_from_name(helper.return_ty))
            .with_body(body);
        for (name, ty) in helper.params {
            function
                .params
                .push(ClientParam::new(*name, client_type_from_name(ty)));
        }
        module.add_function(function);
        module.add_export(rsjs_ir::ClientExport::function(
            format!("{safe_name}__{}", helper.name),
            function_id,
        ));
    }

    Ok(module)
}

fn client_module_asset_stem(module_path: &str) -> &str {
    module_path.rsplit("::").next().unwrap_or(module_path)
}

/// The set of runtime ES-module asset URLs needed by a runtime plan.
///
/// [`Manifest::runtime`] / [`Manifest::runtime_with_prefix`] create a coarse
/// global plan for every hydrated island registered in the binary.
/// [`Manifest::runtime_for_islands`] /
/// [`Manifest::runtime_for_islands_with_prefix`] create a page-local plan for
/// island ids recorded by the current response. If that response rendered no
/// hydrated islands, those helpers return `Ok(None)` and no runtime tags should
/// be emitted.
///
/// The query runtime (`/rsjs-runtime-query.js`) is required only when the
/// selected islands include query or mutation work. Loading the query runtime
/// on a page with no queries or mutations is harmless but wasteful.
///
/// Order matters for the user-visible `<script>` tag emission
/// order: base runtime first (everything depends on it), query
/// runtime second.
///
/// The fields are `String`-typed (rather than `&'static str`) because
/// [`Manifest::runtime_with_prefix`] and
/// [`Manifest::runtime_for_islands_with_prefix`] build them at runtime by
/// concatenating a caller-supplied prefix with the asset filename. The
/// convenience helpers use the canonical root-mounted shape
/// (`/rsjs-runtime.js`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RuntimeAssets {
    /// Base runtime module URL for this runtime plan.
    pub base: String,
    /// Optional query-runtime module URL. `None` when no island
    /// selected by the plan uses queries or mutations.
    pub query: Option<String>,
}

/// One logical rsjs browser asset whose public URL is chosen by the
/// application.
///
/// The linked manifest owns asset identity and dependency edges, while the
/// application owns deployment topology. A resolver can therefore map the
/// same logical plan to origin-relative development URLs, content-hashed
/// production URLs, or a CDN without duplicating page-planning logic. This
/// type selects response-plan URLs only; see [`RsjsAssetUrlResolver`] for the
/// corresponding generated-module source contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RsjsAssetRef<'a> {
    /// Shared base rsjs runtime.
    Runtime,
    /// Shared query/mutation runtime.
    QueryRuntime,
    /// Per-island browser module.
    Island {
        /// Stable island asset id from [`IslandManifestEntry`].
        asset_id: &'a str,
    },
    /// Reachable `#[rsjs::client_module]` bundle.
    ClientModule {
        /// Canonical Rust module path from [`ClientModuleManifestEntry`].
        module_path: &'a str,
    },
    /// Explicit Rust-authored JavaScript class module.
    ClientClass {
        /// Canonical Rust module path from [`ClientClassManifestEntry`].
        module_path: &'a str,
    },
    /// Reachable descriptor-backed component fragment.
    ComponentFragment {
        /// Stable fragment asset id from [`ComponentFragmentManifestEntry`].
        asset_id: &'a str,
    },
}

/// Application-owned mapping from logical rsjs assets to browser URLs.
///
/// Implementations should be deterministic for the lifetime of one response.
/// The returned URL is HTML-attribute escaped by [`PageRuntimePlan::write_html`]
/// and JavaScript-string escaped by the deferred bootstrap helper.
///
/// # Generated ESM imports
///
/// This resolver changes URLs in [`PageRuntimePlan`]; it does **not** rewrite
/// static import specifiers already embedded in generated island, client
/// module, client-class, or component-fragment JavaScript. An adapter that moves those
/// assets to hashed paths or a CDN must also rewrite their served ESM imports
/// to the corresponding resolved URLs (or serve stable aliases that preserve
/// the generated specifiers). Rewriting only the plan can load the entry
/// module successfully and still fail when that module imports the canonical
/// runtime or a relative dependency.
pub trait RsjsAssetUrlResolver {
    /// Resolve one logical asset to the URL the browser should request.
    fn resolve(&self, asset: RsjsAssetRef<'_>) -> String;
}

impl<F> RsjsAssetUrlResolver for F
where
    F: for<'a> Fn(RsjsAssetRef<'a>) -> String,
{
    fn resolve(&self, asset: RsjsAssetRef<'_>) -> String {
        self(asset)
    }
}

/// Canonical same-origin rsjs URL mapping.
///
/// With the default `"/"` prefix this produces:
///
/// - `/rsjs-runtime.js`
/// - `/rsjs-runtime-query.js`
/// - `/rsjs/island/<island-id>.js`
/// - `/rsjs/island/<client-module>.js`
/// - `/rsjs/island/<client-class>.js`
/// - `/rsjs/island/component_<fragment-id>.js`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalRsjsAssetUrls<'a> {
    prefix: &'a str,
}

impl<'a> CanonicalRsjsAssetUrls<'a> {
    /// Mount canonical rsjs paths below `prefix`.
    pub const fn new(prefix: &'a str) -> Self {
        Self { prefix }
    }

    /// Return the configured URL prefix.
    pub const fn prefix(&self) -> &'a str {
        self.prefix
    }

    fn join(&self, path: &str) -> String {
        let prefix = self.prefix.trim_end_matches('/');
        if prefix.is_empty() {
            format!("/{path}")
        } else {
            format!("{prefix}/{path}")
        }
    }
}

impl<'a> Default for CanonicalRsjsAssetUrls<'a> {
    fn default() -> Self {
        Self::new("/")
    }
}

impl RsjsAssetUrlResolver for CanonicalRsjsAssetUrls<'_> {
    fn resolve(&self, asset: RsjsAssetRef<'_>) -> String {
        match asset {
            RsjsAssetRef::Runtime => self.join("rsjs-runtime.js"),
            RsjsAssetRef::QueryRuntime => self.join("rsjs-runtime-query.js"),
            RsjsAssetRef::Island { asset_id } => self.join(&format!("rsjs/island/{asset_id}.js")),
            RsjsAssetRef::ClientModule { module_path } => self.join(&format!(
                "rsjs/island/{}",
                client_module_filename(module_path)
            )),
            RsjsAssetRef::ClientClass { module_path } => self.join(&format!(
                "rsjs/island/{}",
                client_class_filename(module_path)
            )),
            RsjsAssetRef::ComponentFragment { asset_id } => self.join(&format!(
                "rsjs/island/{}",
                component_fragment_filename(asset_id)
            )),
        }
    }
}

/// Rewrite canonical runtime module specifiers in generated RSJS ESM source.
///
/// Island, client-module, client-class, and component-fragment sources use
/// `/rsjs-runtime.js` and `/rsjs-runtime-query.js` as stable logical imports.
/// Asset pipelines mounted below a base path, on a CDN, or at content-hashed
/// URLs should resolve those two assets and pass the resulting browser URLs
/// here before serving or materializing the generated module.
///
/// Relative imports between generated component assets are intentionally left
/// unchanged because their final URLs depend on the owning bundler's layout.
pub fn rewrite_runtime_import_specifiers(
    source: &str,
    runtime_url: &str,
    runtime_query_url: &str,
) -> String {
    let mut runtime_specifier = String::new();
    write_json_string(&mut runtime_specifier, runtime_url);
    let mut query_specifier = String::new();
    write_json_string(&mut query_specifier, runtime_query_url);

    source
        .replace(r#""/rsjs-runtime-query.js""#, &query_specifier)
        .replace(r#""/rsjs-runtime.js""#, &runtime_specifier)
}

/// Reachable shared-runtime features for one set of rendered islands.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeFeatureSet {
    /// At least one hydrated island needs the base runtime.
    pub core: bool,
    /// At least one hydrated island has reachable query work.
    pub query: bool,
    /// At least one hydrated island has reachable mutation work.
    pub mutation: bool,
}

impl RuntimeFeatureSet {
    /// Whether the shared query/mutation runtime bundle is required.
    pub const fn needs_query_runtime(self) -> bool {
        self.query || self.mutation
    }
}

/// One rendered island module selected for this response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IslandModuleAsset {
    /// Stable manifest asset id.
    pub asset_id: &'static str,
    /// Application-resolved browser URL.
    pub module_url: String,
    /// Hydration scheduling strategy.
    pub strategy: IslandHydrationStrategy,
}

/// One page-local reachable client-module bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientModuleAsset {
    /// Canonical Rust module path from the linked registry.
    pub module_path: &'static str,
    /// Application-resolved browser URL.
    pub module_url: String,
}

/// One page-local reachable component-fragment bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentFragmentAsset {
    /// Stable fragment asset id.
    pub asset_id: &'static str,
    /// Application-resolved browser URL.
    pub module_url: String,
}

/// Deterministic browser-asset plan for the hydrated rsjs islands rendered by
/// one response.
///
/// This type is deliberately data-only. URL selection happens while the plan
/// is built through [`RsjsAssetUrlResolver`], while request-specific CSP nonce
/// application happens later through [`Self::write_html`]. That separation
/// keeps a logical plan reusable and easy to inspect.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PageRuntimePlan {
    /// Rendered hydrated island ids, sorted by asset id and deduplicated.
    pub rendered_islands: Vec<&'static str>,
    /// Runtime features across every rendered island, including deferred ones.
    pub runtime_features: RuntimeFeatureSet,
    /// Runtime features needed eagerly by immediate/interaction islands.
    pub eager_runtime_features: RuntimeFeatureSet,
    /// Page runtime URLs. `base` exists for every non-empty plan so deferred
    /// bootstrap code can import it. `query` exists only when an eager island
    /// needs the query/mutation runtime.
    pub runtime: Option<RuntimeAssets>,
    /// Immediate and interaction island modules, sorted by asset id.
    /// Immediate entries emit normal module tags; interaction entries use an
    /// eager generation-fenced import so listeners arm only after readiness.
    pub eager_islands: Vec<IslandModuleAsset>,
    /// Visible, idle, and media island modules, sorted by asset id.
    pub deferred_islands: Vec<IslandModuleAsset>,
    /// Client modules reachable from any rendered island.
    pub client_modules: Vec<ClientModuleAsset>,
    /// Component fragments reachable from any rendered island.
    pub component_fragments: Vec<ComponentFragmentAsset>,
    /// Deduplicated modulepreload URLs for eager islands and their reachable
    /// client-module/component-fragment dependencies, sorted by URL.
    pub module_preloads: Vec<String>,
}

/// Request-scoped record of tags already emitted from one or more
/// [`PageRuntimePlan`]s.
///
/// Streaming response finalizers should keep one state for the whole response
/// and pass it to [`PageRuntimePlan::write_html_delta`] for the shell and every
/// later boundary. The state deduplicates exact resolved URLs within each tag
/// category. Deferred island instructions are deduplicated by their complete
/// runtime URL, island id, module URL, and hydration strategy.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PageRuntimeEmissionState {
    emitted_runtime_urls: BTreeSet<String>,
    emitted_preload_urls: BTreeSet<String>,
    emitted_eager_module_urls: BTreeSet<String>,
    emitted_deferred_islands: BTreeSet<DeferredIslandEmissionKey>,
}

impl PageRuntimeEmissionState {
    /// Create empty request-scoped emission state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return true before any runtime artifact has been emitted.
    pub fn is_empty(&self) -> bool {
        self.emitted_runtime_urls.is_empty()
            && self.emitted_preload_urls.is_empty()
            && self.emitted_eager_module_urls.is_empty()
            && self.emitted_deferred_islands.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DeferredIslandEmissionKey {
    runtime_url: String,
    asset_id: &'static str,
    module_url: String,
    strategy: IslandHydrationStrategy,
}

impl PageRuntimePlan {
    /// Return true when this response rendered no hydrated rsjs islands.
    pub fn is_empty(&self) -> bool {
        self.rendered_islands.is_empty()
    }

    /// Render the plan's tags into a growable UTF-8 string.
    ///
    /// CSP is request data, so the nonce is intentionally supplied here rather
    /// than stored in the deterministic plan.
    pub fn to_html(&self, nonce: Option<&proa_core::CspNonce>) -> String {
        let mut state = PageRuntimeEmissionState::new();
        self.to_html_delta(&mut state, nonce)
    }

    /// Render only tags not yet recorded in this request's emission state.
    ///
    /// Use the same `state` and CSP nonce for the shell and all streamed
    /// boundaries in one response.
    pub fn to_html_delta(
        &self,
        state: &mut PageRuntimeEmissionState,
        nonce: Option<&proa_core::CspNonce>,
    ) -> String {
        let mut out = Vec::new();
        self.write_html_delta(&mut out, state, nonce)
            .expect("writing PageRuntimePlan HTML to Vec cannot fail");
        String::from_utf8(out).expect("PageRuntimePlan emits UTF-8 HTML")
    }

    /// Append runtime tags in stable dependency order:
    ///
    /// 1. eager base runtime;
    /// 2. eager query/mutation runtime;
    /// 3. sorted modulepreloads;
    /// 4. sorted immediate module tags and managed interaction imports;
    /// 5. deferred strategy bootstrap.
    pub fn write_html<B: proa_core::WriteBuf>(
        &self,
        out: &mut B,
        nonce: Option<&proa_core::CspNonce>,
    ) -> Result<(), proa_core::WriteError> {
        let mut state = PageRuntimeEmissionState::new();
        self.write_html_delta(out, &mut state, nonce)
    }

    /// Append only runtime tags not already emitted for this response.
    ///
    /// State is updated after each tag is written successfully. If writing
    /// fails, the failing artifact remains unrecorded.
    pub fn write_html_delta<B: proa_core::WriteBuf>(
        &self,
        out: &mut B,
        state: &mut PageRuntimeEmissionState,
        nonce: Option<&proa_core::CspNonce>,
    ) -> Result<(), proa_core::WriteError> {
        if self.is_empty() {
            return Ok(());
        }

        let runtime = self
            .runtime
            .as_ref()
            .expect("non-empty PageRuntimePlan must carry runtime URLs");
        if !self.eager_islands.is_empty() {
            if !state.emitted_runtime_urls.contains(runtime.base.as_str()) {
                write_module_script_tag(out, runtime.base.as_str(), nonce)?;
                state.emitted_runtime_urls.insert(runtime.base.clone());
            }
            if let Some(query) = runtime.query.as_deref() {
                if !state.emitted_runtime_urls.contains(query) {
                    write_module_script_tag(out, query, nonce)?;
                    state.emitted_runtime_urls.insert(query.to_owned());
                }
            }
        }

        for url in &self.module_preloads {
            if !state.emitted_preload_urls.contains(url) {
                write_module_preload_tag(out, url)?;
                state.emitted_preload_urls.insert(url.clone());
            }
        }
        let mut pending_interaction_urls = Vec::new();
        let mut pending_interaction_islands = Vec::new();
        for island in &self.eager_islands {
            if state
                .emitted_eager_module_urls
                .contains(island.module_url.as_str())
            {
                continue;
            }
            if matches!(island.strategy, IslandHydrationStrategy::Interaction { .. }) {
                pending_interaction_urls.push(island.module_url.clone());
                pending_interaction_islands.push(DeferredIslandAsset {
                    asset_id: island.asset_id,
                    module_url: island.module_url.as_str(),
                    strategy: island.strategy,
                });
            } else {
                write_module_script_tag(out, island.module_url.as_str(), nonce)?;
                state
                    .emitted_eager_module_urls
                    .insert(island.module_url.clone());
            }
        }
        if !pending_interaction_islands.is_empty() {
            let tag = interaction_island_bootstrap_tag_with_nonce(
                runtime.base.as_str(),
                &pending_interaction_islands,
                nonce,
            );
            out.extend(tag.as_bytes())?;
            state
                .emitted_eager_module_urls
                .extend(pending_interaction_urls);
        }

        if !self.deferred_islands.is_empty() {
            let mut pending_keys = BTreeSet::new();
            let mut pending_islands = Vec::new();
            for island in &self.deferred_islands {
                let key = DeferredIslandEmissionKey {
                    runtime_url: runtime.base.clone(),
                    asset_id: island.asset_id,
                    module_url: island.module_url.clone(),
                    strategy: island.strategy,
                };
                if !state.emitted_deferred_islands.contains(&key)
                    && pending_keys.insert(key.clone())
                {
                    pending_islands.push(DeferredIslandAsset {
                        asset_id: island.asset_id,
                        module_url: island.module_url.as_str(),
                        strategy: island.strategy,
                    });
                }
            }
            if !pending_islands.is_empty() {
                let tag = deferred_island_bootstrap_tag_with_nonce(
                    runtime.base.as_str(),
                    &pending_islands,
                    nonce,
                );
                out.extend(tag.as_bytes())?;
                state.emitted_deferred_islands.extend(pending_keys);
            }
        }
        Ok(())
    }
}

fn write_module_script_tag<B: proa_core::WriteBuf>(
    out: &mut B,
    url: &str,
    nonce: Option<&proa_core::CspNonce>,
) -> Result<(), proa_core::WriteError> {
    out.extend_static(br#"<script type="module" src=""#)?;
    proa_core::shared::escape::escape_attr_value(out, url)?;
    out.extend_static(b"\"")?;
    if let Some(nonce) = nonce {
        nonce.write_html_attr(out)?;
    }
    out.extend_static(b"></script>")
}

fn write_module_preload_tag<B: proa_core::WriteBuf>(
    out: &mut B,
    url: &str,
) -> Result<(), proa_core::WriteError> {
    out.extend_static(br#"<link rel="modulepreload" href=""#)?;
    proa_core::shared::escape::escape_attr_value(out, url)?;
    out.extend_static(b"\">")
}

/// A rendered island id was not present in the linked rsjs manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAssetPlanError {
    /// Rendered asset id that could not be resolved.
    pub asset_id: String,
}

impl std::fmt::Display for RuntimeAssetPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rendered rsjs island `{}` was not found in the runtime manifest",
            self.asset_id
        )
    }
}

impl std::error::Error for RuntimeAssetPlanError {}

/// Failure while resolving a response-local [`PageRuntimePlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageRuntimePlanError {
    /// A rendered hydrated island id was absent from the linked manifest.
    UnknownRenderedIsland {
        /// Rendered asset id that could not be resolved.
        asset_id: String,
    },
    /// A selected island references a client-module bundle absent from the
    /// linked client-module registry.
    MissingClientModule {
        /// Manifest import path that could not be resolved.
        module_path: String,
    },
    /// A selected island references a component fragment absent from the
    /// linked fragment registry.
    MissingComponentFragment {
        /// Fragment asset id that could not be resolved.
        asset_id: String,
    },
}

impl std::fmt::Display for PageRuntimePlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownRenderedIsland { asset_id } => write!(
                f,
                "rendered rsjs island `{asset_id}` was not found in the runtime manifest"
            ),
            Self::MissingClientModule { module_path } => write!(
                f,
                "rendered rsjs island requires client module `{module_path}`, but it was not found in the runtime manifest"
            ),
            Self::MissingComponentFragment { asset_id } => write!(
                f,
                "rendered rsjs island requires component fragment `{asset_id}`, but it was not found in the runtime manifest"
            ),
        }
    }
}

impl std::error::Error for PageRuntimePlanError {}

impl Manifest {
    /// Build the deterministic response-local runtime plan using canonical
    /// same-origin rsjs URLs.
    pub fn page_runtime_plan<I, S>(
        &self,
        asset_ids: I,
    ) -> Result<PageRuntimePlan, PageRuntimePlanError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.page_runtime_plan_with_urls(asset_ids, &CanonicalRsjsAssetUrls::default())
    }

    /// Build the deterministic response-local runtime plan using an
    /// application-supplied asset URL resolver.
    ///
    /// `asset_ids` should come from the response's rendered-island artifact
    /// set. Input order and duplicates do not affect the output. Unknown
    /// rendered ids are errors; a hydrated wrapper without a corresponding
    /// linked manifest entry is a build/integration defect rather than
    /// `ServerOnly` output.
    ///
    /// This method resolves response-plan URLs only. Hashed/CDN asset servers
    /// must also rewrite import specifiers in the generated ESM sources as
    /// described by [`RsjsAssetUrlResolver`].
    pub fn page_runtime_plan_with_urls<I, S, R>(
        &self,
        asset_ids: I,
        urls: &R,
    ) -> Result<PageRuntimePlan, PageRuntimePlanError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        R: RsjsAssetUrlResolver + ?Sized,
    {
        let requested: BTreeSet<String> = asset_ids
            .into_iter()
            .map(|asset_id| asset_id.as_ref().to_owned())
            .collect();
        if requested.is_empty() {
            return Ok(PageRuntimePlan::default());
        }

        let islands_by_id: BTreeMap<&'static str, &'static IslandManifestEntry> = self
            .islands
            .iter()
            .copied()
            .map(|entry| (entry.asset_id, entry))
            .collect();
        let mut selected = Vec::with_capacity(requested.len());
        for asset_id in requested {
            let Some(entry) = islands_by_id.get(asset_id.as_str()).copied() else {
                return Err(PageRuntimePlanError::UnknownRenderedIsland { asset_id });
            };
            selected.push(entry);
        }
        selected.sort_by_key(|entry| entry.asset_id);

        let mut runtime_features = RuntimeFeatureSet {
            core: true,
            ..RuntimeFeatureSet::default()
        };
        let mut eager_runtime_features = RuntimeFeatureSet::default();
        let mut eager_entries = Vec::new();
        let mut deferred_entries = Vec::new();
        let mut all_client_imports: BTreeMap<String, &'static str> = BTreeMap::new();
        let mut eager_client_files = BTreeSet::new();
        let mut all_fragment_ids = BTreeSet::new();
        let mut eager_fragment_ids = BTreeSet::new();

        for entry in &selected {
            runtime_features.query |= entry.uses_queries;
            runtime_features.mutation |= entry.uses_mutations;
            let deferred = should_defer_island_asset(&entry.hydration_strategy);
            if deferred {
                deferred_entries.push(*entry);
            } else {
                eager_runtime_features.core = true;
                eager_runtime_features.query |= entry.uses_queries;
                eager_runtime_features.mutation |= entry.uses_mutations;
                eager_entries.push(*entry);
            }

            for module_path in entry.client_module_imports.iter().copied() {
                let filename = client_module_filename(module_path);
                all_client_imports
                    .entry(filename.clone())
                    .or_insert(module_path);
                if !deferred {
                    eager_client_files.insert(filename);
                }
            }
            for fragment_id in entry.component_fragment_imports.iter().copied() {
                all_fragment_ids.insert(fragment_id);
                if !deferred {
                    eager_fragment_ids.insert(fragment_id);
                }
            }
        }

        let runtime = Some(RuntimeAssets {
            base: urls.resolve(RsjsAssetRef::Runtime),
            query: eager_runtime_features
                .needs_query_runtime()
                .then(|| urls.resolve(RsjsAssetRef::QueryRuntime)),
        });

        let mut module_preloads = BTreeSet::new();
        let eager_islands: Vec<_> = eager_entries
            .into_iter()
            .map(|entry| {
                let module_url = urls.resolve(RsjsAssetRef::Island {
                    asset_id: entry.asset_id,
                });
                // Interaction entries are imported through a runtime-issued,
                // single-use `rsjs-load` URL. A preload for the unticketed URL
                // cannot satisfy that import and may cause a duplicate fetch.
                if !matches!(
                    entry.hydration_strategy,
                    IslandHydrationStrategy::Interaction { .. }
                ) {
                    module_preloads.insert(module_url.clone());
                }
                IslandModuleAsset {
                    asset_id: entry.asset_id,
                    module_url,
                    strategy: entry.hydration_strategy,
                }
            })
            .collect();
        let deferred_islands = deferred_entries
            .into_iter()
            .map(|entry| IslandModuleAsset {
                asset_id: entry.asset_id,
                module_url: urls.resolve(RsjsAssetRef::Island {
                    asset_id: entry.asset_id,
                }),
                strategy: entry.hydration_strategy,
            })
            .collect();

        let mut registered_client_modules = self.client_modules.to_vec();
        registered_client_modules.sort_by_key(|entry| entry.module_path);
        let registered_client_modules: BTreeMap<String, _> = registered_client_modules
            .into_iter()
            .map(|entry| (client_module_filename(entry.module_path), entry))
            .collect();
        let mut client_modules = Vec::with_capacity(all_client_imports.len());
        for (filename, requested_path) in all_client_imports {
            let Some(entry) = registered_client_modules.get(&filename).copied() else {
                return Err(PageRuntimePlanError::MissingClientModule {
                    module_path: requested_path.to_owned(),
                });
            };
            let module_url = urls.resolve(RsjsAssetRef::ClientModule {
                module_path: entry.module_path,
            });
            if eager_client_files.contains(&filename) {
                module_preloads.insert(module_url.clone());
            }
            client_modules.push(ClientModuleAsset {
                module_path: entry.module_path,
                module_url,
            });
        }

        let mut registered_fragments = self.component_fragments.to_vec();
        registered_fragments.sort_by_key(|entry| entry.asset_id);
        let registered_fragments: BTreeMap<_, _> = registered_fragments
            .into_iter()
            .map(|entry| (entry.asset_id, entry))
            .collect();
        let mut component_fragments = Vec::with_capacity(all_fragment_ids.len());
        for asset_id in all_fragment_ids {
            let Some(entry) = registered_fragments.get(asset_id).copied() else {
                return Err(PageRuntimePlanError::MissingComponentFragment {
                    asset_id: asset_id.to_owned(),
                });
            };
            let module_url = urls.resolve(RsjsAssetRef::ComponentFragment {
                asset_id: entry.asset_id,
            });
            if eager_fragment_ids.contains(asset_id) {
                module_preloads.insert(module_url.clone());
            }
            component_fragments.push(ComponentFragmentAsset {
                asset_id: entry.asset_id,
                module_url,
            });
        }

        Ok(PageRuntimePlan {
            rendered_islands: selected.iter().map(|entry| entry.asset_id).collect(),
            runtime_features,
            eager_runtime_features,
            runtime,
            eager_islands,
            deferred_islands,
            client_modules,
            component_fragments,
            module_preloads: module_preloads.into_iter().collect(),
        })
    }

    /// Compute the coarse runtime asset URLs this manifest's registered
    /// islands may need.
    ///
    /// This global helper is useful for compatibility and diagnostics. V2
    /// framework adapters should prefer [`Self::runtime_for_islands`] so
    /// pages that rendered only `ServerOnly` rsjs output emit no runtime
    /// tags, and pages that did not render query/mutation islands omit the
    /// query runtime.
    ///
    /// M9 note: mutations and queries share the same ESM bundle
    /// (`/rsjs-runtime-query.js`); a page is loaded if either
    /// `uses_queries` or `uses_mutations` is true on any island.
    /// This lets us keep the page's import count flat at <=2 while
    /// preserving the granular per-island flags for tooling.
    pub fn runtime(&self) -> RuntimeAssets {
        self.runtime_with_prefix("/")
    }

    /// Compute the coarse runtime asset URLs this manifest's registered
    /// islands may need,
    /// mounted under the caller-supplied URL `prefix`.
    ///
    /// Round-3 review #6: framework adapters that mount the runtime
    /// under a sub-path (e.g. `/_static/rsjs/`) need a way to override
    /// the root-mounted defaults that [`Manifest::runtime`] returns.
    /// This helper joins `prefix` with each asset filename via a
    /// single `/` separator (collapsing duplicates: `/foo/` + `bar.js`
    /// becomes `/foo/bar.js`, not `/foo//bar.js`). Pass `"/"` for the
    /// canonical root-mounted shape; `Manifest::runtime` is the
    /// shorthand for that case.
    ///
    /// `prefix` is a free-form URL prefix; the helper does not
    /// normalise it beyond the trailing-slash collapse. Adapters that
    /// need full URL normalisation (percent-encoding, scheme
    /// stripping) should preprocess the string themselves.
    pub fn runtime_with_prefix(&self, prefix: &str) -> RuntimeAssets {
        let needs_query_runtime = self
            .islands
            .iter()
            .any(|i| i.uses_queries || i.uses_mutations);
        Self::runtime_assets_from_flags(prefix, needs_query_runtime)
    }

    /// Compute runtime asset URLs for the island ids rendered by one response.
    ///
    /// Compatibility helper for callers that only need coarse runtime URLs.
    /// New response finalizers should pass
    /// `WebContext::rsjs_artifacts_snapshot().rendered_islands()` to
    /// [`Self::page_runtime_plan`] so island modules, dependencies, deferred
    /// instructions, and preloads come from the same planner. An empty set
    /// returns `Ok(None)`; unknown ids are errors.
    pub fn runtime_for_islands<I, S>(
        &self,
        asset_ids: I,
    ) -> Result<Option<RuntimeAssets>, RuntimeAssetPlanError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.runtime_for_islands_with_prefix("/", asset_ids)
    }

    /// Compute page-local runtime asset URLs for rendered island ids mounted
    /// under the caller-supplied URL `prefix`.
    ///
    /// This is the page-local counterpart to [`Self::runtime_with_prefix`].
    pub fn runtime_for_islands_with_prefix<I, S>(
        &self,
        prefix: &str,
        asset_ids: I,
    ) -> Result<Option<RuntimeAssets>, RuntimeAssetPlanError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut saw_island = false;
        let mut needs_query_runtime = false;
        for asset_id in asset_ids {
            let asset_id = asset_id.as_ref();
            let Some(entry) = self.islands.iter().find(|entry| entry.asset_id == asset_id) else {
                return Err(RuntimeAssetPlanError {
                    asset_id: asset_id.to_string(),
                });
            };
            saw_island = true;
            needs_query_runtime |= entry.uses_queries || entry.uses_mutations;
        }
        if !saw_island {
            return Ok(None);
        }
        Ok(Some(Self::runtime_assets_from_flags(
            prefix,
            needs_query_runtime,
        )))
    }

    /// Client-module bundles reachable from hydrated island JS.
    ///
    /// `manifest.client_modules` is the complete linked registry, including
    /// helper bundles that are declared in the binary but unused by any island.
    /// Asset emitters should use this helper when deciding which client-module
    /// `.js` files to materialise for a deploy.
    pub fn reachable_client_modules(&self) -> Vec<&'static ClientModuleManifestEntry> {
        let files = self.reachable_client_module_files();
        self.client_modules
            .iter()
            .copied()
            .filter(|bundle| files.contains(&client_module_filename(bundle.module_path)))
            .collect()
    }

    fn reachable_client_module_files(&self) -> BTreeSet<String> {
        self.islands
            .iter()
            .flat_map(|island| island.client_module_imports.iter().copied())
            .map(client_module_filename)
            .collect()
    }

    fn runtime_assets_from_flags(prefix: &str, needs_query_runtime: bool) -> RuntimeAssets {
        let join = |name: &str| -> String {
            if prefix.ends_with('/') {
                format!("{prefix}{name}")
            } else {
                format!("{prefix}/{name}")
            }
        };
        RuntimeAssets {
            base: join("rsjs-runtime.js"),
            query: if needs_query_runtime {
                Some(join("rsjs-runtime-query.js"))
            } else {
                None
            },
        }
    }
}

/// Build the inline module script body that defers selected island module
/// imports until their hydration trigger has fired.
///
/// The generated island module checks the `rsjs-triggered` query flag and
/// initializes only the root whose visible/idle/media gate fired. Sibling
/// instances retain independent gates and tickets.
///
/// This body performs dynamic ESM imports through the shared runtime. A CSP
/// nonce authorizes the inline bootstrap itself; the response policy must also
/// allow the resolved runtime/island source URLs, or use nonce-based
/// `strict-dynamic` trust propagation for the transitive import graph.
pub fn deferred_island_bootstrap_js(
    runtime_url: &str,
    islands: &[DeferredIslandAsset<'_>],
) -> String {
    if islands.is_empty() {
        return String::new();
    }

    let needs_visible = islands
        .iter()
        .any(|asset| matches!(asset.strategy, IslandHydrationStrategy::Visible));
    let needs_idle = islands
        .iter()
        .any(|asset| matches!(asset.strategy, IslandHydrationStrategy::Idle));
    let needs_media = islands
        .iter()
        .any(|asset| matches!(asset.strategy, IslandHydrationStrategy::Media { .. }));

    let mut out = String::new();
    out.push_str("import { ");
    let mut first = true;
    if needs_visible {
        push_js_import(&mut out, &mut first, "hydrate_when_visible");
    }
    if needs_idle {
        push_js_import(&mut out, &mut first, "hydrate_when_idle");
    }
    if needs_media {
        push_js_import(&mut out, &mut first, "hydrate_when_media");
    }
    push_js_import(&mut out, &mut first, "defer_island_module");
    out.push_str(" } from ");
    write_json_string(&mut out, runtime_url);
    out.push_str(";\n");

    for asset in islands {
        match asset.strategy {
            IslandHydrationStrategy::Visible => {
                out.push_str("defer_island_module(");
                write_json_string(&mut out, asset.asset_id);
                out.push_str(", ");
                write_json_string(&mut out, &triggered_module_url(asset.module_url));
                out.push_str(", hydrate_when_visible);\n");
            }
            IslandHydrationStrategy::Idle => {
                out.push_str("defer_island_module(");
                write_json_string(&mut out, asset.asset_id);
                out.push_str(", ");
                write_json_string(&mut out, &triggered_module_url(asset.module_url));
                out.push_str(", hydrate_when_idle);\n");
            }
            IslandHydrationStrategy::Media { query } => {
                out.push_str("defer_island_module(");
                write_json_string(&mut out, asset.asset_id);
                out.push_str(", ");
                write_json_string(&mut out, &triggered_module_url(asset.module_url));
                out.push_str(", hydrate_when_media, ");
                write_json_string(&mut out, query);
                out.push_str(");\n");
            }
            IslandHydrationStrategy::Immediate | IslandHydrationStrategy::Interaction { .. } => {}
        }
    }

    out
}

/// Build the inline module body that eagerly loads interaction island
/// factories through the generation-fenced runtime loader.
///
/// The module is fetched immediately, but the generated factory retains its
/// interaction strategy. Capture listeners are therefore armed only after the
/// import and all dependencies have evaluated successfully.
///
/// As with deferred bootstraps, a CSP nonce authorizes this inline module but
/// dynamic imports additionally require an allowed source or `strict-dynamic`.
fn interaction_island_bootstrap_js(
    runtime_url: &str,
    islands: &[DeferredIslandAsset<'_>],
) -> String {
    if islands.is_empty() {
        return String::new();
    }
    let mut out =
        String::from("import { hydrate_on_interaction, load_island_module_for_strategy } from ");
    write_json_string(&mut out, runtime_url);
    out.push_str(";\n");
    for island in islands {
        debug_assert!(matches!(
            island.strategy,
            IslandHydrationStrategy::Interaction { .. }
        ));
        out.push_str("load_island_module_for_strategy(");
        write_json_string(&mut out, island.asset_id);
        out.push_str(", null, ");
        write_json_string(&mut out, island.module_url);
        out.push_str(", hydrate_on_interaction");
        if let IslandHydrationStrategy::Interaction { event: Some(event) } = island.strategy {
            out.push_str(", [");
            write_json_string(&mut out, event);
            out.push(']');
        }
        out.push_str(");\n");
    }
    out
}

fn interaction_island_bootstrap_tag_with_nonce(
    runtime_url: &str,
    islands: &[DeferredIslandAsset<'_>],
    nonce: Option<&proa_core::CspNonce>,
) -> String {
    let js = interaction_island_bootstrap_js(runtime_url, islands);
    if js.is_empty() {
        return String::new();
    }
    let mut out = String::from(r#"<script type="module">"#);
    if let Some(nonce) = nonce {
        out.truncate(out.len() - 1);
        out.push_str(r#" nonce=""#);
        out.push_str(nonce.as_str());
        out.push_str(r#"">"#);
    }
    out.push_str(&js);
    out.push_str("</script>");
    out
}

/// Build a complete inline `<script type="module">` tag for deferred island
/// imports.
pub fn deferred_island_bootstrap_tag(
    runtime_url: &str,
    islands: &[DeferredIslandAsset<'_>],
) -> String {
    deferred_island_bootstrap_tag_with_nonce(runtime_url, islands, None)
}

/// Build the deferred island bootstrap tag with an optional request CSP nonce.
// proa-perf-ok(scope): the runtime and island URLs are JavaScript-string
// encoded upstream, and the nonce alphabet is validated before direct emission
pub fn deferred_island_bootstrap_tag_with_nonce(
    runtime_url: &str,
    islands: &[DeferredIslandAsset<'_>],
    nonce: Option<&proa_core::CspNonce>,
) -> String {
    let js = deferred_island_bootstrap_js(runtime_url, islands);
    if js.is_empty() {
        return String::new();
    }
    let mut out = String::from(r#"<script type="module">"#);
    if let Some(nonce) = nonce {
        out.truncate(out.len() - 1);
        out.push_str(r#" nonce=""#);
        out.push_str(nonce.as_str());
        out.push_str(r#"">"#);
    }
    out.push_str(&js);
    out.push_str("</script>");
    out
}

fn push_js_import(out: &mut String, first: &mut bool, name: &str) {
    if !*first {
        out.push_str(", ");
    }
    *first = false;
    out.push_str(name);
}

fn triggered_module_url(module_url: &str) -> String {
    let (base, hash) = module_url
        .split_once('#')
        .map_or((module_url, ""), |(base, hash)| (base, hash));
    let separator = if base.contains('?') { '&' } else { '?' };
    let mut out = String::with_capacity(module_url.len() + 17);
    out.push_str(base);
    out.push(separator);
    out.push_str("rsjs-triggered=1");
    if !hash.is_empty() {
        out.push('#');
        out.push_str(hash);
    }
    out
}

/// Encode the [`Manifest`] as a JSON document (hand-written, no
/// `serde` dep).
///
/// Schema:
/// ```json
/// {
///   "islands": [
///     {
///       "asset_id": "counter",
///       "struct_name": "Counter",
///       "is_async": false,
///       "hydration_strategy": { "type": "immediate" },
///       "client_module_imports": ["crate::pricing"],
///       "component_fragment_imports": ["calendar_1234abcd"]
///     }
///   ],
///   "client_modules": [
///     { "module_path": "crate::pricing" }
///   ],
///   "client_classes": [
///     { "module_path": "crate::counter_logic", "export_names": ["Counter"] }
///   ],
///   "component_fragments": [
///     { "asset_id": "calendar_1234abcd", "struct_name": "Calendar", "export_name": "mount_calendar_1234abcd" }
///   ]
/// }
/// ```
///
/// JS bodies are intentionally **not** serialised into `manifest.json`:
/// they can be many KB per island and bloat the manifest file beyond what
/// makes sense for a discovery document. JS bodies are written alongside as
/// `<asset_id>.js` / `<module_canonical>.js` files by
/// [`write_manifest_assets`]; the manifest gives consumers the keys they
/// need to find them.
pub fn manifest_json(m: &Manifest) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"islands\":[");
    for (i, island) in m.islands.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        out.push_str("\"asset_id\":");
        write_json_string(&mut out, island.asset_id);
        out.push_str(",\"struct_name\":");
        write_json_string(&mut out, island.struct_name);
        out.push_str(",\"is_async\":");
        out.push_str(if island.is_async { "true" } else { "false" });
        out.push_str(",\"hydration_strategy\":");
        write_hydration_strategy_json(&mut out, &island.hydration_strategy);
        out.push_str(",\"client_module_imports\":[");
        for (j, path) in island.client_module_imports.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            write_json_string(&mut out, path);
        }
        out.push(']');
        out.push_str(",\"component_fragment_imports\":[");
        for (j, asset_id) in island.component_fragment_imports.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            write_json_string(&mut out, asset_id);
        }
        out.push(']');
        out.push_str(",\"uses_queries\":");
        out.push_str(if island.uses_queries { "true" } else { "false" });
        out.push_str(",\"uses_mutations\":");
        out.push_str(if island.uses_mutations {
            "true"
        } else {
            "false"
        });
        out.push('}');
    }
    out.push_str("],\"client_modules\":[");
    for (i, bundle) in m.client_modules.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        out.push_str("\"module_path\":");
        write_json_string(&mut out, bundle.module_path);
        out.push('}');
    }
    out.push_str("],\"client_classes\":[");
    for (i, class) in m.client_classes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        out.push_str("\"module_path\":");
        write_json_string(&mut out, class.module_path);
        out.push_str(",\"export_names\":[");
        for (j, name) in class.export_names.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            write_json_string(&mut out, name);
        }
        out.push_str("]}");
    }
    out.push_str("],\"component_fragments\":[");
    for (i, fragment) in m.component_fragments.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        out.push_str("\"asset_id\":");
        write_json_string(&mut out, fragment.asset_id);
        out.push_str(",\"struct_name\":");
        write_json_string(&mut out, fragment.struct_name);
        out.push_str(",\"export_name\":");
        write_json_string(&mut out, fragment.export_name);
        out.push('}');
    }
    out.push_str("]}");
    out
}

/// Write `manifest.json` describing the linked manifest into `out_dir`.
///
/// Creates the directory if it does not exist. Overwrites any existing
/// `manifest.json` at the target path. Errors propagate as
/// [`io::Error`].
///
/// **Atomic**: the JSON is first written to `manifest.json.tmp` and
/// then renamed into place. `fs::rename` is atomic on POSIX and on
/// Windows when the source and destination are on the same volume,
/// so concurrent calls with the same `out_dir` (e.g. parallel test
/// runs) cannot leave a half-written `manifest.json` behind.
pub fn write_manifest_json<P: AsRef<Path>>(out_dir: P) -> io::Result<()> {
    let dir = out_dir.as_ref();
    fs::create_dir_all(dir)?;
    let json = manifest_json(&manifest());
    write_atomic(&dir.join("manifest.json"), json.as_bytes())
}

/// Write per-island and per-bundle `.js` source files into `out_dir`.
///
/// For each island in the registry, writes `<asset_id>.js` containing
/// the unminified per-island JS source. For each client-module bundle,
/// writes `<module_canonical>.js` (with `::` replaced by `__`). Explicit
/// client-class modules are written under [`client_class_filename`]. For
/// each component fragment, writes `component_<asset_id>.js`. Creates the
/// directory if it does not exist.
///
/// **Idempotent and self-cleaning**: before writing the new layout,
/// the helper enumerates every `*.js` file already in `out_dir` and
/// removes any whose name is not in the set the current manifest
/// expects. This prevents stale per-island JS files from lingering
/// after an island has been removed from the source tree. Non-`.js`
/// files in `out_dir` are left untouched; the helper does not touch
/// foreign content (e.g. `manifest.json`, sourcemaps, user-provided
/// assets) under any circumstance.
///
/// **Cleanup ownership**: cleanup is gated on a `.rsjs-managed`
/// marker file at the top of `out_dir`. The first call into a
/// brand-new (or empty) directory creates the marker and proceeds
/// normally. On subsequent calls, the marker signals "rsjs owns the
/// `*.js` files here" and the cleanup pass runs. If the marker is
/// **absent** but the directory contains other content (e.g. the
/// caller pointed at a populated app-shell directory), cleanup is
/// skipped with a warning so user-placed files such as `polyfill.js`
/// or `app-shell.js` are not deleted. To opt in afterwards, create
/// an empty `.rsjs-managed` file at the directory root.
///
/// **Orphan tmp cleanup**: the atomic-write path stages each output
/// in a sibling `<file>.tmp.<pid>.<tid>.<counter>` file before
/// renaming it into place. A SIGKILL between `fs::write` and
/// `fs::rename` would otherwise leave that staging file behind.
/// The cleanup pass therefore also removes any regular file in
/// `out_dir` whose name contains the substring `.tmp.` and whose
/// modification time is older than the internal 60-second orphan threshold
/// (`ORPHAN_TMP_MAX_AGE`). The mtime gate avoids racing a concurrent writer:
/// a peer's in-flight staging file is freshly created and survives
/// the cleanup, while a SIGKILL'd staging file from a previous run
/// is comfortably past the threshold and gets reclaimed. The 60s
/// figure is a heuristic (atomic writes complete in milliseconds in
/// practice); shrink it only if you can guarantee that no caller
/// pauses between `fs::write` and `fs::rename` for that long.
pub fn write_manifest_assets<P: AsRef<Path>>(out_dir: P) -> io::Result<()> {
    let dir = out_dir.as_ref();
    fs::create_dir_all(dir)?;
    let manifest = manifest();

    // Build the set of `.js` filenames the current manifest expects.
    // We allocate up-front so the cleanup pass can do an O(1) lookup
    // per directory entry. The set is small (one entry per island +
    // one per reachable client-module bundle), so the allocation overhead is
    // negligible compared to the file IO that follows.
    let reachable_client_modules = manifest.reachable_client_modules();
    let mut expected: std::collections::HashSet<String> = std::collections::HashSet::with_capacity(
        manifest.islands.len()
            + reachable_client_modules.len()
            + manifest.client_classes.len()
            + manifest.component_fragments.len(),
    );
    for island in &manifest.islands {
        expected.insert(format!("{}.js", island.asset_id));
    }
    for bundle in &reachable_client_modules {
        expected.insert(client_module_filename(bundle.module_path));
    }
    for class in &manifest.client_classes {
        expected.insert(client_class_filename(class.module_path));
    }
    for fragment in &manifest.component_fragments {
        expected.insert(component_fragment_filename(fragment.asset_id));
    }

    // Decide whether the cleanup pass is allowed to run. The
    // `.rsjs-managed` marker tags this directory as "rsjs owns the
    // *.js files here". If the directory is empty (other than what
    // we are about to write), we create the marker and proceed.
    // If the directory already has content but no marker, we treat
    // it as a foreign location and skip cleanup, warning the caller.
    let marker = dir.join(MARKER_FILE);
    let cleanup_allowed = if marker.exists() {
        true
    } else {
        // Read the directory: if it is empty (or only contains
        // entries we cannot read), we own it; otherwise, leave it
        // alone. Non-UTF-8 names count as content (we cannot
        // distinguish them from user-provided files).
        let mut has_other_content = false;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name();
            // Skip the marker itself in case it was created between
            // the .exists() check above and now.
            if name == std::ffi::OsStr::new(MARKER_FILE) {
                continue;
            }
            has_other_content = true;
            break;
        }
        if has_other_content {
            eprintln!(
                "rsjs: out_dir {dir:?} contains files but no {MARKER_FILE} marker; \
                 skipping cleanup to avoid deleting non-rsjs files. Create an empty \
                 {MARKER_FILE} file in the directory to opt in to cleanup."
            );
            false
        } else {
            // Empty directory: claim it.
            fs::write(&marker, b"")?;
            true
        }
    };

    // Cleanup pass: remove any `.js` file in `out_dir` that the
    // manifest does not claim, plus any orphan staging file left
    // behind by a crashed atomic write (filename contains `.tmp.`).
    // Skip directories and symlinks. If the directory read fails
    // (permissions, race), propagate the error. Only runs when the
    // `.rsjs-managed` marker is present (or was just created).
    if cleanup_allowed {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_file() {
                continue;
            }
            let name = entry.file_name();
            let name_str = match name.to_str() {
                Some(s) => s,
                None => continue, // non-UTF8 name, leave alone
            };
            // Never delete the marker itself.
            if name_str == MARKER_FILE {
                continue;
            }
            // Orphan staging file from a crashed run: name contains
            // `.tmp.` (e.g. `counter.js.tmp.1234.5.0`). Only remove
            // if the file's mtime is older than ORPHAN_TMP_MAX_AGE so
            // we don't race a peer thread/process mid-write. Failure
            // to read the metadata (e.g. the file vanished between
            // `read_dir` and now) is treated as "leave it alone".
            if name_str.contains(".tmp.") {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        if modified
                            .elapsed()
                            .map(|age| age >= ORPHAN_TMP_MAX_AGE)
                            .unwrap_or(false)
                        {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
                continue;
            }
            if !name_str.ends_with(".js") {
                continue;
            }
            if expected.contains(name_str) {
                continue;
            }
            fs::remove_file(entry.path())?;
        }
    }

    for island in &manifest.islands {
        let path = dir.join(format!("{}.js", island.asset_id));
        write_atomic(&path, island.js_source.as_bytes())?;
    }
    for bundle in &reachable_client_modules {
        let path = dir.join(client_module_filename(bundle.module_path));
        let js_source = client_module_js_source(bundle);
        write_atomic(&path, js_source.as_bytes())?;
    }
    for class in &manifest.client_classes {
        let path = dir.join(client_class_filename(class.module_path));
        write_atomic(&path, class.js_source.as_bytes())?;
    }
    for fragment in &manifest.component_fragments {
        let path = dir.join(component_fragment_filename(fragment.asset_id));
        write_atomic(&path, fragment.js_source.as_bytes())?;
    }
    Ok(())
}

/// Filename for a component-fragment chunk asset.
pub fn component_fragment_filename(asset_id: &str) -> String {
    format!("component_{asset_id}.js")
}

/// Write `bytes` to `path` atomically by way of a per-call staging
/// file plus `fs::rename`.
///
/// `fs::rename` is atomic on POSIX and on Windows when the source
/// and destination live on the same volume (which they always do
/// here: the staging sibling shares `path`'s parent). Concurrent
/// callers therefore cannot observe a partial write; the worst
/// case is the tail-most rename winning, but the file at `path`
/// is always either the previous complete contents or the latest
/// complete contents, never a torn mix.
///
/// The staging filename adds a `<pid>.<thread_id>.<counter>` suffix
/// so two threads (same process) and two processes (different pid)
/// invoking the helper concurrently never collide on the staging
/// file.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut tmp = path.as_os_str().to_owned();
    let pid = std::process::id();
    let tid = format!("{:?}", std::thread::current().id());
    // ThreadId Debug fmt is `ThreadId(N)`; strip the wrapping so the
    // staging filename stays compact and shell-safe.
    let tid = tid.trim_start_matches("ThreadId(").trim_end_matches(')');
    tmp.push(format!(".tmp.{pid}.{tid}.{n}"));
    let tmp_path = std::path::PathBuf::from(tmp);
    // Best-effort: if a stale tmp survives a previous crash, the
    // `fs::write` below truncates it; the subsequent rename clobbers
    // any same-name destination on POSIX, and on Windows
    // `fs::rename` likewise replaces the destination since Rust 1.5.
    fs::write(&tmp_path, bytes)?;
    if let Err(e) = fs::rename(&tmp_path, path) {
        // Clean up the tmp file so a failed rename does not leave
        // litter in the output directory.
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

/// JSON-string encoder copied from `client_codec::write_json_string`.
///
/// We don't re-export the codec helper because pulling
/// `__codec_internals` into this module's API surface (even
/// transitively) would entangle the manifest writer with the codec
/// vocabulary. The escape rules are tiny; the duplication is
/// acceptable.
fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            // HTML/JS-context escapes: the encoded JSON is shipped inside
            // <script type="application/json"> blocks, so any '<', '>', '&',
            // U+2028, or U+2029 in a string value could break out of the
            // script tag or be interpreted as a JS line terminator.
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_hydration_strategy_json(out: &mut String, strategy: &IslandHydrationStrategy) {
    out.push('{');
    match strategy {
        IslandHydrationStrategy::Immediate => out.push_str(r#""type":"immediate""#),
        IslandHydrationStrategy::Visible => out.push_str(r#""type":"visible""#),
        IslandHydrationStrategy::Idle => out.push_str(r#""type":"idle""#),
        IslandHydrationStrategy::Interaction { event } => {
            out.push_str(r#""type":"interaction""#);
            if let Some(event) = event {
                out.push_str(r#","event":"#);
                write_json_string(out, event);
            }
        }
        IslandHydrationStrategy::Media { query } => {
            out.push_str(r#""type":"media","query":"#);
            write_json_string(out, query);
        }
    }
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_json_empty_has_all_asset_keys() {
        let m = Manifest::default();
        let j = manifest_json(&m);
        assert_eq!(
            j,
            r#"{"islands":[],"client_modules":[],"client_classes":[],"component_fragments":[]}"#
        );
    }

    static FIXTURE_ISLAND: IslandManifestEntry = IslandManifestEntry {
        asset_id: "counter",
        struct_name: "Counter",
        js_source: "/* unused */",
        is_async: false,
        hydration_strategy: IslandHydrationStrategy::Immediate,
        client_module_imports: &["crate::pricing"],
        component_fragment_imports: &["calendar_1234"],
        uses_queries: false,
        uses_mutations: false,
    };

    static FIXTURE_SELF_IMPORT_ISLAND: IslandManifestEntry = IslandManifestEntry {
        asset_id: "cart",
        struct_name: "Cart",
        js_source: "/* unused */",
        is_async: false,
        hydration_strategy: IslandHydrationStrategy::Immediate,
        client_module_imports: &["self::pricing"],
        component_fragment_imports: &[],
        uses_queries: false,
        uses_mutations: false,
    };

    static FIXTURE_QUOTED: IslandManifestEntry = IslandManifestEntry {
        asset_id: "x",
        struct_name: "Has\"Quote",
        js_source: "",
        is_async: true,
        hydration_strategy: IslandHydrationStrategy::Immediate,
        client_module_imports: &[],
        component_fragment_imports: &[],
        uses_queries: false,
        uses_mutations: false,
    };

    static FIXTURE_WITH_QUERY: IslandManifestEntry = IslandManifestEntry {
        asset_id: "stockticker",
        struct_name: "StockTicker",
        js_source: "",
        is_async: true,
        hydration_strategy: IslandHydrationStrategy::Immediate,
        client_module_imports: &[],
        component_fragment_imports: &[],
        uses_queries: true,
        uses_mutations: false,
    };

    static FIXTURE_WITH_MUTATION: IslandManifestEntry = IslandManifestEntry {
        asset_id: "updatecart",
        struct_name: "UpdateCart",
        js_source: "",
        is_async: true,
        hydration_strategy: IslandHydrationStrategy::Immediate,
        client_module_imports: &[],
        component_fragment_imports: &[],
        uses_queries: false,
        uses_mutations: true,
    };

    static FIXTURE_INTERACTION: IslandManifestEntry = IslandManifestEntry {
        asset_id: "actionmenu",
        struct_name: "ActionMenu",
        js_source: "",
        is_async: false,
        hydration_strategy: IslandHydrationStrategy::Interaction {
            event: Some("pointerdown"),
        },
        client_module_imports: &[],
        component_fragment_imports: &[],
        uses_queries: false,
        uses_mutations: false,
    };

    static FIXTURE_VISIBLE_QUERY: IslandManifestEntry = IslandManifestEntry {
        asset_id: "lazyquote",
        struct_name: "LazyQuote",
        js_source: "",
        is_async: true,
        hydration_strategy: IslandHydrationStrategy::Visible,
        client_module_imports: &["crate::analytics"],
        component_fragment_imports: &["chart_5678"],
        uses_queries: true,
        uses_mutations: false,
    };

    static FIXTURE_IDLE_MUTATION: IslandManifestEntry = IslandManifestEntry {
        asset_id: "savequeue",
        struct_name: "SaveQueue",
        js_source: "",
        is_async: true,
        hydration_strategy: IslandHydrationStrategy::Idle,
        client_module_imports: &[],
        component_fragment_imports: &[],
        uses_queries: false,
        uses_mutations: true,
    };

    static FIXTURE_MEDIA: IslandManifestEntry = IslandManifestEntry {
        asset_id: "widechart",
        struct_name: "WideChart",
        js_source: "",
        is_async: false,
        hydration_strategy: IslandHydrationStrategy::Media {
            query: "(min-width: 72rem)",
        },
        client_module_imports: &[],
        component_fragment_imports: &[],
        uses_queries: false,
        uses_mutations: false,
    };

    static FIXTURE_BUNDLE: ClientModuleManifestEntry = ClientModuleManifestEntry {
        module_path: "crate::pricing",
        helpers: &[],
    };

    static FIXTURE_ANALYTICS_BUNDLE: ClientModuleManifestEntry = ClientModuleManifestEntry {
        module_path: "crate::analytics",
        helpers: &[],
    };

    static FIXTURE_UNUSED_BUNDLE: ClientModuleManifestEntry = ClientModuleManifestEntry {
        module_path: "crate::unused",
        helpers: &[],
    };

    fn eval_fixture_subtotal(
        args: &[(&'static str, crate::__runtime::ClientArg)],
    ) -> crate::__runtime::ClientValue {
        crate::__runtime::client_binary_op(
            "mul",
            crate::__runtime::client_arg_value(args, 0, "price"),
            crate::__runtime::client_arg_value(args, 1, "qty"),
        )
    }

    fn eval_fixture_adjusted_subtotal(
        _args: &[(&'static str, crate::__runtime::ClientArg)],
    ) -> crate::__runtime::ClientValue {
        crate::__runtime::ClientValue::Unit
    }

    static FIXTURE_HELPER_SUBTOTAL: ClientHelperMeta = ClientHelperMeta {
        name: "subtotal",
        params: &[("price", "i64"), ("qty", "i64")],
        return_ty: "i64",
        body_ir_version: CLIENT_IR_SCHEMA_VERSION,
        body_ir_json: r#"{"kind":"binary","op":"mul","left":{"kind":"local","local":0},"right":{"kind":"local","local":1}}"#,
        server_eval: eval_fixture_subtotal,
    };

    static FIXTURE_HELPER_ADJUSTED_SUBTOTAL: ClientHelperMeta = ClientHelperMeta {
        name: "adjusted_subtotal",
        params: &[("price", "i64"), ("qty", "i64")],
        return_ty: "i64",
        body_ir_version: CLIENT_IR_SCHEMA_VERSION,
        body_ir_json: r#"{"kind":"block","stmts":[{"kind":"let","local":2,"name":"subtotal","ty":{"kind":"unknown"},"value":{"kind":"binary","op":"mul","left":{"kind":"local","local":0},"right":{"kind":"local","local":1}}},{"kind":"return","value":{"kind":"binary","op":"add","left":{"kind":"local","local":2},"right":{"kind":"literal","value":{"kind":"int","value":5}}}}]}"#,
        server_eval: eval_fixture_adjusted_subtotal,
    };

    static FIXTURE_METADATA_BUNDLE: ClientModuleManifestEntry = ClientModuleManifestEntry {
        module_path: "crate::helpers::pricing",
        helpers: &[&FIXTURE_HELPER_SUBTOTAL],
    };

    static FIXTURE_METADATA_BLOCK_BUNDLE: ClientModuleManifestEntry = ClientModuleManifestEntry {
        module_path: "crate::helpers::pricing",
        helpers: &[&FIXTURE_HELPER_ADJUSTED_SUBTOTAL],
    };

    static FIXTURE_HELPER_OLD_IR_VERSION: ClientHelperMeta = ClientHelperMeta {
        name: "legacy",
        params: &[],
        return_ty: "()",
        body_ir_version: 2,
        body_ir_json: r#"{"kind":"block","stmts":[]}"#,
        server_eval: eval_fixture_adjusted_subtotal,
    };

    static FIXTURE_OLD_IR_VERSION_BUNDLE: ClientModuleManifestEntry = ClientModuleManifestEntry {
        module_path: "crate::helpers::legacy",
        helpers: &[&FIXTURE_HELPER_OLD_IR_VERSION],
    };

    static FIXTURE_CLIENT_CLASS: ClientClassManifestEntry = ClientClassManifestEntry {
        module_path: "crate::logic::counter",
        export_names: &["Counter"],
        js_source: "export class Counter {}",
    };

    static FIXTURE_FRAGMENT: ComponentFragmentManifestEntry = ComponentFragmentManifestEntry {
        asset_id: "calendar_1234",
        struct_name: "Calendar",
        export_name: "mount_calendar_1234",
        js_source: "export function mount_calendar_1234(root) {}",
    };

    static FIXTURE_CHART_FRAGMENT: ComponentFragmentManifestEntry =
        ComponentFragmentManifestEntry {
            asset_id: "chart_5678",
            struct_name: "Chart",
            export_name: "mount_chart_5678",
            js_source: "export function mount_chart_5678(root) {}",
        };

    static FIXTURE_UNUSED_FRAGMENT: ComponentFragmentManifestEntry =
        ComponentFragmentManifestEntry {
            asset_id: "unused_9999",
            struct_name: "Unused",
            export_name: "mount_unused_9999",
            js_source: "export function mount_unused_9999(root) {}",
        };

    fn page_runtime_fixture_manifest() -> Manifest {
        Manifest {
            islands: vec![
                &FIXTURE_MEDIA,
                &FIXTURE_WITH_MUTATION,
                &FIXTURE_VISIBLE_QUERY,
                &FIXTURE_ISLAND,
                &FIXTURE_INTERACTION,
                &FIXTURE_IDLE_MUTATION,
                &FIXTURE_WITH_QUERY,
            ],
            client_modules: vec![
                &FIXTURE_UNUSED_BUNDLE,
                &FIXTURE_BUNDLE,
                &FIXTURE_ANALYTICS_BUNDLE,
            ],
            client_classes: Vec::new(),
            component_fragments: vec![
                &FIXTURE_UNUSED_FRAGMENT,
                &FIXTURE_CHART_FRAGMENT,
                &FIXTURE_FRAGMENT,
            ],
        }
    }

    struct FixtureAssetUrls;

    impl RsjsAssetUrlResolver for FixtureAssetUrls {
        fn resolve(&self, asset: RsjsAssetRef<'_>) -> String {
            match asset {
                RsjsAssetRef::Runtime => "/assets/runtime.core.js?build=7&debug=0".to_owned(),
                RsjsAssetRef::QueryRuntime => "/assets/runtime.query.js".to_owned(),
                RsjsAssetRef::Island { asset_id } => {
                    format!("/assets/islands/{asset_id}.mjs")
                }
                RsjsAssetRef::ClientModule { module_path } => {
                    format!("/assets/clients/{}", client_module_filename(module_path))
                }
                RsjsAssetRef::ClientClass { module_path } => {
                    format!("/assets/classes/{}", client_class_filename(module_path))
                }
                RsjsAssetRef::ComponentFragment { asset_id } => {
                    format!("/assets/fragments/{asset_id}.mjs")
                }
            }
        }
    }

    #[test]
    fn generated_module_runtime_imports_can_follow_a_custom_base_path() {
        let source = concat!(
            "import { signal } from \"/rsjs-runtime.js\";\n",
            "import { query } from \"/rsjs-runtime-query.js\";\n",
            "import { local } from \"./component.js\";\n",
        );
        let rewritten = rewrite_runtime_import_specifiers(
            source,
            "/app/assets/rsjs-runtime.1234.js",
            "/app/assets/rsjs-runtime-query.5678.js",
        );

        assert_eq!(
            rewritten,
            concat!(
                "import { signal } from \"/app/assets/rsjs-runtime.1234.js\";\n",
                "import { query } from \"/app/assets/rsjs-runtime-query.5678.js\";\n",
                "import { local } from \"./component.js\";\n",
            )
        );
    }

    #[test]
    fn manifest_json_serialises_island_fields() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let j = manifest_json(&m);
        assert!(j.contains(r#""asset_id":"counter""#), "got: {j}");
        assert!(j.contains(r#""struct_name":"Counter""#), "got: {j}");
        assert!(j.contains(r#""is_async":false"#), "got: {j}");
        assert!(
            j.contains(r#""hydration_strategy":{"type":"immediate"}"#),
            "got: {j}"
        );
        assert!(
            j.contains(r#""client_module_imports":["crate::pricing"]"#),
            "got: {j}"
        );
        assert!(
            j.contains(r#""component_fragment_imports":["calendar_1234"]"#),
            "got: {j}"
        );
    }

    #[test]
    fn manifest_json_escapes_quotes_in_struct_name() {
        let m = Manifest {
            islands: vec![&FIXTURE_QUOTED],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let j = manifest_json(&m);
        assert!(j.contains(r#""struct_name":"Has\"Quote""#), "got: {j}");
        assert!(j.contains(r#""is_async":true"#), "got: {j}");
    }

    #[test]
    fn hydration_strategy_json_serialises_variants() {
        let mut out = String::new();
        write_hydration_strategy_json(
            &mut out,
            &IslandHydrationStrategy::Interaction {
                event: Some("focusin"),
            },
        );
        assert_eq!(out, r#"{"type":"interaction","event":"focusin"}"#);

        out.clear();
        write_hydration_strategy_json(
            &mut out,
            &IslandHydrationStrategy::Media {
                query: "(min-width: 768px)",
            },
        );
        assert_eq!(out, r#"{"type":"media","query":"(min-width: 768px)"}"#);
    }

    #[test]
    fn deferred_asset_gate_skips_immediate_and_interaction() {
        assert!(!should_defer_island_asset(
            &IslandHydrationStrategy::Immediate
        ));
        assert!(!should_defer_island_asset(
            &IslandHydrationStrategy::Interaction {
                event: Some("click")
            }
        ));
        assert!(should_defer_island_asset(&IslandHydrationStrategy::Visible));
        assert!(should_defer_island_asset(&IslandHydrationStrategy::Idle));
        assert!(should_defer_island_asset(&IslandHydrationStrategy::Media {
            query: "(min-width: 768px)"
        }));
    }

    #[test]
    fn deferred_island_bootstrap_uses_strategy_triggers() {
        let js = deferred_island_bootstrap_js(
            "/rsjs-runtime.abc12345.js",
            &[
                DeferredIslandAsset {
                    asset_id: "hero",
                    module_url: "/rsjs/island/hero.def67890.js",
                    strategy: IslandHydrationStrategy::Visible,
                },
                DeferredIslandAsset {
                    asset_id: "summary",
                    module_url: "/rsjs/island/summary.js?cache=1#main",
                    strategy: IslandHydrationStrategy::Media {
                        query: "(min-width: 768px)",
                    },
                },
            ],
        );

        assert!(js.contains(
            r#"import { hydrate_when_visible, hydrate_when_media, defer_island_module } from "/rsjs-runtime.abc12345.js";"#
        ));
        assert!(js.contains(r#"defer_island_module("hero", "/rsjs/island/hero.def67890.js?rsjs-triggered=1", hydrate_when_visible);"#));
        assert!(js.contains(r#"defer_island_module("summary", "/rsjs/island/summary.js?cache=1\u0026rsjs-triggered=1#main", hydrate_when_media, "(min-width: 768px)");"#));
        assert!(!js.contains("import(url)"));
        assert!(!js.contains(".catch("));
    }

    #[test]
    fn interaction_island_bootstrap_eagerly_loads_through_generation_fence() {
        let js = interaction_island_bootstrap_js(
            "/assets/runtime.js",
            &[DeferredIslandAsset {
                asset_id: "actionmenu",
                module_url: "/assets/actionmenu.js",
                strategy: IslandHydrationStrategy::Interaction {
                    event: Some("pointerdown"),
                },
            }],
        );

        assert!(js.contains(
            r#"import { hydrate_on_interaction, load_island_module_for_strategy } from "/assets/runtime.js";"#
        ));
        assert!(js.contains(
            r#"load_island_module_for_strategy("actionmenu", null, "/assets/actionmenu.js", hydrate_on_interaction, ["pointerdown"]);"#
        ));
        assert!(!js.contains(r#"hydrate_on_interaction("actionmenu""#));
    }

    #[test]
    fn deferred_island_bootstrap_tag_uses_csp_nonce() {
        let nonce = proa_core::CspNonce::new("cnNqcy1ub25jZQ==").unwrap();
        let tag = deferred_island_bootstrap_tag_with_nonce(
            "/rsjs-runtime.js",
            &[DeferredIslandAsset {
                asset_id: "hero",
                module_url: "/hero.js",
                strategy: IslandHydrationStrategy::Visible,
            }],
            Some(&nonce),
        );
        assert!(tag.starts_with(r#"<script type="module" nonce="cnNqcy1ub25jZQ==">"#));
        assert!(tag.ends_with("</script>"));
    }

    #[test]
    fn write_json_string_escapes_html_runes() {
        let mut out = String::new();
        write_json_string(
            &mut out,
            "</script><script>alert(1)</script>&\u{2028}\u{2029}",
        );
        assert!(!out.contains("</script>"));
        assert!(!out.contains('<'));
        assert!(!out.contains('>'));
        assert!(!out.contains('&'));
        assert!(!out.contains('\u{2028}'));
        assert!(!out.contains('\u{2029}'));
        assert!(out.contains("\\u003c"));
        assert!(out.contains("\\u003e"));
        assert!(out.contains("\\u0026"));
        assert!(out.contains("\\u2028"));
        assert!(out.contains("\\u2029"));
    }

    #[test]
    fn page_runtime_plan_empty_rendered_set_is_empty() {
        let plan = page_runtime_fixture_manifest()
            .page_runtime_plan(std::iter::empty::<&str>())
            .unwrap();

        assert!(plan.is_empty());
        assert_eq!(plan, PageRuntimePlan::default());
        assert_eq!(plan.to_html(None), "");

        let mut state = PageRuntimeEmissionState::new();
        assert!(state.is_empty());
        assert_eq!(plan.to_html_delta(&mut state, None), "");
        assert!(state.is_empty());
    }

    #[test]
    fn page_runtime_plan_reports_unknown_and_missing_manifest_assets() {
        let error = page_runtime_fixture_manifest()
            .page_runtime_plan(["missing"])
            .unwrap_err();
        assert_eq!(
            error,
            PageRuntimePlanError::UnknownRenderedIsland {
                asset_id: "missing".to_owned()
            }
        );

        let missing_client = Manifest {
            islands: vec![&FIXTURE_ISLAND],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: vec![&FIXTURE_FRAGMENT],
        }
        .page_runtime_plan(["counter"])
        .unwrap_err();
        assert_eq!(
            missing_client,
            PageRuntimePlanError::MissingClientModule {
                module_path: "crate::pricing".to_owned()
            }
        );

        let missing_fragment = Manifest {
            islands: vec![&FIXTURE_ISLAND],
            client_modules: vec![&FIXTURE_BUNDLE],
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        }
        .page_runtime_plan(["counter"])
        .unwrap_err();
        assert_eq!(
            missing_fragment,
            PageRuntimePlanError::MissingComponentFragment {
                asset_id: "calendar_1234".to_owned()
            }
        );
    }

    #[test]
    fn page_runtime_plan_dedupes_and_sorts_rendered_islands() {
        let manifest = page_runtime_fixture_manifest();
        let plan = manifest
            .page_runtime_plan(["stockticker", "counter", "counter"])
            .unwrap();
        let reversed = manifest
            .page_runtime_plan(["counter", "stockticker"])
            .unwrap();

        assert_eq!(plan, reversed);
        assert_eq!(plan.rendered_islands, vec!["counter", "stockticker"]);
        assert_eq!(
            plan.eager_islands
                .iter()
                .map(|island| island.asset_id)
                .collect::<Vec<_>>(),
            vec!["counter", "stockticker"]
        );
        assert_eq!(
            plan.module_preloads,
            vec![
                "/rsjs/island/component_calendar_1234.js",
                "/rsjs/island/counter.js",
                "/rsjs/island/pricing.js",
                "/rsjs/island/stockticker.js",
            ]
        );
    }

    #[test]
    fn page_runtime_plan_defers_visible_idle_and_media_only() {
        let plan = page_runtime_fixture_manifest()
            .page_runtime_plan([
                "widechart",
                "savequeue",
                "counter",
                "lazyquote",
                "actionmenu",
            ])
            .unwrap();

        assert_eq!(
            plan.eager_islands
                .iter()
                .map(|island| island.asset_id)
                .collect::<Vec<_>>(),
            vec!["actionmenu", "counter"]
        );
        assert_eq!(
            plan.deferred_islands
                .iter()
                .map(|island| island.asset_id)
                .collect::<Vec<_>>(),
            vec!["lazyquote", "savequeue", "widechart"]
        );
        assert_eq!(
            plan.runtime_features,
            RuntimeFeatureSet {
                core: true,
                query: true,
                mutation: true,
            }
        );
        assert_eq!(
            plan.eager_runtime_features,
            RuntimeFeatureSet {
                core: true,
                query: false,
                mutation: false,
            }
        );
        assert_eq!(
            plan.runtime
                .as_ref()
                .and_then(|runtime| runtime.query.as_ref()),
            None,
            "query/mutation work on deferred islands must not eagerly load the query runtime"
        );

        let eager_query = page_runtime_fixture_manifest()
            .page_runtime_plan(["stockticker"])
            .unwrap();
        assert_eq!(
            eager_query
                .runtime
                .as_ref()
                .and_then(|runtime| runtime.query.as_deref()),
            Some("/rsjs-runtime-query.js")
        );

        let eager_mutation = page_runtime_fixture_manifest()
            .page_runtime_plan(["updatecart"])
            .unwrap();
        assert_eq!(
            eager_mutation
                .runtime
                .as_ref()
                .and_then(|runtime| runtime.query.as_deref()),
            Some("/rsjs-runtime-query.js")
        );
    }

    #[test]
    fn page_runtime_plan_keeps_dependencies_page_local_and_preloads_only_eager_graph() {
        let plan = page_runtime_fixture_manifest()
            .page_runtime_plan(["lazyquote", "counter"])
            .unwrap();

        assert_eq!(
            plan.client_modules
                .iter()
                .map(|module| module.module_path)
                .collect::<Vec<_>>(),
            vec!["crate::analytics", "crate::pricing"]
        );
        assert_eq!(
            plan.component_fragments
                .iter()
                .map(|fragment| fragment.asset_id)
                .collect::<Vec<_>>(),
            vec!["calendar_1234", "chart_5678"]
        );
        assert_eq!(
            plan.module_preloads,
            vec![
                "/rsjs/island/component_calendar_1234.js",
                "/rsjs/island/counter.js",
                "/rsjs/island/pricing.js",
            ]
        );
        assert!(plan
            .client_modules
            .iter()
            .all(|module| module.module_path != "crate::unused"));
        assert!(plan
            .component_fragments
            .iter()
            .all(|fragment| fragment.asset_id != "unused_9999"));
    }

    #[test]
    fn page_runtime_plan_uses_application_owned_asset_urls() {
        let plan = page_runtime_fixture_manifest()
            .page_runtime_plan_with_urls(["stockticker", "counter"], &FixtureAssetUrls)
            .unwrap();
        let runtime = plan.runtime.as_ref().unwrap();

        assert_eq!(runtime.base, "/assets/runtime.core.js?build=7&debug=0");
        assert_eq!(runtime.query.as_deref(), Some("/assets/runtime.query.js"));
        assert_eq!(
            plan.eager_islands
                .iter()
                .map(|island| island.module_url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/assets/islands/counter.mjs",
                "/assets/islands/stockticker.mjs"
            ]
        );
        assert_eq!(
            plan.client_modules[0].module_url,
            "/assets/clients/pricing.js"
        );
        assert_eq!(
            plan.component_fragments[0].module_url,
            "/assets/fragments/calendar_1234.mjs"
        );
    }

    #[test]
    fn page_runtime_plan_html_order_and_nonce_are_stable() {
        let plan = page_runtime_fixture_manifest()
            .page_runtime_plan_with_urls(["lazyquote", "stockticker", "counter"], &FixtureAssetUrls)
            .unwrap();
        let nonce = proa_core::CspNonce::new("cGFnZS1ub25jZQ==").unwrap();
        let html = plan.to_html(Some(&nonce));

        let expected_in_order = [
            r#"<script type="module" src="/assets/runtime.core.js?build=7&amp;debug=0" nonce="cGFnZS1ub25jZQ=="></script>"#,
            r#"<script type="module" src="/assets/runtime.query.js" nonce="cGFnZS1ub25jZQ=="></script>"#,
            r#"<link rel="modulepreload" href="/assets/clients/pricing.js">"#,
            r#"<link rel="modulepreload" href="/assets/fragments/calendar_1234.mjs">"#,
            r#"<link rel="modulepreload" href="/assets/islands/counter.mjs">"#,
            r#"<link rel="modulepreload" href="/assets/islands/stockticker.mjs">"#,
            r#"<script type="module" src="/assets/islands/counter.mjs" nonce="cGFnZS1ub25jZQ=="></script>"#,
            r#"<script type="module" src="/assets/islands/stockticker.mjs" nonce="cGFnZS1ub25jZQ=="></script>"#,
            r#"<script type="module" nonce="cGFnZS1ub25jZQ==">"#,
            r#"defer_island_module("lazyquote""#,
        ];
        let mut cursor = 0;
        for expected in expected_in_order {
            let offset = html[cursor..]
                .find(expected)
                .unwrap_or_else(|| panic!("missing ordered fragment `{expected}` in {html}"));
            cursor += offset + expected.len();
        }
        assert_eq!(html.matches(r#" nonce="cGFnZS1ub25jZQ==""#).count(), 5);
        assert_eq!(html, plan.to_html(Some(&nonce)));
    }

    #[test]
    fn interaction_page_module_is_managed_before_trigger_listener_arming() {
        let plan = page_runtime_fixture_manifest()
            .page_runtime_plan_with_urls(["actionmenu"], &FixtureAssetUrls)
            .unwrap();
        let nonce = proa_core::CspNonce::new("aW50ZXJhY3Rpb24=").unwrap();
        let html = plan.to_html(Some(&nonce));

        assert!(
            !html.contains(r#"<link rel="modulepreload" href="/assets/islands/actionmenu.mjs">"#),
            "the unticketed interaction URL cannot satisfy its managed import"
        );
        assert!(html.contains(
            r#"import { hydrate_on_interaction, load_island_module_for_strategy } from "/assets/runtime.core.js?build=7\u0026debug=0";"#
        ));
        assert!(html.contains(
            r#"load_island_module_for_strategy("actionmenu", null, "/assets/islands/actionmenu.mjs", hydrate_on_interaction, ["pointerdown"]);"#
        ));
        assert!(!html.contains(r#"<script type="module" src="/assets/islands/actionmenu.mjs""#));
        assert_eq!(html.matches(r#" nonce="aW50ZXJhY3Rpb24=""#).count(), 2);
    }

    #[test]
    fn page_runtime_plan_delta_dedupes_shell_and_streamed_boundaries() {
        let manifest = page_runtime_fixture_manifest();
        let shell = manifest
            .page_runtime_plan(["lazyquote", "counter"])
            .unwrap();
        let boundary = manifest
            .page_runtime_plan(["stockticker", "savequeue", "lazyquote", "counter"])
            .unwrap();
        let nonce = proa_core::CspNonce::new("c3RyZWFtLW5vbmNl").unwrap();
        let mut state = PageRuntimeEmissionState::new();

        let shell_html = shell.to_html_delta(&mut state, Some(&nonce));
        assert!(!state.is_empty());
        let boundary_html = boundary.to_html_delta(&mut state, Some(&nonce));
        let duplicate_html = boundary.to_html_delta(&mut state, Some(&nonce));
        let combined = format!("{shell_html}{boundary_html}");

        assert_eq!(duplicate_html, "");
        assert_eq!(
            combined
                .matches(r#"<script type="module" src="/rsjs-runtime.js""#)
                .count(),
            1
        );
        assert_eq!(
            combined
                .matches(r#"<script type="module" src="/rsjs-runtime-query.js""#)
                .count(),
            1
        );
        assert_eq!(
            combined
                .matches(r#"<link rel="modulepreload" href="/rsjs/island/counter.js">"#)
                .count(),
            1
        );
        assert_eq!(
            combined
                .matches(r#"<link rel="modulepreload" href="/rsjs/island/pricing.js">"#)
                .count(),
            1
        );
        assert_eq!(
            combined
                .matches(r#"<script type="module" src="/rsjs/island/counter.js""#)
                .count(),
            1
        );
        assert_eq!(
            combined
                .matches(r#"defer_island_module("lazyquote""#)
                .count(),
            1
        );
        assert_eq!(
            combined
                .matches(r#"defer_island_module("savequeue""#)
                .count(),
            1
        );
        assert!(!boundary_html.contains("/rsjs/island/counter.js"));
        assert!(!boundary_html.contains(r#"defer_island_module("lazyquote""#));
        assert!(boundary_html.contains("/rsjs/island/stockticker.js"));
        assert!(boundary_html.contains(r#"defer_island_module("savequeue""#));

        assert_eq!(shell.to_html(Some(&nonce)), shell.to_html(Some(&nonce)));
    }

    #[test]
    fn runtime_assets_omits_query_module_when_no_island_uses_queries() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let r = m.runtime();
        assert_eq!(r.base, "/rsjs-runtime.js");
        assert!(
            r.query.is_none(),
            "expected query-runtime to be omitted when no island uses_queries"
        );
    }

    #[test]
    fn runtime_assets_includes_query_module_when_any_island_uses_queries() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND, &FIXTURE_WITH_QUERY],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let r = m.runtime();
        assert_eq!(r.base, "/rsjs-runtime.js");
        assert_eq!(r.query.as_deref(), Some("/rsjs-runtime-query.js"));
    }

    #[test]
    fn manifest_json_uses_queries_round_trips() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND, &FIXTURE_WITH_QUERY],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let j = manifest_json(&m);
        // Counter has no queries; StockTicker does. Both fields
        // serialise with the canonical JSON-boolean spelling.
        assert!(
            j.contains(r#""asset_id":"counter""#) && j.contains(r#""uses_queries":false"#),
            "Counter must serialise uses_queries=false; got: {j}"
        );
        assert!(
            j.contains(r#""asset_id":"stockticker""#) && j.contains(r#""uses_queries":true"#),
            "StockTicker must serialise uses_queries=true; got: {j}"
        );
    }

    #[test]
    fn runtime_assets_includes_query_module_when_any_island_uses_mutations() {
        // M9: a page with a mutation-only island still needs the
        // query runtime module (mutations and queries share the
        // bundle). The flag flows through `uses_mutations`.
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND, &FIXTURE_WITH_MUTATION],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let r = m.runtime();
        assert_eq!(r.base, "/rsjs-runtime.js");
        assert_eq!(r.query.as_deref(), Some("/rsjs-runtime-query.js"));
    }

    #[test]
    fn runtime_for_islands_omits_runtime_for_empty_rendered_set() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND, &FIXTURE_WITH_QUERY],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let r = m.runtime_for_islands(std::iter::empty::<&str>()).unwrap();
        assert_eq!(r, None);
    }

    #[test]
    fn runtime_for_islands_uses_rendered_island_features_only() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND, &FIXTURE_WITH_QUERY, &FIXTURE_WITH_MUTATION],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };

        let counter_only = m.runtime_for_islands(["counter"]).unwrap().unwrap();
        assert_eq!(counter_only.base, "/rsjs-runtime.js");
        assert!(
            counter_only.query.is_none(),
            "unrendered query/mutation islands must not force the query runtime"
        );

        let stock = m.runtime_for_islands(["stockticker"]).unwrap().unwrap();
        assert_eq!(stock.base, "/rsjs-runtime.js");
        assert_eq!(stock.query.as_deref(), Some("/rsjs-runtime-query.js"));

        let mutation = m.runtime_for_islands(["updatecart"]).unwrap().unwrap();
        assert_eq!(mutation.query.as_deref(), Some("/rsjs-runtime-query.js"));
    }

    #[test]
    fn runtime_for_islands_with_prefix_uses_rendered_set() {
        let m = Manifest {
            islands: vec![&FIXTURE_WITH_QUERY],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let r = m
            .runtime_for_islands_with_prefix("/_static/rsjs", ["stockticker"])
            .unwrap()
            .unwrap();
        assert_eq!(r.base, "/_static/rsjs/rsjs-runtime.js");
        assert_eq!(
            r.query.as_deref(),
            Some("/_static/rsjs/rsjs-runtime-query.js")
        );
    }

    #[test]
    fn runtime_for_islands_reports_unknown_rendered_id() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let err = m
            .runtime_for_islands(["missing"])
            .expect_err("unknown rendered island ids should be errors");
        assert_eq!(err.asset_id, "missing");
        assert!(
            err.to_string()
                .contains("not found in the runtime manifest"),
            "{err}"
        );
    }

    #[test]
    fn runtime_with_prefix_prepends_user_supplied_prefix() {
        // Round-3 review #6. Framework adapters mounted under a
        // sub-path (e.g. /_static/rsjs/) need a way to override the
        // root-mounted defaults. The helper joins prefix + filename
        // via a single `/`, collapsing duplicate trailing slashes.
        let m = Manifest {
            islands: vec![&FIXTURE_WITH_QUERY],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        // Prefix without trailing slash gets one inserted.
        let r = m.runtime_with_prefix("/_static/rsjs");
        assert_eq!(r.base, "/_static/rsjs/rsjs-runtime.js");
        assert_eq!(
            r.query.as_deref(),
            Some("/_static/rsjs/rsjs-runtime-query.js"),
        );
        // Prefix with trailing slash does not produce a doubled slash.
        let r = m.runtime_with_prefix("/_static/rsjs/");
        assert_eq!(r.base, "/_static/rsjs/rsjs-runtime.js");
        assert_eq!(
            r.query.as_deref(),
            Some("/_static/rsjs/rsjs-runtime-query.js"),
        );
        // Empty prefix is treated as "/" via the trailing-slash path:
        // the helper does not normalise to root, callers must pass "/".
        let r = m.runtime_with_prefix("/");
        assert_eq!(r.base, "/rsjs-runtime.js");
    }

    #[test]
    fn manifest_json_uses_mutations_round_trips() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND, &FIXTURE_WITH_MUTATION],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let j = manifest_json(&m);
        assert!(
            j.contains(r#""asset_id":"counter""#) && j.contains(r#""uses_mutations":false"#),
            "Counter must serialise uses_mutations=false; got: {j}"
        );
        assert!(
            j.contains(r#""asset_id":"updatecart""#) && j.contains(r#""uses_mutations":true"#),
            "UpdateCart must serialise uses_mutations=true; got: {j}"
        );
    }

    static FIXTURE_ISLAND_Z: IslandManifestEntry = IslandManifestEntry {
        asset_id: "zzz",
        struct_name: "Zebra",
        js_source: "",
        is_async: false,
        hydration_strategy: IslandHydrationStrategy::Immediate,
        client_module_imports: &[],
        component_fragment_imports: &[],
        uses_queries: false,
        uses_mutations: false,
    };

    static FIXTURE_ISLAND_A: IslandManifestEntry = IslandManifestEntry {
        asset_id: "aaa",
        struct_name: "Alpha",
        js_source: "",
        is_async: false,
        hydration_strategy: IslandHydrationStrategy::Immediate,
        client_module_imports: &[],
        component_fragment_imports: &[],
        uses_queries: false,
        uses_mutations: false,
    };

    static FIXTURE_ISLAND_A_DUP: IslandManifestEntry = IslandManifestEntry {
        asset_id: "aaa",
        struct_name: "AlphaClone",
        js_source: "",
        is_async: false,
        hydration_strategy: IslandHydrationStrategy::Immediate,
        client_module_imports: &[],
        component_fragment_imports: &[],
        uses_queries: false,
        uses_mutations: false,
    };

    static FIXTURE_BUNDLE_Z: ClientModuleManifestEntry = ClientModuleManifestEntry {
        module_path: "crate::zeta",
        helpers: &[],
    };

    static FIXTURE_BUNDLE_A: ClientModuleManifestEntry = ClientModuleManifestEntry {
        module_path: "crate::alpha",
        helpers: &[],
    };

    static FIXTURE_BUNDLE_ALPHA_FILE_COLLISION: ClientModuleManifestEntry =
        ClientModuleManifestEntry {
            module_path: "other::alpha",
            helpers: &[],
        };

    #[test]
    fn manifest_sorts_islands_and_client_modules_by_key() {
        // Build two manifests with identical contents but different
        // input orderings; assert they sort to the same order so the
        // serialised output is byte-for-byte reproducible across
        // compilations. We can't trigger actual linkme reordering
        // from a unit test, so we verify the in-`manifest()` sort
        // contract through a hand-built equivalent.
        let mut a_islands = vec![&FIXTURE_ISLAND_Z, &FIXTURE_ISLAND_A];
        let mut b_islands = vec![&FIXTURE_ISLAND_A, &FIXTURE_ISLAND_Z];
        a_islands.sort_by_key(|i| i.asset_id);
        b_islands.sort_by_key(|i| i.asset_id);
        let mut a_modules = vec![&FIXTURE_BUNDLE_Z, &FIXTURE_BUNDLE_A];
        let mut b_modules = vec![&FIXTURE_BUNDLE_A, &FIXTURE_BUNDLE_Z];
        a_modules.sort_by_key(|m| m.module_path);
        b_modules.sort_by_key(|m| m.module_path);
        let a = Manifest {
            islands: a_islands,
            client_modules: a_modules,
            client_classes: Vec::new(),
            component_fragments: vec![&FIXTURE_FRAGMENT],
        };
        let b = Manifest {
            islands: b_islands,
            client_modules: b_modules,
            client_classes: Vec::new(),
            component_fragments: vec![&FIXTURE_FRAGMENT],
        };
        assert_eq!(a, b);
        // And verify the actual ordering is the alphabetical one,
        // not the insertion order.
        assert_eq!(a.islands[0].asset_id, "aaa");
        assert_eq!(a.islands[1].asset_id, "zzz");
        assert_eq!(a.client_modules[0].module_path, "crate::alpha");
        assert_eq!(a.client_modules[1].module_path, "crate::zeta");
    }

    #[test]
    #[should_panic(expected = "duplicate island asset id `aaa`")]
    fn manifest_validation_rejects_duplicate_island_asset_ids() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND_A, &FIXTURE_ISLAND_A_DUP],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        assert_manifest_keys_unique(&m);
    }

    #[test]
    #[should_panic(expected = "duplicate client module asset filename `alpha.js`")]
    fn manifest_validation_rejects_client_module_filename_collisions() {
        let m = Manifest {
            islands: Vec::new(),
            client_modules: vec![&FIXTURE_BUNDLE_A, &FIXTURE_BUNDLE_ALPHA_FILE_COLLISION],
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        assert_manifest_keys_unique(&m);
    }

    #[test]
    fn manifest_validation_accepts_resolved_import_edges() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND],
            client_modules: vec![&FIXTURE_BUNDLE],
            client_classes: Vec::new(),
            component_fragments: vec![&FIXTURE_FRAGMENT],
        };
        assert_manifest_keys_unique(&m);
    }

    #[test]
    fn reachable_client_modules_include_only_island_import_edges() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND],
            client_modules: vec![&FIXTURE_BUNDLE_A, &FIXTURE_BUNDLE],
            client_classes: Vec::new(),
            component_fragments: vec![&FIXTURE_FRAGMENT],
        };

        let reachable = m.reachable_client_modules();
        assert_eq!(reachable.len(), 1);
        assert_eq!(reachable[0].module_path, "crate::pricing");
    }

    #[test]
    fn reachable_client_modules_match_import_edges_by_asset_filename() {
        let m = Manifest {
            islands: vec![&FIXTURE_SELF_IMPORT_ISLAND],
            client_modules: vec![&FIXTURE_BUNDLE],
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        assert_manifest_keys_unique(&m);

        let reachable = m.reachable_client_modules();
        assert_eq!(reachable.len(), 1);
        assert_eq!(reachable[0].module_path, "crate::pricing");
    }

    #[test]
    #[should_panic(
        expected = "missing client module `crate::pricing` imported by island `counter`"
    )]
    fn manifest_validation_rejects_missing_client_module_imports() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND],
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: vec![&FIXTURE_FRAGMENT],
        };
        assert_manifest_keys_unique(&m);
    }

    #[test]
    #[should_panic(
        expected = "missing component fragment asset `calendar_1234` imported by island `counter`"
    )]
    fn manifest_validation_rejects_missing_component_fragment_imports() {
        let m = Manifest {
            islands: vec![&FIXTURE_ISLAND],
            client_modules: vec![&FIXTURE_BUNDLE],
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        assert_manifest_keys_unique(&m);
    }

    #[test]
    fn manifest_json_serialises_client_modules() {
        let m = Manifest {
            islands: Vec::new(),
            client_modules: vec![&FIXTURE_BUNDLE],
            client_classes: Vec::new(),
            component_fragments: Vec::new(),
        };
        let j = manifest_json(&m);
        assert!(
            j.contains(r#""client_modules":[{"module_path":"crate::pricing"}]"#),
            "got: {j}"
        );
    }

    #[test]
    fn manifest_json_and_filename_serialise_client_classes() {
        let m = Manifest {
            islands: Vec::new(),
            client_modules: Vec::new(),
            client_classes: vec![&FIXTURE_CLIENT_CLASS],
            component_fragments: Vec::new(),
        };
        let j = manifest_json(&m);
        assert!(
            j.contains(
                r#""client_classes":[{"module_path":"crate::logic::counter","export_names":["Counter"]}]"#
            ),
            "got: {j}"
        );
        let filename = client_class_filename(FIXTURE_CLIENT_CLASS.module_path);
        assert!(filename.starts_with("client_class_counter_"), "{filename}");
        assert!(filename.ends_with(".js"), "{filename}");
        assert_eq!(
            filename,
            client_class_filename(FIXTURE_CLIENT_CLASS.module_path),
            "class asset filenames must be deterministic"
        );
        assert_ne!(
            filename,
            client_class_filename("other::logic::counter"),
            "same-named modules under different parents must not collide"
        );
    }

    #[test]
    fn client_module_js_source_stitches_helpers_from_metadata() {
        assert_eq!(
            client_module_filename(FIXTURE_METADATA_BUNDLE.module_path),
            "pricing.js"
        );
        let js = client_module_js_source(&FIXTURE_METADATA_BUNDLE);
        assert!(
            js.contains("function __fn_0(price, qty) {\n    return (price * qty);\n}"),
            "got: {js}"
        );
        assert!(
            js.contains("export { __fn_0 as pricing__subtotal };"),
            "got: {js}"
        );
    }

    #[test]
    fn client_module_js_source_parses_helper_local_lets() {
        let js = client_module_js_source(&FIXTURE_METADATA_BLOCK_BUNDLE);
        assert!(
            js.contains(
                "function __fn_0(price, qty) {\n    let __local_2 = (price * qty);\n    return (__local_2 + 5);\n}"
            ),
            "got: {js}"
        );
        assert!(
            js.contains("export { __fn_0 as pricing__adjusted_subtotal };"),
            "got: {js}"
        );
    }

    #[test]
    #[should_panic(expected = "unsupported helper body_ir_version 2")]
    fn client_module_js_source_rejects_stale_v2_helper_metadata() {
        let _ = client_module_js_source(&FIXTURE_OLD_IR_VERSION_BUNDLE);
    }

    #[test]
    fn manifest_json_serialises_component_fragments() {
        let m = Manifest {
            islands: Vec::new(),
            client_modules: Vec::new(),
            client_classes: Vec::new(),
            component_fragments: vec![&FIXTURE_FRAGMENT],
        };
        let j = manifest_json(&m);
        assert!(
            j.contains(
                r#""component_fragments":[{"asset_id":"calendar_1234","struct_name":"Calendar","export_name":"mount_calendar_1234"}]"#
            ),
            "got: {j}"
        );
    }

    #[test]
    fn component_fragment_filename_is_namespaced() {
        assert_eq!(
            component_fragment_filename("calendar_1234"),
            "component_calendar_1234.js"
        );
    }

    #[test]
    fn write_manifest_assets_removes_orphan_tmp_files() {
        // Simulate a SIGKILL'd previous run that left `*.tmp.*`
        // staging files behind. The cleanup pass removes only the
        // ones whose mtime is past ORPHAN_TMP_MAX_AGE; a fresh tmp
        // file from a "concurrent peer" must survive. Non-`.tmp.`
        // foreign files (e.g. `manifest.json`) must always survive.
        use std::time::SystemTime;

        let dir = std::env::temp_dir().join(format!(
            "rsjs_manifest_tmp_orphans_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("setup mkdir");
        // Opt in to cleanup; without this marker the helper would
        // refuse to delete files in the (populated) test directory.
        fs::write(dir.join(".rsjs-managed"), b"").expect("setup marker");

        let stale_orphan = dir.join("counter.js.tmp.1234.5.0");
        fs::write(&stale_orphan, b"/* aborted staging */").expect("setup write");
        // Backdate the orphan to two hours ago so it's well past the
        // 60s threshold regardless of clock skew or test scheduling.
        let two_hours_ago = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        fs::File::options()
            .write(true)
            .open(&stale_orphan)
            .expect("open stale orphan")
            .set_modified(two_hours_ago)
            .expect("backdate stale orphan");

        let fresh_peer = dir.join("counter.js.tmp.9999.0.0");
        fs::write(&fresh_peer, b"/* peer mid-write */").expect("setup write");

        let foreign = dir.join("manifest.json");
        fs::write(&foreign, b"{}").expect("setup write");

        write_manifest_assets(&dir).expect("write_manifest_assets failed");

        assert!(
            !stale_orphan.exists(),
            "expected stale orphan tmp file to be removed"
        );
        assert!(
            fresh_peer.exists(),
            "fresh tmp file (mid-write peer) must not be removed"
        );
        assert!(
            foreign.exists(),
            "non-tmp file (manifest.json) must not be touched"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_manifest_assets_removes_stale_js_files() {
        // The unit-test binary has no RSJS-analyzed components registered, so
        // `ISLAND_REGISTRY` and `CLIENT_MODULE_REGISTRY` are empty.
        // Every `.js` file we pre-seed in the directory should
        // therefore be removed; non-`.js` files must survive.
        let dir = std::env::temp_dir().join(format!(
            "rsjs_manifest_assets_clean_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("setup mkdir");
        // Opt in to cleanup; without this marker the helper would
        // refuse to delete files in the (populated) test directory.
        fs::write(dir.join(".rsjs-managed"), b"").expect("setup marker");

        let stale_js = dir.join("ghost.js");
        fs::write(&stale_js, b"/* stale island js */").expect("setup write");
        let foreign = dir.join("manifest.json");
        fs::write(&foreign, b"{}").expect("setup write");
        let foreign_map = dir.join("ghost.js.map");
        fs::write(&foreign_map, b"{}").expect("setup write");

        write_manifest_assets(&dir).expect("write_manifest_assets failed");

        assert!(!stale_js.exists(), "expected stale .js file to be removed");
        assert!(
            foreign.exists(),
            "non-.js file (manifest.json) must not be touched"
        );
        assert!(
            foreign_map.exists(),
            "non-.js file (.js.map) must not be touched"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_manifest_assets_creates_marker_on_first_run_in_empty_dir() {
        // Fresh empty directory: the helper claims it by creating the
        // marker and (since the registry is empty) writes nothing else.
        let dir = std::env::temp_dir().join(format!(
            "rsjs_marker_first_run_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("setup mkdir");

        write_manifest_assets(&dir).expect("write_manifest_assets failed");

        assert!(
            dir.join(".rsjs-managed").exists(),
            "expected .rsjs-managed marker to be created on first run"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_manifest_assets_skips_cleanup_without_marker() {
        // Populated directory without a marker: the helper must not
        // touch the user's foreign `.js` files. This guards against
        // accidentally pointing the collector at a non-rsjs dist dir.
        let dir = std::env::temp_dir().join(format!(
            "rsjs_marker_no_cleanup_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("setup mkdir");

        let user_polyfill = dir.join("polyfill.js");
        fs::write(&user_polyfill, b"// user polyfill").expect("setup write");
        let user_app_shell = dir.join("app-shell.js");
        fs::write(&user_app_shell, b"// user app shell").expect("setup write");

        write_manifest_assets(&dir).expect("write_manifest_assets failed");

        assert!(
            user_polyfill.exists(),
            "user-placed polyfill.js must survive when marker is absent"
        );
        assert!(
            user_app_shell.exists(),
            "user-placed app-shell.js must survive when marker is absent"
        );
        // Marker must NOT be created when the dir already had foreign content.
        assert!(
            !dir.join(".rsjs-managed").exists(),
            "marker must not be created when populated dir has no marker"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_manifest_assets_runs_cleanup_when_marker_present() {
        // Populated directory WITH a marker: the helper owns the dir
        // and removes stale `.js` files. Foreign non-`.js` files still
        // survive (they are not in the manifest's responsibility).
        let dir = std::env::temp_dir().join(format!(
            "rsjs_marker_cleanup_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("setup mkdir");
        fs::write(dir.join(".rsjs-managed"), b"").expect("setup marker");

        let stale_js = dir.join("ghost.js");
        fs::write(&stale_js, b"// stale").expect("setup write");

        write_manifest_assets(&dir).expect("write_manifest_assets failed");

        assert!(
            !stale_js.exists(),
            "stale .js file must be removed when marker is present"
        );
        assert!(
            dir.join(".rsjs-managed").exists(),
            "marker must survive the cleanup pass"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
