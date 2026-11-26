// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The deployable output of a build: a bundle written once and only read.
//!
//! A bundle holds each file at the path of its URL, under `public/`, so the
//! same directory serves a container's own server and a static host:
//!
//! ```text
//! app.assets/
//!   manifest.json                         read by the server, never served
//!   public/
//!     favicon.ico                         root files, served by the origin
//!     _proa/
//!       immutable/styles.<hash>.css       cached for a year
//!       mutable/data.json                 revalidated on every load
//! ```
//!
//! It is assembled beside its destination and moved into place whole, so an
//! export that fails while building leaves the previous bundle untouched.
//! The move itself is two renames (the old bundle out, the new one in). A
//! process killed between them leaves no bundle at the destination and the
//! old one in a `.proa-export-*` directory beside it, which the next export
//! replaces. There is no lock file, staging area, or history in a bundle:
//! nothing writes to it again.

use super::*;

/// One of the framework's own modules, to be stored with the public files.
pub struct FrameworkFile<'a> {
    /// The canonical name the framework knows the module by.
    pub name: &'a str,
    /// Path below [`PUBLIC_ROUTE`], inside the immutable directory.
    pub path: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Debug)]
pub struct Export {
    pub directory: PathBuf,
    pub manifest: AssetManifest,
    /// See [`PublishReport::diagnostics`].
    pub diagnostics: Vec<Diagnostic>,
}

/// Write the bundle for `source` and `framework` at `destination`, replacing
/// a bundle or store an earlier build left there.
///
/// Fails with [`AssetError::InUse`] while a server is reading the output it
/// would replace, and with [`AssetError::NotOwned`] when `destination` is
/// something Proa did not write.
pub fn export(
    destination: &Path,
    source: Option<&Path>,
    framework: &[FrameworkFile<'_>],
) -> Result<Export> {
    export_with_framework_source(destination, source, framework, None)
}

/// [`export`], recording which linked sources `framework` was made from.
pub fn export_with_framework_source(
    destination: &Path,
    source: Option<&Path>,
    framework: &[FrameworkFile<'_>],
    framework_source: Option<&str>,
) -> Result<Export> {
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    at(parent, fs::create_dir_all(parent))?;
    let source = source
        .map(|source| at(source, source.canonicalize()))
        .transpose()?;
    if let Some(source) = &source {
        let parent = at(parent, parent.canonicalize())?;
        let destination = parent.join(destination.file_name().unwrap_or_default());
        if source.starts_with(&destination) || destination.starts_with(source) {
            return Err(AssetError::OverlappingDirectories);
        }
    }
    sweep_abandoned(parent);
    // Everything is built here first. Dropping this on any error removes it.
    let work = at(
        parent,
        tempfile::Builder::new()
            .prefix(WORK_PREFIX)
            .tempdir_in(parent),
    )?;
    // The marker is what lets a later export recognize this directory as its
    // own leftover, should this one be killed.
    let work_marker = work.path().join(WORK_MARKER_FILE);
    at(&work_marker, fs::write(&work_marker, WORK_MARKER_BYTES))?;
    let bundle = work.path().join("bundle");
    at(&bundle, fs::create_dir(&bundle))?;
    set_traversable_directory_permissions(&bundle)?;
    let marker = bundle.join(OWNER_FILE);
    at(&marker, fs::write(&marker, OWNER_BYTES))?;
    set_readable_file_permissions(&marker, &at(&marker, File::open(&marker))?)?;

    let public = bundle.join(BUNDLE_PUBLIC_DIRECTORY);
    let reserved = PUBLIC_ROUTE.trim_start_matches('/');
    let mut manifest = AssetManifest::default();
    let mut diagnostics = Vec::new();
    if let Some(source) = &source {
        let stage = work.path().join("stage");
        at(&stage, fs::create_dir(&stage))?;
        // Names are reserved against one another only within this build: a
        // bundle is complete in itself and remembers no earlier one.
        let mut history = History {
            version: MANIFEST_VERSION,
            assignments: BTreeMap::new(),
        };
        let snapshot = snapshot(source, &stage, &mut history)?;
        for (index, logical) in snapshot.files.iter().enumerate() {
            let entry = &snapshot.manifest.assets[logical];
            let relative = format!("{reserved}/{}", entry.url_path());
            let staged = stage.join(index.to_string());
            if !place(&public, &relative, Placement::Move(&staged))? {
                // Two logical paths with the same bytes share one file.
                at(&staged, fs::remove_file(&staged))?;
            }
        }
        manifest = snapshot.manifest;
        diagnostics = snapshot.diagnostics;
        for (path, logical) in &manifest.root {
            let entry = &manifest.assets[logical];
            let published = public.join(reserved).join(entry.url_path());
            place(&public, path, Placement::Copy(&published))?;
        }
    }
    for file in framework {
        let relative = format!("{reserved}/{}", file.path);
        place(&public, &relative, Placement::Write(file.bytes))?;
        manifest.framework.insert(
            file.name.into(),
            FrameworkEntry {
                path: file.path.into(),
                sha256: sha256(file.bytes),
                integrity: integrity(file.bytes),
                byte_length: file.bytes.len() as u64,
            },
        );
    }
    manifest.framework_source = framework_source.map(str::to_owned);
    manifest.layout = Layout::Bundle;
    manifest.validate()?;
    write_json(&bundle, work.path(), MANIFEST_FILE, &manifest)?;

    // Take the old output out of the way and put this one in its place. The
    // claim is held across both moves so no server starts on a directory
    // that is about to change.
    let previous = work.path().join("previous");
    let _claim: Option<PendingRemoval> = match fs::symlink_metadata(destination) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(AssetError::Io {
                path: destination.into(),
                source,
            })
        }
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(AssetError::NotOwned(destination.into()));
            }
            if is_output(destination) {
                let claim = Some(
                    reserve_bundle_removal(destination)?
                        .ok_or_else(|| AssetError::InUse(destination.into()))?,
                );
                // Windows cannot move a directory that holds an open file.
                #[cfg(not(unix))]
                let claim = {
                    drop(claim);
                    None
                };
                at(destination, fs::rename(destination, &previous))?;
                claim
            } else {
                // Only what an interrupted writer left may be replaced.
                if !OutputStore::accepts(destination) {
                    return Err(AssetError::NotOwned(destination.into()));
                }
                at(destination, fs::remove_dir_all(destination))?;
                None
            }
        }
    };
    if let Err(source) = fs::rename(&bundle, destination) {
        if previous.exists() && fs::rename(&previous, destination).is_err() {
            // The old bundle could not be put back. Keep it where it is
            // instead of deleting it with the working directory, and say
            // where, so it can be restored by hand.
            return Err(AssetError::Io {
                path: work.keep().join("previous"),
                source,
            });
        }
        return Err(AssetError::Io {
            path: destination.into(),
            source,
        });
    }
    Ok(Export {
        directory: destination.into(),
        manifest,
        diagnostics,
    })
}

