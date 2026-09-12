// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Cargo-compatible registry authentication without handling secrets itself.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde::Serialize;

#[derive(Debug, Args)]
pub(crate) struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// Delegate token storage to Cargo's configured credential provider.
    Login,
    /// Check registry protocol, credential provider, and credential availability.
    Status(AuthStatusArgs),
}

#[derive(Debug, Args)]
struct AuthStatusArgs {
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusReport {
    registry_url: String,
    sparse_protocol: bool,
    credential_provider: bool,
    credential_available: bool,
    credential_source: &'static str,
    ci_environment_supported: bool,
    ready: bool,
}

pub(crate) fn run(args: AuthArgs) -> Result<()> {
    match args.command {
        AuthCommand::Login => login(),
        AuthCommand::Status(args) => status(args),
    }
}

fn login() -> Result<()> {
    let status = Command::new("cargo")
        .args(["login", "--registry", "proa"])
        .status()
        .context("start `cargo login --registry proa`")?;
    if !status.success() {
        bail!("Cargo registry login failed with {status}");
    }
    println!("Proa registry credential stored by Cargo");
    Ok(())
}

fn status(args: AuthStatusArgs) -> Result<()> {
    let project_config = args
        .root
        .or_else(|| std::env::current_dir().ok())
        .map(|root| root.join(".cargo/config.toml"));
    let cargo_root = cargo_root();
    let cargo_config_toml = cargo_root.as_ref().map(|root| root.join("config.toml"));
    let cargo_config_legacy = cargo_root.as_ref().map(|root| root.join("config"));
    let sources = [
        project_config.as_deref(),
        cargo_config_toml.as_deref(),
        cargo_config_legacy.as_deref(),
    ];
    let mut registry_url = String::new();
    let mut credential_provider = false;
    for source in sources.into_iter().flatten() {
        if let Ok(contents) = fs::read_to_string(source) {
            if let Ok(value) = contents.parse::<toml::Value>() {
                registry_url = value
                    .get("registries")
                    .and_then(|registries| registries.get("proa"))
                    .and_then(|registry| registry.get("index"))
                    .and_then(toml::Value::as_str)
                    .unwrap_or(&registry_url)
                    .to_string();
                credential_provider |= value
                    .get("registry")
                    .and_then(|registry| registry.get("global-credential-providers"))
                    .and_then(toml::Value::as_array)
                    .is_some_and(|providers| {
                        providers
                            .iter()
                            .any(|provider| provider.as_str() == Some("cargo:token"))
                    });
            }
        }
    }
    let env_credential = std::env::var_os("CARGO_REGISTRIES_PROA_TOKEN").is_some();
    let stored_credential = cargo_root
        .as_ref()
        .is_some_and(|root| cargo_credentials_contain_proa(root));
    let credential_source = if env_credential {
        "environment (redacted)"
    } else if stored_credential {
        "Cargo credential store (redacted)"
    } else {
        "none"
    };
    let sparse_protocol = registry_url.starts_with("sparse+");
    let credential_available = env_credential || stored_credential;
    let report = StatusReport {
        registry_url,
        sparse_protocol,
        credential_provider,
        credential_available,
        credential_source,
        ci_environment_supported: true,
        ready: sparse_protocol && credential_provider && credential_available,
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Registry    {}",
            if report.sparse_protocol {
                "sparse protocol ready"
            } else {
                "missing or incompatible"
            }
        );
        println!(
            "Provider    {}",
            if report.credential_provider {
                "cargo:token"
            } else {
                "missing"
            }
        );
        println!("Credential  {}", report.credential_source);
        println!("CI          CARGO_REGISTRIES_PROA_TOKEN supported");
        if !report.ready {
            println!("\nNext: proa auth login");
        }
    }
    if !report.sparse_protocol || !report.credential_provider {
        bail!("Proa registry configuration is incomplete");
    }
    Ok(())
}

fn cargo_root() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cargo")))
}

fn cargo_credentials_contain_proa(root: &Path) -> bool {
    ["credentials.toml", "credentials"].into_iter().any(|name| {
        fs::read_to_string(root.join(name))
            .ok()
            .and_then(|source| source.parse::<toml::Value>().ok())
            .and_then(|value| {
                value
                    .get("registries")
                    .and_then(|registries| registries.get("proa"))
                    .and_then(|registry| registry.get("token"))
                    .cloned()
            })
            .is_some()
    })
}
