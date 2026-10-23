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

const DOWNLOAD_BASE: &str = "https://proa.so/downloads/cli";
const REGISTRY_METADATA: &str = "https://crates.io/api/v1/crates/proa-cli";
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Args)]
pub(crate) struct UpdateArgs {
    /// Show the available version without changing any files.
    #[arg(long)]
    check: bool,
    /// Install an exact published version (also permits an explicit downgrade).
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
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
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("proa/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let requested = args.version.as_deref().map(parse_version).transpose()?;
    let version = match requested {
        Some(version) => version,
        None => latest_version(&client, DOWNLOAD_BASE, REGISTRY_METADATA)?,
    };
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
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
    if args.version.is_none() && version <= current {
        println!("No update needed.");
        return Ok(());
    }

    let executable = std::env::current_exe().context("locate the installed Proa CLI")?;
    executable_directory(&executable)?;
    let staging = tempfile::Builder::new().prefix("proa-update-").tempdir()?;
    let prepared = prepare_update(&client, DOWNLOAD_BASE, &version, staging.path())?;
    validate_binaries(&prepared, &version)?;
    install_pair(&executable, &prepared)?;
    println!("Updated proa and cargo-proa to {version}. Run proa --version to verify.");
    Ok(())
}

fn parse_version(value: &str) -> Result<Version> {
    let normalized = value.strip_prefix("proa-cli-").unwrap_or(value);
    Version::parse(normalized.strip_prefix('v').unwrap_or(normalized)).with_context(|| {
        format!("invalid release version `{value}`; expected a version such as 0.1.3")
    })
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
) -> Result<PathBuf> {
    if let Some(target) = release_target() {
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

    println!("No prebuilt archive for this platform. Building proa-cli {version} from crates.io; this can take several minutes.");
    let cargo_root = staging.join("cargo");
    let status = Command::new("cargo")
        .current_dir(staging)
        .env("CARGO_TARGET_DIR", staging.join("build"))
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
            .with_context(|| format!("verify downloaded binary `{}`", binary.display()))?;
        ensure!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).trim() == format!("proa {version}"),
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

fn install_pair(executable: &Path, prepared: &Path) -> Result<()> {
    install_pair_with(executable, prepared, |source| {
        self_replace::self_replace(source)
    })
}

fn install_pair_with(
    executable: &Path,
    prepared: &Path,
    replace_running: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<()> {
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

    #[test]
    fn release_versions_are_normalized_and_cannot_inject_paths_or_arguments() {
        for input in ["1.2.3", "v1.2.3", "proa-cli-v1.2.3"] {
            assert_eq!(parse_version(input).unwrap(), Version::new(1, 2, 3));
        }
        for input in ["../1.2.3", "1.2.3/other", "--help", "latest", "1.2.3\n", ""] {
            assert!(parse_version(input).is_err(), "{input}");
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
            let result = prepare_update(&test_client(), &base, &version, temp.path());
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
    fn concurrent_updates_cannot_modify_the_same_installation() {
        let temp = tempfile::tempdir().unwrap();
        let lock = fs::File::create(temp.path().join(".proa-update.lock")).unwrap();
        lock.lock().unwrap();
        let executable = temp.path().join(&binary_names()[0]);
        let result = install_pair_with(&executable, temp.path(), |_| {
            panic!("must not replace any binary")
        });
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("another Proa update"));
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
