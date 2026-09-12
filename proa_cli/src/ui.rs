// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Write,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use reqwest::{
    blocking::{Client, RequestBuilder},
    StatusCode, Url,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::{
    self, ComponentLockFile, InstalledCargoDependency, InstalledComponent, InstalledRustModules,
    ProjectConfig, LOCK_FILE,
};

pub const COMPONENT_MANIFEST_SCHEMA: &str = "https://proa.so/schema/ui-component-manifest-v1.json";
const COMPONENT_MANIFEST_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_COMPONENT_FILES: usize = 512;
const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TRANSACTION_BYTES: usize = 64 * 1024 * 1024;
const PUBLIC_REGISTRY: &str = "@proa";
const PRO_REGISTRY: &str = "@proa-pro";

#[derive(Debug, Clone)]
pub struct AddOptions {
    pub root: PathBuf,
    pub component: String,
    pub registry: Option<String>,
    pub dry_run: bool,
    /// Overwrite components already recorded in the lock file.
    pub force: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddReport {
    pub root: PathBuf,
    pub requested: String,
    pub resolved: String,
    pub registry: String,
    pub version: String,
    pub dry_run: bool,
    pub components: Vec<InstalledComponentReport>,
    pub files_written: Vec<PathBuf>,
    pub files_unchanged: Vec<PathBuf>,
    pub lock_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledComponentReport {
    pub name: String,
    pub kind: RegistryItemKind,
    pub registry: String,
    pub version: String,
    pub tier: ComponentTier,
    pub dependency: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentTier {
    Free,
    Pro,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RegistryItemKind {
    #[default]
    Component,
    Block,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ComponentManifest {
    #[serde(rename = "$schema")]
    schema: String,
    schema_version: u32,
    name: String,
    version: String,
    registry: String,
    #[serde(default)]
    kind: RegistryItemKind,
    tier: ComponentTier,
    license: String,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    rust_modules: Vec<ComponentRustModules>,
    #[serde(default)]
    cargo_dependencies: Vec<ComponentCargoDependency>,
    files: Vec<ComponentManifestFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ComponentManifestFile {
    source: String,
    target: String,
    sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ComponentRustModules {
    target: String,
    modules: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ComponentCargoDependency {
    name: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    git: Option<String>,
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    default_features: Option<bool>,
}

#[derive(Debug, Clone)]
enum ResourceLocation {
    Http(Url),
    File(PathBuf),
}

impl ResourceLocation {
    fn display(&self) -> String {
        match self {
            Self::Http(url) => url.to_string(),
            Self::File(path) => path.display().to_string(),
        }
    }

    fn resolve_relative(&self, relative: &str) -> Result<Self> {
        if relative.contains('\\')
            || relative.contains('?')
            || relative.contains('#')
            || relative.starts_with("//")
            || Url::parse(relative).is_ok()
        {
            bail!("artifact source `{relative}` must be a plain relative URL path");
        }
        validate_relative_path(Path::new(relative), "artifact source")?;
        match self {
            Self::Http(url) => {
                let resolved = url
                    .join(relative)
                    .with_context(|| format!("resolve artifact `{relative}` relative to {url}"))?;
                ensure_supported_http_url(&resolved)?;
                if resolved.origin() != url.origin() {
                    bail!(
                        "artifact source `{relative}` resolves outside manifest origin {}",
                        url.origin().ascii_serialization()
                    );
                }
                Ok(Self::Http(resolved))
            }
            Self::File(path) => {
                let parent = path.parent().ok_or_else(|| {
                    anyhow::anyhow!(
                        "manifest path {} has no parent for artifact `{relative}`",
                        path.display()
                    )
                })?;
                Ok(Self::File(parent.join(relative)))
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum FetchError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Failed(String),
}

#[derive(Debug)]
struct LoadedManifest {
    location: ResourceLocation,
    manifest: ComponentManifest,
}

#[derive(Debug, Clone)]
struct PreparedFile {
    target: PathBuf,
    target_relative: PathBuf,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct PreparedComponent {
    manifest: ComponentManifest,
    files: Vec<PreparedFile>,
    source_hash: String,
    dependency_ids: Vec<String>,
    rust_modules: Vec<PreparedRustModules>,
    cargo_dependencies: Vec<PreparedCargoDependency>,
}

#[derive(Debug)]
struct PreparedRustModules {
    target_relative: PathBuf,
    modules: Vec<String>,
}

#[derive(Debug)]
struct PreparedCargoDependency {
    name: String,
    source: PreparedCargoDependencySource,
    features: Vec<String>,
    default_features: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreparedCargoDependencySource {
    Path(PathBuf),
    Version {
        version: String,
        registry: Option<String>,
    },
    Git(String),
}

struct Resolver<'a> {
    root: &'a Path,
    config: &'a ProjectConfig,
    client: Client,
    components: BTreeMap<String, PreparedComponent>,
    order: Vec<String>,
    stack: Vec<String>,
    total_bytes: usize,
}

pub fn add(options: AddOptions) -> Result<AddReport> {
    let root = canonical_project_root(&options.root)?;
    let config = config::load_project_config(&root)?;
    // Registry slugs are underscored because a component's slug is also its
    // Cargo feature name. Hyphens are what people type, so accept them and
    // resolve to the canonical spelling rather than 404ing on `alert-dialog`.
    let requested = canonical_component_reference(&options.component);
    let (qualified_registry, slug) = parse_component_reference(&requested)?;
    let requested_registry = normalize_registry_option(options.registry.as_deref())?;

    if let (Some(from_name), Some(from_flag)) = (qualified_registry, requested_registry) {
        if from_name != from_flag {
            bail!(
                "component `{}` selects registry `{from_name}`, but --registry selected `{from_flag}`",
                options.component
            );
        }
    }

    let explicit_registry = qualified_registry.or(requested_registry);
    let client = Client::builder()
        .user_agent(concat!("proa-cli/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .context("build Proa UI registry client")?;
    let mut resolver = Resolver {
        root: &root,
        config: &config,
        client,
        components: BTreeMap::new(),
        order: Vec::new(),
        stack: Vec::new(),
        total_bytes: 0,
    };

    let (resolved_registry, loaded) = resolver.resolve_top_level(explicit_registry, slug)?;
    let resolved_id = component_id(resolved_registry, slug);
    resolver.resolve_loaded(resolved_registry, slug, loaded)?;

    let existing_lock = config::load_component_lock(&root)?;
    let mut next_lock = existing_lock.clone();
    for id in &resolver.order {
        let component = resolver
            .components
            .get(id)
            .expect("resolver order only contains resolved components");
        next_lock.components.insert(
            id.clone(),
            InstalledComponent {
                version: component.manifest.version.clone(),
                registry: component.manifest.registry.clone(),
                mode: "source".to_string(),
                source_hash: component.source_hash.clone(),
                local_hash_at_install: component.source_hash.clone(),
                installed_files: component
                    .files
                    .iter()
                    .map(|file| file.target_relative.clone())
                    .collect(),
                dependencies: component.dependency_ids.clone(),
                rust_modules: component
                    .rust_modules
                    .iter()
                    .map(|modules| InstalledRustModules {
                        target: modules.target_relative.clone(),
                        modules: modules.modules.clone(),
                    })
                    .collect(),
                cargo_dependencies: component
                    .cargo_dependencies
                    .iter()
                    .map(|dependency| InstalledCargoDependency {
                        name: dependency.name.clone(),
                        path: match &dependency.source {
                            PreparedCargoDependencySource::Path(path) => Some(path.clone()),
                            _ => None,
                        },
                        version: match &dependency.source {
                            PreparedCargoDependencySource::Version { version, .. } => {
                                Some(version.clone())
                            }
                            _ => None,
                        },
                        git: match &dependency.source {
                            PreparedCargoDependencySource::Git(url) => Some(url.clone()),
                            _ => None,
                        },
                        registry: match &dependency.source {
                            PreparedCargoDependencySource::Version { registry, .. } => {
                                registry.clone()
                            }
                            _ => None,
                        },
                        features: dependency.features.clone(),
                        default_features: dependency.default_features,
                    })
                    .collect(),
            },
        );
    }

    let transaction = prepare_transaction(
        &root,
        &existing_lock,
        &next_lock,
        &resolver.components,
        &resolver.order,
        options.force,
    )?;

    if !options.dry_run {
        commit_transaction(&root, &transaction.mutations, &next_lock)?;
    }

    let top = resolver
        .components
        .get(&resolved_id)
        .expect("top-level component was resolved");
    let components = resolver
        .order
        .iter()
        .map(|id| {
            let component = resolver
                .components
                .get(id)
                .expect("resolver order only contains resolved components");
            InstalledComponentReport {
                name: component.manifest.name.clone(),
                kind: component.manifest.kind,
                registry: component.manifest.registry.clone(),
                version: component.manifest.version.clone(),
                tier: component.manifest.tier,
                dependency: id != &resolved_id,
            }
        })
        .collect();

    Ok(AddReport {
        root: root.clone(),
        requested: options.component,
        resolved: resolved_id,
        registry: resolved_registry.to_string(),
        version: top.manifest.version.clone(),
        dry_run: options.dry_run,
        components,
        files_written: transaction
            .mutations
            .iter()
            .map(|mutation| mutation.target_relative.clone())
            .collect(),
        files_unchanged: transaction.files_unchanged,
        lock_path: root.join(LOCK_FILE),
    })
}

pub fn print_add_report(report: &AddReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    let action = if report.dry_run {
        "Would install"
    } else {
        "Installed"
    };
    println!(
        "{action} {} {} from {}",
        report.resolved, report.version, report.registry
    );
    for component in &report.components {
        if component.dependency {
            println!(
                "  dependency {}/{} {}",
                component.registry, component.name, component.version
            );
        }
    }
    let written = report.files_written.len();
    let unchanged = report.files_unchanged.len();
    println!(
        "  {written} file(s) {}, {unchanged} already current",
        if report.dry_run { "planned" } else { "written" }
    );
    if !report.dry_run {
        println!("  lock {}", report.lock_path.display());
    }
    Ok(())
}

impl<'a> Resolver<'a> {
    fn resolve_top_level(
        &self,
        explicit_registry: Option<&str>,
        slug: &str,
    ) -> Result<(&'static str, LoadedManifest)> {
        if let Some(registry) = explicit_registry {
            let registry = canonical_registry(registry)?;
            return self
                .fetch_manifest(registry, slug)
                .map(|manifest| (registry, manifest))
                .map_err(|error| fetch_error_to_anyhow(registry, slug, error));
        }

        let public_not_found = if self.config.registries.contains_key(PUBLIC_REGISTRY) {
            match self.fetch_manifest(PUBLIC_REGISTRY, slug) {
                Ok(manifest) => return Ok((PUBLIC_REGISTRY, manifest)),
                Err(FetchError::NotFound(detail)) => Some(detail),
                Err(error) => {
                    return Err(fetch_error_to_anyhow(PUBLIC_REGISTRY, slug, error));
                }
            }
        } else {
            None
        };

        if self.config.registries.contains_key(PRO_REGISTRY) {
            match self.fetch_manifest(PRO_REGISTRY, slug) {
                Ok(manifest) => return Ok((PRO_REGISTRY, manifest)),
                Err(error) => {
                    if let Some(public_detail) = public_not_found {
                        return Err(bare_registry_fallback_error(slug, &public_detail, error));
                    }
                    return Err(fetch_error_to_anyhow(PRO_REGISTRY, slug, error));
                }
            }
        }

        if let Some(public_detail) = public_not_found {
            bail!(
                "component `{slug}` was not found in the public Free registry \
                 `{PUBLIC_REGISTRY}` ({public_detail}), and `{PRO_REGISTRY}` is not configured. \
                 Free registry access does not require a bearer token. No project files were changed."
            );
        }

        bail!(
            "component `{slug}` was not found in {PUBLIC_REGISTRY} or {PRO_REGISTRY}; \
             check the slug with `proa registry list`, or select a registry explicitly"
        )
    }

    fn resolve(&mut self, registry: &'static str, slug: &str) -> Result<()> {
        let id = component_id(registry, slug);
        if self.components.contains_key(&id) {
            return Ok(());
        }
        let loaded = self
            .fetch_manifest(registry, slug)
            .map_err(|error| fetch_error_to_anyhow(registry, slug, error))?;
        self.resolve_loaded(registry, slug, loaded)
    }

    fn resolve_loaded(
        &mut self,
        registry: &'static str,
        slug: &str,
        loaded: LoadedManifest,
    ) -> Result<()> {
        let id = component_id(registry, slug);
        if self.components.contains_key(&id) {
            return Ok(());
        }
        if let Some(position) = self.stack.iter().position(|item| item == &id) {
            let mut cycle = self.stack[position..].to_vec();
            cycle.push(id);
            bail!("component dependency cycle: {}", cycle.join(" -> "));
        }

        validate_manifest(registry, slug, &loaded.manifest)?;
        self.stack.push(id.clone());

        let mut dependency_ids = Vec::with_capacity(loaded.manifest.dependencies.len());
        let mut seen_dependencies = BTreeSet::new();
        for dependency in &loaded.manifest.dependencies {
            let (dependency_registry, dependency_slug) =
                parse_dependency_reference(registry, dependency)?;
            let dependency_id = component_id(dependency_registry, dependency_slug);
            if !seen_dependencies.insert(dependency_id.clone()) {
                bail!("manifest `{id}` lists dependency `{dependency_id}` more than once");
            }
            self.resolve(dependency_registry, dependency_slug)?;
            let child = self
                .components
                .get(&dependency_id)
                .expect("resolved dependency exists");
            if loaded.manifest.tier == ComponentTier::Free
                && child.manifest.tier == ComponentTier::Pro
            {
                bail!("Free component `{id}` cannot depend on Pro component `{dependency_id}`");
            }
            dependency_ids.push(dependency_id);
        }

        let files = self.prepare_files(&id, &loaded)?;
        let rust_modules = self.prepare_rust_modules(&id, &loaded)?;
        let cargo_dependencies = self.prepare_cargo_dependencies(&id, &loaded)?;
        let source_hash = hash_component_source(&files, &rust_modules, &cargo_dependencies);
        let manifest = loaded.manifest;
        self.stack.pop();
        self.order.push(id.clone());
        self.components.insert(
            id.clone(),
            PreparedComponent {
                manifest,
                files,
                source_hash,
                dependency_ids,
                rust_modules,
                cargo_dependencies,
            },
        );
        Ok(())
    }

    fn prepare_files(&mut self, id: &str, loaded: &LoadedManifest) -> Result<Vec<PreparedFile>> {
        let mut files = Vec::with_capacity(loaded.manifest.files.len());
        let mut seen_targets = BTreeSet::new();

        for entry in &loaded.manifest.files {
            let (target, target_relative) = resolve_target(self.root, self.config, &entry.target)?;
            if !seen_targets.insert(target_relative.clone()) {
                bail!(
                    "manifest `{id}` maps more than one artifact to `{}`",
                    target_relative.display()
                );
            }
            let expected_hash = normalize_sha256(&entry.sha256)
                .with_context(|| format!("manifest `{id}` target `{}`", entry.target))?;
            let source = loaded.location.resolve_relative(&entry.source)?;
            let bytes = self
                .fetch_bytes(&source, MAX_FILE_BYTES, loaded.manifest.registry.as_str())
                .map_err(|error| {
                    fetch_error_to_anyhow(&loaded.manifest.registry, &loaded.manifest.name, error)
                })?;
            let actual_hash = sha256_bytes(&bytes);
            if actual_hash != expected_hash {
                bail!(
                    "artifact hash mismatch for `{}` in `{id}`: expected {}, got {}",
                    entry.target,
                    expected_hash,
                    actual_hash
                );
            }
            self.total_bytes = self
                .total_bytes
                .checked_add(bytes.len())
                .ok_or_else(|| anyhow::anyhow!("component transaction size overflow"))?;
            if self.total_bytes > MAX_TRANSACTION_BYTES {
                bail!(
                    "component transaction exceeds the {} MiB safety limit",
                    MAX_TRANSACTION_BYTES / (1024 * 1024)
                );
            }
            files.push(PreparedFile {
                target,
                target_relative,
                bytes,
            });
        }

        Ok(files)
    }

    fn prepare_rust_modules(
        &self,
        id: &str,
        loaded: &LoadedManifest,
    ) -> Result<Vec<PreparedRustModules>> {
        let mut prepared = Vec::with_capacity(loaded.manifest.rust_modules.len());
        let mut seen_targets = BTreeSet::new();
        for entry in &loaded.manifest.rust_modules {
            let (_, target_relative) = resolve_target(self.root, self.config, &entry.target)?;
            if !seen_targets.insert(target_relative.clone()) {
                bail!(
                    "manifest `{id}` declares Rust modules for `{}` more than once",
                    target_relative.display()
                );
            }
            if entry.modules.is_empty() {
                bail!(
                    "manifest `{id}` has an empty Rust module declaration for `{}`",
                    entry.target
                );
            }
            let mut modules = BTreeSet::new();
            for module in &entry.modules {
                validate_rust_module_name(module).with_context(|| {
                    format!("manifest `{id}` Rust module target `{}`", entry.target)
                })?;
                if !modules.insert(module.clone()) {
                    bail!("manifest `{id}` declares Rust module `{module}` more than once");
                }
            }
            prepared.push(PreparedRustModules {
                target_relative,
                modules: modules.into_iter().collect(),
            });
        }
        Ok(prepared)
    }

    fn prepare_cargo_dependencies(
        &self,
        id: &str,
        loaded: &LoadedManifest,
    ) -> Result<Vec<PreparedCargoDependency>> {
        let mut prepared = Vec::with_capacity(loaded.manifest.cargo_dependencies.len());
        let mut seen = BTreeSet::new();
        for entry in &loaded.manifest.cargo_dependencies {
            validate_cargo_dependency_name(&entry.name)
                .with_context(|| format!("manifest `{id}` Cargo dependency"))?;
            if !seen.insert(entry.name.clone()) {
                bail!(
                    "manifest `{id}` declares Cargo dependency `{}` more than once",
                    entry.name
                );
            }
            let source = match (&entry.path, &entry.version, &entry.git) {
                (Some(path), None, None) => {
                    if entry.registry.is_some() {
                        bail!(
                            "manifest `{id}` Cargo dependency `{}` cannot combine `path` and `registry`",
                            entry.name
                        );
                    }
                    let (_, path_relative) = resolve_target(self.root, self.config, path)?;
                    PreparedCargoDependencySource::Path(path_relative)
                }
                (None, Some(version), None) => {
                    validate_cargo_version(version)
                        .with_context(|| format!("manifest `{id}` Cargo dependency `{}`", entry.name))?;
                    if let Some(registry) = &entry.registry {
                        validate_cargo_registry_name(registry).with_context(|| {
                            format!("manifest `{id}` Cargo dependency `{}`", entry.name)
                        })?;
                    }
                    PreparedCargoDependencySource::Version {
                        version: version.clone(),
                        registry: entry.registry.clone(),
                    }
                }
                (None, None, Some(git)) => {
                    if entry.registry.is_some() {
                        bail!(
                            "manifest `{id}` Cargo dependency `{}` cannot combine `git` and `registry`",
                            entry.name
                        );
                    }
                    validate_cargo_git_url(git).with_context(|| {
                        format!("manifest `{id}` Cargo dependency `{}`", entry.name)
                    })?;
                    PreparedCargoDependencySource::Git(git.clone())
                }
                (None, None, None) => bail!(
                    "manifest `{id}` Cargo dependency `{}` must declare `version`",
                    entry.name
                ),
                _ => bail!(
                    "manifest `{id}` Cargo dependency `{}` must declare exactly one of `path`, `version`, or `git`",
                    entry.name
                ),
            };
            let mut features = BTreeSet::new();
            for feature in &entry.features {
                validate_cargo_feature_name(feature)
                    .with_context(|| format!("manifest `{id}` Cargo feature"))?;
                if !features.insert(feature.clone()) {
                    bail!("manifest `{id}` declares Cargo feature `{feature}` more than once");
                }
            }
            prepared.push(PreparedCargoDependency {
                name: entry.name.clone(),
                source,
                features: features.into_iter().collect(),
                default_features: entry.default_features,
            });
        }
        Ok(prepared)
    }

    fn fetch_manifest(
        &self,
        registry: &str,
        slug: &str,
    ) -> std::result::Result<LoadedManifest, FetchError> {
        let template = self.config.registries.get(registry).ok_or_else(|| {
            FetchError::Failed(format!(
                "registry `{registry}` is not configured in {}",
                config::CONFIG_FILE
            ))
        })?;
        let location = registry_manifest_location(self.root, template, slug)
            .map_err(|error| FetchError::Failed(error.to_string()))?;
        let bytes = self.fetch_bytes(&location, MAX_MANIFEST_BYTES, registry)?;
        let manifest: ComponentManifest = serde_json::from_slice(&bytes).map_err(|error| {
            FetchError::Failed(format!(
                "parse component manifest {}: {error}",
                location.display()
            ))
        })?;
        Ok(LoadedManifest { location, manifest })
    }

    fn fetch_bytes(
        &self,
        location: &ResourceLocation,
        limit: usize,
        registry: &str,
    ) -> std::result::Result<Vec<u8>, FetchError> {
        match location {
            ResourceLocation::File(path) => {
                let metadata = fs::metadata(path).map_err(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        FetchError::NotFound(format!("{} was not found", path.display()))
                    } else {
                        FetchError::Failed(format!("read {}: {error}", path.display()))
                    }
                })?;
                if metadata.len() > limit as u64 {
                    return Err(FetchError::Failed(format!(
                        "{} exceeds the {} byte safety limit",
                        path.display(),
                        limit
                    )));
                }
                fs::read(path).map_err(|error| {
                    FetchError::Failed(format!("read {}: {error}", path.display()))
                })
            }
            ResourceLocation::Http(url) => {
                let mut request = self.client.get(url.clone());
                if registry == PRO_REGISTRY {
                    let token = pro_registry_bearer_token()
                        .map_err(|error| FetchError::Failed(error.to_string()))?;
                    request = with_registry_bearer_auth(request, registry, token.as_deref());
                }
                let response = request
                    .send()
                    .map_err(|error| FetchError::Failed(format!("request {url}: {error}")))?;
                if response.url().origin() != url.origin() {
                    return Err(FetchError::Failed(format!(
                        "request {url} redirected across origins to {}",
                        response.url()
                    )));
                }
                let status = response.status();
                if status == StatusCode::NOT_FOUND {
                    return Err(FetchError::NotFound(format!("{url} returned {status}")));
                }
                if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                    return Err(FetchError::Unauthorized(format!("{url} returned {status}")));
                }
                let response = response
                    .error_for_status()
                    .map_err(|error| FetchError::Failed(format!("request {url}: {error}")))?;
                if response
                    .content_length()
                    .is_some_and(|length| length > limit as u64)
                {
                    return Err(FetchError::Failed(format!(
                        "{url} exceeds the {limit} byte safety limit"
                    )));
                }
                let bytes = response.bytes().map_err(|error| {
                    FetchError::Failed(format!("read response body from {url}: {error}"))
                })?;
                if bytes.len() > limit {
                    return Err(FetchError::Failed(format!(
                        "{url} exceeds the {limit} byte safety limit"
                    )));
                }
                Ok(bytes.to_vec())
            }
        }
    }
}

fn validate_manifest(registry: &str, slug: &str, manifest: &ComponentManifest) -> Result<()> {
    if manifest.schema != COMPONENT_MANIFEST_SCHEMA {
        bail!(
            "manifest `{registry}/{slug}` uses unsupported schema `{}` (expected `{COMPONENT_MANIFEST_SCHEMA}`)",
            manifest.schema
        );
    }
    if manifest.schema_version != COMPONENT_MANIFEST_VERSION {
        bail!(
            "manifest `{registry}/{slug}` uses unsupported schemaVersion {} (expected {})",
            manifest.schema_version,
            COMPONENT_MANIFEST_VERSION
        );
    }
    let manifest_slug = validate_slug(&manifest.name)?;
    if manifest_slug != slug {
        bail!(
            "registry returned component `{}` for requested slug `{slug}`",
            manifest.name
        );
    }
    if manifest.registry != registry {
        bail!(
            "manifest `{registry}/{slug}` declares registry `{}`",
            manifest.registry
        );
    }
    match (registry, manifest.tier) {
        (PUBLIC_REGISTRY, ComponentTier::Free) | (PRO_REGISTRY, ComponentTier::Pro) => {}
        (PUBLIC_REGISTRY, ComponentTier::Pro) => {
            bail!(
                "Pro component `{slug}` cannot be served from public registry `{PUBLIC_REGISTRY}`"
            )
        }
        (PRO_REGISTRY, ComponentTier::Free) => {
            bail!("Free component `{slug}` must be served from public registry `{PUBLIC_REGISTRY}`")
        }
        _ => bail!("unsupported component registry `{registry}`"),
    }
    if manifest.tier == ComponentTier::Free && manifest.license != "MIT OR Apache-2.0" {
        bail!(
            "Free component `{registry}/{slug}` must declare license `MIT OR Apache-2.0`, got `{}`",
            manifest.license
        );
    }
    if manifest.license.trim().is_empty() {
        bail!("manifest `{registry}/{slug}` has an empty license");
    }
    if manifest.version.trim().is_empty() || manifest.version.len() > 128 {
        bail!("manifest `{registry}/{slug}` has an invalid version");
    }
    if manifest.files.is_empty() {
        bail!("manifest `{registry}/{slug}` contains no files");
    }
    if manifest.files.len() > MAX_COMPONENT_FILES {
        bail!(
            "manifest `{registry}/{slug}` contains {} files; maximum is {MAX_COMPONENT_FILES}",
            manifest.files.len()
        );
    }
    Ok(())
}

/// Rewrite hyphens in the component slug to underscores, leaving any
/// `@registry/` prefix alone (registry aliases are hyphenated: `@proa-pro`).
fn canonical_component_reference(input: &str) -> String {
    match input
        .strip_prefix('@')
        .and_then(|rest| rest.split_once('/'))
    {
        Some((registry, slug)) => format!("@{registry}/{}", slug.replace('-', "_")),
        None => input.replace('-', "_"),
    }
}

fn parse_component_reference(input: &str) -> Result<(Option<&'static str>, &str)> {
    if let Some(rest) = input.strip_prefix('@') {
        let (registry_name, slug) = rest.split_once('/').ok_or_else(|| {
            anyhow::anyhow!(
                "qualified component `{input}` must use `@proa/<slug>` or `@proa-pro/<slug>`"
            )
        })?;
        let registry = canonical_registry(&format!("@{registry_name}"))?;
        return Ok((Some(registry), validate_slug(slug)?));
    }
    Ok((None, validate_slug(input)?))
}

fn parse_dependency_reference<'a>(
    parent_registry: &'static str,
    input: &'a str,
) -> Result<(&'static str, &'a str)> {
    let (registry, slug) = parse_component_reference(input)?;
    Ok((registry.unwrap_or(parent_registry), slug))
}

fn normalize_registry_option(input: Option<&str>) -> Result<Option<&'static str>> {
    input.map(canonical_registry).transpose()
}

fn canonical_registry(input: &str) -> Result<&'static str> {
    match input {
        PUBLIC_REGISTRY | "proa" => Ok(PUBLIC_REGISTRY),
        PRO_REGISTRY | "proa-pro" => Ok(PRO_REGISTRY),
        _ => bail!(
            "unsupported registry `{input}` (expected `{PUBLIC_REGISTRY}` or `{PRO_REGISTRY}`)"
        ),
    }
}

fn validate_slug(input: &str) -> Result<&str> {
    if input.is_empty()
        || input.len() > 128
        || !input.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
        || !input
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !input
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
    {
        bail!("invalid component slug `{input}`; use lowercase ASCII letters, digits, `-`, or `_`");
    }
    Ok(input)
}

fn validate_rust_module_name(input: &str) -> Result<()> {
    const KEYWORDS: &[&str] = &[
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
        "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
        "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
        "use", "where", "while", "async", "await", "dyn",
    ];
    let mut bytes = input.bytes();
    let Some(first) = bytes.next() else {
        bail!("Rust module name cannot be empty");
    };
    if !(first.is_ascii_lowercase() || first == b'_')
        || !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || KEYWORDS.contains(&input)
    {
        bail!("invalid Rust module name `{input}`");
    }
    Ok(())
}

fn validate_cargo_dependency_name(input: &str) -> Result<()> {
    if input.is_empty()
        || input.len() > 128
        || !input
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        || !input
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
    {
        bail!("invalid Cargo dependency name `{input}`");
    }
    Ok(())
}

fn validate_cargo_feature_name(input: &str) -> Result<()> {
    if input.is_empty()
        || input.len() > 128
        || !input.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'/'
        })
    {
        bail!("invalid Cargo feature name `{input}`");
    }
    Ok(())
}

fn validate_cargo_version(input: &str) -> Result<()> {
    if input.trim() != input
        || input.is_empty()
        || input.len() > 128
        || !input.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'.' | b',' | b'-' | b'+' | b'^' | b'~' | b'<' | b'>' | b'=' | b'*' | b' '
                )
        })
    {
        bail!("invalid Cargo version requirement `{input}`");
    }
    Ok(())
}

fn validate_cargo_git_url(input: &str) -> Result<()> {
    if input.len() > 256 || !input.starts_with("https://") || Url::parse(input).is_err() {
        bail!("invalid Cargo git dependency URL `{input}` (https URLs only)");
    }
    Ok(())
}

fn validate_cargo_registry_name(input: &str) -> Result<()> {
    if input.is_empty()
        || input.len() > 64
        || !input
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("invalid Cargo registry name `{input}`");
    }
    Ok(())
}

fn component_id(registry: &str, slug: &str) -> String {
    format!("{registry}/{slug}")
}

fn registry_manifest_location(root: &Path, template: &str, slug: &str) -> Result<ResourceLocation> {
    if !template.contains("{name}") {
        bail!("registry template `{template}` must contain `{{name}}`");
    }
    let rendered = template.replace("{name}", slug);
    if rendered.starts_with("https://") || rendered.starts_with("http://") {
        let url =
            Url::parse(&rendered).with_context(|| format!("parse registry URL `{rendered}`"))?;
        ensure_supported_http_url(&url)?;
        return Ok(ResourceLocation::Http(url));
    }
    if rendered.starts_with("file://") {
        let url = Url::parse(&rendered)
            .with_context(|| format!("parse registry file URL `{rendered}`"))?;
        let path = url
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("invalid registry file URL `{rendered}`"))?;
        return Ok(ResourceLocation::File(path));
    }
    let path = PathBuf::from(rendered);
    Ok(ResourceLocation::File(if path.is_absolute() {
        path
    } else {
        root.join(path)
    }))
}

fn ensure_supported_http_url(url: &Url) -> Result<()> {
    match url.scheme() {
        "https" => Ok(()),
        "http" if matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1")) => Ok(()),
        "http" => {
            bail!("registry URL `{url}` is insecure; HTTP is only allowed for localhost fixtures")
        }
        scheme => bail!("unsupported registry URL scheme `{scheme}` in `{url}`"),
    }
}

fn resolve_target(root: &Path, config: &ProjectConfig, target: &str) -> Result<(PathBuf, PathBuf)> {
    let (alias, relative) = target.split_once('/').ok_or_else(|| {
        anyhow::anyhow!(
            "artifact target `{target}` must use `@components/<path>` or `@assets/<path>`"
        )
    })?;
    if !alias.starts_with('@') {
        bail!("artifact target `{target}` must start with a configured `@` alias");
    }
    let legacy_lib = Path::new("src/lib");
    let alias_root = match alias {
        "@components" => config.paths.components.as_path(),
        "@assets" => config.paths.assets.as_path(),
        // Compatibility for already-published v1 manifests. New manifests
        // must use the flat @components layout, but retaining this resolver
        // keeps Pro installs and rolling deployments from breaking while
        // registries migrate independently of the CLI.
        "@lib" if relative == "proa_ui" || relative.starts_with("proa_ui/") => legacy_lib,
        _ => bail!(
            "artifact target `{target}` uses unsupported alias `{alias}`; expected `@components` or `@assets`"
        ),
    };
    validate_relative_path(alias_root, &format!("configured path for `{alias}`"))?;
    validate_relative_path(Path::new(relative), "artifact target")?;
    let target_relative = alias_root.join(relative);
    validate_relative_path(&target_relative, "resolved artifact target")?;
    let absolute = root.join(&target_relative);
    ensure_no_symlink_path(root, &target_relative)?;
    Ok((absolute, target_relative))
}

fn validate_relative_path(path: &Path, label: &str) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!(
            "{label} `{}` must be a non-empty relative path",
            path.display()
        );
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!(
                "{label} `{}` contains an unsafe path component",
                path.display()
            ),
        }
    }
    Ok(())
}

