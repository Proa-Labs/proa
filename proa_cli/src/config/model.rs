// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Shared by the CLI and the host executable that evaluates proa.config.rs.
use std::{borrow::Borrow, collections::BTreeMap};

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "proa.config.rs";
pub const DEFAULT_PUBLIC_DIRECTORY: &str = "public";
pub const DEFAULT_DEV_COMMAND: &str = "proa dev";
pub const DEFAULT_BUILD_COMMAND: &str = "proa build";

/// Text storage for authored config. String literals work with the default type.
pub trait ConfigText: Borrow<str> + Ord + ConfigPath {}
impl<T: Borrow<str> + Ord + ConfigPath> ConfigText for T {}

/// Path storage for authored config. Use an owned type for computed paths.
pub trait ConfigPath: Default + From<&'static str> {}
impl<T: Default + From<&'static str>> ConfigPath for T {}

fn text_is_empty(text: &impl ConfigText) -> bool {
    text.borrow().is_empty()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config<Text: ConfigText = &'static str, Path: ConfigPath = Text> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<ApplicationConfig<Text>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseConfig<Text, Path>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy: Option<DeployConfig<Text>>,
    pub scripts: ScriptsConfig<Text, Path>,
    pub paths: ProjectPaths<Path>,
    pub registries: RegistriesConfig<Text>,
    pub build: BuildConfig<Text, Path>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistriesConfig<Text: ConfigText = &'static str> {
    #[serde(rename = "@proa", skip_serializing_if = "Option::is_none")]
    pub proa: Option<Text>,
    #[serde(rename = "@proa-pro", skip_serializing_if = "Option::is_none")]
    pub proa_pro: Option<Text>,
}

impl<Text: ConfigText> RegistriesConfig<Text> {
    pub fn public_only() -> Self {
        Self {
            proa: Some("https://ui.proa.so/r/{name}.json".into()),
            proa_pro: None,
        }
    }

    pub fn get(&self, alias: &str) -> Option<&str> {
        match alias {
            "@proa" => self.proa.as_ref(),
            "@proa-pro" => self.proa_pro.as_ref(),
            _ => None,
        }
        .map(Text::borrow)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        [
            ("@proa", self.proa.as_ref()),
            ("@proa-pro", self.proa_pro.as_ref()),
        ]
        .into_iter()
        .filter_map(|(alias, url)| url.map(|url| (alias, url.borrow())))
    }
}

impl<Text: ConfigText> Default for RegistriesConfig<Text> {
    fn default() -> Self {
        Self {
            proa_pro: Some("https://console.proa.so/r/{name}.json".into()),
            ..Self::public_only()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApplicationConfig<Text: ConfigText = &'static str> {
    #[serde(skip_serializing_if = "text_is_empty")]
    pub binary: Text,
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
pub struct DatabaseConfig<Text: ConfigText = &'static str, Path: ConfigPath = Text> {
    pub driver: DatabaseDriver,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migrations: Option<Path>,
    #[serde(skip_serializing_if = "text_is_empty")]
    pub integration: Text,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeployConfig<Text: ConfigText = &'static str> {
    #[serde(skip_serializing_if = "text_is_empty")]
    pub provider: Text,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdn: Option<CdnConfig<Text>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "provider", rename_all = "lowercase", deny_unknown_fields)]
pub enum CdnConfig<Text: ConfigText = &'static str> {
    Cloudflare {
        #[serde(rename = "zoneId")]
        zone_id: Text,
        hostname: Text,
    },
    Command {
        command: Vec<Text>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ScriptsConfig<Text: ConfigText = &'static str, Path: ConfigPath = Text> {
    /// Shell command for `proa run dev`. Defaults to `proa dev`.
    pub dev: Text,
    /// Shell command for `proa run build`. Defaults to `proa build`.
    pub build: Text,
    /// Command that builds `styles.css` in the public directory.
    #[serde(skip_serializing_if = "text_is_empty")]
    pub css: Text,
    #[serde(skip_serializing_if = "ScriptHooks::is_empty")]
    pub before: ScriptHooks<Text>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub named: BTreeMap<Text, ScriptConfig<Text, Path>>,
}

/// Named tasks to run before each linked lifecycle.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ScriptHooks<Text: ConfigText = &'static str> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dev: Vec<Text>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<Text>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub test: Vec<Text>,
}

