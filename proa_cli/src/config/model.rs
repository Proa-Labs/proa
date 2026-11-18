// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Shared by the CLI and the host executable that evaluates proa.config.rs.
use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "proa.config.rs";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(
    default,
    bound(deserialize = "Command: Deserialize<'de> + From<&'static str>")
)]
pub struct Config<Command = &'static str> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<ApplicationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub representations: Option<RepresentationsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub styling: Option<StylingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy: Option<DeployConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ContentConfig>,
    pub scripts: ScriptsConfig<Command>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<TestConfig>,
    pub paths: ProjectPaths,
    pub registries: BTreeMap<String, String>,
    pub dev: DevConfig,
    pub build: BuildConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApplicationConfig {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub binary: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub package: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub starter: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub template: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseDriver {
    #[default]
    None,
    Sqlite,
}

impl DatabaseDriver {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Sqlite => "sqlite",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct DatabaseConfig {
    pub driver: DatabaseDriver,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migrations: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_migrations_on_startup: Option<bool>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub integration: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub provider: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RepresentationsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<HtmlRepresentationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<MarkdownRepresentationConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HtmlRepresentationConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkdownRepresentationConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negotiate_on_canonical_path: Option<bool>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub explicit_suffix: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub root_path: String,
}

impl MarkdownRepresentationConfig {
    pub fn site() -> Self {
        Self {
            enabled: true,
            negotiate_on_canonical_path: Some(true),
            explicit_suffix: ".md".into(),
            root_path: "/index.md".into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StylingConfig {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeployConfig {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdn: Option<CdnConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "provider", rename_all = "lowercase", deny_unknown_fields)]
pub enum CdnConfig {
    Cloudflare {
        #[serde(rename = "zoneId")]
        zone_id: String,
        hostname: String,
    },
    Command {
        command: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContentConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<ContentCollectionConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blog: Option<ContentCollectionConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContentCollectionConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TestConfig {
    pub before: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(
    default,
    bound(deserialize = "Command: Deserialize<'de> + From<&'static str>")
)]
pub struct ScriptsConfig<Command = &'static str> {
    /// Project script or linked Cargo shorthand for `proa dev`.
    pub dev: Command,
    /// Project script or linked Cargo shorthand for `proa build`.
    pub build: Command,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub named: BTreeMap<String, ScriptConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ScriptConfig {
    pub description: String,
    pub command: Vec<String>,
    pub cwd: PathBuf,
    #[serde(alias = "dependencies")]
    pub depends_on: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<PathBuf>,
    pub env: BTreeMap<String, String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct DevConfig {
    pub host: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    pub watch: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BuildConfig {
    /// Minify the generated RSJS modules and runtimes served by Axum. Defaults to true.
    pub minify_rsjs: bool,
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

impl<Command: From<&'static str>> Default for Config<Command> {
    fn default() -> Self {
        Self {
            application: None,
            database: None,
            ui: None,
            representations: None,
            styling: None,
            deploy: None,
            content: None,
            scripts: ScriptsConfig::default(),
            test: None,
            paths: ProjectPaths::default(),
            registries: default_registries(),
            dev: DevConfig::default(),
            build: BuildConfig::default(),
        }
    }
}

impl<Command: From<&'static str>> Default for ScriptsConfig<Command> {
    fn default() -> Self {
        Self {
            dev: "".into(),
            build: "".into(),
            named: BTreeMap::new(),
        }
    }
}

impl Default for ScriptConfig {
    fn default() -> Self {
        Self {
            description: String::new(),
            command: Vec::new(),
            cwd: ".".into(),
            depends_on: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            env: BTreeMap::new(),
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
            minify_rsjs: true,
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

pub(super) fn default_registries() -> BTreeMap<String, String> {
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
pub(super) fn default_watch_paths() -> Vec<PathBuf> {
    [
        "src",
        "styles",
        "migrations",
        "Cargo.toml",
        "build.rs",
        CONFIG_FILE,
        "proa.lock.json",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}