fn ensure_no_symlink_path(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for part in relative.components() {
        let Component::Normal(part) = part else {
            bail!("unsafe target path `{}`", relative.display());
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "refusing to install through symlink `{}`",
                    current.display()
                )
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect target path {}", current.display()))
            }
        }
    }
    Ok(())
}

fn normalize_sha256(input: &str) -> Result<String> {
    let hex = input.strip_prefix("sha256:").unwrap_or(input);
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("invalid SHA-256 `{input}`; expected 64 lowercase hexadecimal characters");
    }
    Ok(format!("sha256:{hex}"))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

fn hash_component_source(
    files: &[PreparedFile],
    rust_modules: &[PreparedRustModules],
    cargo_dependencies: &[PreparedCargoDependency],
) -> String {
    let mut ordered: Vec<_> = files.iter().collect();
    ordered.sort_by(|left, right| left.target_relative.cmp(&right.target_relative));
    let mut hasher = Sha256::new();
    hasher.update(b"proa-ui-source-v1\0");
    for file in ordered {
        let path = file.target_relative.to_string_lossy();
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path.as_bytes());
        hasher.update((file.bytes.len() as u64).to_be_bytes());
        hasher.update(&file.bytes);
    }
    let mut modules: Vec<_> = rust_modules.iter().collect();
    modules.sort_by(|left, right| left.target_relative.cmp(&right.target_relative));
    for entry in modules {
        hasher.update(b"modules\0");
        hash_path(&mut hasher, &entry.target_relative);
        for module in &entry.modules {
            hasher.update((module.len() as u64).to_be_bytes());
            hasher.update(module.as_bytes());
        }
    }
    let mut dependencies: Vec<_> = cargo_dependencies.iter().collect();
    dependencies.sort_by(|left, right| left.name.cmp(&right.name));
    for dependency in dependencies {
        hasher.update(b"cargo\0");
        hasher.update((dependency.name.len() as u64).to_be_bytes());
        hasher.update(dependency.name.as_bytes());
        match &dependency.source {
            PreparedCargoDependencySource::Path(path) => {
                hasher.update(b"path\0");
                hash_path(&mut hasher, path);
            }
            PreparedCargoDependencySource::Version { version, registry } => {
                hasher.update(b"version\0");
                hasher.update((version.len() as u64).to_be_bytes());
                hasher.update(version.as_bytes());
                if let Some(registry) = registry {
                    hasher.update((registry.len() as u64).to_be_bytes());
                    hasher.update(registry.as_bytes());
                }
            }
            PreparedCargoDependencySource::Git(url) => {
                hasher.update(b"git\0");
                hasher.update((url.len() as u64).to_be_bytes());
                hasher.update(url.as_bytes());
            }
        }
        for feature in &dependency.features {
            hasher.update((feature.len() as u64).to_be_bytes());
            hasher.update(feature.as_bytes());
        }
        hasher.update([u8::from(dependency.default_features.unwrap_or(true))]);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn hash_path(hasher: &mut Sha256, path: &Path) {
    let path = path.to_string_lossy();
    hasher.update((path.len() as u64).to_be_bytes());
    hasher.update(path.as_bytes());
}

#[derive(Debug)]
struct FileMutation {
    target: PathBuf,
    target_relative: PathBuf,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}

struct PreparedTransaction {
    mutations: Vec<FileMutation>,
    files_unchanged: Vec<PathBuf>,
}

fn prepare_transaction(
    root: &Path,
    current_lock: &ComponentLockFile,
    next_lock: &ComponentLockFile,
    components: &BTreeMap<String, PreparedComponent>,
    order: &[String],
    force: bool,
) -> Result<PreparedTransaction> {
    let mut target_contents: BTreeMap<PathBuf, PreparedFile> = BTreeMap::new();

    for id in order {
        let component = components
            .get(id)
            .expect("resolver order only contains resolved components");
        if let Some(installed) = current_lock.components.get(id) {
            if !force
                && (installed.version != component.manifest.version
                    || installed.registry != component.manifest.registry
                    || installed.source_hash != component.source_hash)
            {
                // Same version still lands here when only the resolved targets
                // moved (an edited `paths.assets`, say), so do not claim an
                // upgrade that is not happening.
                if installed.version == component.manifest.version {
                    bail!(
                        "`{id}` is already installed at version {} but resolves to \
                         different files now; `proa ui add` will not overwrite it. \
                         Pass `--force` to reinstall it.",
                        installed.version
                    );
                }
                bail!(
                    "`{id}` is already installed at version {}; \
                     `proa ui add` will not overwrite or upgrade it. \
                     Pass `--force` to reinstall it at {}.",
                    installed.version,
                    component.manifest.version
                );
            }
        }

        for file in &component.files {
            if let Some(existing) = target_contents.get(&file.target_relative) {
                if existing.bytes != file.bytes {
                    bail!(
                        "component manifests disagree on shared target `{}`",
                        file.target_relative.display()
                    );
                }
                continue;
            }
            target_contents.insert(file.target_relative.clone(), file.clone());
        }
    }

    let mut mutations = Vec::new();
    let mut files_unchanged = Vec::new();
    for file in target_contents.values() {
        ensure_no_symlink_path(root, &file.target_relative)?;
        match fs::read(&file.target) {
            Ok(existing) if existing == file.bytes => {
                files_unchanged.push(file.target_relative.clone());
            }
            Ok(_) => {
                bail!(
                    "refusing to overwrite locally different file `{}`; move it aside or choose another alias path",
                    file.target_relative.display()
                )
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                mutations.push(FileMutation {
                    target: file.target.clone(),
                    target_relative: file.target_relative.clone(),
                    before: None,
                    after: file.bytes.clone(),
                });
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read install target {}", file.target.display()))
            }
        }
    }

    let current_immutable_files = collect_locked_immutable_files(current_lock)?;
    let current_modules = collect_locked_modules(current_lock)?;
    let next_modules = collect_locked_modules(next_lock)?;
    for (target_relative, modules) in &next_modules {
        if target_contents.contains_key(target_relative) {
            bail!(
                "immutable artifact and managed Rust modules both target `{}`",
                target_relative.display()
            );
        }
        if current_immutable_files.contains(target_relative) {
            bail!(
                "managed Rust modules cannot update `{}` because an installed component owns it as an immutable artifact; registries must publish byte-identical shared glue or use managed modules consistently",
                target_relative.display()
            );
        }
        validate_relative_path(target_relative, "locked Rust module target")?;
        ensure_no_symlink_path(root, target_relative)?;
        let target = root.join(target_relative);
        let before = match fs::read(&target) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read Rust module target {}", target.display()))
            }
        };
        let expected_current = current_modules
            .get(target_relative)
            .cloned()
            .unwrap_or_default();
        let after = update_managed_rust_modules(
            before.as_deref(),
            &expected_current,
            modules,
            target_relative,
        )?;
        if before.as_deref() == Some(after.as_slice()) {
            files_unchanged.push(target_relative.clone());
        } else {
            mutations.push(FileMutation {
                target,
                target_relative: target_relative.clone(),
                before,
                after,
            });
        }
    }

    let cargo_dependencies = collect_locked_cargo_dependencies(next_lock)?;
    if !cargo_dependencies.is_empty() {
        let cargo_relative = PathBuf::from("Cargo.toml");
        if target_contents.contains_key(&cargo_relative)
            || next_modules.contains_key(&cargo_relative)
        {
            bail!("component manifest cannot replace the project Cargo.toml");
        }
        let cargo_path = root.join(&cargo_relative);
        let before = fs::read(&cargo_path)
            .with_context(|| format!("read project manifest {}", cargo_path.display()))?;
        let after = update_cargo_dependencies(&before, &cargo_dependencies)?;
        if before == after {
            files_unchanged.push(cargo_relative);
        } else {
            mutations.push(FileMutation {
                target: cargo_path,
                target_relative: cargo_relative,
                before: Some(before),
                after,
            });
        }
    }

    let mut seen_mutations = BTreeSet::new();
    for mutation in &mutations {
        if !seen_mutations.insert(mutation.target_relative.clone()) {
            bail!(
                "transaction contains multiple writes to `{}`",
                mutation.target_relative.display()
            );
        }
    }

    Ok(PreparedTransaction {
        mutations,
        files_unchanged,
    })
}

