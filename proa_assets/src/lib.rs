// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A public-file catalog shared by the build engine and native server adapters.
//!
//! Rust keeps logical paths. The build copies and hashes files, then atomically
//! publishes a manifest. Loading a catalog verifies its files once and protects
//! them from local cleanup for the lifetime of the catalog. This crate has no
//! dependency on the compiler, renderer, HTTP framework, or JavaScript storage.

#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

use base64::prelude::{Engine as _, BASE64_STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha384};
use thiserror::Error;

#[cfg(feature = "build")]
pub mod build;
#[cfg(feature = "build")]
mod stylesheet;

pub const MANIFEST_VERSION: u32 = 2;
pub const MANIFEST_FILE: &str = "manifest.json";
/// Runtime-only input: never pass this variable to rustc or build scripts.
pub const OUTPUT_ENV: &str = "PROA_PUBLIC_ASSETS";
/// Every catalog URL starts here. The path below it is the file's place in
/// an exported bundle, so a bucket holding that tree serves the same URLs.
pub const PUBLIC_ROUTE: &str = "/_proa";
/// Content-hashed files: a name is never reused for different bytes.
pub const IMMUTABLE_DIRECTORY: &str = "immutable";
/// Files that keep their logical name and are revalidated on every load.
pub const MUTABLE_DIRECTORY: &str = "mutable";
/// Hex characters of the SHA-256 carried in an immutable file name. A bucket
/// that keeps every deploy's files forever needs more margin than a local
/// output does, and 64 bits leaves collisions out of reach.
pub const FINGERPRINT_LENGTH: usize = 16;
/// Appended to an exported executable's file name to name its catalog.
pub const BUNDLED_SUFFIX: &str = ".assets";
/// The directory of a bundle that mirrors browser URLs: the file served at
/// `/_proa/immutable/a.<hash>.png` is `public/_proa/immutable/a.<hash>.png`.
pub const BUNDLE_PUBLIC_DIRECTORY: &str = "public";
const OWNER_FILE: &str = ".proa-assets";
const OWNER_BYTES: &[u8] = b"proa-public-assets-v1\n";
const READERS_FILE: &str = ".readers.lock";
const LIVE_READERS_FILE: &str = ".live-readers.lock";

pub type Result<T> = std::result::Result<T, AssetError>;

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("asset I/O at `{path}`: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("invalid asset manifest: {0}")]
    InvalidManifest(String),
    #[error("invalid public asset path `{0}`")]
    InvalidPath(String),
    #[error("asset output `{0}` is not owned by Proa")]
    NotOwned(PathBuf),
    #[error("public asset input and output directories overlap")]
    OverlappingDirectories,
    #[error("another writer is publishing asset output `{0}`")]
    WriterBusy(PathBuf),
    #[error("public file `{0}` is under `_proa/`, which holds the published files")]
    ReservedPath(String),
    #[error("asset output `{0}` is being read by a running server")]
    InUse(PathBuf),
    #[error("stylesheet `{0}` imports itself through its own imports")]
    ImportCycle(String),
    #[error("asset `{0}` changed during publication; retry the build")]
    SourceChanged(String),
    #[error("asset fingerprint collision at `{path}`: {previous} versus {current}")]
    Collision {
        path: String,
        previous: String,
        current: String,
    },
    #[error("asset `{0}` does not match its recorded SHA-256 and length")]
    Integrity(String),
    #[error("asset JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CachePolicy {
    Immutable,
    Revalidate,
}

