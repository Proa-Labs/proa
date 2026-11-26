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

/// A reference in a public file that publication could not point at its
/// published target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Logical path of the file that holds the reference.
    pub file: String,
    pub problem: Problem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// A relative URL that names no public file.
    Missing(String),
    /// A URL from the origin root, such as `url(/images/a.png)`. It is not a
    /// path into the catalog, and it breaks under a base path or once the
    /// file is served from another host.
    RootRelative(String),
    /// The stylesheet could not be read as CSS, so nothing in it was changed.
    Unreadable,
    /// A stylesheet reference that leads back to the stylesheet itself,
    /// directly or through the stylesheets it refers to. Its target's name
    /// depends on this file's bytes, so it cannot be written into them.
    Cycle(String),
    /// A relative reference from a file that keeps its name to a file that is
    /// published under a content-hashed one. Nothing is served at the path
    /// the reference names.
    Hashed(String),
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file = &self.file;
        match &self.problem {
            Problem::Missing(url) => write!(
                f,
                "`{file}` refers to `{url}`, which is not a file in the public directory"
            ),
            Problem::RootRelative(url) => write!(
                f,
                "`{file}` refers to `{url}` from the origin root; use a path relative to the stylesheet so it follows the published file"
            ),
            Problem::Unreadable => write!(
                f,
                "`{file}` could not be read as CSS, so its references were not rewritten"
            ),
            Problem::Cycle(url) => write!(
                f,
                "`{file}` refers to `{url}`, which refers back to it; that reference was left as written and will not load"
            ),
            Problem::Hashed(url) => write!(
                f,
                "`{file}` refers to `{url}`, which is published under a content-hashed name, so nothing is served at that path; render the file's URL into the page with `cx.url(\"public/...\")` and read it from there"
            ),
        }
    }
}

/// A verified copy of a public directory: file `n` of `files` is at
/// `<stage>/<n>`, holding the bytes `manifest` describes.
struct Snapshot {
    files: Vec<String>,
    manifest: AssetManifest,
    diagnostics: Vec<Diagnostic>,
}

/// Copy `source` into `stage`, settle every file's published name, and
/// confirm the source did not change underneath the copy.
fn snapshot(source: &Path, stage: &Path, history: &mut History) -> Result<Snapshot> {
    let files = collect_files(source)?;
    let reserved = PUBLIC_ROUTE.trim_start_matches('/');
    if let Some(file) = files
        .iter()
        .find(|file| file.split('/').next() == Some(reserved))
    {
        // A bundle keeps every published file under `public/_proa/`.
        return Err(AssetError::ReservedPath(file.clone()));
    }
    let mut sources = Vec::with_capacity(files.len());
    for (index, logical) in files.iter().enumerate() {
        let input = confined_file(source, logical)?;
        let snapshot = stage.join(index.to_string());
        at(&snapshot, fs::copy(&input, &snapshot))?;
        // The owned snapshot must be writable: a stylesheet is rewritten
        // in place, and Windows also needs it for FlushFileBuffers and GC.
        // A copy carries a read-only source's mode with it.
        make_owner_writable(&snapshot)?;
        sources.push(hash_file(&snapshot)?);
    }
    let mut pass = Pass {
        files: &files,
        stage,
        history,
        entries: vec![None; files.len()],
        visiting: vec![false; files.len()],
        diagnostics: Vec::new(),
    };
    for index in 0..files.len() {
        pass.entry(index)?;
    }
    let mut diagnostics = std::mem::take(&mut pass.diagnostics);
    let mut manifest = AssetManifest::default();
    for (logical, entry) in files.iter().zip(pass.entries) {
        manifest
            .assets
            .insert(logical.clone(), entry.expect("every file was published"));
    }
    for (index, logical) in files.iter().enumerate() {
        if refers_by_path(logical) {
            let staged = stage.join(index.to_string());
            diagnostics.extend(hashed_references(logical, &staged, &manifest)?);
        }
    }
    for path in AssetManifest::default_root_files() {
        if manifest.assets.contains_key(path) {
            manifest.root.insert(path.into(), path.into());
        }
    }
    manifest.validate()?;
    // Checking bytes, rather than only metadata, also handles editor writes
    // that preserve size/mtime. A failure leaves the old manifest in place.
    if collect_files(source)? != files {
        return Err(AssetError::SourceChanged(source.display().to_string()));
    }
    for (logical, snapshot) in files.iter().zip(&sources) {
        let current = hash_file(&confined_file(source, logical)?)?;
        if current.sha256 != snapshot.sha256 || current.byte_length != snapshot.byte_length {
            return Err(AssetError::SourceChanged(logical.clone()));
        }
    }
    Ok(Snapshot {
        files,
        manifest,
        diagnostics,
    })
}

/// One publication's walk over the snapshot. A stylesheet's bytes hold the
/// published names of the files it refers to, so those are settled first.
struct Pass<'a> {
    files: &'a [String],
    stage: &'a Path,
    history: &'a mut History,
    entries: Vec<Option<AssetEntry>>,
    visiting: Vec<bool>,
    diagnostics: Vec<Diagnostic>,
}

