// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use proa_core::{ctx::Ctx, WebRenderSync};
use proa_macros::html_sync;
use proa_react::{
    react_bootstrap, render_import_map_entries, render_react_import_map, HydrationStrategy,
    ReactBootstrapConfig, ReactModuleDescriptor,
};

use crate::manifest::{HydrationStrategyConfig, IslandEntry, IslandManifest};

pub fn render_bootstrap_for_islands(
    cx: &mut Ctx,
    manifest: &IslandManifest,
    islands: impl IntoIterator<Item = impl AsRef<str>>,
    config: &ReactBootstrapConfig<'_>,
) -> Result<()> {
    render_bootstrap_for_islands_with_import_map(cx, manifest, islands, config, true)
}

pub(crate) fn render_bootstrap_for_islands_with_import_map(
    cx: &mut Ctx,
    manifest: &IslandManifest,
    islands: impl IntoIterator<Item = impl AsRef<str>>,
    config: &ReactBootstrapConfig<'_>,
    include_import_map: bool,
) -> Result<()> {
    let mut unique = BTreeSet::new();
    let mut collected = Vec::new();

    for name in islands {
        let owned = name.as_ref().to_string();
        if !unique.insert(owned.clone()) {
            continue;
        }
        let entry = manifest
            .resolve_entry(&owned)
            .with_context(|| format!("No island named '{}' in manifest", owned))?;
        collected.push((owned, entry));
    }

    let descriptors: Vec<_> = collected
        .iter()
        .map(|(name, entry)| convert_entry(name.as_str(), entry))
        .collect();

    render_module_preloads(cx, &collected)?;

    if include_import_map && config.use_external_runtime {
        if !manifest.import_map.is_empty() {
            render_import_map_entries(cx, &manifest.import_map)?;
        } else {
            render_react_import_map(cx, config)?;
        }
    }

    react_bootstrap(&descriptors, config).render(cx)?;
    Ok(())
}

fn render_module_preloads(cx: &mut Ctx, entries: &[(String, &IslandEntry)]) -> Result<()> {
    let mut preloads = BTreeSet::new();
    for (_, entry) in entries {
        preloads.extend(entry.module_preloads.iter().map(String::as_str));
    }
    for href in preloads {
        html_sync! {
            <link rel="modulepreload" href={href} />
        }
        .render(cx)?;
    }
    Ok(())
}

fn convert_entry<'a>(name: &'a str, entry: &'a IslandEntry) -> ReactModuleDescriptor<'a> {
    ReactModuleDescriptor {
        island: name,
        module_path: &entry.module_path,
        export: entry.export.as_deref(),
        strategy: map_strategy(&entry.strategy),
    }
}

fn map_strategy(config: &HydrationStrategyConfig) -> HydrationStrategy<'_> {
    match config {
        HydrationStrategyConfig::Immediate => HydrationStrategy::Immediate,
        HydrationStrategyConfig::Idle => HydrationStrategy::Idle,
        HydrationStrategyConfig::Visible => HydrationStrategy::Visible,
        HydrationStrategyConfig::Interaction { event } => HydrationStrategy::Interaction {
            event: event.as_str(),
        },
        HydrationStrategyConfig::Media { query } => HydrationStrategy::Media {
            query: query.as_str(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ManifestAssets;
    use std::collections::HashMap;

    fn entry(module_path: &str, module_preloads: &[&str]) -> IslandEntry {
        IslandEntry {
            module_path: module_path.to_string(),
            export: None,
            strategy: HydrationStrategyConfig::Immediate,
            module_preloads: module_preloads
                .iter()
                .map(|preload| (*preload).to_string())
                .collect(),
            providers: Vec::new(),
            hydrate_fn: None,
        }
    }

    #[test]
    fn bootstrap_emits_deduped_modulepreloads_for_rendered_islands() {
        let mut islands = HashMap::new();
        islands.insert(
            "booking".to_string(),
            entry(
                "/static/booking.js",
                &[
                    "/rsjs/island/component_calendar.abc12345.js",
                    "/static/shared.js",
                ],
            ),
        );
        islands.insert(
            "filters".to_string(),
            entry("/static/filters.js", &["/static/shared.js"]),
        );
        islands.insert(
            "unused".to_string(),
            entry("/static/unused.js", &["/static/unused-fragment.js"]),
        );
        let manifest = IslandManifest {
            islands,
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };

        let mut cx = Ctx::new();
        render_bootstrap_for_islands(
            &mut cx,
            &manifest,
            ["booking", "filters", "booking"],
            &ReactBootstrapConfig::default(),
        )
        .expect("bootstrap should render");
        let html = cx.into_output().into_string();

        assert_eq!(
            html.matches(r#"rel="modulepreload""#).count(),
            2,
            "module preloads should be deduped by URL, got:\n{html}"
        );
        assert!(html.contains(r#"href="/rsjs/island/component_calendar.abc12345.js""#));
        assert!(html.contains(r#"href="/static/shared.js""#));
        assert!(!html.contains("/static/unused-fragment.js"));
    }

    #[test]
    fn bootstrap_threads_media_strategy() {
        let mut islands = HashMap::new();
        let mut chart = entry("/static/chart.js", &[]);
        chart.strategy = HydrationStrategyConfig::Media {
            query: "(min-width: 768px)".to_string(),
        };
        islands.insert("chart".to_string(), chart);
        let manifest = IslandManifest {
            islands,
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        };

        let mut cx = Ctx::new();
        render_bootstrap_for_islands(
            &mut cx,
            &manifest,
            ["chart"],
            &ReactBootstrapConfig::default(),
        )
        .expect("bootstrap should render");
        let html = cx.into_output().into_string();

        assert!(
            html.contains(r#"strategy: { type: "media", query: "(min-width: 768px)" }"#),
            "bootstrap should preserve media strategy, got:\n{html}"
        );
    }
}
