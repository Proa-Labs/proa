// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Single-writer local publication. This is not a deployment retention service.

use std::{collections::BTreeSet, fs::TryLockError, io::Write};

use super::*;

const WRITER_FILE: &str = ".writer.lock";
const HISTORY_FILE: &str = "fingerprints.json";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub removed_objects: usize,
    pub live_readers: bool,
}

#[derive(Debug)]
pub struct PublishReport {
    pub output_directory: PathBuf,
    pub manifest: AssetManifest,
    /// Maintenance failures do not turn an already activated catalog into a
    /// failed publication. Callers report these and retry cleanup later.
    pub cleanup: Result<CleanupReport>,
    pub durability_error: Option<AssetError>,
}

/// Exclusive writer ownership. Dropping this value releases the OS lock.
pub struct OutputStore {
    root: PathBuf,
    _writer: File,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct History {
    version: u32,
    /// Reservations outlive object deletion, so cleanup cannot permit a short
    /// fingerprint URL to be reassigned to different bytes later.
    assignments: BTreeMap<String, String>,
}

impl OutputStore {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref();
        at(directory, fs::create_dir_all(directory))?;
        let root = at(directory, directory.canonicalize())?;
        let writer_path = root.join(WRITER_FILE);
        let writer = open_lock(&writer_path)?;
        match writer.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(AssetError::WriterBusy(root)),
            Err(TryLockError::Error(source)) => {
                return Err(AssetError::Io {
                    path: writer_path,
                    source,
                })
            }
        }
        let marker = root.join(OWNER_FILE);
        if marker.try_exists().map_err(|source| AssetError::Io {
            path: marker.clone(),
            source,
        })? {
            canonical_owned(&root)?;
        } else {
            // Never adopt an existing application/Cargo directory as an output
            // root. Only the writer lock we just created is allowed here.
            for entry in at(&root, fs::read_dir(&root))? {
                let entry = at(&root, entry)?;
                if entry.file_name() != WRITER_FILE {
                    return Err(AssetError::NotOwned(root));
                }
            }
            let mut file = at(&marker, File::create_new(&marker))?;
            at(&marker, file.write_all(OWNER_BYTES))?;
            at(&marker, file.sync_all())?;
        }
        open_lock(&root.join(READERS_FILE))?;
        ensure_directory(&root, "objects")?;
        ensure_directory(&root, "staging")?;
        let store = Self {
            root,
            _writer: writer,
        };
        store.clean_staging()?;
        Ok(store)
    }

    pub fn directory(&self) -> &Path {
        &self.root
    }

    /// Snapshot public inputs, publish objects, then replace the manifest.
    /// Existing catalogs continue to read their own immutable object paths.
    pub fn publish(&mut self, source: impl AsRef<Path>) -> Result<PublishReport> {
        let source = source.as_ref();
        let source = at(source, source.canonicalize())?;
        if source.starts_with(&self.root) || self.root.starts_with(&source) {
            return Err(AssetError::OverlappingDirectories);
        }
        let files = collect_files(&source)?;
        let staging_root = self.root.join("staging");
        let stage = at(
            &staging_root,
            tempfile::Builder::new()
                .prefix("attempt-")
                .tempdir_in(&staging_root),
        )?;
        let mut manifest = AssetManifest::default();
        let mut history = self.history()?;
        let mut staged = Vec::new();
        for (index, logical) in files.iter().enumerate() {
            let input = confined_file(&source, logical)?;
            let snapshot = stage.path().join(index.to_string());
            at(&snapshot, fs::copy(&input, &snapshot))?;
            #[cfg(windows)]
            {
                // CopyFile preserves the read-only attribute. The owned
                // snapshot must remain writable for FlushFileBuffers and GC.
                let mut permissions = at(&snapshot, fs::metadata(&snapshot))?.permissions();
                permissions.set_readonly(false);
                at(&snapshot, fs::set_permissions(&snapshot, permissions))?;
            }
            let (digest, byte_length) = hash_file(&snapshot)?;
            let object = fingerprinted_path(logical, &digest);
            reserve(&mut history, &object, &digest)?;
            // A new logical alias cannot take over an earlier immutable URL.
            if let Some(previous) = history.assignments.get(logical) {
                if previous != &digest {
                    return Err(collision(logical, previous, &digest));
                }
            }
            let cache_policy = cache_policy(logical);
            let public_path = match cache_policy {
                CachePolicy::Immutable => object.clone(),
                CachePolicy::Revalidate => logical.clone(),
            };
            manifest.assets.insert(
                logical.clone(),
                AssetEntry {
                    object: object.clone(),
                    public_path,
                    sha256: digest.clone(),
                    byte_length,
                    content_type: mime_guess::from_path(logical)
                        .first_or_octet_stream()
                        .essence_str()
                        .into(),
                    cache_policy,
                },
            );
            staged.push((snapshot, object, digest, byte_length));
        }
        manifest.validate()?;
        // Checking bytes, rather than only metadata, also handles editor writes
        // that preserve size/mtime. A failure leaves the old manifest in place.
        if collect_files(&source)? != files {
            return Err(AssetError::SourceChanged(source.display().to_string()));
        }
        for (logical, entry) in &manifest.assets {
            if hash_file(&confined_file(&source, logical)?)?
                != (entry.sha256.clone(), entry.byte_length)
            {
                return Err(AssetError::SourceChanged(logical.clone()));
            }
        }
        let objects = self.root.join("objects");
        let mut object_directories = BTreeSet::new();
        for (snapshot, object, digest, byte_length) in staged {
            let parent = object.rsplit_once('/').map_or("", |(parent, _)| parent);
            ensure_directory(&objects, parent)?;
            let destination = objects.join(&object);
            match fs::symlink_metadata(&destination) {
                Ok(metadata) => {
                    if !metadata.file_type().is_file() {
                        return Err(AssetError::InvalidPath(object));
                    }
                    if hash_file(&destination)? != (digest, byte_length) {
                        return Err(AssetError::Integrity(object));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    // Windows flushing requires GENERIC_WRITE. Unix permits
                    // fsync on a read handle, including read-only source copies.
                    let file = at(
                        &snapshot,
                        fs::OpenOptions::new()
                            .read(true)
                            .write(cfg!(windows))
                            .open(&snapshot),
                    )?;
                    at(&snapshot, file.sync_all())?;
                    drop(file);
                    at(&destination, fs::rename(&snapshot, &destination))?;
                }
                Err(source) => {
                    return Err(AssetError::Io {
                        path: destination,
                        source,
                    })
                }
            }
            // Include existing objects too: an earlier failed attempt may
            // have moved them without completing its directory flushes.
            for directory in destination
                .parent()
                .expect("object parent")
                .ancestors()
                .take_while(|directory| directory.starts_with(&objects))
            {
                object_directories.insert(directory.to_path_buf());
            }
        }
        // Reverse path order puts every child before its parent. File data was
        // already flushed above; persist the tree before publishing metadata.
        // Each directory is flushed once, regardless of its number of files.
        for directory in object_directories.iter().rev() {
            sync_directory(directory)?;
        }
        // Reservations precede the public pointer. Extra reservations after an
        // interrupted publish are harmless; forgotten reservations are not.
        write_json(&self.root, stage.path(), HISTORY_FILE, &history)?;
        sync_directory(&self.root)?;
        write_json(&self.root, stage.path(), MANIFEST_FILE, &manifest)?;
        let durability_error = sync_directory(&self.root).err();
        drop(stage);
        let cleanup = self.cleanup();
        Ok(PublishReport {
            output_directory: self.root.clone(),
            manifest,
            cleanup,
            durability_error,
        })
    }

    /// Remove stale local objects only when no server/catalog is reading them.
    /// Call again after the dev supervisor has stopped the old child.
    pub fn cleanup(&mut self) -> Result<CleanupReport> {
        let lock_path = self.root.join(READERS_FILE);
        let lock = open_lock(&lock_path)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Ok(CleanupReport {
                    live_readers: true,
                    ..Default::default()
                })
            }
            Err(TryLockError::Error(source)) => {
                return Err(AssetError::Io {
                    path: lock_path,
                    source,
                })
            }
        }
        let manifest_path = self.root.join(MANIFEST_FILE);
        if !at(&manifest_path, manifest_path.try_exists())? {
            return Ok(CleanupReport::default());
        }
        plain_file(&manifest_path)?;
        let manifest: AssetManifest =
            serde_json::from_slice(&at(&manifest_path, fs::read(&manifest_path))?)?;
        manifest.validate()?;
        let keep: BTreeSet<_> = manifest
            .assets
            .values()
            .map(|entry| entry.object.as_str())
            .collect();
        let objects = self.root.join("objects");
        // Refuse to prune around a damaged active generation.
        for entry in manifest.assets.values() {
            let path = confined_file(&objects, &entry.object)?;
            if hash_file(&path)? != (entry.sha256.clone(), entry.byte_length) {
                return Err(AssetError::Integrity(entry.object.clone()));
            }
        }
        let mut removed_objects = 0;
        for path in collect_files(&objects)? {
            if !keep.contains(path.as_str()) {
                let path = confined_file(&objects, &path)?;
                at(&path, fs::remove_file(&path))?;
                removed_objects += 1;
            }
        }
        remove_empty_directories(&objects)?;
        Ok(CleanupReport {
            removed_objects,
            live_readers: false,
        })
    }

    fn history(&self) -> Result<History> {
        let path = self.root.join(HISTORY_FILE);
        if !at(&path, path.try_exists())? {
            // Missing history on an established output is corruption, not
            // permission to reuse previously published short URLs.
            if self.root.join(MANIFEST_FILE).exists() {
                return Err(AssetError::InvalidManifest(
                    "missing fingerprint history".into(),
                ));
            }
            return Ok(History {
                version: MANIFEST_VERSION,
                assignments: BTreeMap::new(),
            });
        }
        plain_file(&path)?;
        let history: History = serde_json::from_slice(&at(&path, fs::read(&path))?)?;
        if history.version != MANIFEST_VERSION {
            return Err(AssetError::InvalidManifest(
                "unsupported fingerprint history version".into(),
            ));
        }
        for (path, digest) in &history.assignments {
            validate_relative(path)?;
            validate_digest(digest)?;
        }
        Ok(history)
    }

    fn clean_staging(&self) -> Result<()> {
        let path = self.root.join("staging");
        for item in at(&path, fs::read_dir(&path))? {
            let item = at(&path, item)?;
            let item_path = item.path();
            if !at(&item_path, item.file_type())?.is_dir()
                || !item.file_name().to_string_lossy().starts_with("attempt-")
            {
                return Err(AssetError::InvalidPath(item_path.display().to_string()));
            }
            at(&item_path, fs::remove_dir_all(&item_path))?;
        }
        Ok(())
    }
}

