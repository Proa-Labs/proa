// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "proa.config.json";
pub const LOCK_FILE: &str = "proa.lock.json";
pub const CONFIG_SCHEMA_URL: &str = "https://proa.so/schema/proa.config.json";
pub const LOCK_SCHEMA_URL: &str = "https://proa.so/schema/proa.lock.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ProjectConfig {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub representations: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub styling: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub tasks: BTreeMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<serde_json::Value>,
    pub paths: ProjectPaths,
    pub registries: BTreeMap<String, String>,
    pub dev: DevConfig,
    pub build: BuildConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ProjectPaths {
    pub components: PathBuf,
    pub assets: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layouts: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public: Option<PathBuf>,
    #[serde(rename = "runtimeData", skip_serializing_if = "Option::is_none")]
    pub runtime_data: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DevConfig {
    pub command: Vec<String>,
    pub host: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    pub watch: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BuildConfig {
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    /// `None` when the key is absent, so a Tailwind asset can be inferred.
    /// `Some(vec![])` is an explicit "build no assets" and is left alone --
    /// with a plain `Vec` the two were indistinguishable and an empty array
    /// silently grew a Tailwind step back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assets: Option<Vec<AssetConfig>>,
}

impl BuildConfig {
    pub fn assets(&self) -> &[AssetConfig] {
        self.assets.as_deref().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AssetConfig {
    pub kind: String,
    pub command: Vec<String>,
    pub input: PathBuf,
    pub output: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ComponentLockFile {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub components: BTreeMap<String, InstalledComponent>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct InstalledComponent {
    pub version: String,
    pub registry: String,
    pub mode: String,
    pub source_hash: String,
    pub local_hash_at_install: String,
    pub installed_files: Vec<PathBuf>,
    pub dependencies: Vec<String>,
    pub rust_modules: Vec<InstalledRustModules>,
    pub cargo_dependencies: Vec<InstalledCargoDependency>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledRustModules {
    pub target: PathBuf,
    pub modules: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledCargoDependency {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    pub features: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_features: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConfigInitReport {
    pub root: PathBuf,
    pub config_path: PathBuf,
    pub lock_path: PathBuf,
    pub wrote_config: bool,
    pub wrote_lock: bool,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            schema: Some(CONFIG_SCHEMA_URL.into()),
            application: None,
            database: None,
            ui: None,
            representations: None,
            styling: None,
            deploy: None,
            content: None,
            tasks: BTreeMap::new(),
            test: None,
            paths: ProjectPaths::default(),
            registries: default_registries(),
            dev: DevConfig::default(),
            build: BuildConfig::default(),
        }
    }
}

impl Default for ProjectPaths {
    fn default() -> Self {
        Self {
            components: PathBuf::from("src/components"),
            // Must sit under the directory the generated server actually serves.
            // `static/` is the URL prefix, not the folder: the axum scaffold mounts
            // `public/` at `/static`, so assets written to `static/proa` were never
            // reachable and every documented `/static/proa/...` link 404'd.
            assets: PathBuf::from("public/proa"),
            pages: None,
            endpoints: None,
            layouts: None,
            models: None,
            public: None,
            runtime_data: None,
        }
    }
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            command: vec!["cargo".into(), "run".into()],
            host: "127.0.0.1".into(),
            port: 3000,
            before: Vec::new(),
            watch: default_watch_paths(),
        }
    }
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            command: vec!["cargo".into(), "build".into(), "--release".into()],
            before: Vec::new(),
            assets: None,
        }
    }
}

impl Default for AssetConfig {
    fn default() -> Self {
        Self {
            kind: "command".into(),
            command: Vec::new(),
            input: PathBuf::new(),
            output: PathBuf::new(),
            config: None,
        }
    }
}

impl Default for ComponentLockFile {
    fn default() -> Self {
        Self {
            schema: Some(LOCK_SCHEMA_URL.into()),
            components: BTreeMap::new(),
        }
    }
}

impl Default for InstalledComponent {
    fn default() -> Self {
        Self {
            version: String::new(),
            registry: String::new(),
            mode: "source".into(),
            source_hash: String::new(),
            local_hash_at_install: String::new(),
            installed_files: Vec::new(),
            dependencies: Vec::new(),
            rust_modules: Vec::new(),
            cargo_dependencies: Vec::new(),
        }
    }
}

pub fn load_project_config(root: &Path) -> Result<ProjectConfig> {
    let (config, unknown_keys) = load_project_config_with_unknown_keys(root)?;
    // A misspelled key silently falling back to its default is the most
    // confusing config failure mode; always say something.
    for key in &unknown_keys {
        eprintln!("warning: {CONFIG_FILE}: unknown key `{key}` is ignored");
    }
    Ok(config)
}

pub fn load_project_config_with_unknown_keys(root: &Path) -> Result<(ProjectConfig, Vec<String>)> {
    let path = root.join(CONFIG_FILE);
    if path.exists() {
        let source =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut unknown_keys = Vec::new();
        let mut deserializer = serde_json::Deserializer::from_str(&source);
        let mut config: ProjectConfig = serde_ignored::deserialize(&mut deserializer, |ignored| {
            unknown_keys.push(ignored.to_string());
        })
        .with_context(|| format!("parse {}", path.display()))?;
        fill_inferred_defaults(root, &mut config);
        Ok((config, unknown_keys))
    } else {
        Ok((inferred_project_config(root), Vec::new()))
    }
}

#[derive(Debug, Serialize)]
pub struct ConfigValidation {
    pub config_path: Option<PathBuf>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

pub fn validate_project_config(root: &Path) -> Result<ConfigValidation> {
    let path = root.join(CONFIG_FILE);
    let mut validation = ConfigValidation {
        config_path: path.exists().then_some(path),
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    let (config, unknown_keys) = load_project_config_with_unknown_keys(root)?;
    if validation.config_path.is_none() {
        validation
            .warnings
            .push(format!("{CONFIG_FILE} not found; using inferred defaults"));
    }
    for key in unknown_keys {
        validation
            .warnings
            .push(format!("unknown key `{key}` is ignored"));
    }

    if config.dev.command.is_empty() {
        validation
            .errors
            .push("dev.command is empty; `proa dev` cannot start a server".to_string());
    }
    if config.build.command.is_empty() {
        validation
            .errors
            .push("build.command is empty; `proa build` has nothing to run".to_string());
    }
    if config.dev.watch.is_empty() {
        validation
            .warnings
            .push("dev.watch is empty; `proa dev` will never restart on changes".to_string());
    }

    for (name, path) in [
        ("paths.components", &config.paths.components),
        ("paths.assets", &config.paths.assets),
    ] {
        if let Err(error) = validate_project_relative_path(path) {
            validation.errors.push(format!("{name}: {error}"));
        }
    }
    if config.paths.components == config.paths.assets {
        validation
            .errors
            .push("paths.components and paths.assets must be different".to_string());
    }

    if !config.registries.contains_key("@proa") {
        validation
            .errors
            .push("registries.@proa is required".to_string());
    }
    for (registry, template) in &config.registries {
        if let Err(error) = validate_registry_template(template) {
            validation
                .errors
                .push(format!("registries.{registry}: {error}"));
        }
    }

    for (index, asset) in config.build.assets().iter().enumerate() {
        match asset.kind.as_str() {
            "tailwind" => {
                if !root.join(&asset.input).exists() {
                    validation.warnings.push(format!(
                        "build.assets[{index}]: tailwind input `{}` does not exist; the asset build will be skipped",
                        asset.input.display()
                    ));
                }
                if asset.output.as_os_str().is_empty() {
                    validation.errors.push(format!(
                        "build.assets[{index}]: tailwind asset has no output path"
                    ));
                }
            }
            "command" => {
                if asset.command.is_empty() {
                    validation.warnings.push(format!(
                        "build.assets[{index}]: command asset with an empty command does nothing"
                    ));
                }
            }
            other => validation.errors.push(format!(
                "build.assets[{index}]: unknown kind `{other}` (expected `tailwind` or `command`)"
            )),
        }
    }

    Ok(validation)
}

fn validate_project_relative_path(path: &Path) -> std::result::Result<(), String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(format!(
            "`{}` must be a non-empty project-relative path",
            path.display()
        ));
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "`{}` contains an unsafe path component",
            path.display()
        ));
    }
    Ok(())
}

fn validate_registry_template(template: &str) -> std::result::Result<(), String> {
    if template.matches("{name}").count() != 1 {
        return Err(format!(
            "`{template}` must contain exactly one `{{name}}` placeholder"
        ));
    }
    let rendered = template.replace("{name}", "button");
    if rendered.starts_with("https://") || rendered.starts_with("http://") {
        let url = Url::parse(&rendered)
            .map_err(|error| format!("`{template}` is not a valid registry URL: {error}"))?;
        if url.scheme() == "http"
            && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
        {
            return Err("HTTP is only allowed for localhost registries; use HTTPS".to_string());
        }
        return Ok(());
    }
    if rendered.starts_with("file://") {
        let url = Url::parse(&rendered)
            .map_err(|error| format!("`{template}` is not a valid file URL: {error}"))?;
        url.to_file_path()
            .map_err(|_| format!("`{template}` is not a valid file URL"))?;
        return Ok(());
    }
    if rendered.contains("://") {
        return Err(format!("`{template}` uses an unsupported URL scheme"));
    }
    if rendered.trim().is_empty() {
        return Err("registry path cannot be empty".to_string());
    }
    Ok(())
}

pub fn load_component_lock(root: &Path) -> Result<ComponentLockFile> {
    let path = root.join(LOCK_FILE);
    if path.exists() {
        let source =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut lock: ComponentLockFile =
            serde_json::from_str(&source).with_context(|| format!("parse {}", path.display()))?;
        if lock.schema.is_none() {
            lock.schema = Some(LOCK_SCHEMA_URL.into());
        }
        Ok(lock)
    } else {
        Ok(ComponentLockFile::default())
    }
}

pub fn generated_project_config(tailwind: bool) -> Result<String> {
    let mut config = ProjectConfig::default();
    // Always write the key, even empty. A generated `--no-tailwind` project
    // states "no asset builds" outright, so dropping a `styles/input.css` in
    // later cannot silently reintroduce a Tailwind step nobody configured.
    config.build.assets = Some(if tailwind {
        vec![tailwind_asset()]
    } else {
        Vec::new()
    });
    Ok(serde_json::to_string_pretty(&config)? + "\n")
}

pub fn generated_component_lock() -> Result<String> {
    Ok(serde_json::to_string_pretty(&ComponentLockFile::default())? + "\n")
}

pub fn init_project_files(root: &Path, force: bool) -> Result<ConfigInitReport> {
    fs::create_dir_all(root).with_context(|| format!("create {}", root.display()))?;

    let config_path = root.join(CONFIG_FILE);
    let lock_path = root.join(LOCK_FILE);
    let mut wrote_config = false;
    let mut wrote_lock = false;

    if force || !config_path.exists() {
        fs::write(&config_path, generated_project_config(false)?)
            .with_context(|| format!("write {}", config_path.display()))?;
        wrote_config = true;
    }

    if force || !lock_path.exists() {
        fs::write(&lock_path, generated_component_lock()?)
            .with_context(|| format!("write {}", lock_path.display()))?;
        wrote_lock = true;
    }

    Ok(ConfigInitReport {
        root: root.to_path_buf(),
        config_path,
        lock_path,
        wrote_config,
        wrote_lock,
    })
}

fn inferred_project_config(root: &Path) -> ProjectConfig {
    let mut config = ProjectConfig::default();
    fill_inferred_defaults(root, &mut config);
    config
}

fn fill_inferred_defaults(root: &Path, config: &mut ProjectConfig) {
    if config.schema.is_none() {
        config.schema = Some(CONFIG_SCHEMA_URL.into());
    }
    if config.paths.components.as_os_str().is_empty() {
        config.paths.components = ProjectPaths::default().components;
    }
    if config.paths.assets.as_os_str().is_empty() {
        config.paths.assets = ProjectPaths::default().assets;
    }
    if config.registries.is_empty() {
        config.registries = default_registries();
    }
    if config.dev.command.is_empty() {
        config.dev.command = DevConfig::default().command;
    }
    if config.dev.host.is_empty() {
        config.dev.host = DevConfig::default().host;
    }
    if config.dev.port == 0 {
        config.dev.port = DevConfig::default().port;
    }
    if config.dev.watch.is_empty() {
        config.dev.watch = default_watch_paths();
    }
    if config.build.command.is_empty() {
        config.build.command = BuildConfig::default().command;
    }
    if config.build.assets.is_none() && root.join("styles/input.css").exists() {
        config.build.assets = Some(vec![tailwind_asset()]);
    }
}

fn default_registries() -> BTreeMap<String, String> {
    [
        ("@proa", "https://ui.proa.so/r/{name}.json"),
        ("@proa-pro", "https://console.proa.so/r/{name}.json"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect()
}

// Cargo.lock is intentionally not watched: `cargo run` can rewrite it during
// the build the watcher just triggered, which restarts the dev loop forever.
fn default_watch_paths() -> Vec<PathBuf> {
    ["src", "styles", "Cargo.toml", "build.rs", CONFIG_FILE]
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

fn tailwind_asset() -> AssetConfig {
    AssetConfig {
        kind: "tailwind".into(),
        command: Vec::new(),
        input: PathBuf::from("styles/input.css"),
        output: PathBuf::from("public/styles.css"),
        // Tailwind v4 discovers sources from the CSS entrypoint's `@source`
        // directives. The optional config field remains supported for older
        // and custom projects, but generated v4 sites do not need `-c`.
        config: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_reports_unknown_keys_and_bad_asset_kinds() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(CONFIG_FILE),
            r#"{
                "dev": { "comand": ["cargo", "run"] },
                "build": { "assets": [{ "kind": "webpack", "input": "a", "output": "b" }] }
            }"#,
        )
        .unwrap();

        let validation = validate_project_config(temp.path()).unwrap();
        assert!(
            validation
                .warnings
                .iter()
                .any(|warning| warning.contains("dev.comand")),
            "{validation:?}"
        );
        assert!(
            validation
                .errors
                .iter()
                .any(|error| error.contains("unknown kind `webpack`")),
            "{validation:?}"
        );
    }

    #[test]
    fn validate_passes_defaults_and_flags_missing_config() {
        let temp = tempfile::tempdir().unwrap();
        let validation = validate_project_config(temp.path()).unwrap();
        assert!(validation.errors.is_empty(), "{validation:?}");
        assert!(
            validation
                .warnings
                .iter()
                .any(|warning| warning.contains("not found")),
            "{validation:?}"
        );
    }

    #[test]
    fn validate_rejects_unsafe_paths_and_broken_registry_templates() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(CONFIG_FILE),
            r#"{
                "paths": { "components": "../outside", "assets": "/tmp/assets" },
                "registries": {
                    "@proa": "https://ui.proa.so/r/component.json",
                    "@proa-pro": "http://console.proa.so/r/{name}.json"
                }
            }"#,
        )
        .unwrap();

        let validation = validate_project_config(temp.path()).unwrap();
        assert!(
            validation
                .errors
                .iter()
                .any(|error| error.contains("paths.components")),
            "{validation:?}"
        );
        assert!(
            validation
                .errors
                .iter()
                .any(|error| error.contains("paths.assets")),
            "{validation:?}"
        );
        assert!(
            validation
                .errors
                .iter()
                .any(|error| error.contains("exactly one `{name}`")),
            "{validation:?}"
        );
        assert!(
            validation
                .errors
                .iter()
                .any(|error| error.contains("use HTTPS")),
            "{validation:?}"
        );
    }

    #[test]
    fn load_reports_unknown_keys() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(CONFIG_FILE), r#"{ "prot": 3001 }"#).unwrap();

        let (_, unknown) = load_project_config_with_unknown_keys(temp.path()).unwrap();
        assert_eq!(unknown, ["prot"]);
    }

    #[test]
    fn generated_config_includes_tailwind_asset_when_requested() {
        let json = generated_project_config(true).unwrap();

        assert!(json.contains("\"$schema\": \"https://proa.so/schema/proa.config.json\""));
        assert!(json.contains("\"components\": \"src/components\""));
        assert!(json.contains("\"@proa\""));
        assert!(json.contains("https://console.proa.so/r/{name}.json"));
        assert!(json.contains("\"command\": ["));
        assert!(json.contains("\"cargo\""));
        assert!(json.contains("\"kind\": \"tailwind\""));
        assert!(json.contains("\"styles/input.css\""));
        assert!(!json.contains("\"config\""));
        assert!(!json.contains("frontend"));
    }

    #[test]
    fn generated_config_can_omit_assets() {
        let json = generated_project_config(false).unwrap();

        assert!(json.contains("\"assets\": []"));
        assert!(!json.contains("\"kind\": \"tailwind\""));
    }

    #[test]
    fn generated_lock_starts_empty() {
        let json = generated_component_lock().unwrap();

        assert!(json.contains("\"$schema\": \"https://proa.so/schema/proa.lock.json\""));
        assert!(json.contains("\"components\": {}"));
    }

    #[test]
    fn init_project_files_writes_config_and_lock() {
        let temp = tempfile::tempdir().unwrap();
        let report = init_project_files(temp.path(), false).unwrap();

        assert!(report.wrote_config);
        assert!(report.wrote_lock);
        assert!(temp.path().join(CONFIG_FILE).exists());
        assert!(temp.path().join(LOCK_FILE).exists());

        let second = init_project_files(temp.path(), false).unwrap();
        assert!(!second.wrote_config);
        assert!(!second.wrote_lock);
    }
}