impl CachePolicy {
    /// The directory under [`PUBLIC_ROUTE`] that holds files of this policy.
    /// One path rule per directory is the whole cache configuration of a CDN.
    pub const fn directory(self) -> &'static str {
        match self {
            Self::Immutable => IMMUTABLE_DIRECTORY,
            Self::Revalidate => MUTABLE_DIRECTORY,
        }
    }

    pub const fn cache_control(self) -> &'static str {
        match self {
            Self::Immutable => "public, max-age=31536000, immutable",
            Self::Revalidate => "public, no-cache",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetEntry {
    /// Path relative to `objects/`, always tied to the delivered bytes.
    pub object: String,
    /// Browser path relative to the directory of its cache policy.
    pub public_path: String,
    /// SHA-256 of the delivered bytes. For a stylesheet these are the bytes
    /// after its references were pointed at their published targets.
    pub sha256: String,
    /// Subresource Integrity value for the delivered bytes: `sha384-<base64>`.
    pub integrity: String,
    pub byte_length: u64,
    pub content_type: String,
    pub cache_policy: CachePolicy,
}

impl AssetEntry {
    /// Path below [`PUBLIC_ROUTE`]: `immutable/images/logo.<hash>.svg` or
    /// `mutable/data.json`. This is both the URL and the bundle location.
    pub fn url_path(&self) -> String {
        format!("{}/{}", self.cache_policy.directory(), self.public_path)
    }

    fn matches_contents(&self, path: &Path) -> Result<bool> {
        let hashed = hash_file(path)?;
        // The integrity value is what a page hands the browser, so a file
        // that does not match it would load from disk and then be refused.
        Ok(hashed.sha256 == self.sha256
            && hashed.integrity == self.integrity
            && hashed.byte_length == self.byte_length)
    }
}

/// How an output directory arranges the files its manifest lists.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// The local output `proa dev`, `proa run`, and `proa test` republish
    /// into: every file under `objects/`, beside the locks and staging area
    /// that let a writer replace the manifest under a running server.
    #[default]
    Store,
    /// What `proa build --artifact-out` writes: every file at
    /// `public/<its URL path>`, and nothing else. It is written once and
    /// only read afterwards, and `public/` can be handed to a static host.
    Bundle,
}

impl Layout {
    fn is_store(&self) -> bool {
        *self == Self::Store
    }
}

/// A file the framework itself serves, exported from the built executable.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrameworkEntry {
    /// Path below [`PUBLIC_ROUTE`], such as `immutable/rsjs/runtime.<hash>.js`.
    pub path: String,
    pub sha256: String,
    /// Subresource Integrity value: `sha384-<base64>`.
    pub integrity: String,
    pub byte_length: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetManifest {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Layout::is_store")]
    pub layout: Layout,
    /// Keys are package-public logical paths, such as `images/logo.svg`.
    pub assets: BTreeMap<String, AssetEntry>,
    /// Files a browser asks for at a fixed path from the origin root, such as
    /// `favicon.ico`, by the logical path that holds their bytes. They are
    /// served there in addition to their catalog URL.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub root: BTreeMap<String, String>,
    /// The framework's own modules, by the canonical name the framework
    /// knows each one by. Only a bundle holds them: they are exported from
    /// the executable the bundle sits beside.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub framework: BTreeMap<String, FrameworkEntry>,
    /// Identifies the linked sources `framework` was made from. An
    /// executable uses these files only when it reports the same value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework_source: Option<String>,
}

impl Default for AssetManifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            layout: Layout::Store,
            assets: BTreeMap::new(),
            root: BTreeMap::new(),
            framework: BTreeMap::new(),
            framework_source: None,
        }
    }
}