const WORK_PREFIX: &str = ".proa-export-";
/// Marks an export's working directory. It is deliberately not the marker a
/// bundle or store carries: a real output whose name happens to start with
/// [`WORK_PREFIX`] must never pass for a leftover.
pub(super) const WORK_MARKER_FILE: &str = ".proa-export-work";
pub(super) const WORK_MARKER_BYTES: &[u8] = b"proa-export-work-v1\n";

/// Remove working directories an export left beside its destination when it
/// was killed. A directory goes only when it carries the marker an export
/// writes into its own working directory, never on its name alone: anything
/// else there, such as an application directory that happens to share the
/// prefix, belongs to someone else. Only old ones go, because a young one may
/// belong to an export that is running now. Failing to clean up is never a
/// reason to fail an export.
fn sweep_abandoned(parent: &Path) {
    const ABANDONED_AFTER: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let named = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(WORK_PREFIX));
        let old = fs::symlink_metadata(&path)
            .ok()
            .filter(|metadata| metadata.is_dir())
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > ABANDONED_AFTER);
        let marker = path.join(WORK_MARKER_FILE);
        let marked = fs::symlink_metadata(&marker).is_ok_and(|metadata| metadata.is_file())
            && fs::read(&marker).is_ok_and(|bytes| bytes == WORK_MARKER_BYTES);
        if named && old && marked {
            let _ = fs::remove_dir_all(path);
        }
    }
}

enum Placement<'a> {
    Move(&'a Path),
    Copy(&'a Path),
    Write(&'a [u8]),
}

/// Put a file at `relative` under `root`, readable by any user. Returns
/// `false` when a file is already there.
fn place(root: &Path, relative: &str, placement: Placement<'_>) -> Result<bool> {
    validate_relative(relative)?;
    let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
    at(root, fs::create_dir_all(root))?;
    set_traversable_directory_permissions(root)?;
    ensure_directory(root, parent)?;
    let destination = root.join(relative);
    if fs::symlink_metadata(&destination).is_ok() {
        return Ok(false);
    }
    match placement {
        Placement::Move(staged) => at(&destination, fs::rename(staged, &destination))?,
        Placement::Copy(from) => {
            at(&destination, fs::copy(from, &destination))?;
        }
        Placement::Write(bytes) => at(&destination, fs::write(&destination, bytes))?,
    }
    let file = at(&destination, File::open(&destination))?;
    set_readable_file_permissions(&destination, &file)?;
    Ok(true)
}

/// Claim an output of either layout for deletion without deleting it yet, or
/// return `None` while a server is reading it. Dropping the claim leaves the
/// output untouched; [`PendingRemoval::commit`] deletes it.
pub fn reserve_bundle_removal(directory: &Path) -> Result<Option<PendingRemoval>> {
    validate_owner(directory)?;
    if fs::symlink_metadata(directory.join(WRITER_FILE)).is_ok() {
        return OutputStore::open(directory)?.reserve_removal();
    }
    let root = at(directory, directory.canonicalize())?;
    let marker = root.join(OWNER_FILE);
    let readers = at(&marker, File::open(&marker))?;
    match readers.try_lock() {
        Ok(()) => Ok(Some(PendingRemoval {
            root,
            _store: None,
            readers,
        })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(source)) => Err(AssetError::Io {
            path: marker,
            source,
        }),
    }
}

/// Delete an output of either layout unless a server is reading it, and
/// report whether it went.
pub fn remove_bundle(directory: &Path) -> Result<bool> {
    match reserve_bundle_removal(directory)? {
        Some(removal) => removal.commit().map(|()| true),
        None => Ok(false),
    }
}
