// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use proa_assets::{build::OutputStore, sha256, OUTPUT_ENV};
use serde::Serialize;

const START_MANIFEST_SCHEMA_VERSION: u16 = 1;
const START_MANIFEST_FILE: &str = "start-manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartManifest {
    pub schema_version: u16,
    pub binary: String,
    pub profile: String,
    pub executable: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_assets: Option<PathBuf>,
}

pub(crate) fn start_manifest_path(target_dir: &Path) -> PathBuf {
    target_dir
        .join(TOOL_NAMESPACE_DIR)
        .join(START_MANIFEST_FILE)
}

pub(crate) fn record_start_manifest(
    target_dir: &Path,
    binary: &str,
    profile: &str,
    executable: &Path,
    public_assets: Option<&Path>,
) -> Result<()> {
    let path = start_manifest_path(target_dir);
    let parent = path
        .parent()
        .context("start manifest must have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let manifest = StartManifest {
        schema_version: START_MANIFEST_SCHEMA_VERSION,
        binary: binary.to_string(),
        profile: profile.to_string(),
        executable: std::path::absolute(executable).unwrap_or_else(|_| executable.to_path_buf()),
        public_assets: public_assets
            .map(|dir| std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf())),
    };
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    let tmp = parent.join(format!(
        ".{START_MANIFEST_FILE}.proa-{}.tmp",
        std::process::id()
    ));
    fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("commit {}", path.display()))?;
    Ok(())
}

pub(crate) fn load_start_manifest(target_dir: &Path) -> Option<StartManifest> {
    let bytes = fs::read(start_manifest_path(target_dir)).ok()?;
    let manifest: StartManifest = serde_json::from_slice(&bytes).ok()?;
    (manifest.schema_version == START_MANIFEST_SCHEMA_VERSION).then_some(manifest)
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
    println!(
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
