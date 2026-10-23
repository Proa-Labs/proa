// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Self-update from the same release bridge and archive contract as the installers.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};
use clap::Args;
use reqwest::blocking::Client;
use reqwest::StatusCode;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

mod cargo_install;

const DOWNLOAD_BASE: &str = "https://proa.so/downloads/cli";
const REGISTRY_METADATA: &str = "https://crates.io/api/v1/crates/proa-cli";
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Args)]
pub(crate) struct UpdateArgs {
    /// Show the available version without changing any files.
    #[arg(long)]
    check: bool,
    /// Install an exact stable release (also permits an explicit downgrade).
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// Build with Cargo instead of downloading a prebuilt archive (for example on older glibc).
    #[arg(long)]
    build_from_source: bool,
}

#[derive(Deserialize)]
struct LatestRelease {
    version: String,
}

#[derive(Deserialize)]
struct RegistryMetadata {
    #[serde(rename = "crate")]
    package: RegistryPackage,
}

#[derive(Deserialize)]
struct RegistryPackage {
    max_stable_version: Option<String>,
}

pub(crate) fn run(args: UpdateArgs) -> Result<()> {
    let client = Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(30))
        // The blocking response's Read implementation resets this per read.
        // Keep streaming slow downloads; only a stalled read should time out.
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("proa/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let requested = args
        .version
        .as_deref()
        .map(parse_requested_version)
        .transpose()?;
    let executable =
        resolve_executable(&std::env::current_exe().context("locate the installed Proa CLI")?)?;
    // Take the lock before version selection and keep it through preparation.
    // Another invocation must not install a newer release while this one builds.
    // The process image may have started before another updater finished. Read
    // the version now installed on disk under the lock, not that image's version.
    let (_lock, current) = if args.check {
        (None, Version::parse(env!("CARGO_PKG_VERSION"))?)
    } else {
        let (lock, current) = lock_and_read_installation(&executable)?;
        (Some(lock), current)
    };
    let version = match requested {
        Some(version) => version,
        None => latest_version(&client, DOWNLOAD_BASE, REGISTRY_METADATA)?,
    };
    println!(
        "Installed: {current}\n{}: {version}",
        if args.version.is_some() {
            "Requested"
        } else {
            "Latest stable"
        }
    );
    if args.check {
        return Ok(());
    }
    if !needs_update(&current, &version, args.version.is_some()) {
        println!("No update needed.");
        return Ok(());
    }

    let staging = update_staging(&executable)?;
    let prepared = prepare_update(
        &client,
        DOWNLOAD_BASE,
        &version,
        staging.path(),
        args.build_from_source,
    )?;
    validate_binaries(&prepared, &version)?;
    cargo_install::with_updated_records(&executable, &prepared, &version, || {
        replace_pair(&executable, &prepared, |source| {
            replace_running_executable(&executable, source)
        })
    })?;
    println!("Updated proa and cargo-proa to {version}. Run proa --version to verify.");
    Ok(())
}

fn parse_version(value: &str) -> Result<Version> {
    let normalized = value.strip_prefix("proa-cli-").unwrap_or(value);
    Version::parse(normalized.strip_prefix('v').unwrap_or(normalized)).with_context(|| {
        format!("invalid release version `{value}`; expected a version such as 0.1.3")
    })
}

fn parse_requested_version(value: &str) -> Result<Version> {
    let version = parse_version(value)?;
    ensure!(
        version.pre.is_empty() && version.build.is_empty(),
        "proa update --version supports stable releases only; use the Proa installer for main snapshots"
    );
    Ok(version)
}

fn resolve_executable(executable: &Path) -> Result<PathBuf> {
    executable
        .canonicalize()
        .context("resolve the installed CLI path")
}

fn update_staging(executable: &Path) -> Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix(".proa-update-staging-")
        .tempdir_in(executable_directory(executable)?)
        .context("create update staging beside the installed CLI; the installation directory must be writable")
}

