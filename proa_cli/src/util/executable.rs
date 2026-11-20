// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

static EXECUTABLE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Pin the resolved executable when the CLI starts. On macOS and Windows,
/// current_exe can return the public launcher path, whose bundle may change
/// while a dev process is running. Its compiler wrappers must stay on the
/// same version as the process that prepares their inputs.
pub(crate) fn current_executable() -> io::Result<PathBuf> {
    if let Some(Some(path)) = EXECUTABLE.get() {
        return Ok(path.clone());
    }
    let path = std::env::current_exe()?.canonicalize()?;
    let pinned = EXECUTABLE.get_or_init(|| {
        // Only managed bundles have immutable locations. For legacy installs,
        // let the OS keep tracking an executable renamed during migration.
        let releases = path.parent()?.parent()?;
        (releases.file_name()? == "releases"
            && releases.parent()?.join("proa-install.json").is_file())
        .then(|| path.clone())
    });
    Ok(pinned.as_ref().unwrap_or(&path).clone())
}

/// Shells and installers do not consistently accept the verbatim prefixes
/// returned by Rust's Windows filesystem APIs.
pub(crate) fn external_path(path: &Path) -> PathBuf {
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
