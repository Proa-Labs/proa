// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Installation-aware updates through the original installer or Cargo.

use anyhow::{ensure, Context, Result};
use clap::Args;
use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;
use std::{fs, io::Read, path::Path, process::Command, time::Duration};

mod bundle;
mod installation;
pub(crate) use bundle::{install_self, InstallSelfArgs};

const DOWNLOAD_BASE: &str = "https://proa.so/downloads/cli";
const MAX_RESPONSE: u64 = 1024 * 1024;

#[derive(Debug, Args)]
pub(crate) struct UpdateArgs {
    /// Show the available version without changing files.
    #[arg(long)]
    check: bool,
    /// Select an exact stable release (also permits a downgrade).
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// Build a standalone update locally with Cargo.
    #[arg(long)]
    build_from_source: bool,
}

pub(crate) fn run(args: UpdateArgs) -> Result<()> {
    let requested = args
        .version
        .as_deref()
        .map(parse_requested_version)
        .transpose()?;
    if !args.check {
        ensure!(
            !cfg!(debug_assertions),
            "self-update is disabled in debug builds; use a release installation"
        );
    }
    let client = Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("proa/", env!("CARGO_PKG_VERSION")))
        .build()?;
    if args.check {
        let version = match requested {
            Some(version) => version,
            None => latest_version(&client)?,
        };
        println!(
            "Installed: {}\n{}: {version}",
            env!("CARGO_PKG_VERSION"),
            if args.version.is_some() {
                "Requested"
            } else {
                "Latest stable"
            }
        );
        return Ok(());
    }
    let executable = installation::external_path(&std::env::current_exe()?.canonicalize()?);
    match installation::detect(&executable)? {
        installation::Installation::Cargo { root } => cargo_update(&root, requested.as_ref()),
        installation::Installation::Standalone { root, bin_dir } => {
            run_installer(&client, &executable, &root, &bin_dir, &args)
        }
        installation::Installation::Unknown => anyhow::bail!("cannot identify who installed this CLI; rerun its original installer or package-manager command"),
    }
}

fn parse_requested_version(value: &str) -> Result<Version> {
    let normalized = value.strip_prefix("proa-cli-").unwrap_or(value);
    let version = Version::parse(normalized.strip_prefix('v').unwrap_or(normalized))
        .with_context(|| format!("invalid release version `{value}`"))?;
    ensure!(version.pre.is_empty() && version.build.is_empty(),
        "proa update --version supports stable releases only; use the Proa installer for main snapshots");
    Ok(version)
}

fn read_response(response: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response.take(MAX_RESPONSE + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_RESPONSE,
        "update response exceeds its size limit"
    );
    Ok(bytes)
}

fn latest_version(client: &Client) -> Result<Version> {
    #[derive(Deserialize)]
    struct Latest {
        version: String,
    }
    let from_bridge = (|| -> Result<Version> {
        let response = client
            .get(format!("{DOWNLOAD_BASE}/latest.json"))
            .send()?
            .error_for_status()?;
        let latest: Latest = serde_json::from_slice(&read_response(response)?)?;
        parse_requested_version(&latest.version)
    })();
    if let Ok(version) = from_bridge {
        return Ok(version);
    }
    #[derive(Deserialize)]
    struct Registry {
        #[serde(rename = "crate")]
        package: Package,
    }
    #[derive(Deserialize)]
    struct Package {
        max_stable_version: String,
    }
    let response = client
        .get("https://crates.io/api/v1/crates/proa-cli")
        .send()?
        .error_for_status()?;
    let registry: Registry = serde_json::from_slice(&read_response(response)?)?;
    parse_requested_version(&registry.package.max_stable_version)
}

fn cargo_update(root: &Path, version: Option<&Version>) -> Result<()> {
    let mut command = Command::new("cargo");
    command
        .args([
            "install",
            "--locked",
            "proa-cli",
            "--registry",
            "crates-io",
            "--root",
        ])
        .arg(root);
    if let Some(version) = version {
        command.args(["--version", &format!("={version}"), "--force"]);
    } else {
        command.args(["--version", &format!(">={}", env!("CARGO_PKG_VERSION"))]);
    }
    let cargo = installation::find_program("cargo")?;
    let arguments: Vec<_> = command.get_args().map(|arg| arg.to_owned()).collect();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = Command::new(cargo)
            .args(&arguments)
            .current_dir(root)
            .exec();
        Err(error).context("run Cargo to update its own installation")
    }
    #[cfg(not(unix))]
    {
        let quote =
            |value: &std::ffi::OsStr| format!("'{}'", value.to_string_lossy().replace('\'', "''"));
        anyhow::bail!(
            "Cargo owns this installation. Exit Proa, then run in PowerShell:\n& {} {}",
            quote(cargo.as_os_str()),
            arguments
                .iter()
                .map(|arg| quote(arg))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

fn run_installer(
    client: &Client,
    executable: &Path,
    root: &Path,
    bin_dir: &Path,
    args: &UpdateArgs,
) -> Result<()> {
    let version = match args.version.as_deref() {
        Some(version) => parse_requested_version(version)?,
        None => latest_version(client)?,
    };
    let staging = tempfile::Builder::new()
        .prefix(".update-")
        .tempdir_in(root)?;
    let filename = if cfg!(windows) {
        "install.ps1"
    } else {
        "install.sh"
    };
    let response = client
        .get(format!("https://proa.so/{filename}"))
        .send()?
        .error_for_status()?;
    let script = staging.path().join(filename);
    fs::write(&script, read_response(response)?)?;
    let program = if cfg!(windows) {
        "powershell.exe"
    } else {
        "sh"
    };
    let mut command = Command::new(installation::find_program(program)?);
    if cfg!(windows) {
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
    }
    command
        .arg(&script)
        .current_dir(staging.path())
        .env("PROA_INSTALL_HELPER", executable)
        .env("PROA_INSTALL_DIR", bin_dir)
        .env("PROA_DOWNLOAD_BASE", DOWNLOAD_BASE)
        .env("PROA_REPO", "Proa-Labs/proa")
        .env("PROA_VERSION", version.to_string())
        .env("PROA_NO_MODIFY_PATH", "1")
        .env("PROA_YES", "1")
        .env("PROA_TAILWIND", "skip")
        .env(
            "PROA_BUILD_FROM_SOURCE",
            if args.build_from_source { "1" } else { "" },
        )
        .env("PROA_STANDALONE_SOURCE", "1")
        .env("CARGO_TARGET_DIR", staging.path().join("build"))
        .env(
            "PROA_ALLOW_DOWNGRADE",
            if args.version.is_some() { "1" } else { "" },
        )
        .env_remove("PROA_PREBUILT_ONLY")
        .env_remove("PROA_TAILWIND_ONLY")
        .env("TMPDIR", staging.path())
        .env("TEMP", staging.path())
        .env("TMP", staging.path());
    ensure!(
        command
            .status()
            .context("run the Proa standalone installer")?
            .success(),
        "the Proa installer failed; see its output above"
    );
    Ok(())
}