fn replace_running_executable(executable: &Path, source: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        // Use the same canonical destination as the companion and installation
        // lock. self-replace 1.5 only follows one link from current_exe on Unix.
        let temporary = tempfile::NamedTempFile::new_in(executable.parent().unwrap())?;
        fs::copy(source, temporary.path())?;
        fs::set_permissions(temporary.path(), executable.metadata()?.permissions())?;
        temporary.persist(executable).map_err(|error| error.error)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = executable;
        self_replace::self_replace(source)
    }
}

fn needs_update(current: &Version, selected: &Version, explicitly_pinned: bool) -> bool {
    explicitly_pinned || selected > current
}

fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "release response exceeds the {limit}-byte limit"
    );
    Ok(bytes)
}

fn latest_version(client: &Client, base: &str, registry: &str) -> Result<Version> {
    let latest = (|| -> Result<Version> {
        let response = client
            .get(format!("{base}/latest.json"))
            .send()?
            .error_for_status()?;
        let release: LatestRelease =
            serde_json::from_slice(&read_limited(response, MAX_METADATA_BYTES)?)?;
        let version = parse_version(&release.version)?;
        ensure!(
            version.pre.is_empty(),
            "the stable release endpoint returned a prerelease"
        );
        Ok(version)
    })();
    if let Ok(version) = latest {
        return Ok(version);
    }
    println!("Prebuilt release metadata is unavailable; checking crates.io.");
    let response = client.get(registry).send()?.error_for_status()?;
    let metadata: RegistryMetadata =
        serde_json::from_slice(&read_limited(response, MAX_METADATA_BYTES)?)?;
    let version = metadata
        .package
        .max_stable_version
        .context("crates.io has no stable proa-cli release")?;
    let version = parse_version(&version)?;
    ensure!(
        version.pre.is_empty(),
        "crates.io returned a prerelease as its stable version"
    );
    Ok(version)
}

fn binary_names() -> [String; 2] {
    ["proa", "cargo-proa"].map(|name| format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn release_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") if cfg!(target_env = "gnu") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") if cfg!(target_env = "gnu") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("windows", "x86_64") if cfg!(target_env = "msvc") => Some("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") if cfg!(target_env = "msvc") => Some("aarch64-pc-windows-msvc"),
        _ => None,
    }
}

fn prepare_update(
    client: &Client,
    base: &str,
    version: &Version,
    staging: &Path,
    build_from_source: bool,
) -> Result<PathBuf> {
    prepare_update_with_source(
        client,
        base,
        version,
        staging,
        build_from_source,
        build_source_update,
    )
}

