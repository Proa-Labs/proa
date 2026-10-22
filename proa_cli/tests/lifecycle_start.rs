// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{env, fs, process::Command};

#[test]
fn cargo_subcommand_dispatches_through_the_installed_executable_name() {
    let temp = tempfile::tempdir().unwrap();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_cargo-proa"));
    let mut paths = vec![executable.parent().unwrap().to_path_buf()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let output = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(temp.path())
        .env("PATH", env::join_paths(paths).unwrap())
        .args(["proa", "--version"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn start_launches_the_host_release_executable_and_preserves_explicit_paths() {
    let temp = tempfile::Builder::new()
        .prefix("proa start with spaces ")
        .tempdir()
        .unwrap();
    let release = temp.path().join("target/release");
    fs::create_dir_all(&release).unwrap();
    let executable = release.join(format!("server.prod{}", env::consts::EXE_SUFFIX));
    let source = temp.path().join("server.rs");
    fs::write(
        &source,
        r#"fn main() {
            println!("{}|{}", std::env::var("PROA_HOST").unwrap(), std::env::var("PORT").unwrap());
        }"#,
    )
    .unwrap();
    let compiled = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let explicit = temp
        .path()
        .join(format!("custom.server{}", env::consts::EXE_SUFFIX));
    let mut cases = vec![("server.prod", false)];
    if cfg!(windows) {
        cases.extend([("server.prod.exe", false), ("server.prod.EXE", false)]);
    }
    cases.push(("server.prod", true));
    for (configured_binary, custom) in cases {
        fs::write(
            temp.path().join("proa.config.json"),
            serde_json::to_vec(&serde_json::json!({
                "application": { "binary": configured_binary }
            }))
            .unwrap(),
        )
        .unwrap();
        if custom {
            fs::rename(&executable, &explicit).unwrap();
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_proa"));
        command
            .current_dir(temp.path())
            .args(["start", "--host", "127.0.0.2", "--port", "8123"]);
        if custom {
            command.arg("--bin").arg(explicit.file_name().unwrap());
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "127.0.0.2|8123"
        );
    }
}