const MODULES_START: &str = "// proa-ui:modules:start";
const MODULES_END: &str = "// proa-ui:modules:end";

fn collect_locked_immutable_files(lock: &ComponentLockFile) -> Result<BTreeSet<PathBuf>> {
    let mut collected = BTreeSet::new();
    for (component_id, component) in &lock.components {
        for target in &component.installed_files {
            validate_relative_path(
                target,
                &format!("locked immutable artifact target for `{component_id}`"),
            )?;
            collected.insert(target.clone());
        }
    }
    Ok(collected)
}

fn collect_locked_modules(lock: &ComponentLockFile) -> Result<BTreeMap<PathBuf, BTreeSet<String>>> {
    let mut collected: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    for (component_id, component) in &lock.components {
        for entry in &component.rust_modules {
            validate_relative_path(
                &entry.target,
                &format!("locked Rust module target for `{component_id}`"),
            )?;
            let target_modules = collected.entry(entry.target.clone()).or_default();
            for module in &entry.modules {
                validate_rust_module_name(module)
                    .with_context(|| format!("locked Rust module for `{component_id}`"))?;
                target_modules.insert(module.clone());
            }
        }
    }
    Ok(collected)
}

fn update_managed_rust_modules(
    existing: Option<&[u8]>,
    expected_current: &BTreeSet<String>,
    next: &BTreeSet<String>,
    target: &Path,
) -> Result<Vec<u8>> {
    let existing = match existing {
        Some(bytes) => std::str::from_utf8(bytes).with_context(|| {
            format!("managed Rust module file {} is not UTF-8", target.display())
        })?,
        None => "",
    };
    let expected_block = managed_module_block(expected_current);
    let next_block = managed_module_block(next);
    let start = existing.find(MODULES_START);
    let end = existing.find(MODULES_END);

    match (start, end) {
        (None, None) => {
            if !expected_current.is_empty() {
                bail!(
                    "managed Rust module block is missing from `{}` even though the lock records modules; refusing to reconstruct an edited file",
                    target.display()
                );
            }
            let mut output = existing.to_string();
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&next_block);
            Ok(output.into_bytes())
        }
        (Some(start), Some(end)) if start < end => {
            let end = end + MODULES_END.len();
            if existing[start..end] != expected_block {
                bail!(
                    "managed Rust module block in `{}` was edited locally; refusing to overwrite it",
                    target.display()
                );
            }
            if existing[end..].contains(MODULES_START) || existing[end..].contains(MODULES_END) {
                bail!(
                    "managed Rust module file `{}` contains duplicate marker blocks",
                    target.display()
                );
            }
            let mut output =
                String::with_capacity(existing.len() - expected_block.len() + next_block.len());
            output.push_str(&existing[..start]);
            output.push_str(&next_block);
            output.push_str(&existing[end..]);
            Ok(output.into_bytes())
        }
        _ => bail!(
            "managed Rust module file `{}` has incomplete or out-of-order markers",
            target.display()
        ),
    }
}