fn prepare_update_with_source(
    client: &Client,
    base: &str,
    version: &Version,
    staging: &Path,
    build_from_source: bool,
    source: impl FnOnce(&Version, &Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    if let Some(target) = release_target().filter(|_| !build_from_source) {
        let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
        let prefix = format!("proa-cli-{version}-{target}");
        let archive = format!("{prefix}.{extension}");
        let url = format!("{base}/{version}/{archive}");
        println!("Downloading proa-cli {version} for {target}...");
        let mut response = client
            .get(&url)
            .send()
            .context("download the CLI release")?;
        if response.status() != StatusCode::NOT_FOUND {
            response.error_for_status_ref()?;
            let archive_path = staging.join(&archive);
            let mut file = fs::File::create(&archive_path)?;
            let size = io::copy(
                &mut response.by_ref().take(MAX_ARCHIVE_BYTES + 1),
                &mut file,
            )?;
            ensure!(
                size <= MAX_ARCHIVE_BYTES,
                "release archive exceeds the size limit"
            );
            file.flush()?;
            let response = client
                .get(format!("{base}/{version}/proa-cli-{version}-SHA256SUMS"))
                .send()?
                .error_for_status()
                .context("release checksum file is missing; refusing an unverified update")?;
            let sums = String::from_utf8(read_limited(response, MAX_METADATA_BYTES)?)?;
            verify_checksum(&archive_path, &archive, &sums)?;
            let extracted = staging.join("extracted");
            fs::create_dir(&extracted)?;
            extract_pair(&archive_path, &prefix, &extracted, extension == "zip")?;
            return Ok(extracted);
        }
    }

    source(version, staging)
}

fn build_source_update(version: &Version, staging: &Path) -> Result<PathBuf> {
    println!("Building proa-cli {version} from crates.io; this can take several minutes.");
    let cargo_root = staging.join("cargo");
    let status = Command::new("cargo")
        .current_dir(staging)
        .env("CARGO_TARGET_DIR", staging.join("build"))
        .env("TMPDIR", staging)
        .env("TEMP", staging)
        .env("TMP", staging)
        .args([
            "install",
            "--locked",
            "proa-cli",
            "--registry",
            "crates-io",
            "--version",
        ])
        .arg(format!("={version}"))
        .arg("--root")
        .arg(&cargo_root)
        .status()
        .context("source updates require Cargo; install Rust from https://rustup.rs")?;
    ensure!(
        status.success(),
        "building proa-cli {version} failed; the installed CLI was not changed"
    );
    Ok(cargo_root.join("bin"))
}

fn verify_checksum(path: &Path, archive: &str, sums: &str) -> Result<()> {
    let matches: Vec<_> = sums
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let hash = fields.next()?;
            let name = fields.next()?.trim_start_matches('*');
            (name == archive && fields.next().is_none()).then_some(hash)
        })
        .collect();
    ensure!(
        matches.len() == 1,
        "checksum file must contain exactly one entry for {archive}"
    );
    let expected = matches[0];
    ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid SHA256 checksum for {archive}"
    );
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let actual = format!("{:x}", base16ct::HexDisplay(&digest.finalize()));
    ensure!(
        actual.eq_ignore_ascii_case(expected),
        "SHA256 checksum mismatch for {archive}; the installed CLI was not changed"
    );
    Ok(())
}

fn extract_pair(archive: &Path, prefix: &str, destination: &Path, zip: bool) -> Result<()> {
    let names = binary_names();
    let mut found = [false; 2];
    if zip {
        let mut archive = zip::ZipArchive::new(fs::File::open(archive)?)?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            for (i, name) in names.iter().enumerate() {
                if entry.name()? == format!("{prefix}/{name}") {
                    ensure!(
                        !entry.is_dir() && !entry.is_symlink(),
                        "release binary must be a regular file"
                    );
                    ensure!(!found[i], "duplicate release binary {name}");
                    write_binary(&mut entry, &destination.join(name))?;
                    found[i] = true;
                }
            }
        }
    } else {
        let decoded = flate2::read::GzDecoder::new(fs::File::open(archive)?);
        let mut archive = tar::Archive::new(decoded.take(MAX_ARCHIVE_BYTES * 3));
        for entry in archive.entries()? {
            let mut entry = entry?;
            for (i, name) in names.iter().enumerate() {
                if entry.path()?.as_ref() == Path::new(prefix).join(name) {
                    ensure!(
                        entry.header().entry_type().is_file(),
                        "release binary must be a regular file"
                    );
                    ensure!(!found[i], "duplicate release binary {name}");
                    write_binary(&mut entry, &destination.join(name))?;
                    found[i] = true;
                }
            }
        }
    }
    ensure!(
        found.iter().all(|present| *present),
        "release archive must contain both proa and cargo-proa; the installed CLI was not changed"
    );
    Ok(())
}

