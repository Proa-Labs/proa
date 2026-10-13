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

#[cfg(unix)]
#[test]
fn proa_dev_opens_the_configured_url_only_after_startup_in_watch_and_once_modes() {
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::{fs::PermissionsExt, process::CommandExt};
    use std::time::{Duration, Instant};

    struct RunningDev(std::process::Child);
    impl Drop for RunningDev {
        fn drop(&mut self) {
            // Stop Cargo too if the test fails during compilation. Proa forwards
            // SIGTERM to its final server's separate process group.
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGTERM);
            }
            let _ = self.0.wait();
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"startup_fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n").unwrap();
    fs::write(
        root.join("src/main.rs"),
        r#"
#![allow(unexpected_cfgs)]
#[cfg(not(rsjs_final))]
fn main() {}
#[cfg(rsjs_final)]
fn main() {
    if std::env::args().any(|arg| arg == "--exit-before-ready") {
        std::process::exit(9);
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let host = std::env::var("PROA_HOST").unwrap();
    let port = std::env::var("PORT").unwrap();
    let listener = std::net::TcpListener::bind(format!("{host}:{port}")).unwrap();
    std::fs::write(std::env::var("PROA_TEST_READY_FILE").unwrap(), "ready").unwrap();
    for connection in listener.incoming() { drop(connection.unwrap()); }
}
"#,
    )
    .unwrap();

    let browser_log = root.join("browser.log");
    let ready_file = root.join("ready");
    let opener = root.join("bin").join(if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    });
    fs::write(&opener, "#!/bin/sh\nif [ -f \"$PROA_TEST_READY_FILE\" ]; then\n  printf 'READY %s\\n' \"$1\"\nelse\n  printf 'EARLY %s\\n' \"$1\"\nfi > \"$PROA_TEST_OPEN_FILE\"\n").unwrap();
    fs::set_permissions(&opener, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(root.join("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_proa"));
        command
            .current_dir(root)
            .env("PATH", &path)
            .env("PROA_TEST_READY_FILE", &ready_file)
            .env("PROA_TEST_OPEN_FILE", &browser_log)
            .env("CARGO_TARGET_DIR", root.join("target"))
            .env_remove("RUSTFLAGS")
            .args(["dev", "--open", "--no-assets", "--no-reload", "--offline"]);
        command
    };

    for once in [false, true] {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        fs::write(
            root.join("proa.config.json"),
            serde_json::json!({
                "dev": { "host": "0.0.0.0", "port": port, "command": ["cargo", "run"] }
            })
            .to_string(),
        )
        .unwrap();
        let _ = fs::remove_file(&ready_file);
        let _ = fs::remove_file(&browser_log);
        let log_path = root.join("dev.log");
        let log = fs::File::create(&log_path).unwrap();
        let mut command = command();
        if once {
            command.arg("--no-restart");
        }
        command
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .process_group(0);
        drop(reservation);
        let mut dev = RunningDev(command.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(60);
        while !browser_log.exists() {
            assert!(
                dev.0.try_wait().unwrap().is_none(),
                "{}",
                fs::read_to_string(&log_path).unwrap()
            );
            assert!(
                Instant::now() < deadline,
                "{}",
                fs::read_to_string(&log_path).unwrap()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        // Wait for the opener's write, rather than racing creation of its file.
        let expected = format!("READY http://127.0.0.1:{port}\n");
        while fs::read_to_string(&browser_log).unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fs::read_to_string(&browser_log).unwrap(), expected);
        assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());
        drop(dev);
        assert!(
            TcpStream::connect(("127.0.0.1", port)).is_err(),
            "server outlived CLI"
        );
    }

    fs::remove_file(&browser_log).unwrap();
    let failed_start = command()
        .args([
            "--no-restart",
            "--",
            "cargo",
            "run",
            "--",
            "--exit-before-ready",
        ])
        .output()
        .unwrap();
    assert!(!failed_start.status.success());
    assert!(!browser_log.exists(), "browser opened for a failed server");

    fs::write(root.join("src/main.rs"), "this does not compile").unwrap();
    let failed_build = command().arg("--no-restart").output().unwrap();
    assert!(!failed_build.status.success());
    assert!(!browser_log.exists(), "browser opened for a failed build");
}
