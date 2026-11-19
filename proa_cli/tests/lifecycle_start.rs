// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{env, fs, process::Command};

#[test]
fn run_scripts_chain_commands_and_use_the_cli_that_launched_them() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    let config = r#"use proa::config::{Config, Result, ScriptsConfig};
pub async fn config() -> Result<Config> {
    Ok(Config {
        scripts: ScriptsConfig {
            dev: DEV_SCRIPT,
            build: "echo configured-build",
            named: std::collections::BTreeMap::from([("report", proa::config::ScriptConfig {
                command: vec!["proa", "--version"],
                ..Default::default()
            })]),
            ..Default::default()
        },
        ..Config::default()
    })
}
"#;
    let script = if cfg!(windows) {
        "proa --version && echo shell-arguments !content!"
    } else {
        "proa --version && echo shell-arguments; # trailing comment"
    };
    fs::write(
        root.join("proa.config.rs"),
        config.replace("DEV_SCRIPT", &format!("{script:?}")),
    )
    .unwrap();
    if cfg!(windows) {
        fs::write(root.join("proa.cmd"), "@echo wrong-cli & exit /b 99\n").unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let wrong_cli = root.join("proa");
        fs::write(&wrong_cli, "#!/bin/sh\necho wrong-cli\nexit 99\n").unwrap();
        fs::set_permissions(wrong_cli, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut paths = vec![root.to_path_buf()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let path = env::join_paths(paths).unwrap();
    let cli = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_proa"))
            .current_dir(root)
            .env("PATH", &path)
            .args(args)
            .output()
            .unwrap()
    };
    let output = cli(&["run", "dev", "--port", "A & B"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "{stdout}");
    assert!(
        stdout.contains("shell-arguments") && stdout.contains("A & B"),
        "{stdout}"
    );
    if cfg!(windows) {
        assert!(stdout.contains("!content!"), "{stdout}");
    }
    assert!(!stdout.contains("wrong-cli"), "{stdout}");

    let output = cli(&["run", "dev"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));

    let output = cli(&["run", "build"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("configured-build"));
    let output = cli(&[
        "build",
        "--root",
        ".",
        "--no-assets",
        "--no-check",
        "--offline",
    ]);
    assert!(
        !output.status.success(),
        "the empty workspace has no target"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("configured-build"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("use `proa run build`"), "{stderr}");

    let output = cli(&["run", "report"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));
    let output = cli(&["run", "--list"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("report"));
}

#[cfg(unix)]
#[test]
fn terminating_a_supervised_script_stops_the_shells_children() {
    use std::{
        io::Write,
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    fs::write(
        root.join("proa.config.rs"),
        r#"use proa::config::{Config, Result, ScriptsConfig};
pub async fn config() -> Result<Config> {
    Ok(Config {
        scripts: ScriptsConfig { dev: "sh watcher.sh && echo finished", ..Default::default() },
        ..Default::default()
    })
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("watcher.sh"),
        "trap 'touch stopped; exit' TERM\n\
         if [ -t 0 ]; then\n\
           IFS= read -r input && [ \"$input\" = start ] || exit 1\n\
         fi\n\
         printf '%s' \"$$\" > ready\n\
         while :; do sleep 1; done\n",
    )
    .unwrap();
    let ready = root.join("ready");
    let stopped = root.join("stopped");
    for interactive in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_proa"));
        command.current_dir(root).args(["run", "dev"]);
        let mut terminal = if interactive {
            Some(attach_test_terminal(&mut command))
        } else {
            command.stdin(Stdio::null());
            None
        };
        let mut cli = command.spawn().unwrap();
        drop(command);
        if let Some(terminal) = &mut terminal {
            terminal.write_all(b"start\n").unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        let watcher = loop {
            if let Some(pid) = fs::read_to_string(&ready)
                .ok()
                .and_then(|pid| pid.parse::<i32>().ok())
            {
                break pid;
            }
            if cli.try_wait().unwrap().is_some() || Instant::now() >= deadline {
                drop(terminal.take());
                let _ = cli.kill();
                let _ = cli.wait();
                panic!("script failed to start (terminal: {interactive})");
            }
            thread::sleep(Duration::from_millis(25));
        };

        assert_eq!(unsafe { libc::kill(cli.id() as i32, libc::SIGTERM) }, 0);
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = cli.try_wait().unwrap().filter(|_| stopped.exists()) {
                break status;
            }
            if Instant::now() >= deadline {
                drop(terminal.take());
                unsafe { libc::kill(watcher, libc::SIGKILL) };
                let _ = cli.kill();
                let _ = cli.wait();
                panic!("script failed to stop (terminal: {interactive})");
            }
            thread::sleep(Duration::from_millis(25));
        };
        assert_eq!(status.code(), Some(128 + libc::SIGTERM));
        fs::remove_file(&ready).unwrap();
        fs::remove_file(&stopped).unwrap();
    }
}

#[cfg(unix)]
fn attach_test_terminal(command: &mut Command) -> fs::File {
    use std::{
        io,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::process::CommandExt,
        },
    };

    let mut controller_fd = -1;
    let mut terminal_fd = -1;
    unsafe {
        assert_eq!(
            libc::openpty(
                &mut controller_fd,
                &mut terminal_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ),
            0
        );
        let controller = OwnedFd::from_raw_fd(controller_fd);
        let terminal = OwnedFd::from_raw_fd(terminal_fd);
        // The test never reads tty output. Disable input echo so the OS
        // does not wait for the controller to drain echoed bytes on exit.
        let mut attributes = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(terminal.as_raw_fd(), &mut attributes), 0);
        attributes.c_lflag &= !libc::ECHO;
        assert_eq!(
            libc::tcsetattr(terminal.as_raw_fd(), libc::TCSANOW, &attributes),
            0
        );
        for fd in [&controller, &terminal] {
            assert_eq!(
                libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC),
                0
            );
        }
        command.stdin(terminal);
        command.pre_exec(|| {
            let request = libc::TIOCSCTTY;
            #[cfg(target_vendor = "apple")]
            let request = libc::c_ulong::from(request);
            if libc::setsid() == -1
                || libc::ioctl(libc::STDIN_FILENO, request, 0) == -1
                || libc::tcsetpgrp(libc::STDIN_FILENO, libc::getpgrp()) == -1
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
        fs::File::from(controller)
    }
}

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
    fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let release = temp.path().join("target/release");
    fs::create_dir_all(&release).unwrap();
    let catalog = temp.path().join("copied catalog");
    fs::create_dir_all(&catalog).unwrap();
    fs::write(catalog.join("manifest.json"), "{}").unwrap();
    let executable = release.join(format!("server.prod{}", env::consts::EXE_SUFFIX));
    let source = temp.path().join("server.rs");
    fs::write(
        &source,
        r#"fn main() {
            let assets = std::env::var_os("PROA_PUBLIC_ASSETS");
            let assets = assets.as_deref().and_then(|path| std::path::Path::new(path).file_name());
            println!("{}|{}|{}", std::env::var("PROA_HOST").unwrap(), std::env::var("PORT").unwrap(), assets.and_then(|name| name.to_str()).unwrap_or_default());
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
            temp.path().join("proa.config.rs"),
            format!(
                r#"use proa::config::{{ApplicationConfig, Config, Result, ScriptsConfig}};
pub async fn config() -> Result<Config> {{
    Ok(Config {{
        scripts: ScriptsConfig {{
            dev: "cargo run",
            build: "cargo build --release",
            ..Default::default()
        }},
        application: Some(ApplicationConfig {{ binary: {configured_binary:?} }}),
        ..Config::default()
    }})
}}
"#,
            ),
        )
        .unwrap();
        if custom {
            fs::write(temp.path().join("proa.config.rs"), "invalid build config").unwrap();
            fs::rename(&executable, &explicit).unwrap();
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_proa"));
        command
            .current_dir(temp.path())
            .env_remove("PROA_PUBLIC_ASSETS")
            .args(["start", "--host", "127.0.0.2", "--port", "8123"]);
        if custom {
            command
                .current_dir(temp.path().parent().unwrap())
                .arg("--root")
                .arg(temp.path().file_name().unwrap())
                .arg("--bin")
                .arg(explicit.file_name().unwrap())
                .arg("--public-assets")
                .arg(catalog.file_name().unwrap());
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            if custom {
                "127.0.0.2|8123|copied catalog"
            } else {
                "127.0.0.2|8123|"
            }
        );
    }
}
