// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cargo_proa(root: &Path, target: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-proa"))
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target)
        .args(arguments)
        .output()
        .expect("run cargo-proa")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn cargo_proa_run_and_test_execute_only_final_phase_with_forwarded_arguments() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::create_dir_all(temp.path().join("tests")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\n\
         name = \"proa_linked_execute_fixture\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\n\
         [workspace]\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/main.rs"),
        r#"
#![allow(unexpected_cfgs)]

#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>().join(",");
    println!(
        "RUN|{}|{}|{}",
        env!("RSJS_FINAL_PHASE"),
        env!("RSJS_PLAN_ID"),
        arguments,
    );
}

#[cfg(all(rsjs_final, test))]
#[test]
fn linked_binary_plan() {
    println!(
        "BIN-TEST|{}|{}",
        env!("RSJS_FINAL_PHASE"),
        env!("RSJS_PLAN_ID"),
    );
}
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("tests/linked.rs"),
        r#"
#![allow(unexpected_cfgs)]

#[cfg(not(rsjs_final))]
#[test]
fn linked_plan() {}

#[cfg(rsjs_final)]
#[test]
fn linked_plan() {
    println!(
        "TEST|{}|{}",
        env!("RSJS_FINAL_PHASE"),
        env!("RSJS_PLAN_ID"),
    );
}
"#,
    )
    .unwrap();

    let target = temp.path().join("outer-target");
    let run = cargo_proa(
        temp.path(),
        &target,
        &[
            "run",
            "--root",
            temp.path().to_str().unwrap(),
            "-p",
            "proa_linked_execute_fixture",
            "--bin",
            "proa_linked_execute_fixture",
            "--offline",
            "--",
            "--probe",
            "42",
        ],
    );
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&run),
        stderr(&run)
    );
    let run_stdout = stdout(&run);
    let run_record = run_stdout
        .lines()
        .find(|line| line.starts_with("RUN|1|"))
        .unwrap_or_else(|| panic!("missing final run output:\n{run_stdout}"));
    assert!(run_record.ends_with("|--probe,42"), "{run_record}");

    let unchanged_run = cargo_proa(
        temp.path(),
        &target,
        &[
            "run",
            "--root",
            temp.path().to_str().unwrap(),
            "-p",
            "proa_linked_execute_fixture",
            "--bin",
            "proa_linked_execute_fixture",
            "--offline",
            "--",
            "--probe",
            "42",
        ],
    );
    assert!(
        unchanged_run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&unchanged_run),
        stderr(&unchanged_run)
    );
    let unchanged_stdout = stdout(&unchanged_run);
    assert!(
        unchanged_stdout.contains("descriptor 1/1 Cargo artifacts reused"),
        "missing descriptor cache reuse:\n{unchanged_stdout}"
    );
    assert!(
        unchanged_stdout.lines().any(|line| line == run_record),
        "the same inputs must execute the same final plan:\n{unchanged_stdout}"
    );

    let test = cargo_proa(
        temp.path(),
        &target,
        &[
            "test",
            "--root",
            temp.path().to_str().unwrap(),
            "-p",
            "proa_linked_execute_fixture",
            "--test",
            "linked",
            "--offline",
            "--",
            "--nocapture",
            "linked_plan",
        ],
    );
    assert!(
        test.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&test),
        stderr(&test)
    );
    let test_stdout = stdout(&test);
    assert!(
        test_stdout.lines().any(|line| line.contains("TEST|1|")),
        "missing final test output:\n{test_stdout}"
    );
    assert!(
        test_stdout.contains("test result: ok"),
        "missing forwarded harness result:\n{test_stdout}"
    );

    let binary_test = cargo_proa(
        temp.path(),
        &target,
        &[
            "test",
            "--root",
            temp.path().to_str().unwrap(),
            "-p",
            "proa_linked_execute_fixture",
            "--bin",
            "proa_linked_execute_fixture",
            "--offline",
            "--",
            "--nocapture",
            "linked_binary_plan",
        ],
    );
    assert!(
        binary_test.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&binary_test),
        stderr(&binary_test)
    );
    assert!(
        stdout(&binary_test)
            .lines()
            .any(|line| line.contains("BIN-TEST|1|")),
        "missing final binary-test output:\n{}",
        stdout(&binary_test)
    );

    let missing_target = cargo_proa(
        temp.path(),
        &target,
        &[
            "test",
            "--root",
            temp.path().to_str().unwrap(),
            "-p",
            "proa_linked_execute_fixture",
            "--offline",
        ],
    );
    assert!(!missing_target.status.success());
    assert!(
        stderr(&missing_target).contains("exactly one target selector"),
        "{}",
        stderr(&missing_target)
    );
}