impl AssetManifest {
    fn read(path: &Path) -> Result<(Self, Vec<u8>)> {
        plain_file(path)?;
        let bytes = at(path, fs::read(path))?;
        let manifest: Self = serde_json::from_slice(&bytes)?;
        manifest.validate()?;
        Ok((manifest, bytes))
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != MANIFEST_VERSION {
            return Err(AssetError::InvalidManifest(format!(
                "unsupported schema version {}",
                self.version
            )));
        }
        let mut urls = BTreeMap::<String, &str>::new();
        for (logical, entry) in &self.assets {
            validate_relative(logical)?;
            validate_relative(&entry.object)?;
            validate_relative(&entry.public_path)?;
            validate_digest(&entry.sha256)?;
            validate_integrity(&entry.integrity)?;
            if entry.object != fingerprinted_path(logical, &entry.sha256) {
                return Err(AssetError::InvalidManifest(format!(
                    "object name does not match digest for `{logical}`"
                )));
            }
            let expected = match entry.cache_policy {
                CachePolicy::Immutable => &entry.object,
                CachePolicy::Revalidate => logical,
            };
            if &entry.public_path != expected {
                return Err(AssetError::InvalidManifest(format!(
                    "public path does not match cache policy for `{logical}`"
                )));
            }
            if entry.content_type.is_empty()
                || entry.content_type.bytes().any(|b| b.is_ascii_control())
            {
                return Err(AssetError::InvalidManifest(format!(
                    "invalid media type for `{logical}`"
                )));
            }
            // A file has exactly one URL. Two logical paths may share it only
            // when their bytes are the same.
            let path = entry.url_path();
            if let Some(previous) = urls.insert(path.clone(), &entry.sha256) {
                if previous != entry.sha256 {
                    return Err(AssetError::Collision {
                        path,
                        previous: previous.into(),
                        current: entry.sha256.clone(),
                    });
                }
            }
        }
        for (path, logical) in &self.root {
            validate_relative(path)?;
            // A root file may not take a path the catalog itself answers.
            let reserved = PUBLIC_ROUTE.trim_start_matches('/');
            if path.split('/').next() == Some(reserved) || !self.assets.contains_key(logical) {
                return Err(AssetError::InvalidManifest(format!(
                    "root file `{path}` does not name a public file"
                )));
            }
        }
        for (name, entry) in &self.framework {
            validate_relative(name)?;
            validate_relative(&entry.path)?;
            validate_digest(&entry.sha256)?;
            validate_integrity(&entry.integrity)?;
            let immutable = entry
                .path
                .strip_prefix(IMMUTABLE_DIRECTORY)
                .is_some_and(|rest| rest.starts_with('/'));
            if !immutable {
                return Err(AssetError::InvalidManifest(format!(
                    "framework file `{name}` is not content-hashed"
                )));
            }
            if let Some(previous) = urls.insert(entry.path.clone(), &entry.sha256) {
                if previous != entry.sha256 {
                    return Err(AssetError::Collision {
                        path: entry.path.clone(),
                        previous: previous.into(),
                        current: entry.sha256.clone(),
                    });
                }
            }
        }
        if self.layout == Layout::Store && !self.framework.is_empty() {
            return Err(AssetError::InvalidManifest(
                "only a bundle holds framework files".into(),
            ));
        }
        Ok(())
    }

    /// The root paths this public directory fills without being asked:
    /// the icons browsers request on their own.
    pub fn default_root_files() -> [&'static str; 3] {
        [
            "favicon.ico",
            "apple-touch-icon.png",
            "apple-touch-icon-precomposed.png",
        ]
    }
}

/// An immutable, verified view that holds a native reader lock.
#[derive(Debug)]
pub struct AssetCatalog {
    output: PathBuf,
    manifest: AssetManifest,
    generation: String,
    _reader: File,
}

impl Drop for AssetCatalog {
    fn drop(&mut self) {
        // Release the shared flock explicitly so a concurrent fork() in the
        // same process cannot retain the reader lock after this catalog drops.
        let _ = self._reader.unlock();
    }
}

/// Where the catalog bundled with `executable` lives: a sibling directory
/// named after the executable, such as `app.assets` beside `app`.
///
/// `proa build` writes the catalog here for every executable it exports, and
/// a server with no [`OUTPUT_ENV`] loads it from here. The pair can be copied
/// or moved together without any configuration.
pub fn bundled_output(executable: &Path) -> Option<PathBuf> {
    let mut name = executable.file_name()?.to_os_string();
    name.push(BUNDLED_SUFFIX);
    Some(executable.with_file_name(name))
}

/// Whether `directory` carries the marker of a Proa public asset output.
pub fn is_output(directory: &Path) -> bool {
    validate_owner(directory).is_ok()
}

impl AssetCatalog {
    pub fn load(output: impl AsRef<Path>) -> Result<Self> {
        let output = output.as_ref();
        let output = at(output, output.canonicalize())?;
        validate_owner(&output)?;
        // A store has a lock file of its own, shared with its writer. A
        // bundle is never written again, so a reader holds the marker
        // itself: that is how an export knows a server is still running on
        // the files it is about to replace.
        let store_lock = output.join(READERS_FILE);
        let reader_path = match fs::symlink_metadata(&store_lock) {
            Ok(_) => store_lock,
            Err(error) if error.kind() == io::ErrorKind::NotFound => output.join(OWNER_FILE),
            Err(source) => {
                return Err(AssetError::Io {
                    path: store_lock,
                    source,
                })
            }
        };
        let reader = lock_current(&reader_path)?;
        // Lock before reading the pointer. Cleanup then cannot remove files
        // referenced by either the old or the new atomically read manifest.
        let (manifest, bytes) = AssetManifest::read(&output.join(MANIFEST_FILE))?;
        let catalog = Self {
            output,
            manifest,
            generation: sha256(&bytes),
            _reader: reader,
        };
        let mut checked = BTreeSet::new();
        for entry in catalog.manifest.assets.values() {
            let (root, relative) = catalog.location(entry);
            if checked.insert(relative.clone()) {
                let path = confined_file(&root, &relative)?;
                if !entry.matches_contents(&path)? {
                    return Err(AssetError::Integrity(relative));
                }
            }
        }
        for entry in catalog.manifest.framework.values() {
            let path = confined_file(&catalog.public_root(), &entry.path)?;
            let hashed = hash_file(&path)?;
            if hashed.sha256 != entry.sha256
                || hashed.integrity != entry.integrity
                || hashed.byte_length != entry.byte_length
            {
                return Err(AssetError::Integrity(entry.path.clone()));
            }
        }
        Ok(catalog)
    }

