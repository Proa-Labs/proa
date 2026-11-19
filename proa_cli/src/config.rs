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

mod model;
mod rust;
pub(crate) use rust::DevConfigRunner;

pub(crate) use model::DEFAULT_PUBLIC_DIRECTORY;
pub use model::{Config, DatabaseDriver, CONFIG_FILE};
pub type ProjectConfig = Config<String, PathBuf>;

macro_rules! owned_config_sections {
    ($($name:ident <$($storage:ty),+ $(,)?>),+ $(,)?) => {
        $(pub type $name = model::$name<$($storage),+>;)*
    };
}

owned_config_sections! {
    ApplicationConfig<String>,
    AssetConfig<String, PathBuf>,
    BuildConfig<String, PathBuf>,
    CdnConfig<String>,
    DatabaseConfig<String, PathBuf>,
    DeployConfig<String>,
    ProjectPaths<PathBuf>,
    RegistriesConfig<String>,
    ScriptConfig<String, PathBuf>,
    ScriptHooks<String>,
    ScriptsConfig<String, PathBuf>,
}

pub const LOCK_FILE: &str = "proa.lock.json";
pub const LOCK_SCHEMA_URL: &str = "https://proa.so/schema/proa.lock.json";
pub(crate) const DEFAULT_DEV_COMMAND: &str = "cargo run";
pub(crate) const DEFAULT_BUILD_COMMAND: &str = "cargo build --release";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ComponentLockFile {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub components: BTreeMap<String, InstalledComponent>,
    // Component installs rewrite this lock. Preserve scaffold metadata and
    // version keys written by the site generator and capability commands.
    #[serde(flatten)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl ComponentLockFile {
    pub(crate) fn scaffold_capabilities(&self) -> impl Iterator<Item = &str> {
        self.metadata
            .get("scaffold")
            .and_then(|scaffold| scaffold.get("capabilities"))
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|capability| capability.get("id")?.as_str())
    }

    pub(crate) fn scaffold_markdown(&self) -> Option<bool> {
        if self
            .scaffold_capabilities()
            .any(|id| id == "representation:markdown")
        {
            Some(true)
        } else if self.scaffold_capabilities().any(|id| id == "base:site") {
            Some(false)
        } else {
            None
        }
    }
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