fn managed_module_block(modules: &BTreeSet<String>) -> String {
    let mut block = String::from(MODULES_START);
    block.push('\n');
    for module in modules {
        block.push_str("pub mod ");
        block.push_str(module);
        block.push_str(";\npub use ");
        block.push_str(module);
        block.push_str("::*;\n");
    }
    block.push_str(MODULES_END);
    block
}

#[derive(Debug)]
struct CargoDependencySpec {
    source: CargoDependencySourceSpec,
    features: BTreeSet<String>,
    default_features: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CargoDependencySourceSpec {
    Path(PathBuf),
    Version {
        version: String,
        registry: Option<String>,
    },
    Git(String),
}

fn collect_locked_cargo_dependencies(
    lock: &ComponentLockFile,
) -> Result<BTreeMap<String, CargoDependencySpec>> {
    let mut collected: BTreeMap<String, CargoDependencySpec> = BTreeMap::new();
    for (component_id, component) in &lock.components {
        for dependency in &component.cargo_dependencies {
            validate_cargo_dependency_name(&dependency.name)
                .with_context(|| format!("locked Cargo dependency for `{component_id}`"))?;
            let source = match (&dependency.path, &dependency.version, &dependency.git) {
                (Some(path), None, None) => {
                    validate_relative_path(
                        path,
                        &format!("locked Cargo dependency path for `{component_id}`"),
                    )?;
                    CargoDependencySourceSpec::Path(path.clone())
                }
                (None, Some(version), None) => {
                    validate_cargo_version(version)
                        .with_context(|| format!("locked Cargo dependency for `{component_id}`"))?;
                    if let Some(registry) = &dependency.registry {
                        validate_cargo_registry_name(registry).with_context(|| {
                            format!("locked Cargo dependency for `{component_id}`")
                        })?;
                    }
                    CargoDependencySourceSpec::Version {
                        version: version.clone(),
                        registry: dependency.registry.clone(),
                    }
                }
                (None, None, Some(git)) => {
                    if dependency.registry.is_some() {
                        bail!(
                            "locked Cargo dependency `{}` for `{component_id}` cannot combine `git` and `registry`",
                            dependency.name
                        );
                    }
                    validate_cargo_git_url(git)
                        .with_context(|| format!("locked Cargo dependency for `{component_id}`"))?;
                    CargoDependencySourceSpec::Git(git.clone())
                }
                _ => bail!(
                    "locked Cargo dependency `{}` for `{component_id}` must have exactly one of `path`, `version`, or `git`",
                    dependency.name
                ),
            };
            let spec =
                collected
                    .entry(dependency.name.clone())
                    .or_insert_with(|| CargoDependencySpec {
                        source: source.clone(),
                        features: BTreeSet::new(),
                        default_features: dependency.default_features,
                    });
            if spec.source != source {
                bail!(
                    "locked components disagree on source for Cargo dependency `{}`",
                    dependency.name
                );
            }
            if let (Some(existing), Some(next)) =
                (spec.default_features, dependency.default_features)
            {
                if existing != next {
                    bail!(
                        "locked components disagree on default features for Cargo dependency `{}`",
                        dependency.name
                    );
                }
            } else if spec.default_features.is_none() {
                spec.default_features = dependency.default_features;
            }
            for feature in &dependency.features {
                validate_cargo_feature_name(feature)
                    .with_context(|| format!("locked Cargo feature for `{component_id}`"))?;
                spec.features.insert(feature.clone());
            }
        }
    }
    Ok(collected)
}

fn update_cargo_dependencies(
    existing: &[u8],
    dependencies: &BTreeMap<String, CargoDependencySpec>,
) -> Result<Vec<u8>> {
    use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

    let source = std::str::from_utf8(existing).context("project Cargo.toml is not valid UTF-8")?;
    let mut document = source
        .parse::<DocumentMut>()
        .context("parse project Cargo.toml for Proa UI dependency")?;
    if !document.as_table().contains_key("dependencies") {
        document["dependencies"] = Item::Table(Table::new());
    }
    let dependency_table = document["dependencies"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("Cargo.toml `[dependencies]` is not a table"))?;

    for (name, desired) in dependencies {
        let Some(existing) = dependency_table.get_mut(name) else {
            let mut table = InlineTable::new();
            match &desired.source {
                CargoDependencySourceSpec::Path(path) => {
                    table.insert("path", Value::from(path_for_toml(path)));
                }
                CargoDependencySourceSpec::Version { version, registry } => {
                    table.insert("version", Value::from(version.as_str()));
                    if let Some(registry) = registry {
                        table.insert("registry", Value::from(registry.as_str()));
                    }
                }
                CargoDependencySourceSpec::Git(url) => {
                    table.insert("git", Value::from(url.as_str()));
                }
            }
            if !desired.features.is_empty() {
                table.insert("features", Value::Array(feature_array(&desired.features)));
            }
            if let Some(default_features) = desired.default_features {
                table.insert("default-features", Value::from(default_features));
            }
            dependency_table.insert(name, Item::Value(Value::InlineTable(table)));
            continue;
        };

        match (&desired.source, existing) {
            (
                CargoDependencySourceSpec::Path(path),
                Item::Value(Value::InlineTable(table)),
            ) => {
                verify_cargo_dependency_path(name, table.get("path"), path)?;
                merge_inline_dependency_options(name, table, desired)?;
            }
            (CargoDependencySourceSpec::Path(path), Item::Table(table)) => {
                let actual = table.get("path").and_then(Item::as_value);
                verify_cargo_dependency_path(name, actual, path)?;
                merge_table_dependency_options(name, table, desired)?;
            }
            (CargoDependencySourceSpec::Path(path), _) => bail!(
                "Cargo dependency `{name}` already exists but is not the required path dependency `{}`",
                path.display()
            ),
            (
                CargoDependencySourceSpec::Version { .. },
                Item::Value(Value::String(version)),
            ) => {
                let version = version.value().to_string();
                let mut table = InlineTable::new();
                table.insert("version", Value::from(version));
                merge_inline_dependency_options(name, &mut table, desired)?;
                *dependency_table
                    .get_mut(name)
                    .expect("dependency still exists") = Item::Value(Value::InlineTable(table));
            }
            (
                CargoDependencySourceSpec::Version { registry, .. },
                Item::Value(Value::InlineTable(table)),
            ) => {
                add_registry_if_unset_inline(table, registry.as_deref());
                merge_inline_dependency_options(name, table, desired)?;
            }
            (
                CargoDependencySourceSpec::Version { registry, .. },
                Item::Table(table),
            ) => {
                add_registry_if_unset_table(table, registry.as_deref());
                merge_table_dependency_options(name, table, desired)?;
            }
            (CargoDependencySourceSpec::Version { .. }, _) => {
                bail!("Cargo dependency `{name}` has an unsupported declaration")
            }
            (
                CargoDependencySourceSpec::Git(url),
                Item::Value(Value::InlineTable(table)),
            ) => {
                add_git_if_unset_inline(table, url);
                merge_inline_dependency_options(name, table, desired)?;
            }
            (CargoDependencySourceSpec::Git(url), Item::Table(table)) => {
                add_git_if_unset_table(table, url);
                merge_table_dependency_options(name, table, desired)?;
            }
            (CargoDependencySourceSpec::Git(_), Item::Value(Value::String(version))) => {
                // The host pins this crate by version; keep its choice and only
                // merge the requested features.
                let version = version.value().to_string();
                let mut table = InlineTable::new();
                table.insert("version", Value::from(version));
                merge_inline_dependency_options(name, &mut table, desired)?;
                *dependency_table
                    .get_mut(name)
                    .expect("dependency still exists") = Item::Value(Value::InlineTable(table));
            }
            (CargoDependencySourceSpec::Git(_), _) => {
                bail!("Cargo dependency `{name}` has an unsupported declaration")
            }
        }
    }
    Ok(document.to_string().into_bytes())
}

fn add_git_if_unset_inline(table: &mut toml_edit::InlineTable, url: &str) {
    if table.get("path").is_none()
        && table.get("git").is_none()
        && table.get("workspace").is_none()
        && table.get("version").is_none()
        && table.get("registry").is_none()
    {
        table.insert("git", toml_edit::Value::from(url));
    }
}

fn add_git_if_unset_table(table: &mut toml_edit::Table, url: &str) {
    if table.get("path").is_none()
        && table.get("git").is_none()
        && table.get("workspace").is_none()
        && table.get("version").is_none()
        && table.get("registry").is_none()
    {
        table.insert("git", toml_edit::Item::Value(toml_edit::Value::from(url)));
    }
}

fn add_registry_if_unset_inline(table: &mut toml_edit::InlineTable, registry: Option<&str>) {
    if table.get("path").is_none()
        && table.get("git").is_none()
        && table.get("workspace").is_none()
        && table.get("registry").is_none()
    {
        if let Some(registry) = registry {
            table.insert("registry", toml_edit::Value::from(registry));
        }
    }
}

fn add_registry_if_unset_table(table: &mut toml_edit::Table, registry: Option<&str>) {
    if table.get("path").is_none()
        && table.get("git").is_none()
        && table.get("workspace").is_none()
        && table.get("registry").is_none()
    {
        if let Some(registry) = registry {
            table.insert(
                "registry",
                toml_edit::Item::Value(toml_edit::Value::from(registry)),
            );
        }
    }
}

fn verify_cargo_dependency_path(
    name: &str,
    value: Option<&toml_edit::Value>,
    desired: &Path,
) -> Result<()> {
    let Some(path) = value.and_then(toml_edit::Value::as_str) else {
        bail!(
            "Cargo dependency `{name}` already exists but is not the required path dependency `{}`",
            desired.display()
        );
    };
    let actual = normalize_existing_cargo_path(path)
        .with_context(|| format!("Cargo dependency `{name}` path"))?;
    if actual != desired {
        bail!(
            "Cargo dependency `{name}` points to `{path}`, but the component registry requires `{}`",
            desired.display()
        );
    }
    Ok(())
}

fn normalize_existing_cargo_path(input: &str) -> Result<PathBuf> {
    let path = Path::new(input);
    if path.is_absolute() {
        bail!("absolute dependency paths are not managed by `proa ui add`");
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            _ => bail!("dependency path contains an unsafe path component"),
        }
    }
    if normalized.as_os_str().is_empty() {
        bail!("dependency path is empty");
    }
    Ok(normalized)
}

