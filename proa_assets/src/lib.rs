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
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[cfg(feature = "build")]
pub mod build;

pub const MANIFEST_VERSION: u32 = 1;
pub const MANIFEST_FILE: &str = "manifest.json";
/// Runtime-only input: never pass this variable to rustc or build scripts.
pub const OUTPUT_ENV: &str = "PROA_PUBLIC_ASSETS";
pub const PUBLIC_ROUTE: &str = "/_proa/assets";
const OWNER_FILE: &str = ".proa-assets";
const OWNER_BYTES: &[u8] = b"proa-public-assets-v1\n";
const READERS_FILE: &str = ".readers.lock";

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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetEntry {
    /// Path relative to `objects/`, always tied to the copied bytes.
    pub object: String,
    /// Preferred browser path relative to the public mount.
    pub public_path: String,
    pub sha256: String,
    pub byte_length: u64,
    pub content_type: String,
    pub cache_policy: CachePolicy,
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
    pub fn validate(&self) -> Result<()> {
        if self.version != MANIFEST_VERSION {
            return Err(AssetError::InvalidManifest(format!(
                "unsupported schema version {}",
                self.version
            )));
        }
        let mut urls = BTreeMap::<&str, &str>::new();
        for (logical, entry) in &self.assets {
            validate_relative(logical)?;
            validate_relative(&entry.object)?;
            validate_relative(&entry.public_path)?;
            validate_digest(&entry.sha256)?;
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
            // Logical aliases preserve relative references in files that are
            // not relocated yet. They cannot shadow another immutable object.
            for path in [logical.as_str(), entry.public_path.as_str()] {
                if let Some(previous) = urls.insert(path, &entry.sha256) {
                    if previous != entry.sha256 {
                        return Err(AssetError::Collision {
                            path: path.into(),
                            previous: previous.into(),
                            current: entry.sha256.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

/// An immutable, verified view. Clones share one native reader lock.
#[derive(Clone, Debug)]
pub struct AssetCatalog(Arc<CatalogInner>);

#[derive(Debug)]
struct CatalogInner {
    output: PathBuf,
    manifest: AssetManifest,
    generation: String,
    _reader: File,
}

impl AssetCatalog {
    pub fn load(output: impl AsRef<Path>) -> Result<Self> {
        let output = canonical_owned(output.as_ref())?;
        let reader_path = output.join(READERS_FILE);
        plain_file(&reader_path)?;
        let reader = at(&reader_path, File::open(&reader_path))?;
        at(&reader_path, reader.lock_shared())?;
        // Lock before reading the pointer. Cleanup then cannot remove files
        // referenced by either the old or the new atomically read manifest.
        let path = output.join(MANIFEST_FILE);
        plain_file(&path)?;
        let bytes = at(&path, fs::read(&path))?;
        let manifest: AssetManifest = serde_json::from_slice(&bytes)?;
        manifest.validate()?;
        let mut checked = BTreeSet::new();
        for entry in manifest.assets.values() {
            if checked.insert(&entry.object) {
                let path = confined_file(&output.join("objects"), &entry.object)?;
                if hash_file(&path)? != (entry.sha256.clone(), entry.byte_length) {
                    return Err(AssetError::Integrity(entry.object.clone()));
                }
            }
        }
        Ok(Self(Arc::new(CatalogInner {
            output,
            manifest,
            generation: sha256(&bytes),
            _reader: reader,
        })))
    }

    pub fn manifest(&self) -> &AssetManifest {
        &self.0.manifest
    }
    /// Include this value and delivery settings in caches containing asset URLs.
    pub fn generation(&self) -> &str {
        &self.0.generation
    }
    pub fn output_directory(&self) -> &Path {
        &self.0.output
    }
    pub fn get(&self, logical: &str) -> Option<&AssetEntry> {
        self.0.manifest.assets.get(logical)
    }
    pub fn object_path(&self, logical: &str) -> Option<PathBuf> {
        self.get(logical)
            .map(|entry| self.0.output.join("objects").join(&entry.object))
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn hash_file(path: &Path) -> Result<(String, u64)> {
    let mut file = at(path, File::open(path))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut length = 0;
    loop {
        let n = at(path, file.read(&mut buffer))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        length += n as u64;
    }
    Ok((hex(&hash.finalize()), length))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
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
    let short = &digest[..8];
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
    let parts: Vec<_> = relative.split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        let kind = at(&path, fs::symlink_metadata(&path))?.file_type();
        if if index + 1 == parts.len() {
            !kind.is_file()
        } else {
            !kind.is_dir()
        } {
            return Err(AssetError::InvalidPath(path.display().to_string()));
        }
    }
    Ok(path)
}

fn canonical_owned(path: &Path) -> Result<PathBuf> {
    let path = at(path, path.canonicalize())?;
    let marker = path.join(OWNER_FILE);
    plain_file(&marker)?;
    if at(&marker, fs::read(&marker))? != OWNER_BYTES {
        return Err(AssetError::NotOwned(path));
    }
    Ok(path)
}
