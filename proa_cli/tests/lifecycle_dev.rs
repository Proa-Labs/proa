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
    if std::env::args().any(|arg| arg == "--slow-exit-before-ready") {
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::process::exit(9);
    }
    assert_eq!(std::env::args().skip(1).collect::<Vec<_>>(), ["Local shop"]);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let host = std::env::var("PROA_HOST").unwrap();
    let port = std::env::var("PORT").unwrap();
    let report = std::env::var("TEST_BROWSER_URL").ok().map(|url| {
        let endpoint = std::env::var("PROA_DEV_READY").unwrap();
        let (port, token) = endpoint.split_once(':').unwrap();
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(format!("127.0.0.1:{port}")).unwrap();
        socket.send(token.as_bytes()).unwrap();
        (socket, format!("{token}\n{url}"))
    });
    let listener = std::net::TcpListener::bind(format!("{host}:{port}")).unwrap();
    // A bound socket alone must never open the HTTP fallback for a TLS app
    // that is still initializing its handlers.
    if report.is_some() { std::thread::sleep(std::time::Duration::from_millis(450)); }
    std::fs::write(std::env::var("PROA_TEST_READY_FILE").unwrap(), "ready").unwrap();
    if let Some((socket, message)) = report { socket.send(message.as_bytes()).unwrap(); }
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
        let _ = fs::remove_file(&ready_file);
        let _ = fs::remove_file(&browser_log);
        let log_path = root.join("dev.log");
        let log = fs::File::create(&log_path).unwrap();
        let mut command = command();
        command.args(["--host", "0.0.0.0", "--port", &port.to_string()]);
        let expected_url = if once {
            let url = format!("https://localhost:{port}/shop");
            command.arg("--no-restart").env("TEST_BROWSER_URL", &url);
            url
        } else {
            format!("http://127.0.0.1:{port}")
        };
        command
            .args(["--", "cargo", "run", "--", "Local shop"])
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
        let expected = format!("READY {expected_url}\n");
        while fs::read_to_string(&browser_log).unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fs::read_to_string(&browser_log).unwrap(), expected);
        assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());
        drop(dev);
        // The CLI forwards SIGTERM and exits; its server may receive that
        // signal only after wait() has observed the CLI's exit.
        let shutdown_deadline = Instant::now() + Duration::from_secs(5);
        while TcpStream::connect(("127.0.0.1", port)).is_ok() {
            assert!(
                Instant::now() < shutdown_deadline,
                "server outlived CLI (once={once}):\n{}",
                fs::read_to_string(&log_path).unwrap()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
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

    let busy = TcpListener::bind("127.0.0.1:0").unwrap();
    let occupied = command()
        .args([
            "--no-restart",
            "--host",
            "127.0.0.1",
            "--port",
            &busy.local_addr().unwrap().port().to_string(),
            "--",
            "cargo",
            "run",
            "--",
            "--slow-exit-before-ready",
        ])
        .output()
        .unwrap();
    assert!(!occupied.status.success());
    assert!(
        !browser_log.exists(),
        "browser opened for a listener owned by another process"
    );
    drop(busy);

    fs::write(root.join("src/main.rs"), "this does not compile").unwrap();
    let failed_build = command().arg("--no-restart").output().unwrap();
    assert!(!failed_build.status.success());
    assert!(!browser_log.exists(), "browser opened for a failed build");
}

#[cfg(unix)]
#[test]
fn proa_dev_watch_uses_single_pass_and_prunes_superseded_snapshots() {
    use std::net::TcpListener;
    use std::os::unix::process::CommandExt;
    use std::time::{Duration, Instant};

    struct RunningDev(std::process::Child);
    impl Drop for RunningDev {
        fn drop(&mut self) {
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGTERM);
            }
            let _ = self.0.wait();
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"watch_single_pass_fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    let write_server = |marker: &str| {
        fs::write(
            root.join("src/main.rs"),
            format!(
                r#"
fn main() {{
    let host = std::env::var("PROA_HOST").unwrap();
    let port = std::env::var("PORT").unwrap();
    let listener = std::net::TcpListener::bind(format!("{{host}}:{{port}}")).unwrap();
    std::fs::write(std::env::var("PROA_MARKER_FILE").unwrap(), {marker:?}).unwrap();
    for connection in listener.incoming() {{
        let _ = connection;
    }}
}}
"#
            ),
        )
        .unwrap();
    };
    write_server("v1");

    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);

    let marker_file = root.join("marker.txt");
    let log_path = root.join("dev.log");
    let log = fs::File::create(&log_path).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_proa"));
    cmd.current_dir(root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("PROA_MARKER_FILE", &marker_file)
        .env_remove("RUSTFLAGS")
        .args([
            "dev",
            "--no-assets",
            "--no-reload",
            "--offline",
            "--verbose",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .process_group(0);

    let mut dev = RunningDev(cmd.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(60);
    while fs::read_to_string(&marker_file).ok().as_deref() != Some("v1") {
        assert!(
            dev.0.try_wait().unwrap().is_none(),
            "dev exited early:\n{}",
            fs::read_to_string(&log_path).unwrap()
        );
        assert!(
            Instant::now() < deadline,
            "timed out waiting for v1:\n{}",
            fs::read_to_string(&log_path).unwrap()
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    assert!(
        !root.join("target/proa-rsjs/shared").exists(),
        "single-pass zero-island dev boot should not create descriptor target directory"
    );

    // Edit source and verify single-pass rebuild + superseded snapshot pruning.
    write_server("v2");
    let rebuild_deadline = Instant::now() + Duration::from_secs(30);
    while fs::read_to_string(&marker_file).ok().as_deref() != Some("v2") {
        assert!(
            dev.0.try_wait().unwrap().is_none(),
            "dev exited during rebuild:\n{}",
            fs::read_to_string(&log_path).unwrap()
        );
        assert!(
            Instant::now() < rebuild_deadline,
            "timed out waiting for v2:\n{}",
            fs::read_to_string(&log_path).unwrap()
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    // The new server writes its marker before the parent prunes the old
    // snapshot, so wait for pruning rather than sleeping a fixed time.
    let snapshot_dirs = || -> Vec<_> {
        walkdir::WalkDir::new(root.join("target/proa-rsjs/final-target"))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_type().is_dir()
                    && entry
                        .path()
                        .parent()
                        .and_then(|parent| parent.file_name())
                        .and_then(|name| name.to_str())
                        == Some("rsjs-final-artifact-snapshots")
            })
            .map(|entry| entry.into_path())
            .collect()
    };
    let prune_deadline = Instant::now() + Duration::from_secs(10);
    while snapshot_dirs().len() != 1 && Instant::now() < prune_deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    drop(dev);

    let log_text = fs::read_to_string(&log_path).unwrap();
    assert!(
        log_text.matches("(single-pass;").count() >= 2,
        "expected both boot and edit rebuild to use single-pass:\n{log_text}"
    );
    assert!(
        !root.join("target/proa-rsjs/shared").exists(),
        "zero-island edit rebuild should not create descriptor target directory"
    );
    let snapshot_dirs = snapshot_dirs();
    assert_eq!(
        snapshot_dirs.len(),
        1,
        "expected superseded v1 snapshot to be pruned, leaving only v2: {snapshot_dirs:?}"
    );
}
