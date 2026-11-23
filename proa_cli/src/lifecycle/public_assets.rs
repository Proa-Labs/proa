// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use proa_assets::{
    build::{OutputStore, PendingRemoval},
    sha256, OUTPUT_ENV,
};
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
    let Some(source) = public_source(root, directory) else {
        return Ok(None);
    };
    publish_with(&mut OutputStore::open(output)?, &source, "published", "to").map(Some)
}

/// The catalog side of an export, prepared before the executable is replaced.
pub(super) struct Bundle {
    /// The catalog now beside the executable, when one was published.
    pub(super) directory: Option<PathBuf>,
    stale: Option<PendingRemoval>,
}

impl Bundle {
    /// Call once the executable is in place. A build that published nothing
    /// removes the catalog an earlier export left, so the new executable
    /// never starts with stale files.
    pub(super) fn finish(self) -> Result<()> {
        match self.stale {
            Some(stale) => stale
                .commit()
                .context("remove the stale public asset catalog"),
            None => Ok(()),
        }
    }
}

/// Place the bundle beside an exported executable, where the server finds it
/// without configuration. The executable and this directory are the complete
/// deployable output: copying both anywhere is enough to run the application,
/// and the bundle's `public/` directory is what a static host would serve.
///
/// This runs before the executable is replaced, and everything that can
/// refuse the export is decided here. The bundle is assembled beside its
/// destination and moved into place whole, so a failure leaves the previous
/// executable with the bundle it had. Removal of a stale bundle is only
/// claimed here; [`Bundle::finish`] performs it.
pub(super) fn bundle(
    root: &Path,
    directory: Option<&Path>,
    executable: &Path,
    framework: Option<&FrameworkExport>,
) -> Result<Bundle> {
    // Asset commands ran since the first check and may have changed the paths.
    check_bundle_destination(root, directory, executable)?;
    let output = bundled_output(executable)?;
    let source = public_source(root, directory);
    if source.is_none() && framework.is_none() {
        return clear_bundle(executable);
    }
    let files = framework.map(FrameworkExport::files).transpose()?;
    let report = proa_assets::build::export_with_framework_source(
        &output,
        source.as_deref(),
        files.as_deref().unwrap_or_default(),
        framework.map(|framework| framework.source.as_str()),
    )
    .map_err(|error| match error {
        proa_assets::AssetError::InUse(path) => anyhow::anyhow!(
            "`{}` is still being read by a running server. Stop that server, or export the executable to a different path",
            path.display()
        ),
        error => anyhow::Error::new(error),
    })
    .with_context(|| format!("bundle public assets beside `{}`", executable.display()))?;
    for diagnostic in &report.diagnostics {
        eprintln!("warning: {diagnostic}");
    }
    let modules = match report.manifest.framework.len() {
        0 => String::new(),
        count => format!(" and {count} RSJS modules"),
    };
    super::dev_ui::detail!(
        "bundled {} public assets{modules} in {}",
        report.manifest.assets.len(),
        report.directory.display()
    );
    Ok(Bundle {
        directory: Some(report.directory),
        stale: None,
    })
}

/// The framework's own browser modules, exported from a built executable.
pub(super) struct FrameworkExport {
    source: String,
    /// `(name, path)`, with `path` relative to the directory.
    index: Vec<(String, String)>,
    contents: Vec<Vec<u8>>,
}

#[derive(Deserialize)]
struct ExportedHost {
    source: String,
    files: Vec<(String, String)>,
}

impl FrameworkExport {
    fn files(&self) -> Result<Vec<proa_assets::build::FrameworkFile<'_>>> {
        let route = proa_assets::PUBLIC_ROUTE.trim_start_matches('/');
        self.index
            .iter()
            .zip(&self.contents)
            .map(|((name, path), bytes)| {
                let path = path
                    .strip_prefix(route)
                    .and_then(|path| path.strip_prefix('/'))
                    .with_context(|| format!("exported module `{path}` is outside `/{route}`"))?;
                Ok(proa_assets::build::FrameworkFile { name, path, bytes })
            })
            .collect()
    }
}