fn merge_inline_dependency_options(
    name: &str,
    table: &mut toml_edit::InlineTable,
    desired: &CargoDependencySpec,
) -> Result<()> {
    let features = merged_feature_set(name, table.get("features"), &desired.features)?;
    if !features.is_empty() {
        table.insert(
            "features",
            toml_edit::Value::Array(feature_array(&features)),
        );
    }
    if table.get("default-features").is_none() {
        if let Some(default_features) = desired.default_features {
            table.insert("default-features", toml_edit::Value::from(default_features));
        }
    }
    Ok(())
}

fn merge_table_dependency_options(
    name: &str,
    table: &mut toml_edit::Table,
    desired: &CargoDependencySpec,
) -> Result<()> {
    let existing = table.get("features").and_then(toml_edit::Item::as_value);
    let features = merged_feature_set(name, existing, &desired.features)?;
    if !features.is_empty() {
        table.insert(
            "features",
            toml_edit::Item::Value(toml_edit::Value::Array(feature_array(&features))),
        );
    }
    if table.get("default-features").is_none() {
        if let Some(default_features) = desired.default_features {
            table.insert(
                "default-features",
                toml_edit::Item::Value(toml_edit::Value::from(default_features)),
            );
        }
    }
    Ok(())
}

fn merged_feature_set(
    name: &str,
    existing: Option<&toml_edit::Value>,
    desired: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut merged = desired.clone();
    if let Some(existing) = existing {
        let array = existing.as_array().ok_or_else(|| {
            anyhow::anyhow!("Cargo dependency `{name}` has a non-array `features` value")
        })?;
        for value in array.iter() {
            let feature = value.as_str().ok_or_else(|| {
                anyhow::anyhow!("Cargo dependency `{name}` has a non-string feature")
            })?;
            merged.insert(feature.to_string());
        }
    }
    Ok(merged)
}