impl Default for ComponentLockFile {
    fn default() -> Self {
        Self {
            schema: Some(LOCK_SCHEMA_URL.into()),
            components: BTreeMap::new(),
            metadata: BTreeMap::new(),
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
    load_project_config_with_runner(root, None)
}

pub(crate) fn load_project_config_with_runner(
    root: &Path,
    runner: Option<&mut DevConfigRunner>,
) -> Result<ProjectConfig> {
    let path = project_config_path(root);
    let config = evaluate_project_config(root, path.as_deref(), runner)?;
    for (field, command) in lifecycle_commands(&config) {
        anyhow::ensure!(
            !command.trim().is_empty(),
            "{field} must specify a command in {CONFIG_FILE}"
        );
    }
    Ok(config)
}

fn evaluate_project_config(
    root: &Path,
    path: Option<&Path>,
    runner: Option<&mut DevConfigRunner>,
) -> Result<ProjectConfig> {
    let mut config = match path {
        Some(path) => rust::load(root, path, runner)
            .with_context(|| format!("evaluate {}", path.display()))?,
        None => inferred_project_config(),
    };
    fill_inferred_defaults(root, &mut config);
    Ok(config)
}

fn lifecycle_commands(config: &ProjectConfig) -> [(&'static str, &str); 2] {
    [
        ("scripts.dev", &config.scripts.dev),
        ("scripts.build", &config.scripts.build),
    ]
}

pub(crate) fn inferred_project_config() -> ProjectConfig {
    ProjectConfig {
        scripts: ScriptsConfig {
            dev: DEFAULT_DEV_COMMAND.to_owned(),
            build: DEFAULT_BUILD_COMMAND.to_owned(),
            ..Default::default()
        },
        ..ProjectConfig::default()
    }
}

pub(crate) fn project_config_path(root: &Path) -> Option<PathBuf> {
    let path = root.join(CONFIG_FILE);
    path.exists().then_some(path)
}

pub(crate) fn generated_rust_config(config: &ProjectConfig) -> Result<String> {
    rust::source(config)
}

pub(crate) fn add_rust_capability(
    source: &str,
    before: &ProjectConfig,
    after: &ProjectConfig,
) -> Result<String> {
    rust::with_capability_changes(source, before, after)
}

#[derive(Debug, Serialize)]
pub struct ConfigValidation {
    pub config_path: Option<PathBuf>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

pub fn validate_project_config(root: &Path) -> Result<ConfigValidation> {
    let mut validation = ConfigValidation {
        config_path: project_config_path(root),
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    let config = evaluate_project_config(root, validation.config_path.as_deref(), None)?;
    if validation.config_path.is_none() {
        validation
            .warnings
            .push(format!("{CONFIG_FILE} not found; using inferred defaults"));
    }

    for (field, command) in lifecycle_commands(&config) {
        if let Err(error) = parse_script_command(field, command) {
            validation.errors.push(error.to_string());
        }
    }
    if let Err(error) = css_script_asset(&config) {
        validation.errors.push(error.to_string());
    }
    for (name, path) in [
        ("paths.components", &config.paths.components),
        ("paths.assets", &config.paths.assets),
    ] {
        if let Err(error) = validate_project_relative_path(path) {
            validation.errors.push(format!("{name}: {error}"));
        }
    }
    if let Some(path) = &config.paths.public {
        if let Err(error) = validate_project_relative_path(path) {
            validation.errors.push(format!("paths.public: {error}"));
        }
    }
    if config.paths.components == config.paths.assets {
        validation
            .errors
            .push("paths.components and paths.assets must be different".to_string());
    }

    if config.registries.proa.is_none() {
        validation
            .errors
            .push("registries.@proa is required".to_string());
    }
    for (registry, template) in config.registries.iter() {
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

pub(crate) fn validate_project_relative_path(path: &Path) -> std::result::Result<(), String> {
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
    let assets = if tailwind {
        r#"vec![proa::config::AssetConfig {
                kind: "tailwind",
                input: "styles/input.css",
                output: "public/styles.css",
                ..Default::default()
            }]"#
    } else {
        "vec![]"
    };
    Ok(format!(
        r#"use proa::config::{{BuildConfig, Config, Result, ScriptsConfig}};

pub async fn config() -> Result<Config> {{
    Ok(Config {{
        scripts: ScriptsConfig {{
            dev: {DEFAULT_DEV_COMMAND:?},
            build: {DEFAULT_BUILD_COMMAND:?},
            ..Default::default()
        }},
        build: BuildConfig {{
            assets: Some({assets}),
            ..Default::default()
        }},
        ..Config::default()
    }})
}}
"#
    ))
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

fn fill_inferred_defaults(root: &Path, config: &mut ProjectConfig) {
    if config.paths.components.as_os_str().is_empty() {
        config.paths.components = ProjectPaths::default().components;
    }
    if config.paths.assets.as_os_str().is_empty() {
        config.paths.assets = ProjectPaths::default().assets;
    }
    if config.registries.proa.is_none() && config.registries.proa_pro.is_none() {
        config.registries = RegistriesConfig::default();
    }
    if config.scripts.css.trim().is_empty()
        && config.build.assets.is_none()
        && root.join("styles/input.css").exists()
    {
        config.build.assets = Some(vec![tailwind_asset(stylesheet_output(config))]);
    }
}

pub(crate) fn css_script_asset(config: &ProjectConfig) -> Result<Option<AssetConfig>> {
    let script = config.scripts.css.trim();
    if script.is_empty() {
        return Ok(None);
    }
    Ok(Some(AssetConfig {
        command: parse_script_command("scripts.css", script)?,
        output: stylesheet_output(config),
        ..Default::default()
    }))
}

fn stylesheet_output(config: &ProjectConfig) -> PathBuf {
    config
        .paths
        .public
        .as_deref()
        .unwrap_or(Path::new(DEFAULT_PUBLIC_DIRECTORY))
        .join("styles.css")
}

pub(crate) fn parse_script_command(field: &str, source: &str) -> Result<Vec<String>> {
    let command = shlex::split(source).with_context(|| format!("{field} has invalid quoting"))?;
    anyhow::ensure!(!command.is_empty(), "{field} must specify a command");
    Ok(command)
}

fn tailwind_asset(output: PathBuf) -> AssetConfig {
    AssetConfig {
        kind: "tailwind".into(),
        command: Vec::new(),
        input: PathBuf::from("styles/input.css"),
        output,
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
    fn adding_a_capability_preserves_an_async_config_and_its_error_type() {
        let temp = tempfile::tempdir().unwrap();
        let source = r#"
use proa::config::{CdnConfig, Config, DeployConfig, ProjectPaths, ScriptsConfig, tokio};

pub async fn config() -> std::io::Result<Config<&'static str, String>> {
    let public = tokio::fs::read_to_string("public-dir.txt").await?;
    return Ok(Config {
        scripts: ScriptsConfig {
            dev: "cargo run",
            build: "cargo build --release",
            ..Default::default()
        },
        paths: ProjectPaths { public: Some(public), ..Default::default() },
        deploy: Some(DeployConfig {
            cdn: Some(CdnConfig::Command { command: vec!["purge-cache"] }),
            ..Default::default()
        }),
        ..Config::default()
    });
}
"#;
        let before = ProjectConfig::default();
        let mut after = before.clone();
        after.database = Some(DatabaseConfig {
            driver: DatabaseDriver::Sqlite,
            ..Default::default()
        });
        after.deploy = Some(DeployConfig {
            provider: "docker".into(),
            ..Default::default()
        });
        let patched = add_rust_capability(source, &before, &after).unwrap();
        fs::write(temp.path().join(CONFIG_FILE), patched).unwrap();
        fs::write(temp.path().join("public-dir.txt"), "public").unwrap();
        let actual = load_project_config(temp.path()).unwrap();
        assert_eq!(actual.paths.public, Some("public".into()));
        assert_eq!(actual.database, after.database);
        let deploy = actual.deploy.unwrap();
        assert_eq!(deploy.provider, "docker");
        assert_eq!(
            deploy.cdn,
            Some(CdnConfig::Command {
                command: vec!["purge-cache".into()]
            })
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
            generated_rust_config(&ProjectConfig {
                paths: ProjectPaths {
                    components: "../outside".into(),
                    assets: "/tmp/assets".into(),
                    public: Some("../public".into()),
                },
                registries: RegistriesConfig {
                    proa: Some("https://ui.proa.so/r/component.json".into()),
                    proa_pro: Some("http://console.proa.so/r/{name}.json".into()),
                },
                ..inferred_project_config()
            })
            .unwrap(),
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
                .any(|error| error.contains("paths.public")),
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
    fn generated_config_has_a_typed_entry_and_explicit_asset_choice() {
        for tailwind in [false, true] {
            let source = generated_project_config(tailwind).unwrap();
            assert!(syn::parse_file(&source).is_ok());
            assert!(source.contains("dev: \"cargo run\""));
            assert!(source.contains("build: \"cargo build --release\""));
            assert_eq!(source.contains("styles/input.css"), tailwind);
            assert_eq!(source.contains("assets: Some(vec![])"), !tailwind);
        }
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
