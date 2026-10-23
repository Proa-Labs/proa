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
    if manifest_path.exists() {
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
        let owner = installation::detect(&bin_dir.join(&names[0]).canonicalize()?)?;
        ensure!(
            !matches!(owner, installation::Installation::Cargo { .. }),
            "Cargo owns this installation; use cargo install --locked proa-cli instead"
        );
        managed = matches!(owner, installation::Installation::Standalone { .. });
    }
    let releases = root.join("releases");
    fs::create_dir_all(&releases)?;
    let staged = tempfile::Builder::new()
        .prefix("release-")
        .tempdir_in(&releases)?;
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
    if !args.allow_downgrade && bin_dir.join(&names[0]).exists() {
        let installed = binary_version(&bin_dir.join(&names[0]))?;
        if installed > version || (managed && installed == version) {
            println!("Proa {installed} is already installed; no update needed.");
            return Ok(());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The published directory must remain traversable for shared installs;
        // tempfile deliberately starts private while validation is in progress.
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

#[cfg(unix)]
fn publish(
    bin_dir: &Path,
    root: &Path,
    release: &Path,
    current: &Path,
    names: &[String; 2],
) -> Result<()> {
    use std::os::unix::fs::symlink;
    fs::create_dir_all(bin_dir)?;
    ensure!(
        fs::symlink_metadata(current).map_or(true, |meta| meta.file_type().is_symlink()),
        "current must be an installer-owned symlink"
    );
    let transaction = tempfile::Builder::new()
        .prefix("legacy-")
        .tempdir_in(root)?;
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
    use std::os::windows::fs::MetadataExt;
    // Dedicated bin directories can migrate; shared directories must never be
    // replaced by an installer-owned junction.
    let is_junction = |path: &Path| {
        fs::symlink_metadata(path).is_ok_and(|meta| meta.file_attributes() & 0x400 != 0)
    };
    let legacy = root.join("legacy-bin");
    let had_current = fs::symlink_metadata(current).is_ok();
    let had_visible_junction = is_junction(bin_dir);
    let mut moved_legacy = false;
    if bin_dir.exists() && !is_junction(bin_dir) {
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
    } else if bin_dir.exists() {
        ensure!(
            bin_dir
                .canonicalize()?
                .starts_with(root.join("releases").canonicalize()?),
            "bin junction belongs to another installation"
        );
    }
    let result = (|| -> Result<()> {
        if current.exists() {
            ensure!(
                is_junction(current)
                    && current
                        .canonicalize()?
                        .starts_with(root.join("releases").canonicalize()?),
                "current junction belongs to another installation"
            );
        }
        // Establish the visible junction before switching current. Existing
        // installations keep both executable entry points untouched.
        if !is_junction(bin_dir) {
            set_junction(bin_dir, current)?;
        }
        set_junction(current, release)?;
        Ok(())
    })();
    if let Err(error) = result {
        if !had_current {
            let _ = fs::remove_dir(current);
        }
        if !had_visible_junction {
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
