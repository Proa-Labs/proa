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

impl Drop for OutputStore {
    fn drop(&mut self) {
        // Explicitly unlock the open file description before closing the fd so
        // an in-flight fork() in another thread cannot hold flock() past drop.
        let _ = self._writer.unlock();
    }
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
    /// Whether `open` would take `directory` as an output: it is absent,
    /// already one, or holds nothing but the lock of an interrupted writer.
    /// A symlink is never accepted, so removal cannot reach through one.
    pub fn accepts(directory: &Path) -> bool {
        match fs::symlink_metadata(directory) {
            Err(error) => error.kind() == io::ErrorKind::NotFound,
            Ok(metadata) if !metadata.is_dir() => false,
            Ok(_) => {
                validate_owner(directory).is_ok()
                    || fs::read_dir(directory).is_ok_and(|mut entries| {
                        entries
                            .all(|entry| entry.is_ok_and(|entry| entry.file_name() == WRITER_FILE))
                    })
            }
        }
    }

    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref();
        at(directory, fs::create_dir_all(directory))?;
        let root = at(directory, directory.canonicalize())?;
        set_traversable_directory_permissions(&root)?;
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
            validate_owner(&root)?;
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
        // A marker written under a restrictive umask stays owner-only.
        set_readable_file_permissions(&marker, &at(&marker, File::open(&marker))?)?;
        let readers = open_lock(&root.join(READERS_FILE))?;
        set_readable_file_permissions(&root.join(READERS_FILE), &readers)?;
        let live_readers = open_lock(&root.join(LIVE_READERS_FILE))?;
        set_readable_file_permissions(&root.join(LIVE_READERS_FILE), &live_readers)?;
        ensure_directory(&root, "objects")?;
        ensure_directory(&root, "staging")?;
        let store = Self {
            root,
            _writer: writer,
        };
        store.clean_staging()?;
        Ok(store)
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
        // Older builds reserved private object names too. An object whose
        // extension requires revalidation could never be an immutable URL.
        history
            .assignments
            .retain(|path, _| cache_policy(path) == CachePolicy::Immutable);
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
            // A new logical alias cannot take over an earlier immutable URL.
            if let Some(previous) = history.assignments.get(logical) {
                if previous != &digest {
                    return Err(collision(logical, previous, &digest));
                }
            }
            let cache_policy = cache_policy(logical);
            let public_path = match cache_policy {
                CachePolicy::Immutable => {
                    reserve(&mut history, &object, &digest)?;
                    object.clone()
                }
                CachePolicy::Revalidate => logical.clone(),
            };
            manifest.assets.insert(
                logical.clone(),
                AssetEntry {
                    object,
                    public_path,
                    sha256: digest,
                    byte_length,
                    content_type: mime_guess::from_path(logical)
                        .first_or_octet_stream()
                        .essence_str()
                        .into(),
                    cache_policy,
                },
            );
        }
        manifest.validate()?;
        // Checking bytes, rather than only metadata, also handles editor writes
        // that preserve size/mtime. A failure leaves the old manifest in place.
        if collect_files(&source)? != files {
            return Err(AssetError::SourceChanged(source.display().to_string()));
        }
        for (logical, entry) in &manifest.assets {
            if !entry.matches_contents(&confined_file(&source, logical)?)? {
                return Err(AssetError::SourceChanged(logical.clone()));
            }
        }
        let objects = self.root.join("objects");
        let mut object_directories = BTreeSet::new();
        for (index, logical) in files.iter().enumerate() {
            let entry = &manifest.assets[logical];
            let object = &entry.object;
            let parent = object.rsplit_once('/').map_or("", |(parent, _)| parent);
            ensure_directory(&objects, parent)?;
            let destination = objects.join(object);
            match fs::symlink_metadata(&destination) {
                Ok(metadata) => {
                    if !metadata.file_type().is_file() {
                        return Err(AssetError::InvalidPath(object.clone()));
                    }
                    if !entry.matches_contents(&destination)? {
                        return Err(AssetError::Integrity(object.clone()));
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if metadata.permissions().mode() & 0o444 != 0o444 {
                            let file = at(&destination, File::open(&destination))?;
                            set_readable_file_permissions(&destination, &file)?;
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let snapshot = stage.path().join(index.to_string());
                    // Windows flushing requires GENERIC_WRITE. Unix permits
                    // fsync on a read handle, including read-only source copies.
                    let file = at(
                        &snapshot,
                        fs::OpenOptions::new()
                            .read(true)
                            .write(cfg!(windows))
                            .open(&snapshot),
                    )?;
                    set_readable_file_permissions(&snapshot, &file)?;
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

    /// Whether a running server registered a [`LiveReader`] for this output.
    ///
    /// `true` means the server picks up the manifest just published without a
    /// restart. `false` covers every other reader: none at all, one that
    /// loaded a fixed catalog, and one that has not finished starting. Each of
    /// those needs a restart to see new files.
    pub fn has_live_reader(&self) -> Result<bool> {
        let lock_path = self.root.join(LIVE_READERS_FILE);
        let lock = open_lock(&lock_path)?;
        match lock.try_lock() {
            Ok(()) => {
                let _ = lock.unlock();
                Ok(false)
            }
            Err(TryLockError::WouldBlock) => Ok(true),
            Err(TryLockError::Error(source)) => Err(AssetError::Io {
                path: lock_path,
                source,
            }),
        }
    }

    /// Delete the whole output unless a catalog is reading it, and report
    /// whether it went. A server resolves its files by path on every request,
    /// so removing them under a live reader turns its routes into 404s.
    pub fn remove(self) -> Result<bool> {
        let lock_path = self.root.join(READERS_FILE);
        let readers = open_lock(&lock_path)?;
        match readers.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(source)) => {
                return Err(AssetError::Io {
                    path: lock_path,
                    source,
                })
            }
        }
        let root = self.root.clone();
        // Retire the pointer while readers are still locked out: a catalog
        // loads only by reading it under the shared lock, so none can come up
        // on files that are about to go.
        let manifest = root.join(MANIFEST_FILE);
        match fs::remove_file(&manifest) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(AssetError::Io {
                    path: manifest,
                    source,
                })
            }
        }
        // Unix deletes with both locks held, so no other writer can begin
        // publishing into the directory while it goes. Windows cannot delete
        // an open file and has to release them first.
        #[cfg(unix)]
        at(&root, fs::remove_dir_all(&root))?;
        let _ = readers.unlock();
        drop(readers);
        drop(self);
        #[cfg(not(unix))]
        at(&root, fs::remove_dir_all(&root))?;
        Ok(true)
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
        let (manifest, _) = AssetManifest::read(&manifest_path)?;
        let keep: BTreeSet<_> = manifest
            .assets
            .values()
            .map(|entry| entry.object.as_str())
            .collect();
        let objects = self.root.join("objects");
        // Refuse to prune around an incomplete active generation. Publication
        // and catalog loading verify bytes; pruning needs only the live set,
        // so it does not reread the whole public tree on every dev restart.
        for entry in manifest.assets.values() {
            let path = confined_file(&objects, &entry.object)?;
            if at(&path, fs::symlink_metadata(&path))?.len() != entry.byte_length {
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
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
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
        set_traversable_directory_permissions(&path)?;
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
    set_readable_file_permissions(&destination, temporary.as_file())?;
    at(root, temporary.as_file().sync_all())?;
    temporary
        .persist(&destination)
        .map_err(|error| AssetError::Io {
            path: destination,
            source: error.error,
        })?;
    Ok(())
}

/// Ensure published catalog files are world-readable (`0o644`) on Unix.
/// `NamedTempFile` defaults to `0o600`, which would block non-root container
/// runtime users from reading `manifest.json` after a multi-stage Docker build.
fn set_readable_file_permissions(path: &Path, file: &File) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Leave a file that is already readable alone: changing its mode
        // needs ownership, and a shared catalog may be group-writable.
        let mode = at(path, file.metadata())?.permissions().mode();
        if mode & 0o644 != 0o644 {
            at(
                path,
                file.set_permissions(fs::Permissions::from_mode(mode | 0o644)),
            )?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, file);
    }
    Ok(())
}

/// Published directories are at least `0o755` on Unix whatever the build's umask, so
/// the readable files inside them can also be reached by another user.
fn set_traversable_directory_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = at(path, fs::metadata(path))?.permissions().mode();
        if mode & 0o755 != 0o755 {
            at(
                path,
                fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o755)),
            )?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
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
