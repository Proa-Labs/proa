// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::installation::{self, Manifest, MANIFEST};
use anyhow::{ensure, Context, Result};
use clap::Args;
use semver::Version;
#[cfg(windows)]
use std::io;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Args)]
pub(crate) struct InstallSelfArgs {
    #[arg(long)]
    source: PathBuf,
    #[arg(long)]
    bin_dir: PathBuf,
    #[arg(long)]
    expected_version: Option<Version>,
    #[arg(long)]
    distribution_version: Option<String>,
    #[arg(long)]
    allow_downgrade: bool,
}

pub(crate) fn install_self(args: InstallSelfArgs) -> Result<()> {
    let bin_dir = std::path::absolute(&args.bin_dir)?;
    let parent = bin_dir
        .parent()
        .context("installation directory needs a parent")?;
    fs::create_dir_all(parent)?;
    let bin_dir = installation::external_path(&parent.canonicalize()?).join(
        bin_dir
            .file_name()
            .context("invalid installation directory")?,
    );
    // Unix keeps a normal bin directory: an alias to it must use the same
    // bundle root. Windows' visible directory is itself our mutable junction.
    #[cfg(unix)]
    let bin_dir = if bin_dir.is_dir() {
        bin_dir.canonicalize()?
    } else {
        bin_dir
    };
    ensure!(
        bin_dir.file_name().is_some(),
        "installation directory must be below the filesystem root"
    );
    let root = installation::root_for_bin(&bin_dir);
    fs::create_dir_all(&root)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("install.lock"))?;
    lock.try_lock()
        .context("another Proa installation/update is running; retry after it finishes")?;
    let manifest_path = root.join(MANIFEST);
    let has_manifest = manifest_path.exists();
    if has_manifest {
        let manifest: Manifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        let same_directory = manifest.bin_dir == bin_dir
            || manifest
                .bin_dir
                .canonicalize()
                .ok()
                .zip(bin_dir.canonicalize().ok())
                .is_some_and(|(existing, requested)| existing == requested);
        ensure!(
            manifest.format == 1 && same_directory,
            "installation manifest belongs to another installation"
        );
    }
    let names = installation::names();
    let mut managed = false;
    if bin_dir.join(&names[0]).exists() {
        let owner =
            installation::detect_with_bundle_lock(&bin_dir.join(&names[0]).canonicalize()?)?;
        ensure!(
            !matches!(owner, installation::Installation::Cargo { .. }),
            "Cargo owns this installation; use cargo install --locked proa-cli instead"
        );
        managed = matches!(owner, installation::Installation::Standalone { .. });
    }
    let releases = root.join("releases");
    fs::create_dir_all(&releases)?;
    let staged = installation::private_tempdir(&releases, "release-")?;
    for name in &names {
        let source = args.source.join(name);
        ensure!(
            fs::symlink_metadata(&source)?.file_type().is_file(),
            "release must contain two regular CLI files"
        );
        fs::copy(source, staged.path().join(name))?;
    }
    let version = binary_version(&staged.path().join(&names[0]))?;
    ensure!(
        args.expected_version
            .as_ref()
            .is_none_or(|expected| expected == &version),
        "release did not report the requested version"
    );
    ensure!(
        binary_version(&staged.path().join(&names[1]))? == version,
        "release CLI versions do not match"
    );
    // Distribution IDs are opaque: a numeric commit SHA with leading zeroes
    // is valid here even though SemVer would reject that prerelease identifier.
    let distribution = args
        .distribution_version
        .unwrap_or_else(|| version.to_string());
    if !args.allow_downgrade {
        let selected_distribution = has_manifest
            .then(|| fs::read_to_string(root.join("current/distribution-version")).ok())
            .flatten();
        // A broken executable must not prevent repair. Its recorded release
        // still protects against a stale automatic update; snapshot IDs use
        // their base version as a conservative bound when the binary cannot run.
        let installed = binary_version(&bin_dir.join(&names[0])).ok().or_else(|| {
            selected_distribution
                .as_deref()
                .and_then(|recorded| Version::parse(recorded.trim().split("-main.").next()?).ok())
        });
        if let Some(installed) = installed {
            if installed > version {
                println!("Keeping Proa {installed}: candidate {version} is older. Select an explicit version to reinstall or downgrade.");
                return Ok(());
            }
            let selected_distribution =
                selected_distribution.unwrap_or_else(|| installed.to_string());
            if managed
                && installed == version
                && selected_distribution.trim() == distribution
                && installed_bundle_is_complete(&bin_dir, &root, &names, &version)
            {
                println!("Proa {installed} is already installed; no update needed.");
                return Ok(());
            }
        }
    }
    fs::write(staged.path().join("distribution-version"), &distribution)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The published directory must remain traversable for shared installs;
        // Staging stays private while validation is in progress.
        fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o755))?;
    }
    // Preserve the optional companion tool when Windows' visible bin directory
    // is a junction to the selected bundle.
    #[cfg(windows)]
    if bin_dir.join("tailwindcss.exe").is_file() {
        fs::copy(
            bin_dir.join("tailwindcss.exe"),
            staged.path().join("tailwindcss.exe"),
        )?;
    }
    // Finish all fallible preparation before touching either entry point.
    fs::write(
        root.join("manifest.next"),
        serde_json::to_vec(&Manifest {
            format: 1,
            bin_dir: bin_dir.clone(),
        })?,
    )?;
    fs::rename(root.join("manifest.next"), &manifest_path)?;
    let release = staged.keep();
    let current = root.join("current");
    if let Err(error) = publish(&bin_dir, &root, &release, &current, &names) {
        let _ = fs::remove_dir_all(&release);
        return Err(error);
    }
    println!("Installed Proa {version} into {}", bin_dir.display());
    Ok(())
}

