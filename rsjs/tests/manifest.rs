// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M7g end-to-end fixture: a real
//! `#[rsjs(component, client)] impl WebRenderSync<::proa_core::NoLoader, ...>`
//! block populates the linkme `ISLAND_REGISTRY` at link time, and
//! `rsjs::manifest()` exposes the registration to the framework
//! adapter.
//!
//! Mirrors the parser_counter integration test in `rsjs_macros`,
//! reused from the runtime crate's vantage point so we exercise the
//! linkme submission pipeline end-to-end (the proc-macro emits a
//! `#[distributed_slice(::rsjs::ISLAND_REGISTRY)]` static; linking
//! this test binary populates the slice; `rsjs::manifest()` reads
//! it back).

use rsjs::manifest;
use rsjs_macros::rsjs;

// `signal` is a stub for the proc-macro analyser. The macro does not
// evaluate it (the generated impl rebinds initial values via
// `__rsjs_initial_signal_<N>` locals); the Rust type-checker still
// resolves the body, so the path has to exist.
#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> {
    Signal { initial }
}

#[allow(dead_code)]
struct Signal<T> {
    initial: T,
}

#[allow(dead_code)]
impl<T: Clone + std::fmt::Display> Signal<T> {
    pub fn get(&self) -> T {
        self.initial.clone()
    }

    pub fn update<F: Fn(T) -> T>(&self, _f: F) {}
}

pub struct ManifestProbe {
    pub initial: i32,
}

#[rsjs(component, client)]
impl<B: proa_core::WriteBuf, L: proa_core::DataLoader> proa_core::WebRenderSync<L, B>
    for ManifestProbe
{
    fn render(self, cx: &mut proa_core::WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| count.update(|c| c - 1)}>"-"</button>
                <span>{count.get()}</span>
                <button on_click={|_| count.update(|c| c + 1)}>"+"</button>
            </div>
        }
        .render(cx)
    }
}

pub struct VisibleManifestProbe;

#[rsjs(component, client, strategy = "visible")]
impl<B: proa_core::WriteBuf, L: proa_core::DataLoader> proa_core::WebRenderSync<L, B>
    for VisibleManifestProbe
{
    fn render(self, cx: &mut proa_core::WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <button type="button" on_click={|_| count.update(|value| value + 1)}>
                {count.get()}
            </button>
        }
        .render(cx)
    }
}

pub struct IdleManifestProbe;

#[rsjs(component, client, strategy = "idle")]
impl<B: proa_core::WriteBuf, L: proa_core::DataLoader> proa_core::WebRenderSync<L, B>
    for IdleManifestProbe
{
    fn render(self, cx: &mut proa_core::WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <button type="button" on_click={|_| count.update(|value| value + 1)}>
                {count.get()}
            </button>
        }
        .render(cx)
    }
}

pub struct InteractionManifestProbe;

#[rsjs(component, client, strategy = "interaction:click")]
impl<B: proa_core::WriteBuf, L: proa_core::DataLoader> proa_core::WebRenderSync<L, B>
    for InteractionManifestProbe
{
    fn render(self, cx: &mut proa_core::WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <button type="button" on_click={|_| count.update(|value| value + 1)}>
                {count.get()}
            </button>
        }
        .render(cx)
    }
}

pub struct MediaManifestProbe;

#[rsjs(component, client, strategy = "media:(min-width: 768px)")]
impl<B: proa_core::WriteBuf, L: proa_core::DataLoader> proa_core::WebRenderSync<L, B>
    for MediaManifestProbe
{
    fn render(self, cx: &mut proa_core::WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <button type="button" on_click={|_| count.update(|value| value + 1)}>
                {count.get()}
            </button>
        }
        .render(cx)
    }
}

/// The asset id is `<lowercased_last_segment>_<hash8>` (see
/// `rsjs_macros::asset`). We discover it from the registry rather than
/// hard-coding it, so the test stays robust to module re-paths.
fn manifest_probe_asset_id() -> &'static str {
    let m = manifest();
    let entry = m
        .islands
        .iter()
        .find(|e| e.struct_name == "ManifestProbe")
        .expect("ManifestProbe registration missing from ISLAND_REGISTRY");
    entry.asset_id
}

fn manifest_entry(struct_name: &str) -> &'static rsjs::IslandManifestEntry {
    let m = manifest();
    m.islands
        .iter()
        .find(|e| e.struct_name == struct_name)
        .copied()
        .unwrap_or_else(|| panic!("{struct_name} registration missing from ISLAND_REGISTRY"))
}

#[test]
fn manifest_contains_registered_island() {
    let entry = manifest_entry("ManifestProbe");
    assert!(
        entry.asset_id.starts_with("manifestprobe_"),
        "expected manifestprobe_<hash> asset id, got: {}",
        entry.asset_id
    );
    assert_eq!(entry.struct_name, "ManifestProbe");
    assert!(!entry.is_async, "sync island must report is_async=false");
    assert_eq!(
        entry.hydration_strategy,
        rsjs::IslandHydrationStrategy::Immediate,
        "default rsjs islands must report immediate hydration",
    );
    assert!(
        entry.client_module_imports.is_empty(),
        "M7g registers an empty client_module_imports list; M7e populates it",
    );
    assert!(
        entry.component_fragment_imports.is_empty(),
        "standalone probe does not inline child components"
    );
    // The JS source baked in via `include_str!`-style literal: it must
    // mention the runtime hook the per-island JS uses to find its
    // hydration root.
    assert!(
        entry.js_source.contains(entry.asset_id),
        "expected the per-island JS to reference the asset_id, got: {}",
        entry.js_source
    );
}