fn reserve(history: &mut History, path: &str, digest: &str) -> Result<()> {
    if let Some(previous) = history.assignments.get(path) {
        if previous != digest {
            return Err(collision(path, previous, digest));
        }
    } else {
        history.assignments.insert(path.into(), digest.into());
    }
    Ok(())
}

fn collision(path: &str, previous: &str, current: &str) -> AssetError {
    AssetError::Collision {
        path: path.into(),
        previous: previous.into(),
        current: current.into(),
    }
}

fn cache_policy(path: &str) -> CachePolicy {
    let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    // These formats can contain dependency URLs. Keep their existing relative
    // addressing until a supported rewrite/bundling pass owns those edges.
    if matches!(
        extension.as_str(),
        "js" | "mjs"
            | "cjs"
            | "css"
            | "html"
            | "htm"
            | "json"
            | "webmanifest"
            | "map"
            | "txt"
            | "xml"
    ) {
        CachePolicy::Revalidate
    } else {
        CachePolicy::Immutable
    }
}

fn open_lock(path: &Path) -> Result<File> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(AssetError::InvalidPath(path.display().to_string()))
        }
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(AssetError::Io {
                path: path.into(),
                source: error,
            })
        }
        _ => {}
    }
    at(
        path,
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path),
    )
}

fn ensure_directory(root: &Path, relative: &str) -> Result<()> {
    let mut path = root.to_path_buf();
    for part in relative.split('/').filter(|part| !part.is_empty()) {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err(AssetError::InvalidPath(path.display().to_string())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                at(&path, fs::create_dir(&path))?;
            }
            Err(source) => return Err(AssetError::Io { path, source }),
        }
    }
    Ok(())
}

