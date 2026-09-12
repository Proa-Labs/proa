// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

/// Configuration for the Proa framework.
///
/// Users can customize paths and behavior by constructing this struct in their main.rs
/// and passing it to the framework. All fields have sensible defaults.
///
/// # Example
///
/// ## Using defaults:
/// ```rust
/// use std::collections::HashMap;
/// use proa_framework_axum::{FrameworkBuilder, ManifestAssets, IslandManifest, ProaConfig};
///
/// # let manifest = IslandManifest {
/// #     islands: HashMap::new(),
/// #     assets: ManifestAssets::default(),
/// #     import_map: HashMap::new(),
/// #     ssr_bundle: None,
/// # };
///
/// let app = FrameworkBuilder::new(manifest)
///     // Config is automatically set to defaults
///     .build();
/// ```
///
/// ## Customizing specific paths:
/// ```rust
/// use std::collections::HashMap;
/// use proa_framework_axum::{FrameworkBuilder, IslandManifest, ManifestAssets, PathsConfig, ProaConfig};
///
/// let config = ProaConfig {
///     paths: PathsConfig {
///         frontend_root: "./client".into(),
///         codegen_output: "./client/src/.gen".into(),
///         ..Default::default()
///     },
///     ..Default::default()
/// };
///
/// # let manifest = IslandManifest {
/// #     islands: HashMap::new(),
/// #     assets: ManifestAssets::default(),
/// #     import_map: HashMap::new(),
/// #     ssr_bundle: None,
/// # };
///
/// let app = FrameworkBuilder::new(manifest)
///     .with_config(config)
///     .build();
/// ```
///
/// ## For non-standard monorepo structure:
/// ```rust
/// use proa_framework_axum::{ProaConfig, PathsConfig};
///
/// let config = ProaConfig {
///     paths: PathsConfig {
///         crates_root: "./backend/crates".into(),
///         frontend_root: "./apps/web".into(),
///         codegen_output: "./apps/web/src/.generated".into(),
///         public_dir: "./apps/web/public".into(),
///         build_output: "./apps/web/dist".into(),
///         ..Default::default()
///     },
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct ProaConfig {
    pub paths: PathsConfig,
    pub codegen: CodegenConfig,
    pub server: ServerConfig,
}

/// Path configuration for the framework.
///
/// All paths should be relative to the project root (where Cargo.toml workspace is).
#[derive(Debug, Clone)]
pub struct PathsConfig {
    /// Root of the Rust crates workspace. Default: "./crates"
    pub crates_root: PathBuf,

    /// Main server crate. Default: "./crates/server"
    pub server_crate: PathBuf,

    /// Frontend application root. Default: "./apps/web"
    pub frontend_root: PathBuf,

    /// Where codegen outputs TypeScript files. Default: "./apps/web/src/.generated"
    pub codegen_output: PathBuf,

    /// Public static assets directory. Default: "./apps/web/public"
    pub public_dir: PathBuf,

    /// Frontend build output directory. Default: "./apps/web/dist"
    pub build_output: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            crates_root: PathBuf::from("./crates"),
            server_crate: PathBuf::from("./crates/server"),
            frontend_root: PathBuf::from("./apps/web"),
            codegen_output: PathBuf::from("./apps/web/src/.generated"),
            public_dir: PathBuf::from("./apps/web/public"),
            build_output: PathBuf::from("./apps/web/dist"),
        }
    }
}

/// Codegen configuration.
#[derive(Debug, Clone)]
pub struct CodegenConfig {
    /// Generate TypeScript route definitions. Default: true
    pub routes: bool,

    /// Generate TypeScript component wrappers. Default: true
    pub components: bool,

    /// Generate island hydration code. Default: true
    pub islands: bool,

    /// Watch mode for auto-regeneration. Default: true in debug, false in release
    pub watch: bool,
}

impl Default for CodegenConfig {
    fn default() -> Self {
        Self {
            routes: true,
            components: true,
            islands: true,
            watch: cfg!(debug_assertions),
        }
    }
}

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Server port. Default: 3000
    pub port: u16,

    /// Server host. Default: "127.0.0.1"
    pub host: String,

    /// Enable auto-reload script in development. Default: true in debug
    pub auto_reload: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            host: "127.0.0.1".to_string(),
            auto_reload: cfg!(debug_assertions),
        }
    }
}