fn write_binary(reader: &mut impl Read, destination: &Path) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let size = io::copy(&mut reader.take(MAX_ARCHIVE_BYTES + 1), &mut file)?;
    ensure!(
        size > 0 && size <= MAX_ARCHIVE_BYTES,
        "invalid release binary size"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn validate_binaries(prepared: &Path, version: &Version) -> Result<()> {
    for name in binary_names() {
        let binary = prepared.join(name);
        ensure!(
            fs::symlink_metadata(&binary)?.file_type().is_file(),
            "release binary must be a regular file"
        );
        let output = Command::new(&binary)
            .arg("--version")
            .output()
            .with_context(|| {
                format!(
                    "cannot run downloaded binary `{}`; try proa update --build-from-source",
                    binary.display()
                )
            })?;
        ensure!(
            output.status.success(),
            "release binary `{}` could not run ({}): {}\nTry proa update --build-from-source. The installed CLI was not changed.",
            binary.display(), output.status, String::from_utf8_lossy(&output.stderr).trim()
        );
        ensure!(
            String::from_utf8_lossy(&output.stdout).trim() == format!("proa {version}"),
            "release binary `{}` did not report proa {version}; the installed CLI was not changed",
            binary.display()
        );
    }
    Ok(())
}

fn executable_directory(executable: &Path) -> Result<&Path> {
    ensure!(
        binary_names().iter().any(|name| executable
            .file_name()
            .is_some_and(|actual| actual == name.as_str())),
        "self-update requires an installed proa or cargo-proa executable"
    );
    executable
        .parent()
        .context("the installed CLI has no parent directory")
}

fn installed_version(executable: &Path) -> Result<Version> {
    let output = Command::new(executable)
        .arg("--version")
        .output()
        .context("read the currently installed CLI version")?;
    ensure!(
        output.status.success(),
        "the installed CLI could not report its version"
    );
    let output = String::from_utf8(output.stdout)?;
    let version = output
        .trim()
        .strip_prefix("proa ")
        .context("unrecognized installed CLI version")?;
    parse_version(version)
}

fn lock_and_read_installation(executable: &Path) -> Result<(fs::File, Version)> {
    let lock = lock_installation(executable)?;
    let version = installed_version(executable)?;
    Ok((lock, version))
}

fn lock_installation(executable: &Path) -> Result<fs::File> {
    let directory = executable_directory(executable)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(".proa-update.lock"))
        .context("the CLI installation directory is not writable")?;
    lock.try_lock()
        .context("another Proa update is running; wait for it to finish and retry")?;
    Ok(lock)
}

