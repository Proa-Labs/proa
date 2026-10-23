// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Preserve Cargo's ownership records when updating a Cargo-managed install.
//! Cargo locks and writes v1 before v2; use that same order and keep both locks
//! through binary replacement. Never create records for an installer-only CLI.

use super::*;
use std::io::{Seek, SeekFrom};

pub(super) fn with_updated_records(
    executable: &Path,
    prepared: &Path,
    version: &Version,
    install: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let Some(mut records) = Records::prepare(executable, prepared, version)? else {
        return install();
    };
    // Write metadata first so a write failure cannot leave new binaries with
    // old records. Cargo cannot observe these records until both locks drop.
    if let Err(error) = records.write_new().and_then(|()| install()) {
        records.restore().with_context(|| {
            format!("update failed ({error:#}); restoring Cargo installation records also failed")
        })?;
        return Err(error);
    }
    Ok(())
}

struct Records {
    v1: fs::File,
    v2: fs::File,
    old_v1: Vec<u8>,
    old_v2: Vec<u8>,
    new_v1: Vec<u8>,
    new_v2: Vec<u8>,
}

impl Records {
    fn prepare(executable: &Path, prepared: &Path, version: &Version) -> Result<Option<Self>> {
        let directory = executable_directory(executable)?;
        if directory.file_name().is_none_or(|name| name != "bin") {
            return Ok(None);
        }
        let root = directory
            .parent()
            .context("Cargo installation has no root")?;
        let mut v1 = match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join(".crates.toml"))
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("open Cargo installation records"),
        };
        lock_record(&v1)?;
        let mut old_v1 = Vec::new();
        v1.read_to_end(&mut old_v1)?;
        let mut listing: toml::Table = toml::from_str(std::str::from_utf8(&old_v1)?)
            .context("parse Cargo .crates.toml installation records")?;
        let Some(entries) = listing.get_mut("v1").and_then(toml::Value::as_table_mut) else {
            return Ok(None);
        };
        let names = binary_names();
        let mut managed = false;
        for (package, bins) in entries.iter() {
            let bins = bins
                .as_array()
                .context("invalid Cargo installed binary list")?;
            if bins.iter().any(|bin| {
                bin.as_str()
                    .is_some_and(|bin| names.iter().any(|name| name == bin))
            }) {
                ensure!(package.split_whitespace().next() == Some("proa-cli"),
                    "another Cargo package owns a Proa binary; use cargo install to resolve the installation");
                managed = true;
            }
        }
        if !managed {
            return Ok(None);
        }

        let mut v2 = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".crates2.json"))
            .context("open Cargo v2 installation records")?;
        lock_record(&v2)?;
        let mut old_v2 = Vec::new();
        v2.read_to_end(&mut old_v2)?;
        let mut details: serde_json::Value = if old_v2.is_empty() {
            serde_json::json!({"installs": {}})
        } else {
            serde_json::from_slice(&old_v2)
                .context("parse Cargo .crates2.json installation records")?
        };
        let installs = details
            .get_mut("installs")
            .and_then(serde_json::Value::as_object_mut)
            .context("invalid Cargo v2 installation records")?;

        // Remove only the binaries being replaced, retaining other packages,
        // unknown fields, and any additional binaries a record may own.
        entries.retain(|_, value| {
            let bins = value.as_array_mut().unwrap();
            bins.retain(|bin| {
                !bin.as_str()
                    .is_some_and(|bin| names.iter().any(|name| name == bin))
            });
            !bins.is_empty()
        });
        installs.retain(|_, info| {
            if let Some(bins) = info
                .get_mut("bins")
                .and_then(serde_json::Value::as_array_mut)
            {
                bins.retain(|bin| {
                    !bin.as_str()
                        .is_some_and(|bin| names.iter().any(|name| name == bin))
                });
                !bins.is_empty()
            } else {
                true
            }
        });
        let package =
            format!("proa-cli {version} (registry+https://github.com/rust-lang/crates.io-index)");
        let mut installed_bins = entries
            .remove(&package)
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        installed_bins.extend(names.iter().cloned().map(toml::Value::String));
        entries.insert(package.clone(), toml::Value::Array(installed_bins.clone()));

        // A source build has fresh Cargo-generated settings. For prebuilt
        // archives keep target/rustc unknown rather than claiming a local
        // build configuration that might incorrectly make Cargo skip a build.
        let staged = prepared
            .parent()
            .and_then(|root| fs::read(root.join(".crates2.json")).ok())
            .map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes))
            .transpose()
            .context("parse staged Cargo installation records")?;
        let mut info = staged
            .as_ref()
            .and_then(|value| value.get("installs"))
            .and_then(|value| value.get(&package))
            .cloned()
            .unwrap_or_else(|| {
                serde_json::json!({
                    "version_req": format!("={version}"), "bins": [], "features": [],
                    "all_features": false, "no_default_features": false,
                    "profile": "release", "target": null, "rustc": null
                })
            });
        info.as_object_mut()
            .context("invalid staged Cargo install information")?
            .insert("bins".into(), serde_json::to_value(&installed_bins)?);
        installs.insert(package, info);
        Ok(Some(Self {
            v1,
            v2,
            old_v1,
            old_v2,
            new_v1: toml::to_string(&listing)?.into_bytes(),
            new_v2: serde_json::to_vec(&details)?,
        }))
    }

    fn write_new(&mut self) -> Result<()> {
        write_record(&mut self.v1, &self.new_v1)?;
        write_record(&mut self.v2, &self.new_v2)?;
        Ok(())
    }

    fn restore(&mut self) -> Result<()> {
        // Attempt both even if restoring the first fails.
        let first = write_record(&mut self.v1, &self.old_v1);
        let second = write_record(&mut self.v2, &self.old_v2);
        first
            .and(second)
            .context("restore Cargo installation records")
    }
}

