// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use proa_assets::{build::OutputStore, sha256, OUTPUT_ENV};
use serde::Serialize;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const START_MANIFEST_SCHEMA_VERSION: u16 = 1;
const START_MANIFESTS_DIR: &str = "start-manifests";
static START_MANIFEST_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartManifestEntry {
    pub schema_version: u16,
    pub package_id: String,
    pub binary: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_triple: Option<String>,
    pub executable: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_assets: Option<PathBuf>,
    pub updated_at_nanos: u128,
}

pub(crate) fn start_manifests_dir(target_dir: &Path) -> PathBuf {
    target_dir
        .join(TOOL_NAMESPACE_DIR)
        .join(START_MANIFESTS_DIR)
}

fn start_manifest_entry_path(
    target_dir: &Path,
    package_id: &str,
    binary: &str,
    profile: &str,
    target_triple: Option<&str>,
    executable: &Path,
) -> Result<PathBuf> {
    let key = serde_json::to_vec(&(
        package_id,
        binary,
        profile,
        target_triple,
        executable.to_string_lossy(),
    ))?;
    Ok(start_manifests_dir(target_dir).join(format!("{}.json", sha256(&key))))
}

pub(crate) fn record_start_manifest(
    target_dir: &Path,
    package_id: &str,
    binary: &str,
    profile: &str,
    target_triple: Option<&str>,
    executable: &Path,
    public_assets: Option<&Path>,
) -> Result<()> {
    let abs_executable =
        std::path::absolute(executable).unwrap_or_else(|_| executable.to_path_buf());
    let abs_public_assets =
        public_assets.map(|dir| std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf()));
    let path = start_manifest_entry_path(
        target_dir,
        package_id,
        binary,
        profile,
        target_triple,
        &abs_executable,
    )?;
    let parent = path
        .parent()
        .context("start manifest entry must have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let updated_at_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let entry = StartManifestEntry {
        schema_version: START_MANIFEST_SCHEMA_VERSION,
        package_id: package_id.to_string(),
        binary: binary.to_string(),
        profile: profile.to_string(),
        target_triple: target_triple.map(ToString::to_string),
        executable: abs_executable,
        public_assets: abs_public_assets,
        updated_at_nanos,
    };
    let bytes = serde_json::to_vec_pretty(&entry)?;
    let seq = START_MANIFEST_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".entry.proa-{}-{seq}.tmp", std::process::id()));
    fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    if let Err(err) = replace_file(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(err).with_context(|| format!("commit {}", path.display()));
    }
    Ok(())
}

/// Every current-schema entry recorded under `target_dir`, oldest first.
pub(crate) fn load_start_manifest(target_dir: &Path) -> Vec<StartManifestEntry> {
    let Ok(items) = fs::read_dir(start_manifests_dir(target_dir)) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for item in items.flatten() {
        let path = item.path();
        if path.extension().and_then(OsStr::to_str) != Some("json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(entry) = serde_json::from_slice::<StartManifestEntry>(&bytes) else {
            continue;
        };
        if entry.schema_version == START_MANIFEST_SCHEMA_VERSION {
            entries.push(entry);
        }
    }
    entries.sort_by_key(|entry| entry.updated_at_nanos);
    entries
}

pub(super) fn configured_path(config: &ProjectConfig, enabled: bool) -> Result<Option<&Path>> {
    if !enabled {
        return Ok(None);
    }
    config
        .paths
        .public
        .as_deref()
        .map(|path| {
            crate::config::validate_project_relative_path(path)
                .map_err(anyhow::Error::msg)
                .context("invalid paths.public")?;
            Ok(path)
        })
        .transpose()
}

/// Source bytes and the CLI executable are deliberately absent: asset edits
/// and wrapper upgrades must not create a new public-output namespace.
pub(super) fn output_directory(
    target: &Path,
    selected: &ExactCargoRoot,
    parsed: &ParsedCargoInvocation,
) -> Result<PathBuf> {
    let selector = serde_json::to_vec(&(
        &selected.package_id,
        &selected.target_name,
        &selected.requested_target_kind,
        &parsed.profile,
        &parsed.target,
        &parsed.features,
        parsed
            .selection_args
            .iter()
            .filter(|arg| matches!(arg.as_str(), "--all-features" | "--no-default-features"))
            .collect::<BTreeSet<_>>(),
    ))?;
    let digest = sha256(&selector);
    Ok(target
        .join("proa-output")
        .join(&selected.target_name)
        .join(digest))
}

pub(super) fn publish(
    root: &Path,
    directory: Option<&Path>,
    output: &Path,
) -> Result<Option<PathBuf>> {
    let Some(directory) = directory else {
        return Ok(None);
    };
    let source = root.join(directory);
    // Asset commands and Cargo build scripts may create the default directory.
    // Check it after they run and leave missing custom paths as errors.
    if directory == Path::new(crate::config::DEFAULT_PUBLIC_DIRECTORY)
        && fs::symlink_metadata(&source).is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    {
        return Ok(None);
    }
    let report = OutputStore::open(output)?
        .publish(&source)
        .with_context(|| format!("publish configured public directory `{}`", source.display()))?;
    if let Err(error) = report.cleanup {
        eprintln!("warning: public asset cleanup deferred: {error}");
    }
    if let Some(error) = report.durability_error {
        eprintln!("warning: public manifest activated, but directory sync failed: {error}");
    }
    super::dev_ui::detail!(
        "published {} public assets to {}",
        report.manifest.assets.len(),
        report.output_directory.display()
    );
    Ok(Some(report.output_directory))
}

pub(super) fn cleanup(output: Option<&Path>) {
    if let Some(output) = output {
        if let Err(error) = OutputStore::open(output).and_then(|mut store| store.cleanup()) {
            eprintln!("warning: public asset cleanup deferred: {error}");
        }
    }
}

pub(super) fn runtime_environment(command: &mut Command, output: Option<&Path>) {
    match output {
        Some(output) => {
            command.env(OUTPUT_ENV, output);
        }
        None => {
            command.env_remove(OUTPUT_ENV);
        }
    }
}

pub(super) fn defer_execution(prepared: &mut PreparedLinkedCargo, action: LinkedCargoAction) {
    let args = &mut prepared.request.final_cargo_args;
    if let Some(separator) = args.iter().position(|arg| arg == "--") {
        args.truncate(separator);
    }
    match action {
        LinkedCargoAction::Run => args[0] = "build".into(),
        LinkedCargoAction::Test => args.push("--no-run".into()),
        _ => unreachable!("only run and test execute in the final Cargo pass"),
    }
}