impl<Text: ConfigText> ScriptHooks<Text> {
    pub(super) fn is_empty(&self) -> bool {
        self.dev.is_empty() && self.build.is_empty() && self.test.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ScriptConfig<Text: ConfigText = &'static str, Path: ConfigPath = Text> {
    pub description: Text,
    pub command: Vec<Text>,
    pub cwd: Path,
    #[serde(alias = "dependencies")]
    pub depends_on: Vec<Text>,
    pub inputs: Vec<Text>,
    pub outputs: Vec<Path>,
    pub env: BTreeMap<Text, Text>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ProjectPaths<Path: ConfigPath = &'static str> {
    pub components: Path,
    pub assets: Path,
    /// Public directory for managed assets. Set to `None` to disable publication.
    pub public: Option<Path>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BuildConfig<Text: ConfigText = &'static str, Path: ConfigPath = Text> {
    /// Minify the generated RSJS modules and runtimes served by Axum. Defaults to true.
    pub minify_rsjs: bool,
    /// `None` when the key is absent, so a Tailwind asset can be inferred.
    /// `Some(vec![])` is an explicit "build no assets" and is left alone --
    /// with a plain `Vec` the two were indistinguishable and an empty array
    /// silently grew a Tailwind step back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assets: Option<Vec<AssetConfig<Text, Path>>>,
}

impl<Text: ConfigText, Path: ConfigPath> BuildConfig<Text, Path> {
    pub fn assets(&self) -> &[AssetConfig<Text, Path>] {
        self.assets.as_deref().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AssetConfig<Text: ConfigText = &'static str, Path: ConfigPath = Text> {
    pub kind: Text,
    pub command: Vec<Text>,
    pub input: Path,
    pub output: Path,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<Path>,
}

impl<Text: ConfigText, Path: ConfigPath> Default for Config<Text, Path> {
    fn default() -> Self {
        Self {
            application: None,
            database: None,
            deploy: None,
            scripts: ScriptsConfig::default(),
            paths: ProjectPaths::default(),
            registries: RegistriesConfig::default(),
            build: BuildConfig::default(),
        }
    }
}

impl<Text: ConfigText, Path: ConfigPath> Default for ScriptsConfig<Text, Path> {
    fn default() -> Self {
        Self {
            dev: DEFAULT_DEV_COMMAND.into(),
            build: DEFAULT_BUILD_COMMAND.into(),
            css: "".into(),
            before: ScriptHooks::default(),
            named: BTreeMap::new(),
        }
    }
}

impl<Text: ConfigText, Path: ConfigPath> Default for ScriptConfig<Text, Path> {
    fn default() -> Self {
        Self {
            description: Text::default(),
            command: Vec::new(),
            cwd: ".".into(),
            depends_on: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            env: BTreeMap::new(),
        }
    }
}

impl<Path: ConfigPath> Default for ProjectPaths<Path> {
    fn default() -> Self {
        Self {
            components: Path::from("src/components"),
            // Must sit under the directory the generated server actually serves.
            // `static/` is the URL prefix, not the folder: the axum scaffold mounts
            // `public/` at `/static`, so assets written to `static/proa` were never
            // reachable and every documented `/static/proa/...` link 404'd.
            assets: Path::from("public/proa"),
            public: Some(Path::from(DEFAULT_PUBLIC_DIRECTORY)),
        }
    }
}

impl<Text: ConfigText, Path: ConfigPath> Default for BuildConfig<Text, Path> {
    fn default() -> Self {
        Self {
            minify_rsjs: true,
            assets: None,
        }
    }
}

impl<Text: ConfigText, Path: ConfigPath> Default for AssetConfig<Text, Path> {
    fn default() -> Self {
        Self {
            kind: "command".into(),
            command: Vec::new(),
            input: Path::default(),
            output: Path::default(),
            config: None,
        }
    }
}
