// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Handing the linked modules to the build tool.
//!
//! The JavaScript an application serves exists only inside its executable:
//! it is assembled when the final crate graph is linked. A deployable output
//! that a static host can serve needs those files on disk, so the build tool
//! starts the executable it just built with [`EXPORT_ENV`] naming a
//! directory. The request is answered before `main` runs: the registered
//! exporters write their files there and the process exits. None of the
//! application's own startup code runs, so an export needs no database, no
//! configuration, and no open port.
//!
//! An executable that can answer carries [`SENTINEL`] in its bytes. The
//! build tool looks for it first and never starts one that does not.

use std::path::Path;

pub use rsjs_transport::framework_export::{ENV as EXPORT_ENV, INDEX_FILE, SENTINEL};

/// What one host wrote into the export directory.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Exported {
    /// Identifies the linked sources the files were made from. The host
    /// compares it at startup to decide whether stored files are its own.
    pub source: String,
    /// `(name, path)` for each file, with `path` relative to the directory.
    pub files: Vec<(String, String)>,
}

/// A function that writes one host's files into the export directory.
pub type Exporter = fn(&Path) -> Result<Exported, String>;

/// Hosts that publish linked modules register here.
#[linkme::distributed_slice]
pub static EXPORTERS: [Exporter] = [..];

#[used]
static SENTINEL_IN_EXECUTABLE: &[u8] = SENTINEL;

/// Runs before `main` on the platforms the build tool supports.
#[used]
#[cfg_attr(
    any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly",
        target_os = "illumos"
    ),
    link_section = ".init_array"
)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__mod_init_func")]
#[cfg_attr(windows, link_section = ".CRT$XCU")]
static ANSWER_BEFORE_MAIN: extern "C" fn() = answer;

extern "C" fn answer() {
    // Read the sentinel so it stays in the executable with this function.
    let Some(directory) =
        std::env::var_os(EXPORT_ENV).filter(|_| !SENTINEL_IN_EXECUTABLE.is_empty())
    else {
        return;
    };
    let code = match export(Path::new(&directory)) {
        Ok(()) => 0,
        Err(reason) => {
            eprintln!("could not export framework assets: {reason}");
            1
        }
    };
    std::process::exit(code);
}

fn export(directory: &Path) -> Result<(), String> {
    let mut exported = Vec::new();
    for exporter in EXPORTERS {
        exported.push(exporter(directory)?);
    }
    let index = serde_json::to_vec(&exported).map_err(|error| error.to_string())?;
    std::fs::write(directory.join(INDEX_FILE), index).map_err(|error| error.to_string())
}