#[test]
fn manifest_contains_strategy_metadata_for_visible_island() {
    let entry = manifest_entry("VisibleManifestProbe");

    assert_eq!(
        entry.hydration_strategy,
        rsjs::IslandHydrationStrategy::Visible,
        "visible rsjs islands must report visible hydration",
    );
    assert!(
        entry.js_source.contains("hydrate_when_visible"),
        "visible strategy should reach generated island JS, got: {}",
        entry.js_source
    );
}

#[test]
fn manifest_contains_strategy_metadata_for_idle_island() {
    let entry = manifest_entry("IdleManifestProbe");

    assert_eq!(
        entry.hydration_strategy,
        rsjs::IslandHydrationStrategy::Idle,
        "idle rsjs islands must report idle hydration",
    );
    assert!(
        entry.js_source.contains("hydrate_when_idle"),
        "idle strategy should reach generated island JS, got: {}",
        entry.js_source
    );
}

#[test]
fn manifest_contains_strategy_metadata_for_interaction_island() {
    let entry = manifest_entry("InteractionManifestProbe");

    assert_eq!(
        entry.hydration_strategy,
        rsjs::IslandHydrationStrategy::Interaction {
            event: Some("click")
        },
        "interaction rsjs islands must report interaction hydration",
    );
    assert!(
        entry.js_source.contains("hydrate_on_interaction"),
        "interaction strategy should reach generated island JS, got: {}",
        entry.js_source
    );
    assert!(
        entry.js_source.contains(r#"["click"]"#),
        "interaction strategy should pass the configured event, got: {}",
        entry.js_source
    );
}

#[test]
fn manifest_contains_strategy_metadata_for_media_island() {
    let entry = manifest_entry("MediaManifestProbe");

    assert_eq!(
        entry.hydration_strategy,
        rsjs::IslandHydrationStrategy::Media {
            query: "(min-width: 768px)"
        },
        "media rsjs islands must report media hydration",
    );
    assert!(
        entry.js_source.contains("hydrate_when_media"),
        "media strategy should reach generated island JS, got: {}",
        entry.js_source
    );
    assert!(
        entry.js_source.contains(r#""(min-width: 768px)""#),
        "media strategy should pass the configured media query, got: {}",
        entry.js_source
    );
}

#[test]
fn write_manifest_json_emits_parseable_file() {
    let asset_id = manifest_probe_asset_id();
    let dir = std::env::temp_dir().join(format!("rsjs_manifest_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    rsjs::write_manifest_json(&dir).expect("write_manifest_json failed");

    let path = dir.join("manifest.json");
    let json = std::fs::read_to_string(&path).expect("manifest.json missing");
    assert!(
        json.starts_with('{') && json.ends_with('}'),
        "manifest.json must be a JSON object, got: {json}"
    );
    assert!(json.contains("\"islands\""), "missing islands key: {json}");
    assert!(
        json.contains("\"client_modules\""),
        "missing client_modules key: {json}"
    );
    assert!(
        json.contains("\"component_fragments\""),
        "missing component_fragments key: {json}"
    );
    let needle = format!("\"asset_id\":\"{asset_id}\"");
    assert!(
        json.contains(&needle),
        "missing fixture island in manifest: {json}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_manifest_assets_emits_per_island_js() {
    let asset_id = manifest_probe_asset_id();
    let dir = std::env::temp_dir().join(format!("rsjs_assets_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    rsjs::write_manifest_assets(&dir).expect("write_manifest_assets failed");

    let js_path = dir.join(format!("{asset_id}.js"));
    let body = std::fs::read_to_string(&js_path).expect("per-island JS missing");
    assert!(
        body.contains(asset_id),
        "expected the per-island JS to reference its own asset_id: {body}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn manifest_contains_registered_component_fragment() {
    let asset_id = manifest_probe_asset_id();
    let m = manifest();
    let fragment = m
        .component_fragments
        .iter()
        .find(|e| e.asset_id == asset_id)
        .expect("ManifestProbe fragment registration missing");
    assert_eq!(fragment.struct_name, "ManifestProbe");
    assert!(
        fragment.export_name.starts_with("mount_manifestprobe_"),
        "expected mount_manifestprobe_<hash> export, got {}",
        fragment.export_name
    );
    assert!(
        fragment
            .js_source
            .contains(&format!("export function {}", fragment.export_name)),
        "fragment JS must export the manifest-named mount function: {}",
        fragment.js_source
    );
}

#[test]
fn write_manifest_assets_emits_component_fragment_js() {
    let asset_id = manifest_probe_asset_id();
    let m = manifest();
    let fragment = m
        .component_fragments
        .iter()
        .find(|e| e.asset_id == asset_id)
        .expect("ManifestProbe fragment registration missing");
    let dir =
        std::env::temp_dir().join(format!("rsjs_fragment_assets_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    rsjs::write_manifest_assets(&dir).expect("write_manifest_assets failed");

    let js_path = dir.join(rsjs::manifest::component_fragment_filename(asset_id));
    let body = std::fs::read_to_string(&js_path).expect("component-fragment JS missing");
    assert!(
        body.contains(&format!("export function {}", fragment.export_name)),
        "expected component fragment JS to export {}, got: {body}",
        fragment.export_name
    );

    let _ = std::fs::remove_dir_all(&dir);
}