/// How long a built executable gets to write its modules. It runs no
/// application code to do so, only the framework's own preparation.
const EXPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Ask `executable` for the framework modules linked into it, so the bundle
/// can hold them as files.
///
/// `None` means there is nothing to store: the executable does not serve
/// such modules, or was built for a target this machine cannot run. The
/// server then prepares and serves them itself, as it does under `proa dev`,
/// and only hosting those files somewhere else is unavailable.
pub(super) fn export_framework_assets(
    executable: &Path,
    explicit_target: bool,
) -> Result<Option<FrameworkExport>> {
    use rsjs_transport::framework_export::{ENV, INDEX_FILE, SENTINEL};
    let bytes = fs::read(executable).with_context(|| format!("read `{}`", executable.display()))?;
    // An executable without the sentinel would not answer the request: it
    // would simply start. Never run one.
    if !bytes
        .windows(SENTINEL.len())
        .any(|window| window == SENTINEL)
    {
        return Ok(None);
    }
    drop(bytes);
    let directory = tempfile::tempdir().context("create a framework asset export directory")?;
    // The child's diagnostics go to a file. A pipe nobody reads until the
    // child exits would stall it as soon as it wrote more than the pipe holds.
    let diagnostics = tempfile::NamedTempFile::new().context("create an export log")?;
    let spawned = Command::new(executable)
        .env(ENV, directory.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(diagnostics.reopen().context("open the export log")?)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        // A cross-compiled executable cannot be started on this machine.
        Err(error) if explicit_target => {
            eprintln!(
                "note: `{}` cannot run on this machine ({error}), so its RSJS modules are not stored in the bundle; the server will serve them itself",
                executable.display()
            );
            return Ok(None);
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("start `{}` to export its modules", executable.display()))
        }
    };
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > EXPORT_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "`{}` did not finish exporting its RSJS modules within {} seconds",
                executable.display(),
                EXPORT_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let index_path = directory.path().join(INDEX_FILE);
    if !status.success() || !index_path.is_file() {
        let reason = fs::read(diagnostics.path())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        bail!(
            "`{}` could not export its RSJS modules ({status}): {}",
            executable.display(),
            reason.trim()
        );
    }
    let hosts: Vec<ExportedHost> = serde_json::from_slice(&fs::read(&index_path)?)
        .context("read the exported module index")?;
    // One host exports today, and it compares this value with its own
    // digest. A second exporter would need the manifest to record a digest
    // for each host.
    let source = hosts
        .iter()
        .map(|host| host.source.as_str())
        .collect::<Vec<_>>()
        .join("+");
    let index: Vec<(String, String)> = hosts.into_iter().flat_map(|host| host.files).collect();
    if index.is_empty() {
        return Ok(None);
    }
    let mut contents = Vec::with_capacity(index.len());
    for (_, path) in &index {
        crate::config::validate_project_relative_path(Path::new(path))
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("exported module path `{path}`"))?;
        contents.push(fs::read(directory.path().join(path))?);
    }
    Ok(Some(FrameworkExport {
        source,
        index,
        contents,
    }))
}

/// Refuse an export whose bundle path cannot be used, from paths alone. The
/// build calls this before compiling, so a bad `--artifact-out` costs nothing.
pub(super) fn check_bundle_destination(
    root: &Path,
    directory: Option<&Path>,
    executable: &Path,
) -> Result<()> {
    if executable.is_dir() {
        bail!(
            "artifact output must name a file, but `{}` is a directory",
            executable.display()
        );
    }
    let output = bundled_output(executable)?;
    // Opening the output store here would leave a lock file inside a
    // directory that belongs to something else.
    if !OutputStore::accepts(&output) {
        bail!(
            "`{}` is in the way: Proa bundles the public asset catalog beside the exported executable `{}`. Move it, or export the executable to a different path",
            output.display(),
            executable.display()
        );
    }
    // Catalog files inside the public source would be published by the next
    // build.
    if let Some(source) = public_source(root, directory) {
        if overlaps(&source, &output) {
            bail!(
                "`{}` overlaps the public directory `{}`: export the executable `{}` outside it",
                output.display(),
                source.display(),
                executable.display()
            );
        }
    }
    Ok(())
}