impl Pass<'_> {
    fn entry(&mut self, index: usize) -> Result<AssetEntry> {
        if let Some(entry) = &self.entries[index] {
            return Ok(entry.clone());
        }
        let logical = &self.files[index];
        // `rewrite_stylesheet` never follows a reference into a file that is
        // still being settled, so a file is entered once.
        debug_assert!(!self.visiting[index], "reference cycle through `{logical}`");
        self.visiting[index] = true;
        let snapshot = self.stage.join(index.to_string());
        if is_stylesheet(logical) {
            self.rewrite_stylesheet(index, &snapshot)?;
        }
        let hashed = hash_file(&snapshot)?;
        let object = fingerprinted_path(logical, &hashed.sha256);
        let cache_policy = cache_policy(logical);
        let public_path = match cache_policy {
            CachePolicy::Immutable => {
                reserve(self.history, &object, &hashed.sha256)?;
                object.clone()
            }
            CachePolicy::Revalidate => logical.clone(),
        };
        let entry = AssetEntry {
            object,
            public_path,
            sha256: hashed.sha256,
            integrity: hashed.integrity,
            byte_length: hashed.byte_length,
            content_type: mime_guess::from_path(logical)
                .first_or_octet_stream()
                .essence_str()
                .into(),
            cache_policy,
        };
        self.visiting[index] = false;
        self.entries[index] = Some(entry.clone());
        Ok(entry)
    }

    fn rewrite_stylesheet(&mut self, index: usize, snapshot: &Path) -> Result<()> {
        let logical = &self.files[index];
        let parsed = at(snapshot, fs::read(snapshot)).map(|bytes| {
            String::from_utf8(bytes)
                .ok()
                .and_then(|text| Some((stylesheet::references(&text).ok()?, text)))
        })?;
        let Some((references, text)) = parsed else {
            self.diagnostics.push(Diagnostic {
                file: logical.clone(),
                problem: Problem::Unreadable,
            });
            return Ok(());
        };
        // The stylesheet keeps its directory when it is renamed, so its own
        // published location is known before its bytes are.
        let from = format!("{}/{logical}", CachePolicy::Immutable.directory());
        let mut urls = Vec::with_capacity(references.len());
        for reference in &references {
            let (problem, url) = match stylesheet::target(logical, &reference.url) {
                stylesheet::Target::External => (None, None),
                stylesheet::Target::RootRelative => {
                    (Some(Problem::RootRelative(reference.url.clone())), None)
                }
                stylesheet::Target::Outside => {
                    (Some(Problem::Missing(reference.url.clone())), None)
                }
                stylesheet::Target::Public {
                    logical: target,
                    suffix,
                } => match self.files.binary_search(&target) {
                    // The target's name is settled only after this file's
                    // bytes are, so the reference cannot hold it. Browsers
                    // ignore an import cycle; this one is left as written.
                    Ok(target) if self.visiting[target] => {
                        (Some(Problem::Cycle(reference.url.clone())), None)
                    }
                    Ok(target) => {
                        let entry = self.entry(target)?;
                        let path = stylesheet::relative(&from, &entry.url_path());
                        (None, Some(format!("{path}{suffix}")))
                    }
                    Err(_) => (Some(Problem::Missing(reference.url.clone())), None),
                },
            };
            if let Some(problem) = problem {
                self.diagnostics.push(Diagnostic {
                    file: logical.clone(),
                    problem,
                });
            }
            urls.push(url);
        }
        let mut urls = urls.into_iter();
        let rewritten = stylesheet::rewrite(&text, &references, |_| urls.next().flatten());
        if rewritten != text {
            at(snapshot, fs::write(snapshot, rewritten))?;
        }
        Ok(())
    }
}

/// Formats that keep their name and commonly hold paths to other files:
/// scripts, pages, and manifests. Nothing rewrites them, so a relative path
/// in one still means the logical location.
fn refers_by_path(logical: &str) -> bool {
    matches!(
        extension(logical).as_str(),
        "js" | "mjs" | "cjs" | "html" | "htm" | "json" | "webmanifest" | "xml"
    )
}

/// Files above this size are not searched for references. They are data or
/// bundles, and the search is only there to explain a broken link.
const REFERENCE_SCAN_LIMIT: u64 = 4 * 1024 * 1024;