    pub fn manifest(&self) -> &AssetManifest {
        &self.manifest
    }
    /// Include this value and delivery settings in caches containing asset URLs.
    pub fn generation(&self) -> &str {
        &self.generation
    }
    /// The file that holds the bytes of a public file.
    pub fn object_path(&self, logical: &str) -> Option<PathBuf> {
        self.manifest.assets.get(logical).map(|entry| {
            let (root, relative) = self.location(entry);
            root.join(relative)
        })
    }
    /// The file that holds one of the framework's own modules.
    pub fn framework_path(&self, name: &str) -> Option<PathBuf> {
        self.manifest
            .framework
            .get(name)
            .map(|entry| self.public_root().join(&entry.path))
    }

    /// Where a bundle keeps the file served at each URL path.
    fn public_root(&self) -> PathBuf {
        self.output
            .join(BUNDLE_PUBLIC_DIRECTORY)
            .join(PUBLIC_ROUTE.trim_start_matches('/'))
    }

    fn location(&self, entry: &AssetEntry) -> (PathBuf, String) {
        match self.manifest.layout {
            Layout::Store => (self.output.join("objects"), entry.object.clone()),
            Layout::Bundle => (self.public_root(), entry.url_path()),
        }
    }
}

/// Proof that a running server follows manifest replacement in place.
///
/// A development server holds this for as long as it reloads the catalog when
/// the manifest changes. The writer asks
/// [`OutputStore::has_live_reader`](build::OutputStore::has_live_reader)
/// before it decides to leave that server running after a publication. A
/// server that loaded a fixed catalog, or one built against a release without
/// in-place reloading, never registers and is restarted instead.
#[derive(Debug)]
pub struct LiveReader {
    lock: File,
}

impl Drop for LiveReader {
    fn drop(&mut self) {
        // Same reasoning as `AssetCatalog`: do not let a concurrent fork()
        // keep the registration alive after this value is gone.
        let _ = self.lock.unlock();
    }
}

