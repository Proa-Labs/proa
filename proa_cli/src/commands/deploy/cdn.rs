// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Deployment-time invalidation selected by deploy.cdn, never by route code.

use std::{
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use clap::Args;
use reqwest::{
    blocking::Client,
    header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    redirect::Policy,
};
use serde::Deserialize;

use crate::{config, util::resolve_config_root};

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Project root containing proa.config.json and its deploy.cdn configuration.
    #[arg(long)]
    root: Option<PathBuf>,

    /// Command that exits successfully only after rollout and traffic switching.
    /// Upload-only or detached deployment commands are not sufficient.
    #[arg(last = true, required = true, num_args = 1..)]
    command: Vec<OsString>,
}

#[derive(Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase", deny_unknown_fields)]
enum CdnConfig {
    Cloudflare {
        #[serde(rename = "zoneId")]
        zone_id: String,
        hostname: String,
    },
    Command {
        command: Vec<String>,
    },
}

enum Invalidation {
    Cloudflare {
        client: Client,
        request: reqwest::blocking::Request,
        hostname: String,
    },
    Command(Vec<String>),
}

pub fn run(args: RunArgs) -> Result<()> {
    let root = resolve_config_root(args.root)?;
    let project = config::load_project_config(&root)?;
    let cdn = project.deploy.as_ref().and_then(|deploy| deploy.get("cdn"))
        .context("configure deploy.cdn in proa.config.json before using proa deploy run; alternatively, use your hosting platform's deployment invalidation")?;
    let cdn: CdnConfig = serde_json::from_value(cdn.clone()).context(
        "invalid deploy.cdn configuration; supported providers are cloudflare and command",
    )?;
    // Prepare the selected integration before changing the running deployment.
    let invalidation = prepare(cdn)?;
    let (program, arguments) = args
        .command
        .split_first()
        .context("a blocking deployment command is required after --")?;
    let status = Command::new(program)
        .args(arguments)
        .current_dir(&root)
        .status()
        .context("could not start deployment command; CDN invalidation was not requested")?;
    if !status.success() {
        bail!("deployment command failed ({status}); CDN invalidation was not requested");
    }
    invalidate(invalidation, &root)
}

fn prepare(config: CdnConfig) -> Result<Invalidation> {
    match config {
        CdnConfig::Command { command } => {
            if command
                .first()
                .is_none_or(|program| program.trim().is_empty())
                || command.iter().any(|arg| arg.contains('\0'))
            {
                bail!("deploy.cdn.command must contain an executable followed by its arguments");
            }
            Ok(Invalidation::Command(command))
        }
        CdnConfig::Cloudflare { zone_id, hostname } => {
            if zone_id.len() != 32 || !zone_id.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("deploy.cdn.zoneId must be a 32-character hexadecimal Cloudflare zone ID");
            }
            if hostname.is_empty()
                || hostname.len() > 253
                || !hostname.contains('.')
                || !hostname.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                bail!("deploy.cdn.hostname must be a DNS hostname without a scheme, port, path, or wildcard");
            }
            let token = std::env::var("CLOUDFLARE_API_TOKEN").context(
                "set CLOUDFLARE_API_TOKEN in the deployment job for the Cloudflare integration",
            )?;
            if token.trim().is_empty() {
                bail!("CLOUDFLARE_API_TOKEN cannot be empty");
            }
            let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
                .context("CLOUDFLARE_API_TOKEN is not a valid HTTP header value")?;
            authorization.set_sensitive(true);
            let client = Client::builder()
                .redirect(Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .build()?;
            // Hostname scope removes the need for application-generated cache tags.
            // This includes cached assets on that hostname, not other zone hosts.
            let request = client
                .post(format!(
                    "https://api.cloudflare.com/client/v4/zones/{zone_id}/purge_cache"
                ))
                .header(AUTHORIZATION, authorization)
                .header(CONTENT_TYPE, "application/json")
                .body(serde_json::to_vec(
                    &serde_json::json!({ "hosts": [&hostname] }),
                )?)
                .build()?;
            Ok(Invalidation::Cloudflare {
                client,
                request,
                hostname,
            })
        }
    }
}

fn invalidate(invalidation: Invalidation, root: &Path) -> Result<()> {
    match invalidation {
        Invalidation::Command(command) => {
            let status = Command::new(&command[0]).args(&command[1..]).current_dir(root).status()
                .context("deployment succeeded, but the CDN invalidation command could not start; old pages may remain cached")?;
            if !status.success() {
                bail!("deployment succeeded, but CDN invalidation failed ({status}); old pages may remain cached");
            }
            println!("Deployment completed; the configured CDN invalidation command succeeded.");
        }
        Invalidation::Cloudflare {
            client,
            request,
            hostname,
        } => {
            let response = client.execute(request).context(
                "deployment succeeded, but Cloudflare purge failed; old pages may remain cached",
            )?;
            let status = response.status();
            if !status.is_success() {
                bail!("deployment succeeded, but Cloudflare purge returned HTTP {status}; old pages may remain cached");
            }
            let mut body = Vec::new();
            response.take(65_537).read_to_end(&mut body).context(
                "deployment succeeded, but the Cloudflare purge response could not be read",
            )?;
            if body.len() > 65_536 {
                bail!("deployment succeeded, but the Cloudflare purge response was too large");
            }
            let result: serde_json::Value = serde_json::from_slice(&body).context(
                "deployment succeeded, but Cloudflare returned an invalid purge response",
            )?;
            if result.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
                bail!("deployment succeeded, but Cloudflare rejected the purge; check the token's Cache Purge permission and zone scope");
            }
            println!(
                "Deployment completed; Cloudflare accepted cache invalidation for {hostname}."
            );
        }
    }
    Ok(())
}