fn lock_record(file: &fs::File) -> Result<()> {
    file.try_lock()
        .context("Cargo is modifying installation records; wait for it to finish and retry")?;
    Ok(())
}

fn write_record(file: &mut fs::File, bytes: &[u8]) -> io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.write_all(bytes)?;
    file.set_len(bytes.len() as u64)?;
    file.sync_data()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "proa-cli 1.2.2 (registry+https://github.com/rust-lang/crates.io-index)";
    const NEW: &str = "proa-cli 1.2.3 (registry+https://github.com/rust-lang/crates.io-index)";
    const OTHER: &str = "other-tool 3.0.0 (registry+https://github.com/rust-lang/crates.io-index)";

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let names = binary_names();
        for name in &names {
            fs::write(bin.join(name), "old").unwrap();
        }
        let listing = toml::Table::from_iter([(
            "v1".into(),
            toml::Value::Table(toml::Table::from_iter([
                (
                    OLD.into(),
                    toml::Value::Array(names.into_iter().map(toml::Value::String).collect()),
                ),
                (
                    OTHER.into(),
                    toml::Value::Array(vec![toml::Value::String("other-tool".into())]),
                ),
            ])),
        )]);
        fs::write(
            temp.path().join(".crates.toml"),
            toml::to_string(&listing).unwrap(),
        )
        .unwrap();
        let info = serde_json::json!({
            "version_req": null, "bins": binary_names(), "features": [],
            "all_features": false, "no_default_features": false, "profile": "release",
            "target": null, "rustc": null
        });
        let mut details = serde_json::json!({"installs": {}, "future_top_level_field": 42});
        details["installs"][OLD] = info.clone();
        let mut other_info = info;
        other_info["bins"] = serde_json::json!(["other-tool"]);
        other_info["future_install_field"] = serde_json::json!("preserve");
        details["installs"][OTHER] = other_info;
        fs::write(
            temp.path().join(".crates2.json"),
            serde_json::to_vec(&details).unwrap(),
        )
        .unwrap();
        let prepared = temp.path().join("staging/cargo/bin");
        fs::create_dir_all(&prepared).unwrap();
        let executable = bin.join(&binary_names()[0]);
        (temp, executable, prepared)
    }

    #[test]
    fn updated_cargo_records_report_the_new_version_and_preserve_other_packages() {
        for source_build in [false, true] {
            let (temp, executable, prepared) = fixture();
            if source_build {
                let details = serde_json::json!({"installs": {(NEW): {
                    "version_req": "=1.2.3", "bins": binary_names(), "features": ["example"],
                    "all_features": false, "no_default_features": false, "profile": "release",
                    "target": "source-build-target", "rustc": "source-build-rustc", "future": 17
                }}});
                fs::write(
                    prepared.parent().unwrap().join(".crates2.json"),
                    serde_json::to_vec(&details).unwrap(),
                )
                .unwrap();
            }
            with_updated_records(&executable, &prepared, &Version::new(1, 2, 3), || Ok(()))
                .unwrap();
            let details: serde_json::Value =
                serde_json::from_slice(&fs::read(temp.path().join(".crates2.json")).unwrap())
                    .unwrap();
            assert!(details["installs"].get(OLD).is_none());
            assert_eq!(
                details["installs"][NEW]["bins"],
                serde_json::json!(binary_names())
            );
            assert_eq!(details["future_top_level_field"], 42);
            assert_eq!(
                details["installs"][OTHER]["future_install_field"],
                "preserve"
            );
            if source_build {
                assert_eq!(details["installs"][NEW]["target"], "source-build-target");
                assert_eq!(
                    details["installs"][NEW]["features"],
                    serde_json::json!(["example"])
                );
                assert_eq!(details["installs"][NEW]["future"], 17);
            } else {
                assert!(details["installs"][NEW]["target"].is_null());
            }
            // Exercise Cargo's real metadata parser instead of only our own.
            let output = Command::new("cargo")
                .args(["install", "--list", "--root"])
                .arg(temp.path())
                .current_dir(temp.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let listed = String::from_utf8(output.stdout).unwrap();
            assert!(listed.contains("proa-cli v1.2.3:"), "{listed}");
            assert!(!listed.contains("proa-cli v1.2.2:"), "{listed}");
            assert!(listed.contains("other-tool v3.0.0:"), "{listed}");
        }
    }

    #[test]
    fn failed_binary_replacement_restores_both_cargo_record_files() {
        let (temp, executable, prepared) = fixture();
        let v1 = temp.path().join(".crates.toml");
        let v2 = temp.path().join(".crates2.json");
        let previous_v1 = fs::read(&v1).unwrap();
        let previous_v2 = fs::read(&v2).unwrap();
        let error = with_updated_records(&executable, &prepared, &Version::new(1, 2, 3), || {
            bail!("replacement failed")
        })
        .unwrap_err();
        assert!(error.to_string().contains("replacement failed"));
        assert_eq!(fs::read(v1).unwrap(), previous_v1);
        assert_eq!(fs::read(v2).unwrap(), previous_v2);
        assert_eq!(fs::read_to_string(executable).unwrap(), "old");
    }

    #[test]
    fn busy_cargo_records_prevent_binary_replacement() {
        for filename in [".crates.toml", ".crates2.json"] {
            let (temp, executable, prepared) = fixture();
            let lock = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(temp.path().join(filename))
                .unwrap();
            lock.lock().unwrap();
            let error =
                with_updated_records(&executable, &prepared, &Version::new(1, 2, 3), || {
                    panic!("must not replace binaries while Cargo holds the metadata lock")
                })
                .unwrap_err();
            assert!(error.to_string().contains("Cargo is modifying"));
            assert_eq!(fs::read_to_string(executable).unwrap(), "old");
        }
    }

    #[test]
    fn installer_only_updates_do_not_create_cargo_records() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        with_updated_records(
            &bin.join(&binary_names()[0]),
            temp.path(),
            &Version::new(1, 2, 3),
            || Ok(()),
        )
        .unwrap();
        assert!(!temp.path().join(".crates.toml").exists());
        assert!(!temp.path().join(".crates2.json").exists());
    }
}
