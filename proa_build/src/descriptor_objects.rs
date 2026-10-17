// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Complete descriptor carriers without depending on rustc's private cache.
//!
//! An rlib is already an authoritative bundle of all codegen-unit objects. For
//! native executable/dynamic outputs, the linker adapter captures the exact
//! object arguments supplied by rustc before those temporary files disappear.
//! Bundles are ordinary archives read by rsjs_transport's existing extractor.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use thiserror::Error;

pub(super) const LINKER_MODE: &str = "descriptor-linker-v1";
pub(super) const LINKER_PROGRAM_ENV: &str = "PROA_RSJS_DESCRIPTOR_LINKER_PROGRAM";
pub(super) const LINKER_CARRIER_ENV: &str = "PROA_RSJS_DESCRIPTOR_LINKER_CARRIER";

#[derive(Debug, Error)]
pub(super) enum CarrierError {
    #[error("descriptor linker environment is missing `{0}`")]
    MissingEnvironment(&'static str),
    #[error("descriptor linker supplied no explicit native object inputs")]
    NoObjects,
    #[error(
        "descriptor linker response/file-list inputs require an explicit supported adapter: `{0:?}`"
    )]
    IndirectInputs(OsString),
    #[error("descriptor object input `{path}` is not a regular file")]
    UnsafeInput { path: PathBuf },
    #[error("descriptor carrier I/O at `{path}`: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("failed to invoke descriptor native linker `{program:?}`: {source}")]
    Spawn {
        program: OsString,
        source: io::Error,
    },
}

/// The adapter forwards every argument unchanged; the archive is only a
/// descriptor carrier and never replaces or participates in the native link.
pub(super) fn run_linker(args: &[OsString]) -> Result<ExitStatus, CarrierError> {
    let program = std::env::var_os(LINKER_PROGRAM_ENV)
        .ok_or(CarrierError::MissingEnvironment(LINKER_PROGRAM_ENV))?;
    let carrier = std::env::var_os(LINKER_CARRIER_ENV)
        .map(PathBuf::from)
        .ok_or(CarrierError::MissingEnvironment(LINKER_CARRIER_ENV))?;
    let objects = explicit_linker_objects(args)?;
    write_archive(&carrier, &objects)?;
    let mut command = Command::new(&program);
    command.args(args);
    super::build::scrub_wrapper_control_environment(&mut command);
    command
        .status()
        .map_err(|source| CarrierError::Spawn { program, source })
}

fn explicit_linker_objects(args: &[OsString]) -> Result<Vec<PathBuf>, CarrierError> {
    let mut objects = Vec::new();
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        if argument == "-o" {
            arguments.next();
            continue;
        }
        let text = argument.to_string_lossy();
        // Failing closed is preferable to silently publishing an incomplete
        // carrier. Add platform-specific response parsers with their own tests.
        let indirect_driver_input = text.strip_prefix("-Wl,").is_some_and(|value| {
            value
                .split(',')
                .any(|argument| argument.starts_with('@') || argument.starts_with("-filelist"))
        });
        if text.starts_with('@') || text.starts_with("-filelist") || indirect_driver_input {
            return Err(CarrierError::IndirectInputs(argument.clone()));
        }
        if text.starts_with('-') {
            continue;
        }
        let path = PathBuf::from(argument);
        if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("o" | "obj")
        ) {
            let metadata = fs::symlink_metadata(&path).map_err(|source| CarrierError::Io {
                path: path.clone(),
                source,
            })?;
            if !metadata.file_type().is_file() {
                return Err(CarrierError::UnsafeInput { path });
            }
            if !objects.contains(&path) {
                objects.push(path);
            }
        }
    }
    if objects.is_empty() {
        return Err(CarrierError::NoObjects);
    }
    Ok(objects)
}

fn write_archive(carrier: &Path, objects: &[PathBuf]) -> Result<(), CarrierError> {
    // A resumed staging directory keeps the carrier from an earlier link of
    // this unit. Remove it (a symlink is unlinked, never followed) so
    // `create_new` still refuses anything that appears concurrently.
    match fs::remove_file(carrier) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(CarrierError::Io {
                path: carrier.to_path_buf(),
                source,
            })
        }
    }
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(carrier)
        .map_err(|source| CarrierError::Io {
            path: carrier.to_path_buf(),
            source,
        })?;
    let write_result = (|| -> Result<(), CarrierError> {
        output
            .write_all(b"!<arch>\n")
            .map_err(|source| CarrierError::Io {
                path: carrier.to_path_buf(),
                source,
            })?;
        for (index, path) in objects.iter().enumerate() {
            let bytes = fs::read(path).map_err(|source| CarrierError::Io {
                path: path.clone(),
                source,
            })?;
            // Short generated names avoid host paths, response syntax, and
            // archive-name truncation. Member order is rustc's argument order.
            let name = format!("o{index:08}.o/");
            let header = format!(
                "{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
                0,
                0,
                0,
                "100644",
                bytes.len()
            );
            output
                .write_all(header.as_bytes())
                .and_then(|()| output.write_all(&bytes))
                .and_then(|()| {
                    if bytes.len() % 2 == 1 {
                        output.write_all(b"\n")
                    } else {
                        Ok(())
                    }
                })
                .map_err(|source| CarrierError::Io {
                    path: carrier.to_path_buf(),
                    source,
                })?;
        }
        output.sync_all().map_err(|source| CarrierError::Io {
            path: carrier.to_path_buf(),
            source,
        })
    })();
    drop(output);
    if write_result.is_err() {
        let _ = fs::remove_file(carrier);
    }
    write_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::read::archive::ArchiveFile;

    #[test]
    fn archive_contains_every_exact_input_in_order_with_padding() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("first.o");
        let b = temp.path().join("second.o");
        fs::write(&a, b"odd").unwrap();
        fs::write(&b, b"even").unwrap();
        let carrier = temp.path().join("bundle.o");
        write_archive(&carrier, &[a, b]).unwrap();
        let bytes = fs::read(&carrier).unwrap();
        let archive = ArchiveFile::parse(bytes.as_slice()).unwrap();
        let members = archive
            .members()
            .map(|member| member.unwrap().data(bytes.as_slice()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(members, vec![b"odd".as_slice(), b"even".as_slice()]);
    }

    #[test]
    fn relinking_a_resumed_unit_replaces_its_previous_carrier() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.o");
        let second = temp.path().join("second.o");
        fs::write(&first, b"old").unwrap();
        fs::write(&second, b"new!").unwrap();
        let carrier = temp.path().join("bundle.o");
        write_archive(&carrier, &[first]).unwrap();
        write_archive(&carrier, std::slice::from_ref(&second)).unwrap();
        let bytes = fs::read(&carrier).unwrap();
        let archive = ArchiveFile::parse(bytes.as_slice()).unwrap();
        let members = archive
            .members()
            .map(|member| member.unwrap().data(bytes.as_slice()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(members, vec![b"new!".as_slice()]);
    }

    #[test]
    fn indirect_linker_inputs_are_rejected_instead_of_omitted() {
        for argument in [
            "@args.rsp",
            "-filelist",
            "-filelist=/tmp/list",
            "-Wl,-filelist,/tmp/list",
            "-Wl,-dead_strip,@args.rsp",
        ] {
            assert!(matches!(
                explicit_linker_objects(&[argument.into()]),
                Err(CarrierError::IndirectInputs(_))
            ));
        }
        assert!(matches!(
            explicit_linker_objects(&["-lfoo".into()]),
            Err(CarrierError::NoObjects)
        ));
    }
}