fn feature_array(features: &BTreeSet<String>) -> toml_edit::Array {
    let mut array = toml_edit::Array::new();
    for feature in features {
        array.push(feature.as_str());
    }
    array
}

fn path_for_toml(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn commit_transaction(
    root: &Path,
    mutations: &[FileMutation],
    lock: &ComponentLockFile,
) -> Result<()> {
    let staging = unique_staging_dir(root)?;
    fs::create_dir(&staging)
        .with_context(|| format!("create transaction staging {}", staging.display()))?;

    let result = (|| -> Result<()> {
        let files_dir = staging.join("files");
        fs::create_dir(&files_dir)
            .with_context(|| format!("create transaction files {}", files_dir.display()))?;
        let backups_dir = staging.join("backups");
        fs::create_dir(&backups_dir)
            .with_context(|| format!("create transaction backups {}", backups_dir.display()))?;

        let mut all_mutations = mutations
            .iter()
            .map(|mutation| FileMutation {
                target: mutation.target.clone(),
                target_relative: mutation.target_relative.clone(),
                before: mutation.before.clone(),
                after: mutation.after.clone(),
            })
            .collect::<Vec<_>>();
        let lock_path = root.join(LOCK_FILE);
        let lock_before = match fs::read(&lock_path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read component lock {}", lock_path.display()))
            }
        };
        let mut lock_after = serde_json::to_vec_pretty(lock).context("serialize proa.lock.json")?;
        lock_after.push(b'\n');
        all_mutations.push(FileMutation {
            target: lock_path,
            target_relative: PathBuf::from(LOCK_FILE),
            before: lock_before,
            after: lock_after,
        });

        let mut staged_files = Vec::with_capacity(all_mutations.len());
        for (index, mutation) in all_mutations.iter().enumerate() {
            let staged = files_dir.join(index.to_string());
            write_new_file(&staged, &mutation.after)?;
            staged_files.push((staged, mutation));
        }

        for (_, mutation) in &staged_files {
            ensure_no_symlink_path(root, &mutation.target_relative)?;
            if let Some(parent) = mutation.target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create install directory {}", parent.display()))?;
            }
        }

        let mut applied: Vec<(PathBuf, Option<PathBuf>)> = Vec::with_capacity(staged_files.len());
        for (index, (staged, mutation)) in staged_files.iter().enumerate() {
            let current = match fs::read(&mutation.target) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    rollback_applied_mutations(&applied);
                    return Err(error).with_context(|| {
                        format!("recheck install target {}", mutation.target.display())
                    });
                }
            };
            if current != mutation.before {
                rollback_applied_mutations(&applied);
                bail!(
                    "install target `{}` changed during the transaction; all published files were rolled back",
                    mutation.target_relative.display()
                );
            }

            let backup = if mutation.before.is_some() {
                let backup = backups_dir.join(index.to_string());
                if let Err(error) = fs::rename(&mutation.target, &backup) {
                    rollback_applied_mutations(&applied);
                    return Err(error).with_context(|| {
                        format!("back up install target {}", mutation.target.display())
                    });
                }
                Some(backup)
            } else {
                None
            };

            if let Err(error) = fs::rename(staged, &mutation.target) {
                if let Some(backup) = &backup {
                    let _ = fs::rename(backup, &mutation.target);
                }
                rollback_applied_mutations(&applied);
                return Err(error).with_context(|| {
                    format!(
                        "publish staged component file {}",
                        mutation.target_relative.display()
                    )
                });
            }
            applied.push((mutation.target.clone(), backup));
        }
        Ok(())
    })();

    let cleanup = fs::remove_dir_all(&staging);
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => {
            eprintln!(
                "warning: could not remove transaction staging {}: {error}",
                staging.display()
            );
            Ok(())
        }
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn rollback_applied_mutations(applied: &[(PathBuf, Option<PathBuf>)]) {
    for (target, backup) in applied.iter().rev() {
        if let Err(error) = fs::remove_file(target) {
            eprintln!(
                "warning: could not remove transaction output {} during rollback: {error}",
                target.display()
            );
        }
        if let Some(backup) = backup {
            if let Err(error) = fs::rename(backup, target) {
                eprintln!(
                    "warning: could not restore {} during rollback: {error}",
                    target.display()
                );
            }
        }
    }
}