impl LiveReader {
    /// Register against an output the writer already prepared. The lock file
    /// is created by the writer; a missing file means the writer predates
    /// in-place reloading and restarts the server itself.
    pub fn register(output: impl AsRef<Path>) -> Result<Self> {
        let output = output.as_ref();
        let output = at(output, output.canonicalize())?;
        validate_owner(&output)?;
        let path = output.join(LIVE_READERS_FILE);
        plain_file(&path)?;
        let lock = at(&path, File::open(&path))?;
        at(&path, lock.lock_shared())?;
        Ok(Self { lock })
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", base16ct::HexDisplay(&Sha256::digest(bytes)))
}

/// The Subresource Integrity value browsers check a fetched file against.
pub fn integrity(bytes: &[u8]) -> String {
    integrity_value(&Sha384::digest(bytes))
}

fn integrity_value(digest: &[u8]) -> String {
    format!("sha384-{}", BASE64_STANDARD.encode(digest))
}

struct Hashed {
    sha256: String,
    integrity: String,
    byte_length: u64,
}

fn hash_file(path: &Path) -> Result<Hashed> {
    let mut file = at(path, File::open(path))?;
    let mut sha256 = Sha256::new();
    let mut sha384 = Sha384::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut byte_length = 0;
    loop {
        let n = at(path, file.read(&mut buffer))?;
        if n == 0 {
            break;
        }
        sha256.update(&buffer[..n]);
        sha384.update(&buffer[..n]);
        byte_length += n as u64;
    }
    Ok(Hashed {
        sha256: format!("{:x}", base16ct::HexDisplay(&sha256.finalize())),
        integrity: integrity_value(&sha384.finalize()),
        byte_length,
    })
}

/// Take a shared lock on the file now at `path`.
///
/// An export replaces a bundle by moving a new directory into its place. A
/// reader that opened the old marker just before that would wait for the
/// export's lock, get it once the export finished, and then hold a lock on a
/// file that is no longer in the directory: the next export would not see it.
/// So after locking, the file at `path` has to still be the one that was
/// opened, or the lock is taken again on its replacement.
fn lock_current(path: &Path) -> Result<File> {
    loop {
        plain_file(path)?;
        let file = at(path, File::open(path))?;
        at(path, file.lock_shared())?;
        if same_file(&file, path)? {
            return Ok(file);
        }
        let _ = file.unlock();
    }
}

#[cfg(unix)]
fn same_file(open: &File, path: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let held = at(path, open.metadata())?;
    Ok(match fs::metadata(path) {
        Ok(current) => held.dev() == current.dev() && held.ino() == current.ino(),
        // Gone for the moment: the next attempt reports it or finds the new one.
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(source) => {
            return Err(AssetError::Io {
                path: path.into(),
                source,
            })
        }
    })
}

/// Windows cannot move a directory that holds an open file, so an export
/// never replaces one under a reader there.
#[cfg(not(unix))]
fn same_file(_open: &File, _path: &Path) -> Result<bool> {
    Ok(true)
}

fn validate_integrity(value: &str) -> Result<()> {
    // 48 digest bytes are always 64 base64 characters, with no padding.
    let valid = value.strip_prefix("sha384-").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
    });
    if !valid {
        return Err(AssetError::InvalidManifest(
            "integrity must be a base64 SHA-384 value".into(),
        ));
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(AssetError::InvalidManifest(
            "digest must be 64 lowercase SHA-256 hex characters".into(),
        ));
    }
    Ok(())
}

fn validate_relative(value: &str) -> Result<()> {
    if value.is_empty()
        || value.contains(['\\', ':', '\0'])
        || value.bytes().any(|b| b.is_ascii_control())
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(AssetError::InvalidPath(value.into()));
    }
    Ok(())
}

fn fingerprinted_path(logical: &str, digest: &str) -> String {
    let (parent, name) = logical
        .rsplit_once('/')
        .map_or(("", logical), |(p, n)| (p, n));
    let (stem, extension) = name
        .rsplit_once('.')
        .filter(|(s, _)| !s.is_empty())
        .map_or((name, ""), |(s, e)| (s, e));
    let short = &digest[..FINGERPRINT_LENGTH];
    // Preserve already-fingerprinted font/other outputs only when the suffix
    // matches their actual bytes, rather than trusting a filename pattern.
    if stem
        .rsplit_once('.')
        .is_some_and(|(_, suffix)| suffix == short)
    {
        return logical.into();
    }
    let suffix = if extension.is_empty() {
        String::new()
    } else {
        format!(".{extension}")
    };
    let name = format!("{stem}.{short}{suffix}");
    if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    }
}

fn at<T>(path: &Path, result: io::Result<T>) -> Result<T> {
    result.map_err(|source| AssetError::Io {
        path: path.into(),
        source,
    })
}

fn plain_file(path: &Path) -> Result<()> {
    if !at(path, fs::symlink_metadata(path))?.file_type().is_file() {
        return Err(AssetError::InvalidPath(path.display().to_string()));
    }
    Ok(())
}

fn confined_file(root: &Path, relative: &str) -> Result<PathBuf> {
    validate_relative(relative)?;
    let mut path = root.to_path_buf();
    if !at(&path, fs::symlink_metadata(&path))?.file_type().is_dir() {
        return Err(AssetError::InvalidPath(path.display().to_string()));
    }
    let mut parts = relative.split('/').peekable();
    while let Some(part) = parts.next() {
        path.push(part);
        let kind = at(&path, fs::symlink_metadata(&path))?.file_type();
        if if parts.peek().is_none() {
            !kind.is_file()
        } else {
            !kind.is_dir()
        } {
            return Err(AssetError::InvalidPath(path.display().to_string()));
        }
    }
    Ok(path)
}

fn validate_owner(path: &Path) -> Result<()> {
    let marker = path.join(OWNER_FILE);
    plain_file(&marker)?;
    if at(&marker, fs::read(&marker))? != OWNER_BYTES {
        return Err(AssetError::NotOwned(path.to_owned()));
    }
    Ok(())
}
