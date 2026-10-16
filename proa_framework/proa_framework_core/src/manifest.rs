// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct IslandManifest {
    pub islands: HashMap<String, IslandEntry>,
    #[serde(default)]
    pub assets: ManifestAssets,
    #[serde(default)]
    pub import_map: HashMap<String, String>,
    /// Filename of the SSR IIFE bundle (e.g. "ssr-entry.js") relative to dist dir.
    #[serde(default)]
    pub ssr_bundle: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ManifestAssets {
    #[serde(default)]
    pub css: Option<CssAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CssAsset {
    pub path: String,
    #[serde(default)]
    pub size: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IslandEntry {
    pub module_path: String,
    #[serde(default)]
    pub export: Option<String>,
    pub strategy: HydrationStrategyConfig,
    #[serde(default)]
    pub module_preloads: Vec<String>,
    #[serde(default)]
    pub providers: Vec<ProviderSpec>,
    #[serde(default)]
    pub hydrate_fn: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HydrationStrategyConfig {
    Immediate,
    Idle,
    Visible,
    Interaction { event: String },
    Media { query: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderSpec {
    pub module: String,
    #[serde(default)]
    pub export: Option<String>,
    #[serde(default)]
    pub props: serde_json::Value,
}

impl IslandManifest {
    /// Resolve application-local URLs against the public mount at startup.
    /// External URLs and import-map keys are preserved. Call once on an
    /// unprefixed manifest; incoming route and filesystem paths are unaffected.
    pub fn with_base_path(mut self, base_path: &proa_core::BasePath) -> Self {
        if base_path.as_str().is_empty() {
            return self;
        }
        for entry in self.islands.values_mut() {
            entry.module_path = base_path.url(&entry.module_path).to_string();
            for preload in &mut entry.module_preloads {
                *preload = base_path.url(preload).to_string();
            }
            for provider in &mut entry.providers {
                provider.module = base_path.url(&provider.module).to_string();
            }
        }
        if let Some(css) = &mut self.assets.css {
            css.path = base_path.url(&css.path).to_string();
        }
        for url in self.import_map.values_mut() {
            *url = base_path.url(url).to_string();
        }
        self
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read manifest at {}", path.display()))?;
        let manifest: IslandManifest = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse manifest JSON at {}", path.display()))?;
        manifest
            .validate()
            .with_context(|| format!("Manifest at {} failed validation", path.display()))?;
        Ok(manifest)
    }

    /// Reject manifest entries whose `module_path` would let an
    /// attacker who controls the manifest serve arbitrary code from a
    /// surprising origin. We accept either:
    ///   * absolute server-relative paths (start with `/`)
    ///   * absolute URLs with `https://` (or `http://` to support local
    ///     dev rigs; CSP / origin allowlists are what gate this in prod)
    ///
    /// Prior versions accepted bare relative paths
    /// (`../../etc/passwd`-style or protocol-less `//evil.com/x.js`) which the
    /// browser resolves
    /// against the page origin. Either form turns a manifest write
    /// into a path-traversal or protocol-relative-URL hijack.
    pub fn validate(&self) -> Result<()> {
        for (name, entry) in &self.islands {
            validate_module_path(name, &entry.module_path)?;
            for preload in &entry.module_preloads {
                validate_module_path(name, preload)?;
            }
            for provider in &entry.providers {
                validate_module_path(name, &provider.module)?;
            }
        }
        Ok(())
    }

    pub fn resolve_entry<'a>(&'a self, name: &str) -> Option<&'a IslandEntry> {
        self.islands.get(name)
    }
}

fn validate_module_path(island: &str, module_path: &str) -> Result<()> {
    if module_path.is_empty() {
        anyhow::bail!("island {island:?} has empty module_path");
    }
    // Protocol-relative URLs `//host/path` resolve against the page
    // origin's scheme — explicitly reject before the `/` check would
    // accept them.
    if module_path.starts_with("//") {
        anyhow::bail!(
            "island {island:?} module_path {module_path:?} is protocol-relative; \
             use an absolute https:// URL or a server-relative /path"
        );
    }
    if module_path.starts_with('/') {
        return Ok(());
    }
    if module_path.starts_with("https://") || module_path.starts_with("http://") {
        return Ok(());
    }
    anyhow::bail!(
        "island {island:?} module_path {module_path:?} must be an absolute URL \
         (https://...) or a server-relative path (/...)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(module_path: &str) -> IslandEntry {
        IslandEntry {
            module_path: module_path.to_string(),
            export: None,
            strategy: HydrationStrategyConfig::Immediate,
            module_preloads: Vec::new(),
            providers: Vec::new(),
            hydrate_fn: None,
        }
    }

    fn manifest_with(island_name: &str, module_path: &str) -> IslandManifest {
        let mut islands = HashMap::new();
        islands.insert(island_name.to_string(), entry(module_path));
        IslandManifest {
            islands,
            assets: ManifestAssets::default(),
            import_map: HashMap::new(),
            ssr_bundle: None,
        }
    }

    #[test]
    fn accepts_server_relative_path() {
        assert!(manifest_with("a", "/static/a.js").validate().is_ok());
    }

    #[test]
    fn accepts_https_url() {
        assert!(manifest_with("a", "https://cdn.example/a.js")
            .validate()
            .is_ok());
    }

    #[test]
    fn rejects_relative_path() {
        let err = manifest_with("a", "../evil.js").validate().unwrap_err();
        assert!(err.to_string().contains("must be an absolute URL"));
    }

    #[test]
    fn rejects_protocol_relative_url() {
        let err = manifest_with("a", "//evil.com/x.js")
            .validate()
            .unwrap_err();
        assert!(err.to_string().contains("protocol-relative"));
    }

    #[test]
    fn rejects_empty() {
        let err = manifest_with("a", "").validate().unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn rejects_bare_filename() {
        let err = manifest_with("a", "a.js").validate().unwrap_err();
        assert!(err.to_string().contains("must be an absolute URL"));
    }

    #[test]
    fn rejects_relative_module_preload() {
        let mut manifest = manifest_with("a", "/static/a.js");
        manifest
            .islands
            .get_mut("a")
            .unwrap()
            .module_preloads
            .push("../fragment.js".to_string());
        let err = manifest.validate().unwrap_err();
        assert!(err.to_string().contains("must be an absolute URL"));
    }

    #[test]
    fn parses_media_hydration_strategy() {
        let json = r#"{
            "islands": {
                "chart": {
                    "module_path": "/static/chart.js",
                    "strategy": { "type": "media", "query": "(min-width: 768px)" }
                }
            }
        }"#;
        let manifest: IslandManifest =
            serde_json::from_str(json).expect("manifest should parse media strategy");
        let strategy = &manifest
            .islands
            .get("chart")
            .expect("chart entry should parse")
            .strategy;
        match strategy {
            HydrationStrategyConfig::Media { query } => {
                assert_eq!(query, "(min-width: 768px)");
            }
            other => panic!("expected media strategy, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod base_path_tests {
    use super::*;

    #[test]
    fn resolves_manifest_urls_without_rewriting_external_urls_or_import_keys() {
        let manifest = IslandManifest {
            islands: HashMap::from([(
                "widget".into(),
                IslandEntry {
                    module_path: "/static/widget.js".into(),
                    export: None,
                    strategy: HydrationStrategyConfig::Immediate,
                    module_preloads: vec![
                        "/static/shared.js".into(),
                        "https://cdn.test/shared.js".into(),
                    ],
                    providers: vec![ProviderSpec {
                        module: "/static/provider.js".into(),
                        export: None,
                        props: serde_json::Value::Null,
                    }],
                    hydrate_fn: None,
                },
            )]),
            assets: ManifestAssets {
                css: Some(CssAsset {
                    path: "/static/app.css".into(),
                    size: Some(42),
                }),
            },
            import_map: HashMap::from([
                ("react".into(), "https://cdn.test/react.js".into()),
                ("widget".into(), "/static/widget.js".into()),
            ]),
            ssr_bundle: Some("ssr-entry.js".into()),
        }
        .with_base_path(&proa_core::BasePath::new("/apps/demo").unwrap());
        let entry = &manifest.islands["widget"];
        assert_eq!(entry.module_path, "/apps/demo/static/widget.js");
        assert_eq!(
            entry.module_preloads,
            ["/apps/demo/static/shared.js", "https://cdn.test/shared.js"]
        );
        assert_eq!(entry.providers[0].module, "/apps/demo/static/provider.js");
        assert_eq!(
            manifest.assets.css.unwrap().path,
            "/apps/demo/static/app.css"
        );
        assert_eq!(manifest.import_map["react"], "https://cdn.test/react.js");
        assert_eq!(manifest.import_map["widget"], "/apps/demo/static/widget.js");
        assert_eq!(manifest.ssr_bundle.as_deref(), Some("ssr-entry.js"));
    }
}
