// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa rsjs`: inspection tools for rsjs compiler artifacts.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct StrategyExplanation {
    pub source: StrategySource,
    pub strategy_mode: String,
    pub render_strategy: String,
    /// Whether this component may originate browser behavior. Absent only for
    /// legacy strategy sidecars written before the permission field existed.
    pub client_origin_permission: Option<String>,
    pub hydration_strategy: Option<ExplainedHydrationStrategy>,
    pub declared_client_modules: Vec<String>,
    pub client_modules: Vec<String>,
    pub unused_client_modules: Vec<String>,
    pub component_fragment_imports: Vec<String>,
    pub runtime_features: Vec<String>,
    pub shared_state_dependencies: Vec<StrategySharedStateDependency>,
    pub server_surfaces: Vec<ExplainedServerSurface>,
    pub crossings: Vec<ExplainedClientCrossing>,
    pub dead_primitives: Vec<ExplainedDeadPrimitive>,
    pub summary: String,
    pub roots: Vec<ExplainedRoot>,
    pub emitted: ExplainedEmission,
    pub next_actions: Vec<String>,
    pub sidecar_path: String,
    /// Set when the anchored source no longer matches the sidecar's input
    /// fingerprint (or cannot be found), meaning this analysis is outdated.
    pub staleness: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategySource {
    pub analysis_id: String,
    pub runtime_asset_id: Option<String>,
    pub struct_name: String,
    pub struct_path: String,
    pub input_fingerprint: Option<String>,
    pub source_anchor: Option<StrategySourceAnchor>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategySourceAnchor {
    pub file: Option<String>,
    pub impl_start_line: Option<usize>,
    pub impl_start_column: Option<usize>,
    pub render_start_line: Option<usize>,
    pub render_start_column: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainedRoot {
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainedServerSurface {
    pub kind: String,
    pub source: Option<String>,
    pub source_anchor: Option<StrategyServerSurfaceAnchor>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainedClientCrossing {
    pub kind: String,
    pub label: Option<String>,
    pub module: Option<String>,
    pub name: Option<String>,
    pub arg_index: Option<u64>,
    pub asset_id: Option<String>,
    pub prop: Option<String>,
    pub root: Option<u64>,
    pub child_signal: Option<u64>,
    pub parent_signal: Option<u64>,
    pub signal: Option<u64>,
    pub query: Option<u64>,
    pub mutation: Option<u64>,
    pub invalidation_index: Option<u64>,
    pub node: Option<u64>,
    pub attr: Option<String>,
    pub surface: Option<String>,
    pub requirement: Option<String>,
    pub source_anchor: Option<StrategyClientCrossingAnchor>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainedDeadPrimitive {
    pub kind: String,
    pub label: Option<String>,
    pub signal: Option<u64>,
    pub handler: Option<u64>,
    pub node_ref: Option<u64>,
    pub query: Option<u64>,
    pub mutation: Option<u64>,
    pub source_anchor: Option<StrategyDeadPrimitiveAnchor>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainedHydrationStrategy {
    pub kind: String,
    pub label: String,
    pub ignored: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainedEmission {
    pub wrapper: bool,
    pub seed_script: bool,
    pub manifest_registration: bool,
    pub js_asset: bool,
    pub component_fragment_asset: bool,
    pub uses_queries: bool,
    pub uses_mutations: bool,
    pub uses_shared_state: bool,
}

#[derive(Debug, Clone)]
pub struct StrategySidecarSummary {
    pub analysis_id: String,
    pub runtime_asset_id: Option<String>,
    pub struct_name: String,
    pub struct_path: String,
    pub input_fingerprint: Option<String>,
    pub source_anchor: Option<StrategySourceAnchor>,
    pub render_strategy: String,
    pub client_origin_permission: Option<String>,
    pub hydration_strategy_kind: Option<String>,
    pub hydration_strategy_label: Option<String>,
    pub declared_client_modules: Vec<String>,
    pub client_modules: Vec<String>,
    pub unused_client_modules: Vec<String>,
    pub component_fragment_imports: Vec<String>,
    pub runtime_features: Vec<String>,
    pub shared_state_dependencies: Vec<StrategySharedStateDependency>,
    pub server_surfaces: Vec<StrategyServerSurfaceSummary>,
    pub crossings: Vec<StrategyClientCrossingSummary>,
    pub dead_primitives: Vec<StrategyDeadPrimitiveSummary>,
    pub component_fragment_mounts: Vec<StrategyFragmentMountSummary>,
    pub sidecar_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyFragmentMountSummary {
    pub root: u64,
    pub asset_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StrategySharedStateDependency {
    pub store: u64,
    pub rust_type: String,
    pub type_key: Option<String>,
    /// Fields this island actually subscribes to.
    pub fields: Vec<String>,
    /// Every field serialized by the provider snapshot, including fields
    /// this island never reads.
    pub serialized_fields: Vec<String>,
    pub provider_serializes_full_snapshot: bool,
    pub source_anchor: Option<StrategyClientCrossingAnchor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyServerSurfaceSummary {
    pub kind: String,
    pub source: Option<String>,
    pub node: Option<u64>,
    pub attr: Option<String>,
    pub reason: Option<String>,
    pub source_anchor: Option<StrategyServerSurfaceAnchor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StrategyServerSurfaceAnchor {
    pub start_line: usize,
    pub start_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StrategyClientCrossingSummary {
    pub kind: String,
    pub label: Option<String>,
    pub module: Option<String>,
    pub name: Option<String>,
    pub arg_index: Option<u64>,
    pub asset_id: Option<String>,
    pub prop: Option<String>,
    pub root: Option<u64>,
    pub child_signal: Option<u64>,
    pub parent_signal: Option<u64>,
    pub signal: Option<u64>,
    pub query: Option<u64>,
    pub mutation: Option<u64>,
    pub invalidation_index: Option<u64>,
    pub node: Option<u64>,
    pub attr: Option<String>,
    pub surface: Option<String>,
    pub requirement: Option<String>,
    pub source_anchor: Option<StrategyClientCrossingAnchor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyDeadPrimitiveSummary {
    pub kind: String,
    pub label: Option<String>,
    pub signal: Option<u64>,
    pub handler: Option<u64>,
    pub node_ref: Option<u64>,
    pub query: Option<u64>,
    pub mutation: Option<u64>,
    pub source_anchor: Option<StrategyDeadPrimitiveAnchor>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrossingReport {
    pub root: String,
    pub selector: Option<String>,
    pub sidecar_count: usize,
    pub crossing_count: usize,
    pub crossings: Vec<CrossingReportEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrossingReportEntry {
    pub sidecar_path: String,
    pub analysis_id: String,
    pub runtime_asset_id: Option<String>,
    pub struct_name: String,
    pub struct_path: String,
    pub render_strategy: String,
    #[serde(flatten)]
    pub crossing: StrategyClientCrossingSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StrategyClientCrossingAnchor {
    pub start_line: usize,
    pub start_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StrategyDeadPrimitiveAnchor {
    pub start_line: usize,
    pub start_column: usize,
}

#[derive(Debug, Deserialize)]
struct StrategySidecar {
    format: String,
    strategy_version: u64,
    strategy_mode: String,
    analysis_id: String,
    #[serde(default)]
    runtime_asset_id: Option<String>,
    struct_name: String,
    struct_path: String,
    #[serde(default)]
    input_fingerprint: Option<String>,
    #[serde(default)]
    source: Option<StrategySourceSidecar>,
    render_strategy: String,
    #[serde(default)]
    client_origin_permission: Option<String>,
    #[serde(default)]
    hydration_strategy: Option<HydrationStrategySidecar>,
    #[serde(default)]
    declared_client_modules: Vec<String>,
    #[serde(default)]
    client_modules: Vec<String>,
    #[serde(default)]
    component_fragment_imports: Vec<String>,
    #[serde(default)]
    runtime_features: Vec<String>,
    #[serde(default)]
    shared_state_dependencies: Vec<SharedStateDependencySidecar>,
    #[serde(default)]
    server_surfaces: Vec<ServerSurfaceSidecar>,
    #[serde(default)]
    crossings: Vec<ClientCrossingSidecar>,
    #[serde(default)]
    dead_primitives: Vec<DeadPrimitiveSidecar>,
    #[serde(default)]
    roots: Vec<StrategyRoot>,
    #[serde(default)]
    emitted: EmittedSidecar,
}

#[derive(Debug, Clone, Deserialize)]
struct StrategySourceSidecar {
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    impl_start_line: Option<usize>,
    #[serde(default)]
    impl_start_column: Option<usize>,
    #[serde(default)]
    render_start_line: Option<usize>,
    #[serde(default)]
    render_start_column: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct SharedStateDependencySidecar {
    store: u64,
    rust_type: String,
    #[serde(default)]
    type_key: Option<String>,
    #[serde(default)]
    fields: Vec<String>,
    #[serde(default)]
    serialized_fields: Vec<String>,
    #[serde(default)]
    provider_serializes_full_snapshot: bool,
    #[serde(default)]
    source: Option<ClientCrossingAnchorSidecar>,
}

#[derive(Debug, Clone, Deserialize)]
struct HydrationStrategySidecar {
    kind: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StrategyRoot {
    kind: String,
    #[serde(default)]
    node: Option<u64>,
    #[serde(default)]
    parent: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    dom_event: Option<String>,
    #[serde(default)]
    node_ref: Option<u64>,
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    index: Option<u64>,
    #[serde(default)]
    container: Option<u64>,
    #[serde(default)]
    root: Option<u64>,
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default)]
    read: Option<String>,
    #[serde(default)]
    effect: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ServerSurfaceSidecar {
    kind: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    node: Option<u64>,
    #[serde(default)]
    attr: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    source_anchor: Option<ServerSurfaceAnchorSidecar>,
}

#[derive(Debug, Clone, Deserialize)]
struct ServerSurfaceAnchorSidecar {
    start_line: usize,
    start_column: usize,
}

#[derive(Debug, Deserialize)]
struct ClientCrossingSidecar {
    kind: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arg_index: Option<u64>,
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default)]
    prop: Option<String>,
    #[serde(default)]
    root: Option<u64>,
    #[serde(default)]
    child_signal: Option<u64>,
    #[serde(default)]
    parent_signal: Option<u64>,
    #[serde(default)]
    signal: Option<u64>,
    #[serde(default)]
    query: Option<u64>,
    #[serde(default)]
    mutation: Option<u64>,
    #[serde(default)]
    invalidation_index: Option<u64>,
    #[serde(default)]
    node: Option<u64>,
    #[serde(default)]
    attr: Option<String>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    requirement: Option<String>,
    #[serde(default)]
    source_anchor: Option<ClientCrossingAnchorSidecar>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientCrossingAnchorSidecar {
    start_line: usize,
    start_column: usize,
}

#[derive(Debug, Deserialize)]
struct DeadPrimitiveSidecar {
    kind: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    signal: Option<u64>,
    #[serde(default)]
    handler: Option<u64>,
    #[serde(default)]
    node_ref: Option<u64>,
    #[serde(default)]
    query: Option<u64>,
    #[serde(default)]
    mutation: Option<u64>,
    #[serde(default)]
    source_anchor: Option<DeadPrimitiveAnchorSidecar>,
}

#[derive(Debug, Clone, Deserialize)]
struct DeadPrimitiveAnchorSidecar {
    start_line: usize,
    start_column: usize,
}

#[derive(Debug, Default, Deserialize)]
struct EmittedSidecar {
    #[serde(default)]
    wrapper: bool,
    #[serde(default)]
    seed_script: bool,
    #[serde(default)]
    manifest_registration: bool,
    #[serde(default)]
    js_asset: bool,
    #[serde(default)]
    component_fragment_asset: bool,
    #[serde(default)]
    uses_queries: bool,
    #[serde(default)]
    uses_mutations: bool,
    #[serde(default)]
    uses_shared_state: bool,
}

pub fn explain_strategy(root: &Path, selector: &str) -> Result<StrategyExplanation> {
    let sidecar_path = find_strategy_sidecar(root, selector)?;
    let text = std::fs::read_to_string(&sidecar_path)
        .with_context(|| format!("read {}", sidecar_path.display()))?;
    let sidecar: StrategySidecar =
        serde_json::from_str(&text).with_context(|| format!("parse {}", sidecar_path.display()))?;
    validate_sidecar(&sidecar, &sidecar_path)?;
    let staleness = sidecar_staleness(root, &sidecar);

    let roots: Vec<_> = sidecar.roots.iter().map(explain_root).collect();
    let render_strategy = human_strategy(&sidecar.render_strategy).to_string();
    let hydration_strategy = sidecar.hydration_strategy.as_ref().map(|strategy| {
        explain_hydration_strategy(strategy, sidecar.render_strategy == "server_only")
    });
    let declared_client_modules = sidecar.declared_client_modules.clone();
    let client_modules = sidecar.client_modules.clone();
    let unused_client_modules = unused_client_modules(&declared_client_modules, &client_modules);
    let component_fragment_imports = sidecar_component_fragment_imports(&sidecar);
    let runtime_features = sidecar_runtime_features(&sidecar);
    let shared_state_dependencies = sidecar
        .shared_state_dependencies
        .iter()
        .map(strategy_shared_state_dependency)
        .collect();
    let server_surfaces: Vec<_> = sidecar
        .server_surfaces
        .iter()
        .map(explain_server_surface)
        .collect();
    let crossings: Vec<_> = sidecar
        .crossings
        .iter()
        .map(explain_client_crossing)
        .collect();
    let dead_primitives: Vec<_> = sidecar
        .dead_primitives
        .iter()
        .map(explain_dead_primitive)
        .collect();
    let summary = if sidecar.render_strategy == "server_only" {
        let mut summary = format!(
            "{} is ServerOnly: no reachable client roots were recorded, so no island JS should be emitted.",
            sidecar.struct_name
        );
        if !declared_client_modules.is_empty() {
            summary.push_str(
                " Client helper grouping metadata is unreachable here; grouping never forces hydration.",
            );
        }
        if !dead_primitives.is_empty() {
            summary.push_str(
                " Declared client primitives are dead here; declarations alone do not force hydration.",
            );
        }
        summary
    } else {
        format!(
            "{} is {}: {} reachable client root(s) require browser JS.",
            sidecar.struct_name,
            render_strategy,
            roots.len()
        )
    };
    let mut next_actions = if sidecar.render_strategy == "server_only" {
        let mut actions = vec![
            "keep #[rsjs(component)] when this type must remain compiler-composable; use an undecorated render trait only for intentionally opaque SSR"
                .to_string(),
        ];
        if sidecar.client_origin_permission.as_deref() == Some("participant_only") {
            actions.push(
                "change to #[rsjs(component, client)] before adding a local signal, handler, query, mutation, shared-state acquisition, or browser capability"
                    .to_string(),
            );
        } else {
            actions.push(
                "add a reachable signal/event/bind/query/ref primitive if browser behavior is intended"
                    .to_string(),
            );
        }
        if hydration_strategy
            .as_ref()
            .is_some_and(|strategy| strategy.ignored)
        {
            actions.push(
                "remove the `strategy = ...` argument because no browser hydration is emitted"
                    .to_string(),
            );
        }
        if !declared_client_modules.is_empty() {
            actions.push(
                "remove obsolete component helper allowlists; annotate each portable origin-free helper once with #[rsjs::client] and call it from reachable Client IR if browser computation is intended"
                    .to_string(),
            );
        }
        if !dead_primitives.is_empty() {
            actions.push(
                "remove dead client primitive declarations or connect them to a reachable template binding/handler"
                    .to_string(),
            );
        }
        actions
    } else {
        vec![
            "review reachable roots to confirm each browser dependency is intentional".to_string(),
            "move opaque SSR-only work behind rsjs::server_only(...) or rsjs::server_attr(...)"
                .to_string(),
        ]
    };
    next_actions.sort();
    next_actions.dedup();

    Ok(StrategyExplanation {
        source: StrategySource {
            analysis_id: sidecar.analysis_id,
            runtime_asset_id: sidecar.runtime_asset_id,
            struct_name: sidecar.struct_name,
            struct_path: sidecar.struct_path,
            input_fingerprint: sidecar.input_fingerprint,
            source_anchor: sidecar.source.map(strategy_source_anchor),
        },
        strategy_mode: sidecar.strategy_mode,
        render_strategy,
        client_origin_permission: sidecar.client_origin_permission,
        hydration_strategy,
        declared_client_modules,
        client_modules,
        unused_client_modules,
        component_fragment_imports,
        runtime_features,
        shared_state_dependencies,
        server_surfaces,
        crossings,
        dead_primitives,
        summary,
        roots,
        emitted: ExplainedEmission {
            wrapper: sidecar.emitted.wrapper,
            seed_script: sidecar.emitted.seed_script,
            manifest_registration: sidecar.emitted.manifest_registration,
            js_asset: sidecar.emitted.js_asset,
            component_fragment_asset: sidecar.emitted.component_fragment_asset,
            uses_queries: sidecar.emitted.uses_queries,
            uses_mutations: sidecar.emitted.uses_mutations,
            uses_shared_state: sidecar.emitted.uses_shared_state,
        },
        next_actions,
        sidecar_path: sidecar_path.display().to_string(),
        staleness,
    })
}

/// Compare the sidecar's recorded input fingerprint against the current
/// source of the anchored file. A mismatch means the analysis predates the
/// latest edit and should not be presented as ground truth.
fn sidecar_staleness(root: &Path, sidecar: &StrategySidecar) -> Option<String> {
    let fingerprint = sidecar.input_fingerprint.as_deref()?;
    let anchor = sidecar.source.as_ref()?;
    let file = anchor.file.as_deref()?;

    let direct = PathBuf::from(file);
    let source_path = if direct.is_absolute() {
        direct
    } else {
        let joined = root.join(file);
        if joined.exists() {
            joined
        } else {
            direct
        }
    };
    if !source_path.exists() {
        return Some(format!(
            "anchored source file `{file}` was not found; re-run the build to refresh target/rsjs/strategy"
        ));
    }

    let source = std::fs::read_to_string(&source_path).ok()?;
    let parsed = syn::parse_file(&source).ok()?;
    let mut fingerprints = Vec::new();
    rsjs_impl_fingerprints(&parsed.items, &sidecar.struct_name, &mut fingerprints);
    if fingerprints.is_empty() {
        return Some(format!(
            "no `#[rsjs(component)]` or `#[rsjs(component, client)]` render impl for `{}` found in `{file}`; re-run the build to refresh target/rsjs/strategy",
            sidecar.struct_name
        ));
    }
    if fingerprints.iter().any(|current| current == fingerprint) {
        return None;
    }
    Some(format!(
        "`{file}` changed since this sidecar was written; re-run the build to refresh target/rsjs/strategy"
    ))
}

fn rsjs_impl_fingerprints(items: &[syn::Item], struct_name: &str, out: &mut Vec<String>) {
    for item in items {
        match item {
            syn::Item::Impl(item_impl) => {
                let is_rsjs = item_impl.attrs.iter().any(|attr| {
                    attr.path()
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "rsjs")
                });
                if !is_rsjs {
                    continue;
                }
                let matches_struct = match item_impl.self_ty.as_ref() {
                    syn::Type::Path(path) => path
                        .path
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == struct_name),
                    _ => false,
                };
                if matches_struct {
                    if let Some(fingerprint) =
                        crate::lint::rsjs_input_fingerprint_for_impl(item_impl)
                    {
                        out.push(fingerprint);
                    }
                }
            }
            syn::Item::Mod(module) => {
                if let Some((_, items)) = &module.content {
                    rsjs_impl_fingerprints(items, struct_name, out);
                }
            }
            _ => {}
        }
    }
}

pub fn strategy_sidecar_summaries(root: &Path) -> Vec<StrategySidecarSummary> {
    let mut summaries = Vec::new();
    for dir in strategy_dirs(root) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if let Ok(summary) = read_strategy_sidecar_summary(&path) {
                summaries.push(summary);
            }
        }
    }
    summaries.sort_by(|left, right| left.sidecar_path.cmp(&right.sidecar_path));
    summaries
}

pub fn crossing_report(root: &Path, selector: Option<&str>) -> Result<CrossingReport> {
    let summaries = if let Some(selector) = selector {
        vec![read_strategy_sidecar_summary(&find_strategy_sidecar(
            root, selector,
        )?)?]
    } else {
        strategy_sidecar_summaries(root)
    };
    let sidecar_count = summaries.len();
    let mut crossings = Vec::new();
    for summary in summaries {
        for crossing in summary.crossings {
            crossings.push(CrossingReportEntry {
                sidecar_path: summary.sidecar_path.display().to_string(),
                analysis_id: summary.analysis_id.clone(),
                runtime_asset_id: summary.runtime_asset_id.clone(),
                struct_name: summary.struct_name.clone(),
                struct_path: summary.struct_path.clone(),
                render_strategy: summary.render_strategy.clone(),
                crossing,
            });
        }
    }
    let crossing_count = crossings.len();
    Ok(CrossingReport {
        root: root.display().to_string(),
        selector: selector.map(ToString::to_string),
        sidecar_count,
        crossing_count,
        crossings,
    })
}

fn read_strategy_sidecar_summary(path: &Path) -> Result<StrategySidecarSummary> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let sidecar: StrategySidecar =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    validate_sidecar(&sidecar, path)?;
    Ok(strategy_sidecar_summary(sidecar, path.to_path_buf()))
}

fn strategy_sidecar_summary(sidecar: StrategySidecar, path: PathBuf) -> StrategySidecarSummary {
    let runtime_features = sidecar_runtime_features(&sidecar);
    let component_fragment_imports = sidecar_component_fragment_imports(&sidecar);
    let shared_state_dependencies = sidecar
        .shared_state_dependencies
        .iter()
        .map(strategy_shared_state_dependency)
        .collect();
    let unused_client_modules =
        unused_client_modules(&sidecar.declared_client_modules, &sidecar.client_modules);
    let server_surfaces = sidecar
        .server_surfaces
        .iter()
        .map(strategy_server_surface_summary)
        .collect();
    let crossings = sidecar
        .crossings
        .iter()
        .map(strategy_client_crossing_summary)
        .collect();
    let dead_primitives = sidecar
        .dead_primitives
        .iter()
        .map(strategy_dead_primitive_summary)
        .collect();
    let component_fragment_mounts = sidecar
        .roots
        .iter()
        .filter_map(strategy_fragment_mount_summary)
        .collect();
    StrategySidecarSummary {
        analysis_id: sidecar.analysis_id,
        runtime_asset_id: sidecar.runtime_asset_id,
        struct_name: sidecar.struct_name,
        struct_path: sidecar.struct_path,
        input_fingerprint: sidecar.input_fingerprint,
        source_anchor: sidecar.source.map(strategy_source_anchor),
        render_strategy: sidecar.render_strategy,
        client_origin_permission: sidecar.client_origin_permission,
        hydration_strategy_kind: sidecar
            .hydration_strategy
            .as_ref()
            .map(|strategy| strategy.kind.clone()),
        hydration_strategy_label: sidecar
            .hydration_strategy
            .as_ref()
            .map(hydration_strategy_label),
        declared_client_modules: sidecar.declared_client_modules,
        client_modules: sidecar.client_modules,
        unused_client_modules,
        component_fragment_imports,
        runtime_features,
        shared_state_dependencies,
        server_surfaces,
        crossings,
        dead_primitives,
        component_fragment_mounts,
        sidecar_path: path,
    }
}

fn strategy_fragment_mount_summary(root: &StrategyRoot) -> Option<StrategyFragmentMountSummary> {
    if root.kind != "component_fragment_mount" {
        return None;
    }
    Some(StrategyFragmentMountSummary {
        root: root.root.unwrap_or_default(),
        asset_id: root
            .asset_id
            .clone()
            .unwrap_or_else(|| "<unknown>".to_string()),
    })
}

pub fn print_strategy_explanation(explanation: &StrategyExplanation, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(explanation)?);
        return Ok(());
    }

    if let Some(staleness) = &explanation.staleness {
        eprintln!("warning: {staleness}");
    }
    println!("RSJS strategy: {}", explanation.render_strategy);
    println!("component: {}", explanation.source.struct_path);
    println!(
        "client origin permission: {}",
        explanation
            .client_origin_permission
            .as_deref()
            .unwrap_or("unknown (legacy sidecar)")
    );
    if let Some(anchor) = &explanation.source.source_anchor {
        match (
            anchor.file.as_deref(),
            anchor.impl_start_line,
            anchor.impl_start_column,
        ) {
            (Some(file), Some(line), Some(column)) => println!("source: {file}:{line}:{column}"),
            (None, Some(line), Some(column)) => println!("source: {line}:{column}"),
            _ => {}
        }
    }
    println!("analysis id: {}", explanation.source.analysis_id);
    if let Some(fingerprint) = &explanation.source.input_fingerprint {
        println!("input fingerprint: {fingerprint}");
    }
    match &explanation.source.runtime_asset_id {
        Some(id) => println!("runtime asset id: {id}"),
        None => println!("runtime asset id: none"),
    }
    if let Some(strategy) = &explanation.hydration_strategy {
        if strategy.ignored {
            println!(
                "hydration strategy: {} (ignored; ServerOnly emitted no JS)",
                strategy.label
            );
        } else {
            println!("hydration strategy: {}", strategy.label);
        }
    }
    println!("mode: {}", explanation.strategy_mode);
    println!("{}", explanation.summary);
    if explanation.declared_client_modules.is_empty() {
        println!("declared client modules: none");
    } else {
        println!("declared client modules:");
        for module in &explanation.declared_client_modules {
            println!("- {module}");
        }
    }
    if explanation.client_modules.is_empty() {
        println!("client modules: none");
    } else {
        println!("client modules:");
        for module in &explanation.client_modules {
            println!("- {module}");
        }
    }
    if !explanation.unused_client_modules.is_empty() {
        println!("unused client module declarations:");
        for module in &explanation.unused_client_modules {
            println!("- {module}");
        }
    }
    if explanation.component_fragment_imports.is_empty() {
        println!("component fragments: none");
    } else {
        println!("component fragments:");
        for asset_id in &explanation.component_fragment_imports {
            println!("- {asset_id}");
        }
    }
    if explanation.runtime_features.is_empty() {
        println!("runtime features: none");
    } else {
        println!("runtime features:");
        for feature in &explanation.runtime_features {
            println!("- {feature}");
        }
    }
    if explanation.shared_state_dependencies.is_empty() {
        println!("shared state: none");
    } else {
        println!("shared state:");
        for dependency in &explanation.shared_state_dependencies {
            let used_fields = if dependency.fields.is_empty() {
                "<no reachable fields>".to_string()
            } else {
                dependency.fields.join(", ")
            };
            // The macro cannot resolve a shared state's Rust type (see
            // `Analyzer::shared_field_client_type`), so the sidecar records the
            // store's Rust type as written and the fields this island reads.
            // The provider's full serialized field list is a property of the
            // type and is only known to the running provider.
            let serialized_fields = if dependency.serialized_fields.is_empty() {
                "<resolved by the provider at run time>".to_string()
            } else {
                dependency.serialized_fields.join(", ")
            };
            let identity = dependency
                .type_key
                .as_deref()
                .unwrap_or(&dependency.rust_type);
            if let Some(anchor) = &dependency.source_anchor {
                println!(
                    "- store {} {} @ {}:{}: used [{}]; provider serializes [{}]",
                    dependency.store,
                    identity,
                    anchor.start_line,
                    anchor.start_column,
                    used_fields,
                    serialized_fields
                );
            } else {
                println!(
                    "- store {} {}: used [{}]; provider serializes [{}]",
                    dependency.store, identity, used_fields, serialized_fields
                );
            }
        }
    }
    if explanation.server_surfaces.is_empty() {
        println!("server surfaces: none");
    } else {
        println!("server surfaces:");
        for surface in &explanation.server_surfaces {
            if let Some(anchor) = &surface.source_anchor {
                println!(
                    "- {} @ {}:{}: {}",
                    surface.kind, anchor.start_line, anchor.start_column, surface.detail
                );
            } else {
                println!("- {}: {}", surface.kind, surface.detail);
            }
        }
    }
    if explanation.crossings.is_empty() {
        println!("client crossings: none");
    } else {
        println!("client crossings:");
        for crossing in &explanation.crossings {
            if let Some(anchor) = &crossing.source_anchor {
                println!(
                    "- {} @ {}:{}: {}",
                    crossing.kind, anchor.start_line, anchor.start_column, crossing.detail
                );
            } else {
                println!("- {}: {}", crossing.kind, crossing.detail);
            }
        }
    }
    if explanation.dead_primitives.is_empty() {
        println!("dead client primitives: none");
    } else {
        println!("dead client primitives:");
        for primitive in &explanation.dead_primitives {
            if let Some(anchor) = &primitive.source_anchor {
                println!(
                    "- {} @ {}:{}: {}",
                    primitive.kind, anchor.start_line, anchor.start_column, primitive.detail
                );
            } else {
                println!("- {}: {}", primitive.kind, primitive.detail);
            }
        }
    }

    if explanation.roots.is_empty() {
        println!("why: no reachable client roots");
    } else {
        println!("why:");
        for root in &explanation.roots {
            println!("- {}: {}", root.kind, root.detail);
        }
    }

    println!("emitted:");
    println!("- wrapper: {}", yes_no(explanation.emitted.wrapper));
    println!("- seed script: {}", yes_no(explanation.emitted.seed_script));
    println!(
        "- manifest registration: {}",
        yes_no(explanation.emitted.manifest_registration)
    );
    println!("- JS asset: {}", yes_no(explanation.emitted.js_asset));
    println!(
        "- component fragment asset: {}",
        yes_no(explanation.emitted.component_fragment_asset)
    );

    println!("next actions:");
    for action in &explanation.next_actions {
        println!("- {action}");
    }

    Ok(())
}

pub fn print_crossing_report(report: &CrossingReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("RSJS client crossings: {}", report.crossing_count);
    println!("strategy sidecars: {}", report.sidecar_count);
    if let Some(selector) = &report.selector {
        println!("selector: {selector}");
    }
    if report.crossings.is_empty() {
        println!("client crossings: none");
        return Ok(());
    }
    for crossing in &report.crossings {
        let label = crossing.crossing.label.as_deref().unwrap_or("<unlabeled>");
        let surface = crossing
            .crossing
            .surface
            .as_deref()
            .unwrap_or("browser_state");
        let requirement = crossing
            .crossing
            .requirement
            .as_deref()
            .unwrap_or("client_encode");
        if let Some(anchor) = &crossing.crossing.source_anchor {
            println!(
                "- {} {} `{label}` -> `{surface}` ({requirement}) @ {}:{}",
                crossing.struct_path,
                crossing.crossing.kind,
                anchor.start_line,
                anchor.start_column
            );
        } else {
            println!(
                "- {} {} `{label}` -> `{surface}` ({requirement})",
                crossing.struct_path, crossing.crossing.kind
            );
        }
    }
    Ok(())
}

fn find_strategy_sidecar(root: &Path, selector: &str) -> Result<PathBuf> {
    let direct = PathBuf::from(selector);
    if direct.exists() {
        return Ok(direct);
    }

    let mut candidates = Vec::new();
    for dir in strategy_dirs(root) {
        let exact = dir.join(format!("{selector}.json"));
        if exact.exists() {
            return Ok(exact);
        }
        candidates.push(dir);
    }

    let mut matches: Vec<(PathBuf, String, String)> = Vec::new();
    for dir in candidates {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(sidecar) = serde_json::from_str::<StrategySidecar>(&text) else {
                continue;
            };
            if sidecar_matches(&sidecar, selector)
                && !matches
                    .iter()
                    .any(|(_, analysis_id, _)| *analysis_id == sidecar.analysis_id)
            {
                matches.push((path, sidecar.analysis_id, sidecar.struct_path));
            }
        }
    }

    match matches.len() {
        0 => bail!(
            "could not find rsjs strategy sidecar for `{selector}` under {}",
            root.display()
        ),
        1 => Ok(matches.remove(0).0),
        _ => {
            let mut listing = String::new();
            for (path, analysis_id, struct_path) in &matches {
                listing.push_str(&format!(
                    "\n  {struct_path} (analysis id {analysis_id}, {})",
                    path.display()
                ));
            }
            bail!(
                "selector `{selector}` matches {} rsjs strategy sidecars; disambiguate with the struct path or analysis id:{listing}",
                matches.len()
            )
        }
    }
}

fn strategy_dirs(root: &Path) -> Vec<PathBuf> {
    let root_local = root.join("target").join("rsjs").join("strategy");
    if let Ok(current) = std::env::current_dir() {
        let same_workspace = current == root || current.starts_with(root);
        if !same_workspace && root_local.exists() {
            // An explicit foreign workspace (including isolated test
            // fixtures) must not be contaminated by this process's own
            // CARGO_TARGET_DIR or current working directory artifacts.
            return vec![root_local];
        }
    }

    let mut dirs = Vec::new();
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        dirs.push(PathBuf::from(target).join("rsjs").join("strategy"));
    }
    dirs.push(root_local);
    if let Some(parent) = root.parent() {
        dirs.push(parent.join("target").join("rsjs").join("strategy"));
    }
    if let Ok(current) = std::env::current_dir() {
        dirs.push(current.join("target").join("rsjs").join("strategy"));
    }

    let mut seen = BTreeSet::new();
    dirs.into_iter()
        .filter(|dir| seen.insert(dir.clone()))
        .collect()
}

fn validate_sidecar(sidecar: &StrategySidecar, path: &Path) -> Result<()> {
    if sidecar.format != "rsjs.strategy" {
        bail!(
            "{} is not an rsjs strategy sidecar (format={:?})",
            path.display(),
            sidecar.format
        );
    }
    if sidecar.strategy_version != 1 {
        bail!(
            "{} uses unsupported rsjs strategy_version {}; this CLI supports 1",
            path.display(),
            sidecar.strategy_version
        );
    }
    Ok(())
}

fn sidecar_matches(sidecar: &StrategySidecar, selector: &str) -> bool {
    sidecar.format == "rsjs.strategy"
        && (sidecar.analysis_id == selector
            || sidecar.runtime_asset_id.as_deref() == Some(selector)
            || sidecar.struct_name == selector
            || sidecar.struct_path == selector)
}

fn explain_hydration_strategy(
    strategy: &HydrationStrategySidecar,
    server_only: bool,
) -> ExplainedHydrationStrategy {
    let label = hydration_strategy_label(strategy);
    ExplainedHydrationStrategy {
        kind: strategy.kind.clone(),
        ignored: server_only && strategy.kind != "immediate",
        label,
    }
}

fn hydration_strategy_label(strategy: &HydrationStrategySidecar) -> String {
    if let Some(label) = &strategy.label {
        return label.clone();
    }
    match strategy.kind.as_str() {
        "interaction" => strategy
            .event
            .as_ref()
            .map(|event| format!("interaction:{event}"))
            .unwrap_or_else(|| "interaction".to_string()),
        "media" => strategy
            .query
            .as_ref()
            .map(|query| format!("media:{query}"))
            .unwrap_or_else(|| "media:<unknown>".to_string()),
        other => other.to_string(),
    }
}

fn sidecar_runtime_features(sidecar: &StrategySidecar) -> Vec<String> {
    let mut features = if sidecar.runtime_features.is_empty() {
        let mut derived = Vec::new();
        if sidecar.emitted.uses_queries {
            derived.push("query".to_string());
        }
        if sidecar.emitted.uses_mutations {
            derived.push("mutation".to_string());
        }
        if sidecar.emitted.uses_shared_state {
            derived.push("shared_state".to_string());
        }
        derived
    } else {
        sidecar.runtime_features.clone()
    };

    let mut seen = BTreeSet::new();
    features.retain(|feature| seen.insert(feature.clone()));
    features
}

/// Sidecar `rust_type` strings are recorded from proc-macro token streams,
/// whose Display form inserts spaces between every token: `state::WorkbenchState`
/// arrives as "state :: WorkbenchState" and `Vec<Item>` as "Vec < Item >".
/// Collapse that spacing back to the canonical source spelling so `explain`
/// prints what the author wrote and the same store type spelled through the
/// same path compares equal across islands in `--json` output. A space is
/// kept only where removing it would glue two word tokens together
/// (`dyn Trait`, `impl Trait`).
fn normalize_rust_type_tokens(raw: &str) -> String {
    fn is_word_char(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            while chars.peek().is_some_and(|next| next.is_whitespace()) {
                chars.next();
            }
            let previous = out.chars().next_back();
            let next = chars.peek().copied();
            if let (Some(previous), Some(next)) = (previous, next) {
                if is_word_char(previous) && is_word_char(next) {
                    out.push(' ');
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn strategy_shared_state_dependency(
    dependency: &SharedStateDependencySidecar,
) -> StrategySharedStateDependency {
    StrategySharedStateDependency {
        store: dependency.store,
        rust_type: normalize_rust_type_tokens(&dependency.rust_type),
        type_key: dependency.type_key.clone(),
        fields: dependency.fields.clone(),
        serialized_fields: dependency.serialized_fields.clone(),
        provider_serializes_full_snapshot: dependency.provider_serializes_full_snapshot,
        source_anchor: dependency
            .source
            .as_ref()
            .map(|anchor| StrategyClientCrossingAnchor {
                start_line: anchor.start_line,
                start_column: anchor.start_column,
            }),
    }
}

fn sidecar_component_fragment_imports(sidecar: &StrategySidecar) -> Vec<String> {
    let mut imports = sidecar.component_fragment_imports.clone();
    for root in &sidecar.roots {
        if root.kind != "component_fragment_mount" {
            continue;
        }
        if let Some(asset_id) = &root.asset_id {
            imports.push(asset_id.clone());
        }
    }

    imports.sort();
    imports.dedup();
    imports
}

fn unused_client_modules(declared: &[String], client_modules: &[String]) -> Vec<String> {
    let shipped: BTreeSet<_> = client_modules.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let mut unused = Vec::new();
    for module in declared {
        if !shipped.contains(module.as_str()) && seen.insert(module.as_str()) {
            unused.push(module.clone());
        }
    }
    unused
}

fn explain_root(root: &StrategyRoot) -> ExplainedRoot {
    ExplainedRoot {
        kind: root.kind.clone(),
        detail: root_detail(root),
    }
}

fn explain_server_surface(surface: &ServerSurfaceSidecar) -> ExplainedServerSurface {
    ExplainedServerSurface {
        kind: surface.kind.clone(),
        source: surface.source.clone(),
        source_anchor: surface.source_anchor.as_ref().map(strategy_surface_anchor),
        detail: server_surface_detail(surface),
    }
}

fn explain_client_crossing(crossing: &ClientCrossingSidecar) -> ExplainedClientCrossing {
    ExplainedClientCrossing {
        kind: crossing.kind.clone(),
        label: crossing.label.clone(),
        module: crossing.module.clone(),
        name: crossing.name.clone(),
        arg_index: crossing.arg_index,
        asset_id: crossing.asset_id.clone(),
        prop: crossing.prop.clone(),
        root: crossing.root,
        child_signal: crossing.child_signal,
        parent_signal: crossing.parent_signal,
        signal: crossing.signal,
        query: crossing.query,
        mutation: crossing.mutation,
        invalidation_index: crossing.invalidation_index,
        node: crossing.node,
        attr: crossing.attr.clone(),
        surface: crossing.surface.clone(),
        requirement: crossing.requirement.clone(),
        source_anchor: crossing
            .source_anchor
            .as_ref()
            .map(strategy_client_crossing_anchor),
        detail: client_crossing_detail(crossing),
    }
}

fn explain_dead_primitive(primitive: &DeadPrimitiveSidecar) -> ExplainedDeadPrimitive {
    ExplainedDeadPrimitive {
        kind: primitive.kind.clone(),
        label: primitive.label.clone(),
        signal: primitive.signal,
        handler: primitive.handler,
        node_ref: primitive.node_ref,
        query: primitive.query,
        mutation: primitive.mutation,
        source_anchor: primitive
            .source_anchor
            .as_ref()
            .map(strategy_dead_primitive_anchor),
        detail: dead_primitive_detail(primitive),
    }
}

fn strategy_server_surface_summary(surface: &ServerSurfaceSidecar) -> StrategyServerSurfaceSummary {
    StrategyServerSurfaceSummary {
        kind: surface.kind.clone(),
        source: surface.source.clone(),
        node: surface.node,
        attr: surface.attr.clone(),
        reason: surface.reason.clone(),
        source_anchor: surface.source_anchor.as_ref().map(strategy_surface_anchor),
    }
}

fn strategy_client_crossing_summary(
    crossing: &ClientCrossingSidecar,
) -> StrategyClientCrossingSummary {
    StrategyClientCrossingSummary {
        kind: crossing.kind.clone(),
        label: crossing.label.clone(),
        module: crossing.module.clone(),
        name: crossing.name.clone(),
        arg_index: crossing.arg_index,
        asset_id: crossing.asset_id.clone(),
        prop: crossing.prop.clone(),
        root: crossing.root,
        child_signal: crossing.child_signal,
        parent_signal: crossing.parent_signal,
        signal: crossing.signal,
        query: crossing.query,
        mutation: crossing.mutation,
        invalidation_index: crossing.invalidation_index,
        node: crossing.node,
        attr: crossing.attr.clone(),
        surface: crossing.surface.clone(),
        requirement: crossing.requirement.clone(),
        source_anchor: crossing
            .source_anchor
            .as_ref()
            .map(strategy_client_crossing_anchor),
    }
}

fn strategy_dead_primitive_summary(
    primitive: &DeadPrimitiveSidecar,
) -> StrategyDeadPrimitiveSummary {
    StrategyDeadPrimitiveSummary {
        kind: primitive.kind.clone(),
        label: primitive.label.clone(),
        signal: primitive.signal,
        handler: primitive.handler,
        node_ref: primitive.node_ref,
        query: primitive.query,
        mutation: primitive.mutation,
        source_anchor: primitive
            .source_anchor
            .as_ref()
            .map(strategy_dead_primitive_anchor),
    }
}

fn strategy_surface_anchor(anchor: &ServerSurfaceAnchorSidecar) -> StrategyServerSurfaceAnchor {
    StrategyServerSurfaceAnchor {
        start_line: anchor.start_line,
        start_column: anchor.start_column,
    }
}

fn strategy_client_crossing_anchor(
    anchor: &ClientCrossingAnchorSidecar,
) -> StrategyClientCrossingAnchor {
    StrategyClientCrossingAnchor {
        start_line: anchor.start_line,
        start_column: anchor.start_column,
    }
}

fn strategy_dead_primitive_anchor(
    anchor: &DeadPrimitiveAnchorSidecar,
) -> StrategyDeadPrimitiveAnchor {
    StrategyDeadPrimitiveAnchor {
        start_line: anchor.start_line,
        start_column: anchor.start_column,
    }
}

fn server_surface_detail(surface: &ServerSurfaceSidecar) -> String {
    let reason = surface
        .reason
        .as_deref()
        .or_else(|| surface.source.as_deref().map(server_surface_source_label))
        .unwrap_or("server rendered");
    match surface.kind.as_str() {
        "server_attr" => format!(
            "node {} attr `{}` rendered on server ({reason})",
            surface.node.unwrap_or_default(),
            surface.attr.as_deref().unwrap_or("<unknown>")
        ),
        "server_slot" => format!(
            "node {} server-rendered slot ({reason})",
            surface.node.unwrap_or_default()
        ),
        _ => format!(
            "node {} server surface ({reason})",
            surface.node.unwrap_or_default()
        ),
    }
}

fn dead_primitive_detail(primitive: &DeadPrimitiveSidecar) -> String {
    let label = primitive.label.as_deref().unwrap_or("<unlabeled>");
    match primitive.kind.as_str() {
        "signal" => format!(
            "`{label}` signal {} was declared but no reachable client root reads or writes it",
            primitive.signal.unwrap_or_default()
        ),
        "memo" => format!(
            "`{label}` memo signal {} was declared but no reachable client root reads it",
            primitive.signal.unwrap_or_default()
        ),
        "handler" => format!(
            "`{label}` handler {} was declared but no reachable event binding uses it",
            primitive.handler.unwrap_or_default()
        ),
        "node_ref" => format!(
            "`{label}` node_ref {} was declared but no reachable ref binding, handler, or expression uses it",
            primitive.node_ref.unwrap_or_default()
        ),
        "query" => format!(
            "`{label}` query {} was declared but no reachable client root observes, refetches, or invalidates it",
            primitive.query.unwrap_or_default()
        ),
        "mutation" => format!(
            "`{label}` mutation {} was declared but no reachable client root reads or runs it",
            primitive.mutation.unwrap_or_default()
        ),
        kind => format!("`{label}` {kind} declaration was not reachable from client roots"),
    }
}

fn client_crossing_detail(crossing: &ClientCrossingSidecar) -> String {
    client_crossing_summary_detail(&strategy_client_crossing_summary(crossing))
}

pub(crate) fn client_crossing_summary_detail(crossing: &StrategyClientCrossingSummary) -> String {
    let label = crossing.label.as_deref().unwrap_or("<unlabeled>");
    let surface = crossing.surface.as_deref().unwrap_or("browser_state");
    let requirement = crossing.requirement.as_deref().unwrap_or("client_encode");
    match crossing.kind.as_str() {
        "signal_initial_value" => format!(
            "`{label}` signal initial value crosses to `{surface}` and requires `{requirement}`"
        ),
        "handler_payload" => format!(
            "`{label}` handler payload crosses to `{surface}` on node {} attr `{}` and requires `{requirement}`",
            crossing.node.unwrap_or_default(),
            crossing.attr.as_deref().unwrap_or("<unknown>")
        ),
        "node_payload" => format!(
            "`{label}` node payload crosses to `{surface}` on node {} attr `{}` and requires `{requirement}`",
            crossing.node.unwrap_or_default(),
            crossing.attr.as_deref().unwrap_or("<unknown>")
        ),
        "query_key" => format!(
            "`{label}` query key crosses to `{surface}` for query {} and requires `{requirement}`",
            crossing.query.unwrap_or_default()
        ),
        "query_fetcher" => format!(
            "`{label}` query fetcher config crosses to `{surface}` for query {} and requires `{requirement}`",
            crossing.query.unwrap_or_default()
        ),
        "query_initial_value" => format!(
            "`{label}` query initial value crosses to `{surface}` for query {} and requires `{requirement}`",
            crossing.query.unwrap_or_default()
        ),
        "mutation_key" => format!(
            "`{label}` mutation key crosses to `{surface}` for mutation {} and requires `{requirement}`",
            crossing.mutation.unwrap_or_default()
        ),
        "mutation_runner" => format!(
            "`{label}` mutation runner config crosses to `{surface}` for mutation {} and requires `{requirement}`",
            crossing.mutation.unwrap_or_default()
        ),
        "mutation_invalidation_key" => format!(
            "`{label}` mutation invalidation key {} crosses to `{surface}` for mutation {} and requires `{requirement}`",
            crossing.invalidation_index.unwrap_or_default(),
            crossing.mutation.unwrap_or_default()
        ),
        // Compatibility for older experimental sidecars. Current v2
        // sidecars report client helper calls as roots/import edges; any
        // public data is recorded by the underlying signal/payload/query/
        // mutation/descriptor crossing surface instead.
        "client_helper_argument" => format!(
            "`{label}` client helper argument {} crosses to `{surface}` for `{}::{}` and requires `{requirement}`",
            crossing.arg_index.unwrap_or_default(),
            crossing.module.as_deref().unwrap_or("<unknown>"),
            crossing.name.as_deref().unwrap_or("<unknown>")
        ),
        "descriptor_reactive_prop" => format!(
            "`{label}` descriptor reactive prop `{}` crosses to `{surface}` for fragment `{}` at root {} (child signal {}, parent signal {}) and requires `{requirement}`",
            crossing.prop.as_deref().unwrap_or("<unknown>"),
            crossing.asset_id.as_deref().unwrap_or("<unknown>"),
            crossing.root.unwrap_or_default(),
            crossing.child_signal.unwrap_or_default(),
            crossing.parent_signal.unwrap_or_default()
        ),
        kind => {
            format!("`{label}` crosses to `{surface}` as `{kind}` and requires `{requirement}`")
        }
    }
}

fn server_surface_source_label(source: &str) -> &'static str {
    match source {
        "explicit_boundary" => "explicit SSR boundary",
        "explicit_simple_ssr_boundary" => "explicit wrapper around simple SSR projection",
        "inferred_simple_ssr" => "inferred simple SSR projection",
        "keyed_list_item" => "keyed-list item SSR template",
        "explicit_island_boundary" => "explicit island boundary",
        _ => "server rendered",
    }
}

fn root_detail(root: &StrategyRoot) -> String {
    match root.kind.as_str() {
        "text_binding" => format_node("text binding", root.node),
        "attr_binding" => format!(
            "node {} dynamic attribute `{}`",
            root.node.unwrap_or_default(),
            root.name.as_deref().unwrap_or("<unknown>")
        ),
        "event_binding" => format!(
            "node {} event `{}` ({})",
            root.node.unwrap_or_default(),
            root.event.as_deref().unwrap_or("<unknown>"),
            root.dom_event.as_deref().unwrap_or("unknown DOM event")
        ),
        "conditional_section" => format_node("conditional section parent", root.parent),
        "input_binding" => format_node("input binding", root.node),
        "node_ref_binding" => format!(
            "node {} bound to node_ref {}",
            root.node.unwrap_or_default(),
            root.node_ref.unwrap_or_default()
        ),
        "node_ref" => format!("node_ref {}", root.id.unwrap_or_default()),
        "signal" => format!("signal {}", root.id.unwrap_or_default()),
        "query" => format!("query {}", root.id.unwrap_or_default()),
        "mutation" => format!("mutation {}", root.id.unwrap_or_default()),
        "browser_read" => format!(
            "browser read `{}`",
            root.read.as_deref().unwrap_or("<unknown>")
        ),
        "browser_effect" => format!(
            "browser effect `{}`",
            root.effect.as_deref().unwrap_or("<unknown>")
        ),
        "client_module_call" => format!(
            "client module `{}` helper `{}`",
            root.module.as_deref().unwrap_or("<unknown>"),
            root.name.as_deref().unwrap_or("<unknown>")
        ),
        "interval" => format!("interval {}", root.index.unwrap_or_default()),
        "keyed_list" => format_node("keyed list container", root.container),
        "component_fragment_mount" => format!(
            "component fragment `{}` mounted at root {} through the schema-v4 parent-mounted fragment path",
            root.asset_id.as_deref().unwrap_or("<unknown>"),
            root.root.unwrap_or_default()
        ),
        _ => "unknown client reachability root".to_string(),
    }
}

fn format_node(label: &str, node: Option<u64>) -> String {
    format!("{label} node {}", node.unwrap_or_default())
}

fn strategy_source_anchor(source: StrategySourceSidecar) -> StrategySourceAnchor {
    StrategySourceAnchor {
        file: source.file,
        impl_start_line: source.impl_start_line,
        impl_start_column: source.impl_start_column,
        render_start_line: source.render_start_line,
        render_start_column: source.render_start_column,
    }
}

fn human_strategy(strategy: &str) -> &str {
    match strategy {
        "server_only" => "ServerOnly",
        "ssr_hydrate" => "SSR + Hydrate",
        "client_only_explicit" => "ClientOnlyExplicit",
        other => other,
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests;