fn binary_version(executable: &Path) -> Result<Version> {
    let output = Command::new(executable)
        .arg("--version")
        .output()
        .with_context(|| {
            format!(
                "run {}; try --build-from-source if this release is incompatible",
                executable.display()
            )
        })?;
    ensure!(
        output.status.success(),
        "release cannot run on this system: {}\nRetry with proa update --build-from-source.",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout)?;
    Version::parse(
        text.trim()
            .strip_prefix("proa ")
            .context("unrecognized Proa version output")?,
    )
    .map_err(Into::into)
}

fn installed_bundle_is_complete(
    bin_dir: &Path,
    root: &Path,
    names: &[String; 2],
    version: &Version,
) -> bool {
    let current = root.join("current");
    let Ok(release) = current.canonicalize() else {
        return false;
    };
    #[cfg(windows)]
    if !is_junction(&current).unwrap_or(false)
        || !is_junction(bin_dir).unwrap_or(false)
        || !fs::read_link(bin_dir)
            .is_ok_and(|target| installation::external_path(&target) == current)
    {
        return false;
    }
    names.iter().all(|name| {
        let visible = bin_dir.join(name);
        #[cfg(unix)]
        if !fs::read_link(&visible).is_ok_and(|target| target == current.join(name)) {
            return false;
        }
        fs::symlink_metadata(release.join(name)).is_ok_and(|meta| meta.file_type().is_file())
            && visible
                .canonicalize()
                .is_ok_and(|path| path == release.join(name))
            && binary_version(&visible).is_ok_and(|installed| &installed == version)
    })
}

