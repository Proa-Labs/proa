// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub(super) enum Installation {
    Standalone { root: PathBuf, bin_dir: PathBuf },
    Cargo { root: PathBuf },
    Unknown,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Manifest {
    pub format: u32,
    pub bin_dir: PathBuf,
}

pub(super) const MANIFEST: &str = "proa-install.json";

pub(super) fn private_tempdir(parent: &Path, prefix: &str) -> Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    Ok(builder.tempdir_in(parent)?)
}

/// External installers, including PowerShell 5.1, do not consistently accept
/// the verbatim prefixes returned by Rust's Windows filesystem APIs.
pub(super) fn external_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(text) = path.to_str() {
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(plain) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(plain);
        }
    }
    path.to_owned()
}

pub(super) fn names() -> [String; 2] {
    ["proa", "cargo-proa"].map(|name| format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

pub(super) fn root_for_bin(bin_dir: &Path) -> PathBuf {
    let mut name = OsString::from(bin_dir.file_name().unwrap());
    name.push(".proa");
    bin_dir.with_file_name(name)
}

pub(super) fn detect(executable: &Path) -> Result<Installation> {
    let _lock = standalone_root(executable)
        .filter(|root| root.join(MANIFEST).is_file())
        .map(|root| -> Result<fs::File> {
            let lock = fs::File::open(root.join("install.lock"))?;
            lock.try_lock_shared()
                .context("another Proa installation/update is running; retry after it finishes")?;
            Ok(lock)
        })
        .transpose()?;
    detect_with_bundle_lock(executable)
}

fn standalone_root(executable: &Path) -> Option<&Path> {
    let releases = executable.parent()?.parent()?;
    (releases.file_name()? == "releases")
        .then(|| releases.parent())
        .flatten()
}

/// The caller must hold this installation's `install.lock` when inspecting a
/// standalone bundle. Publishing already holds it exclusively.
pub(super) fn detect_with_bundle_lock(executable: &Path) -> Result<Installation> {
    if !names().iter().any(|name| {
        executable
            .file_name()
            .is_some_and(|actual| actual == name.as_str())
    }) {
        return Ok(Installation::Unknown);
    }
    let Some(directory) = executable.parent() else {
        return Ok(Installation::Unknown);
    };
    if let Some(releases) = directory
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "releases"))
    {
        if let Some(root) = releases.parent() {
            if root.join(MANIFEST).is_file() {
                let manifest: Manifest = serde_json::from_slice(&fs::read(root.join(MANIFEST))?)?;
                let current = root.join("current").canonicalize()?;
                let registered = manifest.bin_dir.join(&names()[0]).canonicalize()?;
                if manifest.format == 1
                    && manifest.bin_dir.is_absolute()
                    && root_for_bin(&manifest.bin_dir).canonicalize()? == root.canonicalize()?
                    && current.starts_with(releases.canonicalize()?)
                    && registered.parent() == Some(current.as_path())
                {
                    return Ok(Installation::Standalone {
                        root: external_path(root),
                        bin_dir: manifest.bin_dir,
                    });
                }
            }
        }
    }
    if directory.file_name().is_some_and(|name| name == "bin") {
        let root = directory
            .parent()
            .context("Cargo bin directory has no installation root")?;
        let path = root.join(".crates.toml");
        if path.is_file() {
            let mut file = fs::File::open(path)?;
            file.try_lock_shared()
                .context("Cargo is modifying its installation records; retry when it finishes")?;
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            let listing: toml::Table =
                toml::from_str(&text).context("read Cargo installation ownership")?;
            if listing
                .get("v1")
                .and_then(toml::Value::as_table)
                .is_some_and(|entries| {
                    entries.iter().any(|(package, bins)| {
                        package.split_whitespace().next() == Some("proa-cli")
                            && bins.as_array().is_some_and(|bins| {
                                bins.iter().any(|bin| {
                                    bin.as_str()
                                        .is_some_and(|bin| executable.file_name().unwrap() == bin)
                                })
                            })
                    })
                })
            {
                return Ok(Installation::Cargo {
                    root: root.to_owned(),
                });
            }
        }
    }
    Ok(Installation::Unknown)
}

/// Resolve only absolute PATH entries, preserving executable aliases such as
/// Cargo's rustup proxy. Never search the caller's project directory.
pub(super) fn find_program(name: &str) -> Result<PathBuf> {
    let mut filename = OsString::from(name);
    if cfg!(windows) && !name.ends_with(".exe") {
        filename.push(".exe");
    }
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if !directory.is_absolute() {
            continue;
        }
        let program = directory.join(&filename);
        if program.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if fs::metadata(&program)?.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            return Ok(program);
        }
    }
    anyhow::bail!("cannot find {name} on an absolute PATH entry")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_ownership_requires_a_package_record_and_never_rewrites_it() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let executable = bin.join(&names()[0]);
        fs::write(&executable, b"fixture").unwrap();
        assert!(matches!(
            detect(&executable).unwrap(),
            Installation::Unknown
        ));
        let records = temp.path().join(".crates.toml");
        for (owner, expected) in [("another-package", false), ("proa-cli", true)] {
            let contents = format!("[v1]\n\"{owner} 1.0.0 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"{}\"]\n", names()[0]);
            fs::write(&records, &contents).unwrap();
            assert_eq!(
                matches!(detect(&executable).unwrap(), Installation::Cargo { .. }),
                expected
            );
            assert_eq!(fs::read_to_string(&records).unwrap(), contents);
            assert!(!temp.path().join(".crates2.json").exists());
        }
    }

    #[test]
    fn standalone_detection_refuses_to_read_during_publication() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let release = root.join("releases/selected");
        fs::create_dir_all(&release).unwrap();
        fs::write(root.join(MANIFEST), b"being replaced").unwrap();
        let lock = fs::File::create(root.join("install.lock")).unwrap();
        lock.try_lock().unwrap();
        let error = detect(&release.join(&names()[0])).unwrap_err();
        assert!(error.to_string().contains("retry after it finishes"));
    }
}
