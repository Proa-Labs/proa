// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use super::plan::SitePlan;

pub fn ensure_destination_available(destination: &Path) -> Result<()> {
    if !destination.exists() {
        return Ok(());
    }
    if !destination.is_dir() {
        bail!(
            "destination already exists and is not a directory: {}",
            destination.display()
        );
    }
    if fs::read_dir(destination)
        .with_context(|| format!("inspect {}", destination.display()))?
        .next()
        .is_some()
    {
        bail!("destination is not empty: {}", destination.display());
    }
    Ok(())
}

pub fn commit(plan: &SitePlan) -> Result<()> {
    ensure_destination_available(&plan.destination)?;
    let parent = plan
        .destination
        .parent()
        .context("project destination has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;

    let staging = staging_path(&plan.destination)?;
    if staging.exists() {
        bail!("staging path unexpectedly exists: {}", staging.display());
    }
    fs::create_dir(&staging).with_context(|| format!("create {}", staging.display()))?;

    let result = (|| -> Result<()> {
        for file in &plan.files {
            let target = staging.join(&file.path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            fs::write(&target, &file.content)
                .with_context(|| format!("write {}", target.display()))?;
        }
        format_rust(&staging, plan)?;
        refresh_component_hashes(&staging)?;
        validate_staging(&staging, plan)?;

        if plan.destination.exists() {
            fs::remove_dir(&plan.destination).with_context(|| {
                format!("remove empty destination {}", plan.destination.display())
            })?;
        }
        fs::rename(&staging, &plan.destination).with_context(|| {
            format!(
                "commit staged project {} to {}",
                staging.display(),
                plan.destination.display()
            )
        })?;
        Ok(())
    })();

    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn staging_path(destination: &Path) -> Result<PathBuf> {
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .context("project destination must have a UTF-8 final path segment")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(destination.with_file_name(format!(".{name}.proa-stage-{}-{stamp}", std::process::id())))
}

fn format_rust(staging: &Path, plan: &SitePlan) -> Result<()> {
    for file in plan.files.iter().filter(|file| {
        file.path
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("rs")
    }) {
        let target = staging.join(&file.path);
        let source = fs::read_to_string(&target)
            .with_context(|| format!("read generated {}", file.path.display()))?;
        let formatted = crate::fmt::format_file(&file.path, &source)?;
        if !formatted.diagnostics.is_empty() {
            bail!(
                "Proa formatting failed for generated {}: {:?}",
                file.path.display(),
                formatted.diagnostics
            );
        }
        let verification =
            crate::fmt::verify_render_preserving(&file.path, &source, &formatted.output);
        if !verification.is_empty() {
            bail!(
                "Proa formatting changed render semantics for generated {}: {:?}",
                file.path.display(),
                verification
            );
        }
        if formatted.output != source {
            fs::write(&target, &formatted.output)
                .with_context(|| format!("format generated {}", file.path.display()))?;
        }
        let status = match Command::new("rustfmt")
            .arg("--edition")
            .arg("2021")
            .arg(&target)
            .status()
        {
            Ok(status) => status,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("start rustfmt for generated sources"),
        };
        if !status.success() {
            bail!("rustfmt failed for generated {}", file.path.display());
        }
    }
    Ok(())
}

fn validate_staging(staging: &Path, plan: &SitePlan) -> Result<()> {
    for file in &plan.files {
        let target = staging.join(&file.path);
        let metadata = fs::metadata(&target)
            .with_context(|| format!("validate staged {}", target.display()))?;
        if !metadata.is_file() {
            bail!("staged output is not a regular file: {}", target.display());
        }
    }
    Ok(())
}

fn refresh_component_hashes(staging: &Path) -> Result<()> {
    let path = staging.join(crate::config::LOCK_FILE);
    let mut lock: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))?;
    let Some(components) = lock
        .get_mut("components")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok(());
    };
    for component in components.values_mut() {
        let Some(files) = component
            .get_mut("files")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        let mut aggregate = Vec::new();
        for (relative, hash) in files.iter_mut() {
            let bytes = fs::read(staging.join(relative))
                .with_context(|| format!("hash installed component file {relative}"))?;
            let value = format!("sha256:{:x}", Sha256::digest(bytes));
            *hash = serde_json::Value::String(value.clone());
            aggregate.push((relative.clone(), value));
        }
        aggregate.sort_by(|left, right| left.0.cmp(&right.0));
        let joined = aggregate
            .iter()
            .map(|(relative, hash)| format!("{relative}:{hash}"))
            .collect::<Vec<_>>()
            .join("\n");
        let value =
            serde_json::Value::String(format!("sha256:{:x}", Sha256::digest(joined.as_bytes())));
        component["sourceHash"] = value.clone();
        component["localHashAtInstall"] = value;
    }
    fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&lock)?))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn initialize_git(root: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root)
        .status()
        .context("start git init")?;
    if !status.success() {
        bail!("git init failed with {status}");
    }
    Ok(())
}