fn replace_pair(
    executable: &Path,
    prepared: &Path,
    replace_running: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<()> {
    let directory = executable_directory(executable)?;
    let peer_name = binary_names()
        .into_iter()
        .find(|name| executable.file_name().unwrap() != name.as_str())
        .unwrap();
    let peer = directory.join(&peer_name);
    let transaction = tempfile::Builder::new().prefix(".proa-update-").tempdir_in(directory)
        .context("the CLI installation directory is not writable; update it with the account that installed it")?;
    let next_peer = transaction.path().join("next-peer");
    fs::copy(prepared.join(&peer_name), &next_peer)?;
    let previous_executable = transaction.path().join("previous-running");
    fs::copy(executable, &previous_executable)?;
    let backup = transaction.path().join("previous-peer");
    let had_peer = match fs::symlink_metadata(&peer) {
        Ok(metadata) => {
            ensure!(
                metadata.file_type().is_file(),
                "the companion CLI must be a regular file"
            );
            fs::copy(&peer, &backup)?;
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    fs::rename(&next_peer, &peer)
        .context("replace the companion CLI; close other running Proa commands and retry")?;
    if let Err(error) = replace_running(&prepared.join(executable.file_name().unwrap())) {
        // Windows self-replacement moves the running image before copying its
        // replacement. Restore its pathname too if a later step fails.
        let running_restored = if executable.exists() {
            Ok(())
        } else {
            fs::rename(&previous_executable, executable)
        };
        let peer_restored = if had_peer {
            fs::rename(&backup, &peer)
        } else {
            fs::remove_file(&peer)
        };
        if let Err(restore_error) = running_restored.and(peer_restored) {
            let recovery = transaction.keep();
            bail!("self-update failed ({error}) and rollback failed ({restore_error}); recovery files kept at {}", recovery.display());
        }
        bail!("self-update failed; the installed binaries were restored: {error}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::TcpListener;
    use std::thread;

    fn install_pair(executable: &Path, prepared: &Path) -> Result<()> {
        install_pair_with(executable, prepared, |source| {
            replace_running_executable(executable, source)
        })
    }

    fn install_pair_with(
        executable: &Path,
        prepared: &Path,
        replace: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<()> {
        let _lock = lock_installation(executable)?;
        replace_pair(executable, prepared, replace)
    }

    fn fixture_server(responses: Vec<(String, u16, Vec<u8>)>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            for (path, status, body) in responses {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "missing request for {path}"
                            );
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut buffer = [0; 1024];
                    let size = stream.read(&mut buffer).unwrap();
                    assert!(size > 0 && request.len() < 8192);
                    request.extend_from_slice(&buffer[..size]);
                }
                assert!(String::from_utf8_lossy(&request)
                    .starts_with(&format!("GET {path} HTTP/1.1\r\n")));
                write!(
                    stream,
                    "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        (base, server)
    }

    fn test_client() -> Client {
        Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
    }

    fn archive_bytes(prefix: &str, zip: bool, include_peer: bool, link: bool) -> Vec<u8> {
        let names = binary_names();
        if zip {
            let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default();
            for (index, name) in names
                .iter()
                .enumerate()
                .take(if include_peer { 2 } else { 1 })
            {
                if link && index == 0 {
                    archive
                        .add_symlink(format!("{prefix}/{name}"), "../outside", options)
                        .unwrap();
                } else {
                    archive
                        .start_file(format!("{prefix}/{name}"), options)
                        .unwrap();
                    archive.write_all(name.as_bytes()).unwrap();
                }
            }
            archive.finish().unwrap().into_inner()
        } else {
            let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            let mut archive = tar::Builder::new(gzip);
            for (index, name) in names
                .iter()
                .enumerate()
                .take(if include_peer { 2 } else { 1 })
            {
                let mut header = tar::Header::new_gnu();
                header.set_mode(0o755);
                if link && index == 0 {
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_size(0);
                    header.set_link_name("../outside").unwrap();
                    header.set_cksum();
                    archive
                        .append_data(&mut header, format!("{prefix}/{name}"), io::empty())
                        .unwrap();
                } else {
                    header.set_size(name.len() as u64);
                    header.set_cksum();
                    archive
                        .append_data(&mut header, format!("{prefix}/{name}"), name.as_bytes())
                        .unwrap();
                }
            }
            archive.into_inner().unwrap().finish().unwrap()
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_updates_use_the_real_pair_and_stage_beside_it() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        for linked_peer in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let installation = temp.path().join("real installation");
            let links = temp.path().join("links");
            fs::create_dir(&installation).unwrap();
            fs::create_dir(&links).unwrap();
            let names = binary_names();
            let actual = installation.join(&names[0]);
            fs::write(&actual, "old running").unwrap();
            fs::set_permissions(&actual, fs::Permissions::from_mode(0o755)).unwrap();
            fs::write(installation.join(&names[1]), "old peer").unwrap();
            // Include a relative link chain: replacement must use the full
            // canonical destination, rather than following just one link.
            symlink(
                Path::new("../real installation").join(&names[0]),
                links.join("middle"),
            )
            .unwrap();
            symlink("middle", links.join(&names[0])).unwrap();
            if linked_peer {
                symlink(installation.join(&names[1]), links.join(&names[1])).unwrap();
            }
            let executable = resolve_executable(&links.join(&names[0])).unwrap();
            assert_eq!(executable, actual.canonicalize().unwrap());
            let staging = update_staging(&executable).unwrap();
            assert_eq!(
                staging.path().parent().unwrap(),
                installation.canonicalize().unwrap()
            );
            fs::write(staging.path().join(&names[0]), "new running").unwrap();
            fs::write(staging.path().join(&names[1]), "new peer").unwrap();
            install_pair(&executable, staging.path()).unwrap();
            assert_eq!(fs::read_to_string(&actual).unwrap(), "new running");
            assert_eq!(
                fs::read_to_string(installation.join(&names[1])).unwrap(),
                "new peer"
            );
            assert!(fs::symlink_metadata(links.join(&names[0]))
                .unwrap()
                .file_type()
                .is_symlink());
            if linked_peer {
                assert!(fs::symlink_metadata(links.join(&names[1]))
                    .unwrap()
                    .file_type()
                    .is_symlink());
            } else {
                assert!(!links.join(&names[1]).exists());
            }
            assert!(!links.join(".proa-update.lock").exists());
        }
    }

    #[test]
    fn latest_uses_cli_bridge_metadata_and_registry_fallback() {
        let client = test_client();
        let (base, server) = fixture_server(vec![(
            "/latest.json".into(),
            200,
            br#"{"version":"1.2.3"}"#.to_vec(),
        )]);
        assert_eq!(
            latest_version(&client, &base, &format!("{base}/registry")).unwrap(),
            Version::new(1, 2, 3)
        );
        server.join().unwrap();
        for response in [
            b"missing".to_vec(),
            br#"{"version":"1.2.3-main.abc"}"#.to_vec(),
        ] {
            let (base, server) = fixture_server(vec![
                ("/latest.json".into(), 200, response),
                (
                    "/registry".into(),
                    200,
                    br#"{"crate":{"max_stable_version":"1.4.0"}}"#.to_vec(),
                ),
            ]);
            assert_eq!(
                latest_version(&client, &base, &format!("{base}/registry")).unwrap(),
                Version::new(1, 4, 0)
            );
            server.join().unwrap();
        }
    }

    #[test]
    fn checksum_requires_one_matching_valid_digest() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("archive");
        fs::write(&path, b"abc").unwrap();
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        for separator in ["  ", " *"] {
            verify_checksum(&path, "archive", &format!("{hash}{separator}archive\n")).unwrap();
        }
        for sums in [
            format!("{hash}  different"),
            format!("{hash}  archive\n{hash}  archive"),
            format!("{}  archive", "0".repeat(64)),
            "invalid  archive".into(),
        ] {
            assert!(verify_checksum(&path, "archive", &sums).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn binary_validation_reports_loader_failures_and_the_source_update_option() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let names = binary_names();
        for name in &names {
            let binary = temp.path().join(name);
            fs::write(&binary, "#!/bin/sh\nprintf 'proa 1.2.3\\n'\n").unwrap();
            fs::set_permissions(binary, fs::Permissions::from_mode(0o755)).unwrap();
        }
        validate_binaries(temp.path(), &Version::new(1, 2, 3)).unwrap();
        fs::write(
            temp.path().join(&names[1]),
            "#!/bin/sh\nprintf 'GLIBC_2.39 not found\\n' >&2\nexit 127\n",
        )
        .unwrap();
        let error = validate_binaries(temp.path(), &Version::new(1, 2, 3))
            .unwrap_err()
            .to_string();
        assert!(error.contains("GLIBC_2.39 not found"));
        assert!(error.contains("--build-from-source"));
    }

    #[test]
    fn source_updates_can_bypass_archives_and_missing_archives_keep_the_exact_version() {
        let client = test_client();
        let temp = tempfile::tempdir().unwrap();
        let version = Version::new(1, 2, 3);
        let expected = temp.path().join("cargo/bin");
        let result = prepare_update_with_source(
            &client,
            "http://127.0.0.1:1",
            &version,
            temp.path(),
            true,
            |selected, staging| {
                assert_eq!(selected, &version);
                assert_eq!(staging, temp.path());
                Ok(expected.clone())
            },
        )
        .unwrap();
        assert_eq!(result, expected);

        if let Some(target) = release_target() {
            let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
            let (base, server) = fixture_server(vec![(
                format!("/{version}/proa-cli-{version}-{target}.{extension}"),
                404,
                Vec::new(),
            )]);
            let result = prepare_update_with_source(
                &client,
                &base,
                &version,
                temp.path(),
                false,
                |selected, _| {
                    assert_eq!(selected, &version);
                    Ok(expected.clone())
                },
            )
            .unwrap();
            assert_eq!(result, expected);
            server.join().unwrap();
        }
    }

    #[test]
    fn a_stale_update_reads_the_newly_installed_version_under_the_lock() {
        let temp = tempfile::Builder::new()
            .prefix("proa stale update ")
            .tempdir()
            .unwrap();
        let executable = temp.path().join(&binary_names()[0]);
        let source = temp.path().join("version_fixture.rs");
        fs::write(
            &source,
            r#"fn main() {
            let path = std::env::current_exe().unwrap().with_extension("version");
            println!("proa {}", std::fs::read_to_string(path).unwrap());
        }"#,
        )
        .unwrap();
        let compilation = Command::new("rustc")
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            compilation.status.success(),
            "{}",
            String::from_utf8_lossy(&compilation.stderr)
        );
        let stale_selection = Version::new(1, 2, 3);
        fs::write(executable.with_extension("version"), "1.3.0").unwrap();
        let (lock, current) = lock_and_read_installation(&executable).unwrap();
        assert_eq!(current, Version::new(1, 3, 0));
        assert!(!needs_update(&current, &stale_selection, false));
        assert!(needs_update(&current, &stale_selection, true));
        assert!(lock_and_read_installation(&executable).is_err());
        drop(lock);
        fs::write(executable.with_extension("version"), "1.4.0").unwrap();
        let (_lock, refreshed) = lock_and_read_installation(&executable).unwrap();
        assert_eq!(refreshed, Version::new(1, 4, 0));
        assert!(!needs_update(&refreshed, &current, false));
    }

    #[test]
    fn tar_and_zip_extract_both_binaries_and_reject_missing_files_or_links() {
        for zip in [false, true] {
            for (include_peer, link, success) in [
                (true, false, true),
                (false, false, false),
                (true, true, false),
            ] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("archive");
                fs::write(&path, archive_bytes("release", zip, include_peer, link)).unwrap();
                let output = temp.path().join("output");
                fs::create_dir(&output).unwrap();
                let result = extract_pair(&path, "release", &output, zip);
                assert_eq!(
                    result.is_ok(),
                    success,
                    "zip={zip}, peer={include_peer}, link={link}: {result:?}"
                );
                if success {
                    for name in binary_names() {
                        assert_eq!(fs::read(output.join(&name)).unwrap(), name.as_bytes());
                    }
                }
                assert!(!temp.path().join("outside").exists());
            }
        }
    }

    #[test]
    fn downloaded_archive_is_verified_before_extraction() {
        let Some(target) = release_target() else {
            return;
        };
        let version = Version::new(1, 2, 3);
        let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
        let prefix = format!("proa-cli-{version}-{target}");
        let archive = format!("{prefix}.{extension}");
        let bytes = archive_bytes(&prefix, cfg!(windows), true, false);
        let hash = format!("{:x}", base16ct::HexDisplay(&Sha256::digest(&bytes)));
        for correct in [true, false] {
            let hash = if correct {
                hash.clone()
            } else {
                "0".repeat(64)
            };
            let (base, server) = fixture_server(vec![
                (format!("/{version}/{archive}"), 200, bytes.clone()),
                (
                    format!("/{version}/proa-cli-{version}-SHA256SUMS"),
                    200,
                    format!("{hash}  {archive}\n").into_bytes(),
                ),
            ]);
            let temp = tempfile::tempdir().unwrap();
            let result = prepare_update(&test_client(), &base, &version, temp.path(), false);
            assert_eq!(result.is_ok(), correct, "{result:?}");
            assert_eq!(temp.path().join("extracted").exists(), correct);
            server.join().unwrap();
        }
    }

    #[test]
    fn replacement_failure_restores_the_companion() {
        for had_peer in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let names = binary_names();
            let executable = temp.path().join(&names[0]);
            let peer = temp.path().join(&names[1]);
            fs::write(&executable, "old running binary").unwrap();
            if had_peer {
                fs::write(&peer, "old companion").unwrap();
            }
            let prepared = temp.path().join("prepared");
            fs::create_dir(&prepared).unwrap();
            fs::write(prepared.join(&names[1]), "new companion").unwrap();
            let result = install_pair_with(&executable, &prepared, |_| {
                Err(io::Error::other("replacement failed"))
            });
            assert!(result.is_err());
            assert_eq!(
                fs::read_to_string(executable).unwrap(),
                "old running binary"
            );
            if had_peer {
                assert_eq!(fs::read_to_string(peer).unwrap(), "old companion");
            } else {
                assert!(!peer.exists());
            }
        }
    }

    #[test]
    fn a_failed_windows_style_move_restores_the_running_executable_path_too() {
        let temp = tempfile::tempdir().unwrap();
        let names = binary_names();
        let executable = temp.path().join(&names[0]);
        let peer = temp.path().join(&names[1]);
        fs::write(&executable, "old running binary").unwrap();
        fs::write(&peer, "old companion").unwrap();
        let prepared = temp.path().join("prepared");
        fs::create_dir(&prepared).unwrap();
        fs::write(prepared.join(&names[1]), "new companion").unwrap();
        let result = install_pair_with(&executable, &prepared, |_| {
            fs::rename(&executable, temp.path().join("moved-aside")).unwrap();
            Err(io::Error::other(
                "copy failed after moving the running image",
            ))
        });
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(executable).unwrap(),
            "old running binary"
        );
        assert_eq!(fs::read_to_string(peer).unwrap(), "old companion");
    }

    #[test]
    fn running_binary_and_its_companion_are_both_replaced() {
        const CHILD: &str = "PROA_TEST_RUNNING_UPDATE";
        if let Some(prepared) = std::env::var_os(CHILD) {
            install_pair(&std::env::current_exe().unwrap(), Path::new(&prepared)).unwrap();
            return;
        }
        let source = std::env::current_exe().unwrap();
        let names = binary_names();
        for running in 0..2 {
            let temp = tempfile::Builder::new()
                .prefix("proa update with spaces ")
                .tempdir()
                .unwrap();
            let prepared = temp.path().join("prepared");
            fs::create_dir(&prepared).unwrap();
            let executable = temp.path().join(&names[running]);
            let peer = temp.path().join(&names[1 - running]);
            fs::copy(&source, &executable).unwrap();
            fs::write(&peer, "old companion").unwrap();
            fs::copy(&source, prepared.join(&names[running])).unwrap();
            fs::write(prepared.join(&names[1 - running]), "new companion").unwrap();
            let status = Command::new(&executable)
                .args([
                    "--exact",
                    "commands::update::tests::running_binary_and_its_companion_are_both_replaced",
                    "--nocapture",
                ])
                .env(CHILD, &prepared)
                .status()
                .unwrap();
            assert!(status.success());
            assert_eq!(fs::read_to_string(peer).unwrap(), "new companion");
            assert!(Command::new(&executable)
                .arg("--list")
                .output()
                .unwrap()
                .status
                .success());
        }
    }

    #[cfg(windows)]
    #[test]
    fn locked_windows_companion_leaves_both_binaries_unchanged() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let names = binary_names();
        let executable = temp.path().join(&names[0]);
        let peer = temp.path().join(&names[1]);
        fs::write(&executable, "old executable").unwrap();
        fs::write(&peer, "old companion").unwrap();
        let prepared = temp.path().join("prepared");
        fs::create_dir(&prepared).unwrap();
        fs::write(prepared.join(&names[1]), "new companion").unwrap();
        let _lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&peer)
            .unwrap();
        let result = install_pair_with(&executable, &prepared, |_| {
            panic!("must not replace the running executable")
        });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(executable).unwrap(), "old executable");
        assert_eq!(fs::read_to_string(peer).unwrap(), "old companion");
    }
}
