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
//! It is assembled beside its destination and moved into place whole, so a
//! failed export leaves the previous bundle exactly as it was. There is no
//! lock file, staging area, or history in it: nothing writes to it again.

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
    // Everything is built here first. Dropping this on any error removes it.
    let work = at(
        parent,
        tempfile::Builder::new()
            .prefix(".proa-export-")
            .tempdir_in(parent),
    )?;
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
    manifest.layout = Layout::Bundle;
    manifest.validate()?;
    write_json(&bundle, work.path(), MANIFEST_FILE, &manifest)?;

    // Take the old output out of the way and put this one in its place. The
    // claim is held across both moves so no server starts on a directory
    // that is about to change.
    let previous = work.path().join("previous");
    let claim = match fs::symlink_metadata(destination) {
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
                let claim = reserve_bundle_removal(destination)?
                    .ok_or_else(|| AssetError::InUse(destination.into()))?;
                // Windows cannot move a directory that holds an open file.
                #[cfg(not(unix))]
                let claim = drop(claim);
                at(destination, fs::rename(destination, &previous))?;
                Some(claim)
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
        if previous.exists() {
            let _ = fs::rename(&previous, destination);
        }
        return Err(AssetError::Io {
            path: destination.into(),
            source,
        });
    }
    drop(claim);
    Ok(Export {
        directory: destination.into(),
        manifest,
        diagnostics,
    })
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