fn collect_files(root: &Path) -> Result<Vec<String>> {
    fn walk(root: &Path, directory: &Path, files: &mut Vec<String>) -> Result<()> {
        if !at(directory, fs::symlink_metadata(directory))?
            .file_type()
            .is_dir()
        {
            return Err(AssetError::InvalidPath(directory.display().to_string()));
        }
        for item in at(directory, fs::read_dir(directory))? {
            let item = at(directory, item)?;
            let path = item.path();
            let kind = at(&path, item.file_type())?;
            if kind.is_dir() {
                walk(root, &path, files)?;
            } else if kind.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .expect("walked child")
                    .to_str()
                    .ok_or_else(|| AssetError::InvalidPath(path.display().to_string()))?
                    .replace(std::path::MAIN_SEPARATOR, "/");
                validate_relative(&relative)?;
                files.push(relative);
            } else {
                return Err(AssetError::InvalidPath(path.display().to_string()));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn remove_empty_directories(root: &Path) -> Result<()> {
    for entry in at(root, fs::read_dir(root))? {
        let entry = at(root, entry)?;
        let path = entry.path();
        if at(&path, entry.file_type())?.is_dir() {
            remove_empty_directories(&path)?;
            if at(&path, fs::read_dir(&path))?.next().is_none() {
                at(&path, fs::remove_dir(&path))?;
            }
        }
    }
    Ok(())
}

fn write_json(root: &Path, staging: &Path, name: &str, value: &impl Serialize) -> Result<()> {
    let destination = root.join(name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => plain_file(&destination)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(AssetError::Io {
                path: destination,
                source,
            })
        }
    }
    let mut temporary = at(staging, tempfile::NamedTempFile::new_in(staging))?;
    serde_json::to_writer(temporary.as_file_mut(), value)?;
    at(root, temporary.write_all(b"\n"))?;
    at(root, temporary.as_file().sync_all())?;
    temporary
        .persist(&destination)
        .map_err(|error| AssetError::Io {
            path: destination,
            source: error.error,
        })?;
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    at(path, at(path, File::open(path))?.sync_all())?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
