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
        Ok(hashed.sha256 == self.sha256 && hashed.byte_length == self.byte_length)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetManifest {
    pub version: u32,
    /// Keys are package-public logical paths, such as `images/logo.svg`.
    pub assets: BTreeMap<String, AssetEntry>,
}

impl Default for AssetManifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            assets: BTreeMap::new(),
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
        Ok(())
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
        let reader_path = output.join(READERS_FILE);
        plain_file(&reader_path)?;
        let reader = at(&reader_path, File::open(&reader_path))?;
        at(&reader_path, reader.lock_shared())?;
        // Lock before reading the pointer. Cleanup then cannot remove files
        // referenced by either the old or the new atomically read manifest.
        let (manifest, bytes) = AssetManifest::read(&output.join(MANIFEST_FILE))?;
        let objects = output.join("objects");
        let mut checked = BTreeSet::new();
        for entry in manifest.assets.values() {
            if checked.insert(&entry.object) {
                let path = confined_file(&objects, &entry.object)?;
                if !entry.matches_contents(&path)? {
                    return Err(AssetError::Integrity(entry.object.clone()));
                }
            }
        }
        Ok(Self {
            output,
            manifest,
            generation: sha256(&bytes),
            _reader: reader,
        })
    }

    pub fn manifest(&self) -> &AssetManifest {
        &self.manifest
    }
    /// Include this value and delivery settings in caches containing asset URLs.
    pub fn generation(&self) -> &str {
        &self.generation
    }
    pub fn object_path(&self, logical: &str) -> Option<PathBuf> {
        self.manifest
            .assets
            .get(logical)
            .map(|entry| self.output.join("objects").join(&entry.object))
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
    /// Recorded by the writer; a reader verifies the SHA-256 alone.
    #[cfg_attr(not(feature = "build"), allow(dead_code))]
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
