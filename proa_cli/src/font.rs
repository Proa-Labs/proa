// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `cargo proa font` — vendor and inspect self-hosted, metric-matched fonts.
//!
//! Fonts are declared in code with `google_font!` / `local_font!`; the build
//! collector in [`proa_fonts_build::collect`] vendors them during `cargo
//! build`. This command runs the same collector outside a build: `sync`
//! vendors and locks every declaration right now (or, with `--offline`,
//! verifies that `proa-fonts.lock` and the vendored files already satisfy
//! them, the CI gate), and `list` prints what the lock records.

use std::path::PathBuf;

use anyhow::{Context, Result};
use proa_fonts_build::collect::{self, CollectOptions, LOCK_FILE};
use proa_fonts_build::config::FontsLock;

/// Options for `font sync`.
pub struct SyncOptions {
    pub root: PathBuf,
    /// Verify only: fail instead of fetching when a declaration is not
    /// satisfied by the lock and vendored files.
    pub offline: bool,
    /// Override the vendored-files directory (defaults to `public/fonts`,
    /// matching the collector default). Must match the build script's
    /// `CollectOptions` when that is customized.
    pub fonts_dir: Option<PathBuf>,
    /// Override the URL prefix (defaults to `/fonts`).
    pub url_prefix: Option<String>,
}

/// Vendor and lock every font declared in the project's source.
pub fn sync(opts: SyncOptions) -> Result<()> {
    let collect_opts = CollectOptions {
        crate_root: opts.root.clone(),
        // No module emission outside a build script; `cargo build` owns that.
        out_dir: PathBuf::new(),
        fonts_out_dir: opts
            .fonts_dir
            .unwrap_or_else(|| PathBuf::from("public/fonts")),
        url_prefix: opts.url_prefix.unwrap_or_else(|| "/fonts".to_string()),
        lock_path: None,
    };
    let result = collect::sync(&collect_opts, opts.offline)
        .with_context(|| format!("syncing fonts under {}", opts.root.display()))?;

    for path in &result.skipped_sources {
        eprintln!(
            "warning: {} failed to parse; its declarations are preserved unverified",
            path.display()
        );
    }
    if result.reports.is_empty() && result.preserved.is_empty() {
        println!(
            "no google_font!/local_font! declarations found under {}",
            opts.root.join("src").display()
        );
        return Ok(());
    }
    for report in &result.reports {
        let status = if report.repaired {
            "re-vendored (integrity mismatch)"
        } else if report.refreshed {
            "vendored"
        } else {
            "verified"
        };
        println!(
            "font {key}: {family}, {n} file(s) {status}",
            key = report.key,
            family = report.family,
            n = report.files,
        );
    }
    for key in &result.preserved {
        println!("font {key}: preserved (source file did not parse)");
    }
    for filename in &result.removed {
        println!("removed stale {filename}");
    }
    Ok(())
}

/// Print the fonts recorded in `proa-fonts.lock`.
pub fn list(root: PathBuf, json: bool) -> Result<()> {
    let path = root.join(LOCK_FILE);
    let lock: FontsLock = if path.exists() {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
    } else {
        FontsLock::default()
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&lock)?);
        return Ok(());
    }
    if lock.fonts.is_empty() {
        println!(
            "no fonts locked (declare one with google_font!/local_font!, then run \
             `cargo proa font sync` or `cargo build`)"
        );
        return Ok(());
    }
    for (key, e) in &lock.fonts {
        println!(
            "{key:>24}  {family}  {n} file(s)  class `{class}`  var `{var}`",
            family = e.family,
            n = e.files.len(),
            class = e.class_name,
            var = e.css_variable,
        );
    }
    Ok(())
}