#[cfg(unix)]
fn publish(
    bin_dir: &Path,
    root: &Path,
    release: &Path,
    current: &Path,
    names: &[String; 2],
) -> Result<()> {
    use std::os::unix::fs::{symlink, PermissionsExt};
    fs::create_dir_all(bin_dir)?;
    ensure!(
        fs::symlink_metadata(current).map_or(true, |meta| meta.file_type().is_symlink()),
        "current must be an installer-owned symlink"
    );
    let transaction = tempfile::Builder::new()
        .prefix("legacy-")
        .tempdir_in(root)?;
    // Running users must still be able to reopen migrated executables, even
    // when the updater has a restrictive umask. Set access before moving them.
    fs::set_permissions(transaction.path(), fs::Permissions::from_mode(0o755))?;
    let old_current = fs::read_link(current).ok();
    let mut replaced = Vec::new();
    let mut keep_legacy = false;
    let result = (|| -> Result<()> {
        for (index, name) in names.iter().enumerate() {
            let visible = bin_dir.join(name);
            let target = current.join(name);
            if fs::read_link(&visible).ok().as_ref() == Some(&target) {
                continue;
            }
            let backup = transaction.path().join(name);
            let existed = fs::symlink_metadata(&visible).is_ok();
            if existed {
                keep_legacy |= fs::symlink_metadata(&visible)?.file_type().is_file();
                ensure!(
                    !visible.is_dir(),
                    "refusing to replace an entry-point directory"
                );
                fs::rename(&visible, &backup)?;
            }
            replaced.push((visible.clone(), backup, existed));
            let next = transaction.path().join(format!("next-{index}"));
            symlink(target, &next)?;
            fs::rename(next, visible)?;
        }
        let next = transaction.path().join("next-current");
        symlink(release, &next)?;
        fs::rename(next, current)?;
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_error = None;
        for (visible, backup, existed) in replaced.into_iter().rev() {
            let _ = fs::remove_file(&visible);
            if existed {
                if let Err(failure) = fs::rename(backup, visible) {
                    rollback_error = Some(failure);
                }
            }
        }
        if let Some(failure) = rollback_error {
            let recovery = transaction.keep();
            anyhow::bail!("publication failed ({error:#}) and rollback failed ({failure}); entry-point backups kept at {}", recovery.display());
        }
        if let Some(old) = old_current {
            ensure!(
                fs::read_link(current)? == old,
                "selection unexpectedly changed"
            );
        }
        return Err(error);
    }
    // A legacy Unix process may resolve current_exe again for a compiler
    // wrapper. Keep its renamed executable and companion available.
    if keep_legacy {
        let _ = transaction.keep();
    }
    Ok(())
}

#[cfg(windows)]
fn publish(
    bin_dir: &Path,
    root: &Path,
    release: &Path,
    current: &Path,
    names: &[String; 2],
) -> Result<()> {
    // PowerShell can launch this helper from the destination itself. Windows
    // keeps a handle to the process's working directory, blocking its rename.
    std::env::set_current_dir(root)
        .context("leave the installation directory before replacing it")?;
    // Dedicated bin directories can migrate; shared directories must never be
    // replaced by an installer-owned junction.
    let legacy = root.join("legacy-bin");
    let had_current = fs::symlink_metadata(current).is_ok();
    let had_visible_junction = is_junction(bin_dir)?;
    let old_visible_target = had_visible_junction
        .then(|| fs::read_link(bin_dir))
        .transpose()?;
    if had_current {
        ensure!(
            is_junction(current)?
                && current
                    .canonicalize()?
                    .starts_with(root.join("releases").canonicalize()?),
            "current junction belongs to another installation"
        );
    }
    let mut moved_legacy = false;
    if had_visible_junction {
        ensure!(
            bin_dir
                .canonicalize()?
                .starts_with(root.join("releases").canonicalize()?),
            "bin junction belongs to another installation"
        );
    } else if bin_dir.exists() {
        for entry in fs::read_dir(bin_dir)? {
            let name = entry?.file_name();
            ensure!(
                names.iter().any(|expected| name == expected.as_str()) || name == "tailwindcss.exe",
                "use a dedicated Proa install directory; this directory contains other files"
            );
        }
        ensure!(
            !legacy.exists(),
            "a previous migration needs recovery at {}",
            legacy.display()
        );
        fs::rename(bin_dir, &legacy)
            .context("close Proa before migrating this legacy installation")?;
        moved_legacy = true;
    }
    let result = (|| -> Result<()> {
        // Repair an owned junction that points directly to a release, too.
        // It must follow current before selection changes to the new bundle.
        if old_visible_target
            .as_ref()
            .is_none_or(|target| installation::external_path(target) != current)
        {
            set_junction(bin_dir, current)?;
        }
        set_junction(current, release)?;
        Ok(())
    })();
    if let Err(error) = result {
        if !had_current {
            let _ = fs::remove_dir(current);
        }
        if let Some(target) = old_visible_target {
            set_junction(bin_dir, &target).context("restore public bin junction")?;
        } else {
            let _ = fs::remove_dir(bin_dir);
        }
        if moved_legacy {
            fs::rename(&legacy, bin_dir).context("restore legacy installation")?;
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn is_junction(path: &Path) -> Result<bool> {
    use std::os::windows::{fs::MetadataExt, fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FileAttributeTagInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_TAG_INFO,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_attributes() & 0x400 == 0 {
        return Ok(false);
    }
    let file = fs::OpenOptions::new()
        .access_mode(0)
        .share_mode(7)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // Query the link itself: the reparse-point attribute alone also includes
    // symbolic links and other types that cannot be retargeted as junctions.
    let result = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error().into());
    }
    ensure!(
        info.ReparseTag == 0xA0000003,
        "refusing to replace a non-junction reparse point at {}",
        path.display()
    );
    Ok(true)
}

#[cfg(windows)]
fn set_junction(link: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    match fs::symlink_metadata(link) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(link)?,
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let target = std::path::absolute(target)?;
    let text = target.as_os_str().to_string_lossy();
    let normal = text.strip_prefix("\\\\?\\").unwrap_or(&text);
    let substitute = if let Some(unc) = normal.strip_prefix("\\\\") {
        format!("\\??\\UNC\\{unc}")
    } else {
        format!("\\??\\{normal}")
    };
    let substitute: Vec<_> = std::ffi::OsStr::new(&substitute).encode_wide().collect();
    let print: Vec<_> = target.as_os_str().encode_wide().collect();
    let data_len = 8 + (substitute.len() + print.len() + 2) * 2;
    if data_len > u16::MAX as usize {
        return Err(io::Error::other("junction target is too long"));
    }
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&0xA0000003_u32.to_le_bytes());
    for value in [
        data_len as u16,
        0,
        0,
        (substitute.len() * 2) as u16,
        ((substitute.len() + 1) * 2) as u16,
        (print.len() * 2) as u16,
    ] {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    for value in substitute.into_iter().chain([0]).chain(print).chain([0]) {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    let file = fs::OpenOptions::new()
        .access_mode(0x40000000)
        .share_mode(7)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(link)?;
    let mut returned = 0;
    // FSCTL_SET_REPARSE_POINT atomically retargets a mount-point junction. This
    // avoids deleting a directory containing a running Windows executable.
    let result = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            0x000900A4,
            buffer.as_ptr().cast(),
            buffer.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