fn unique_staging_dir(root: &Path) -> Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_nanos();
    for attempt in 0..100_u32 {
        let candidate = root.join(format!(
            ".proa-ui-staging-{}-{nanos}-{attempt}",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!("could not allocate a unique Proa UI transaction staging directory")
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync {}", path.display()))
}

fn canonical_project_root(root: &Path) -> Result<PathBuf> {
    let metadata =
        fs::metadata(root).with_context(|| format!("inspect project root {}", root.display()))?;
    if !metadata.is_dir() {
        bail!("project root {} is not a directory", root.display());
    }
    root.canonicalize()
        .with_context(|| format!("canonicalize project root {}", root.display()))
}

fn with_registry_bearer_auth(
    request: RequestBuilder,
    registry: &str,
    token: Option<&str>,
) -> RequestBuilder {
    if registry != PRO_REGISTRY {
        return request;
    }
    match token.map(str::trim).filter(|token| !token.is_empty()) {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

fn pro_registry_bearer_token() -> Result<Option<String>> {
    let environment_token = pro_token_from_environment(|name| env::var(name))?;
    let cargo_home = cargo_home_dir();
    pro_registry_bearer_token_from_sources(environment_token.as_deref(), cargo_home.as_deref())
}

fn pro_token_from_environment(
    read: impl Fn(&str) -> std::result::Result<String, env::VarError>,
) -> Result<Option<String>> {
    // Keep the old token variable as a compatibility fallback for Pro catalogs.
    for name in ["PROA_PRO_TOKEN", "CARGO_REGISTRIES_PROA_TOKEN"] {
        match read(name) {
            Ok(token) if !token.trim().is_empty() => return Ok(Some(token)),
            Ok(_) | Err(env::VarError::NotPresent) => {}
            Err(env::VarError::NotUnicode(_)) => bail!("{name} is not valid UTF-8"),
        }
    }
    Ok(None)
}

fn cargo_home_dir() -> Option<PathBuf> {
    env::var_os("CARGO_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".cargo"))
        })
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".cargo"))
        })
}