/// Quoted relative paths in `logical` that name a file published under a
/// hashed name. Before files were hashed in place such a path worked; now it
/// is a 404, and saying so at build time is the only place it can be said.
fn hashed_references(
    logical: &str,
    staged: &Path,
    manifest: &AssetManifest,
) -> Result<Vec<Diagnostic>> {
    if at(staged, fs::metadata(staged))?.len() > REFERENCE_SCAN_LIMIT {
        return Ok(Vec::new());
    }
    let Ok(text) = String::from_utf8(at(staged, fs::read(staged))?) else {
        return Ok(Vec::new());
    };
    let mut found = BTreeSet::new();
    for quoted in stylesheet::quoted(&text) {
        let stylesheet::Target::Public {
            logical: target, ..
        } = stylesheet::target(logical, quoted)
        else {
            continue;
        };
        let hashed = manifest
            .assets
            .get(&target)
            .is_some_and(|entry| entry.cache_policy == CachePolicy::Immutable);
        if hashed && target != logical {
            found.insert(quoted.to_owned());
        }
    }
    Ok(found
        .into_iter()
        .map(|url| Diagnostic {
            file: logical.to_owned(),
            problem: Problem::Hashed(url),
        })
        .collect())
}

fn is_stylesheet(logical: &str) -> bool {
    extension(logical) == "css"
}

#[derive(Debug)]
pub struct PublishReport {
    pub output_directory: PathBuf,
    pub manifest: AssetManifest,
    /// Stylesheet references that were left as written. The catalog is
    /// published either way; these say what will not load in a browser.
    pub diagnostics: Vec<Diagnostic>,
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

/// An output claimed for deletion: an exclusive reader lock, and for a store
/// its writer lock too. See [`OutputStore::reserve_removal`] and
/// [`reserve_bundle_removal`].
pub struct PendingRemoval {
    root: PathBuf,
    /// Held so no other writer can begin publishing into a store.
    _store: Option<OutputStore>,
    readers: File,
}

impl Drop for PendingRemoval {
    fn drop(&mut self) {
        let _ = self.readers.unlock();
    }
}

impl PendingRemoval {
    pub fn commit(self) -> Result<()> {
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
        // Unix deletes with the locks held, so nothing can begin using the
        // directory while it goes. Windows cannot delete an open file and
        // has to release them first.
        #[cfg(unix)]
        at(&root, fs::remove_dir_all(&root))?;
        drop(self);
        #[cfg(not(unix))]
        at(&root, fs::remove_dir_all(&root))?;
        Ok(())
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
        // Only now is the directory known to be an output, so only now may
        // its mode change. A marker written under a restrictive umask stays
        // owner-only.
        set_traversable_directory_permissions(&root)?;
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
        let staging_root = self.root.join("staging");
        let stage = at(
            &staging_root,
            tempfile::Builder::new()
                .prefix("attempt-")
                .tempdir_in(&staging_root),
        )?;
        let mut history = self.history()?;
        let Snapshot {
            files,
            manifest,
            diagnostics,
        } = snapshot(&source, stage.path(), &mut history)?;
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
            diagnostics,
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
        match self.reserve_removal()? {
            Some(removal) => removal.commit().map(|()| true),
            None => Ok(false),
        }
    }

    /// Claim the output for deletion without deleting it yet, or return
    /// `None` while a catalog is reading it. The claim keeps writers and new
    /// readers out, so the caller can finish the work that makes the output
    /// stale and only then commit. Dropping the claim leaves it untouched.
    pub fn reserve_removal(self) -> Result<Option<PendingRemoval>> {
        let lock_path = self.root.join(READERS_FILE);
        let readers = open_lock(&lock_path)?;
        match readers.try_lock() {
            Ok(()) => Ok(Some(PendingRemoval {
                root: self.root.clone(),
                _store: Some(self),
                readers,
            })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(source)) => Err(AssetError::Io {
                path: lock_path,
                source,
            }),
        }
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
            // An output written by another release named its files under a
            // different scheme. None of those names can be handed out again,
            // so there is nothing to carry over.
            return Ok(History {
                version: MANIFEST_VERSION,
                assignments: BTreeMap::new(),
            });
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

fn extension(path: &str) -> String {
    path.rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

fn cache_policy(path: &str) -> CachePolicy {
    // These formats can refer to other files by URL, and nothing rewrites
    // those references yet, so they keep their names and their relative
    // addressing to each other. Stylesheets are rewritten, and are hashed.
    if matches!(
        extension(path).as_str(),
        "js" | "mjs" | "cjs" | "html" | "htm" | "json" | "webmanifest" | "map" | "txt" | "xml"
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
            widened(
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
            widened(
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

/// Only a file's owner may change its mode. A catalog shared with another
/// user was already usable by this one, so that refusal is not a failure.
#[cfg(unix)]
fn widened(path: &Path, result: io::Result<()>) -> Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Ok(()),
        result => at(path, result),
    }
}

fn make_owner_writable(path: &Path) -> Result<()> {
    let mut permissions = at(path, fs::metadata(path))?.permissions();
    if permissions.readonly() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(permissions.mode() | 0o200);
        }
        #[cfg(not(unix))]
        #[allow(
            clippy::permissions_set_readonly_false,
            reason = "only Unix widens write permissions for this call; the Unix branch sets the owner mode"
        )]
        permissions.set_readonly(false);
        at(path, fs::set_permissions(path, permissions))?;
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

#[path = "export.rs"]
mod export;
pub use export::{
    export, export_with_framework_source, remove_bundle, reserve_bundle_removal, Export,
    FrameworkFile,
};

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
