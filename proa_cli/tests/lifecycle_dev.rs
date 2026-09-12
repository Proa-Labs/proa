// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn run_dev(
    root: &Path,
    target: &Path,
    extra_cargo_args: &[&str],
    rustflags: Option<&str>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-proa"));
    command
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target)
        .args([
            "dev",
            "--root",
            root.to_str().unwrap(),
            "--no-assets",
            "--no-restart",
            "--",
            "cargo",
            "run",
            "-p",
            "proa_linked_dev_fixture",
            "--bin",
            "proa_linked_dev_fixture",
            "--offline",
        ])
        .args(extra_cargo_args)
        .args(["--", "--probe"]);
    if let Some(rustflags) = rustflags {
        command.env("RUSTFLAGS", rustflags);
    } else {
        command.env_remove("RUSTFLAGS");
    }
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn final_plan(output: &Output) -> String {
    stdout(output)
        .lines()
        .find_map(|line| line.strip_prefix("FINAL|1|"))
        .and_then(|line| line.strip_suffix("|--probe"))
        .unwrap_or_else(|| panic!("missing final-phase child output:\n{}", stdout(output)))
        .to_string()
}

#[test]
fn cargo_proa_dev_reuses_stable_cycles_and_invalidates_source_feature_and_cfg_inputs() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\n\
         name = \"proa_linked_dev_fixture\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\n\
         [workspace]\n\n\
         [features]\n\
         alternate = []\n",
    )
    .unwrap();
    let write_source = |marker: &str| {
        fs::write(
            temp.path().join("src/main.rs"),
            format!(
                r#"
#![allow(unexpected_cfgs)]

#[cfg(not(rsjs_final))]
fn main() {{}}

#[cfg(rsjs_final)]
fn main() {{
    let argument = std::env::args().nth(1).unwrap_or_default();
    println!(
        "FINAL|{{}}|{{}}|{{}}",
        env!("RSJS_FINAL_PHASE"),
        env!("RSJS_PLAN_ID"),
        argument,
    );
    let _marker = {marker:?};
}}
"#
            ),
        )
        .unwrap();
    };
    write_source("first");

    let target = temp.path().join("outer-target");
    let first = run_dev(temp.path(), &target, &[], None);
    assert!(
        first.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&first),
        String::from_utf8_lossy(&first.stderr)
    );
    let first_plan = final_plan(&first);
    assert!(
        stdout(&first).contains("descriptor cache: 0/1"),
        "{}",
        stdout(&first)
    );

    let unchanged = run_dev(temp.path(), &target, &[], None);
    assert!(
        unchanged.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&unchanged),
        String::from_utf8_lossy(&unchanged.stderr)
    );
    assert_eq!(final_plan(&unchanged), first_plan);
    assert!(
        stdout(&unchanged).contains("descriptor cache: 1/1"),
        "{}",
        stdout(&unchanged)
    );
    assert!(
        stdout(&unchanged).contains("final cache: 1/1 reused"),
        "{}",
        stdout(&unchanged)
    );

    write_source("source changed");
    let source_changed = run_dev(temp.path(), &target, &[], None);
    assert!(
        source_changed.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&source_changed),
        String::from_utf8_lossy(&source_changed.stderr)
    );
    let source_plan = final_plan(&source_changed);
    assert_ne!(source_plan, first_plan);
    assert!(
        stdout(&source_changed).contains("descriptor cache: 0/1"),
        "{}",
        stdout(&source_changed)
    );

    let featured = run_dev(temp.path(), &target, &["--features", "alternate"], None);
    assert!(
        featured.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&featured),
        String::from_utf8_lossy(&featured.stderr)
    );
    let feature_plan = final_plan(&featured);
    assert_ne!(feature_plan, source_plan);
    assert!(
        stdout(&featured).contains("descriptor cache: 0/1"),
        "{}",
        stdout(&featured)
    );

    let cfg_changed = run_dev(
        temp.path(),
        &target,
        &["--features", "alternate"],
        Some("--cfg linked_dev_probe"),
    );
    assert!(
        cfg_changed.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&cfg_changed),
        String::from_utf8_lossy(&cfg_changed.stderr)
    );
    assert_ne!(final_plan(&cfg_changed), feature_plan);
    assert!(
        stdout(&cfg_changed).contains("descriptor cache: 0/1"),
        "{}",
        stdout(&cfg_changed)
    );
}