/// With nothing bundled, claim the catalog an earlier export left beside
/// this executable path. A catalog a running server is reading refuses the
/// export here, before the executable changes. A directory Proa did not
/// write is not a catalog and is left alone.
pub(super) fn clear_bundle(executable: &Path) -> Result<Bundle> {
    let output = bundled_output(executable)?;
    let output = output.as_path();
    let mut bundle = Bundle {
        directory: None,
        stale: None,
    };
    let is_symlink = fs::symlink_metadata(output).is_ok_and(|metadata| metadata.is_symlink());
    if is_symlink || !proa_assets::is_output(output) {
        return Ok(bundle);
    }
    let context = || format!("remove stale public asset catalog {}", output.display());
    bundle.stale = proa_assets::build::reserve_bundle_removal(output).with_context(context)?;
    if bundle.stale.is_none() {
        bail!(
            "`{}` is still being read by a running server, and this build publishes no public assets to replace it. Stop that server, or export the executable to a different path",
            output.display()
        );
    }
    Ok(bundle)
}

fn bundled_output(executable: &Path) -> Result<PathBuf> {
    proa_assets::bundled_output(executable).with_context(|| {
        format!(
            "exported executable `{}` has no file name",
            executable.display()
        )
    })
}

/// Whether one directory contains the other. `output` may not exist yet, so
/// its deepest existing ancestor is resolved and the rest appended.
fn overlaps(source: &Path, output: &Path) -> bool {
    let (Ok(source), Ok(output)) = (source.canonicalize(), std::path::absolute(output)) else {
        return false;
    };
    let mut missing = Vec::new();
    let mut existing = output.as_path();
    let base = loop {
        if let Ok(base) = existing.canonicalize() {
            break base;
        }
        let (Some(name), Some(parent)) = (existing.file_name(), existing.parent()) else {
            return false;
        };
        missing.push(name);
        existing = parent;
    };
    let output = missing
        .iter()
        .rev()
        .fold(base, |path, name| path.join(name));
    output.starts_with(&source) || source.starts_with(&output)
}

/// The public directory to publish, when there is one.
fn public_source(root: &Path, directory: Option<&Path>) -> Option<PathBuf> {
    let directory = directory?;
    let source = root.join(directory);
    // Asset commands and Cargo build scripts may create the default directory.
    // Check it after they run and leave missing custom paths as errors.
    let absent_default = directory == Path::new(crate::config::DEFAULT_PUBLIC_DIRECTORY)
        && fs::symlink_metadata(&source)
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound);
    (!absent_default).then_some(source)
}

fn publish_with(
    store: &mut OutputStore,
    source: &Path,
    verb: &str,
    preposition: &str,
) -> Result<PathBuf> {
    let report = store
        .publish(source)
        .with_context(|| format!("publish configured public directory `{}`", source.display()))?;
    if let Err(error) = report.cleanup {
        eprintln!("warning: public asset cleanup deferred: {error}");
    }
    if let Some(error) = report.durability_error {
        eprintln!("warning: public manifest activated, but directory sync failed: {error}");
    }
    for diagnostic in &report.diagnostics {
        eprintln!("warning: {diagnostic}");
    }
    super::dev_ui::detail!(
        "{verb} {} public assets {preposition} {}",
        report.manifest.assets.len(),
        report.output_directory.display()
    );
    Ok(report.output_directory)
}

/// Whether the running server follows the catalog just published in place.
///
/// Any doubt, including a failure to ask, answers `false`: restarting a
/// server that would have reloaded costs a moment, while skipping the restart
/// of one that cannot reload leaves stale files on screen with no error.
pub(super) fn has_live_reader(output: &Path) -> bool {
    OutputStore::open(output)
        .and_then(|store| store.has_live_reader())
        .unwrap_or(false)
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