fn pro_registry_bearer_token_from_sources(
    environment_token: Option<&str>,
    cargo_home: Option<&Path>,
) -> Result<Option<String>> {
    if let Some(token) = environment_token
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        return Ok(Some(token.to_string()));
    }

    let Some(cargo_home) = cargo_home else {
        return Ok(None);
    };

    // Cargo gives the legacy extensionless file precedence when both exist.
    // Preserve existing Pro catalog credentials during migration.
    for name in ["credentials", "credentials.toml"] {
        let path = cargo_home.join(name);
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "read Cargo credentials file {}: {error}",
                    path.display()
                ));
            }
        };
        let credentials: toml::Value = contents.parse().map_err(|_| {
            // Do not attach the parser's source excerpt: this file contains secrets.
            anyhow::anyhow!("parse Cargo credentials file {}", path.display())
        })?;
        let Some(token) = credentials
            .get("registries")
            .and_then(|registries| registries.get("proa"))
            .and_then(|registry| registry.get("token"))
        else {
            return Ok(None);
        };
        let token = token.as_str().ok_or_else(|| {
            anyhow::anyhow!(
                "Cargo credentials entry `registries.proa.token` in {} must be a string",
                path.display()
            )
        })?;
        return Ok((!token.trim().is_empty()).then(|| token.trim().to_string()));
    }

    Ok(None)
}

fn bare_registry_fallback_error(
    slug: &str,
    public_not_found: &str,
    pro_error: FetchError,
) -> anyhow::Error {
    let pro_failure = match pro_error {
        FetchError::NotFound(detail) => format!("also returned not found ({detail})"),
        FetchError::Unauthorized(detail) => {
            format!("rejected the Pro lookup as unauthorized ({detail})")
        }
        FetchError::Failed(detail) => format!("could not be fetched ({detail})"),
    };
    anyhow::anyhow!(
        "component `{slug}` was not found in the public Free registry `{PUBLIC_REGISTRY}` \
         ({public_not_found}). Bare slugs then fall back to the licensed Pro registry \
         `{PRO_REGISTRY}`, which {pro_failure}. Free registry access does not require a bearer \
         token. To limit resolution to the public registry, use `{PUBLIC_REGISTRY}/{slug}` or \
         `--registry {PUBLIC_REGISTRY}`. No project files were changed."
    )
}

fn fetch_error_to_anyhow(registry: &str, slug: &str, error: FetchError) -> anyhow::Error {
    match error {
        FetchError::Unauthorized(detail) if registry == PRO_REGISTRY => anyhow::anyhow!(
            "Pro component `{slug}` requires authenticated `{PRO_REGISTRY}` access ({detail}). \
             Provide its bearer token through PROA_PRO_TOKEN. No project files were changed."
        ),
        FetchError::NotFound(detail) => anyhow::anyhow!(
            "component `{registry}/{slug}` was not found ({detail}). No project files were changed."
        ),
        FetchError::Unauthorized(detail) => anyhow::anyhow!(
            "registry `{registry}` rejected component `{slug}` ({detail}). No project files were changed."
        ),
        FetchError::Failed(detail) => anyhow::anyhow!(
            "could not fetch component `{registry}/{slug}`: {detail}. \
             Component artifacts may not be published at the configured endpoint yet. \
             Check `proa.config.json` with `proa registry list`. No project files were changed."
        ),
    }
}

#[cfg(test)]
mod tests;
