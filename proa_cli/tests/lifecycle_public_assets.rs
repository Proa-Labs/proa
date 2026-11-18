// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_build::assets::{AssetCatalog, AssetManifest, OUTPUT_ENV};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const PUBLIC_CONFIG: &str = r#"
use proa_config::{BuildConfig, Config, DevConfig, ProjectPaths, Result, ScriptsConfig};

pub async fn config() -> Result<Config<&'static str, String>> {
    let public = proa_config::tokio::fs::read_to_string("asset-root.txt")
        .await
        .map_err(|error| format!("read asset-root.txt: {error}"))?;
    Ok(Config {
        scripts: ScriptsConfig {
            dev: "cargo run",
            build: "cargo build --release",
            ..Default::default()
        },
        paths: ProjectPaths {
            public: Some(public),
            ..Default::default()
        },
        build: BuildConfig {
            assets: Some(vec![]),
            ..Default::default()
        },
        dev: DevConfig {
            watch: vec!["src".into()],
            ..Default::default()
        },
        ..Config::default()
    })
}
"#;

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::create_dir(temp.path().join("public")).unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[package]\nname = \"public_asset_fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n").unwrap();
    fs::write(temp.path().join("src/main.rs"), r#"
fn main() {
    assert!(option_env!("PROA_PUBLIC_ASSETS").is_none(), "output path leaked into compilation");
    let path = std::env::var_os("PROA_PUBLIC_ASSETS").expect("runtime public output");
    let manifest = std::fs::read_to_string(std::path::PathBuf::from(path).join("manifest.json")).unwrap();
    println!("MAIN|{}|{}", std::env::args().nth(1).unwrap_or_default(), manifest.trim());
}
"#).unwrap();
    fs::write(temp.path().join("proa.config.rs"), PUBLIC_CONFIG).unwrap();
    fs::write(temp.path().join("asset-root.txt"), "public").unwrap();
    fs::write(temp.path().join("public/logo.svg"), b"first").unwrap();
    temp
}

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-proa"))
        .current_dir(root)
        .env("CARGO_TARGET_DIR", root.join("custom-target"))
        .env_remove(OUTPUT_ENV)
        .args(args)
        .output()
        .unwrap()
}

fn success(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

fn published(stdout: &str) -> PathBuf {
    stdout
        .lines()
        .find_map(|line| {
            line.strip_prefix("published ")?
                .split_once(" public assets to ")
                .map(|(_, path)| path.into())
        })
        .expect("published public catalog")
}

fn main_manifest(stdout: &str) -> AssetManifest {
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.starts_with("MAIN|"))
            .count(),
        1
    );
    let json = stdout
        .lines()
        .find_map(|line| line.strip_prefix("MAIN|--probe|"))
        .expect("main executed after publication, with its argument");
    serde_json::from_str(json).unwrap()
}

#[test]
fn tests_execute_after_publication_with_runtime_only_assets_and_harness_args() {
    let temp = fixture();
    let root = temp.path();
    fs::create_dir(root.join("tests")).unwrap();
    fs::write(
        root.join("build.rs"),
        r#"
fn main() {
    assert!(std::env::var_os("PROA_PUBLIC_ASSETS").is_none());
    std::fs::write("public/generated.svg", b"build output").unwrap();
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("tests/page.rs"),
        r#"
#[test]
fn managed_page() {
    assert!(option_env!("PROA_PUBLIC_ASSETS").is_none());
    let output = std::path::PathBuf::from(std::env::var_os("PROA_PUBLIC_ASSETS").unwrap());
    let manifest = std::fs::read_to_string(output.join("manifest.json")).unwrap();
    assert!(manifest.contains("generated.svg"), "{manifest}");
}

#[test]
fn filtered_out() {
    panic!("the CLI must forward the harness filter");
}
"#,
    )
    .unwrap();
    let output = cli(
        root,
        &[
            "test",
            "--test",
            "page",
            "--offline",
            "--",
            "--exact",
            "managed_page",
        ],
    );
    let stdout = success(&output);
    assert!(stdout.contains("test managed_page ... ok"), "{stdout}");
    let published = published(&stdout);
    let catalog = AssetCatalog::load(published).unwrap();
    assert_eq!(
        fs::read(catalog.object_path("generated.svg").unwrap()).unwrap(),
        b"build output"
    );
}

#[cfg(unix)]
#[test]
fn watcher_recovers_output_and_rechecks_only_config_compile_inputs_on_asset_edits() {
    use std::{
        net::TcpListener,
        os::unix::{fs::PermissionsExt, process::CommandExt},
        time::{Duration, Instant},
    };
    struct Watcher(std::process::Child);
    impl Drop for Watcher {
        fn drop(&mut self) {
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGTERM);
            }
            let _ = self.0.wait();
        }
    }
    let temp = fixture();
    let root = temp.path();
    let artifacts = tempfile::tempdir().unwrap();
    let target = artifacts.path().join("cargo target");
    let cargo_trace = artifacts.path().join("config-cargo.log");
    let cargo = artifacts.path().join("cargo-trace");
    fs::write(
        &cargo,
        r#"#!/bin/sh
case "$1:$*" in build:*proa-config/*) echo build >> "$CONFIG_CARGO_LOG";; esac
exec "$REAL_CARGO" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        root.join("src/main.rs"),
        r#"
fn main() {
    let port = std::env::var("PORT").unwrap();
    let listener = std::net::TcpListener::bind(format!("127.0.0.1:{port}")).unwrap();
    for stream in listener.incoming() { drop(stream.unwrap()); }
}
"#,
    )
    .unwrap();
    fs::write(root.join("public/icon.svg"), "one").unwrap();
    fs::write(root.join("extra-input.txt"), "one").unwrap();
    fs::create_dir(root.join("scripts")).unwrap();
    for name in ["a", "b"] {
        fs::write(
            root.join(format!("scripts/{name}.sh")),
            format!("echo WRAPPER-{name}\nexec \"$PROA_SCRIPT_EXECUTABLE\" dev --linked \"$@\"\n"),
        )
        .unwrap();
    }
    let config = PUBLIC_CONFIG.replace("cargo run", "bash scripts/a.sh").replace("    Ok(Config {", r#"    eprintln!("CONFIG {} {}", include_str!("public/icon.svg"), std::fs::read_to_string("public/logo.svg")?);
    Ok(Config {"#);
    fs::write(root.join("proa.config.rs"), &config).unwrap();
    let log = artifacts.path().join("dev.log");
    let output = fs::File::create(&log).unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut watch = Watcher(
        Command::new(env!("CARGO_BIN_EXE_proa"))
            .current_dir(root)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO", cargo)
            .env("REAL_CARGO", env!("CARGO"))
            .env("CONFIG_CARGO_LOG", &cargo_trace)
            .args([
                "dev",
                "--offline",
                "--no-reload",
                "--watch",
                "extra-input.txt",
                "--port",
                &port.to_string(),
            ])
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    let mut wait = |message, count| {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let text = fs::read_to_string(&log).unwrap();
            if text.matches(message).count() >= count {
                break text;
            }
            assert!(
                watch.0.try_wait().unwrap().is_none() && Instant::now() < deadline,
                "{text}"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    };
    let cargo_calls = || fs::read_to_string(&cargo_trace).unwrap().lines().count();
    let ready = "Proa dev ready:";
    let build_started = "running linked RSJS Cargo dev build";
    let first = wait(ready, 1);
    let output = published(&first);
    let builds = cargo_calls();
    assert_eq!(builds, 2, "{first}");
    let original = fs::read(output.join("manifest.json")).unwrap();
    fs::remove_file(output.join("manifest.json")).unwrap();
    let repaired = wait(ready, 2);
    fs::rename(&output, artifacts.path().join("moved-catalog")).unwrap();
    let recreated = wait(ready, 3);
    for text in [&repaired, &recreated] {
        assert_eq!(text.matches(build_started).count(), 1, "{text}");
    }
    assert_eq!(fs::read(output.join("manifest.json")).unwrap(), original);
    assert_eq!(cargo_calls(), builds);

    fs::write(root.join("public/logo.svg"), "two").unwrap();
    assert!(wait(ready, 4).contains("CONFIG one two"));
    assert_eq!(
        cargo_calls(),
        builds,
        "runtime read should reuse the config executable"
    );
    fs::write(root.join("public/icon.svg"), "new-icon").unwrap();
    assert!(wait(ready, 5).contains("CONFIG new-icon two"));
    assert_eq!(
        cargo_calls(),
        builds + 1,
        "include_str! is a compiler input"
    );
    fs::write(
        root.join("proa.config.rs"),
        config.replace("scripts/a.sh", "scripts/b.sh"),
    )
    .unwrap();
    let changed = wait(ready, 6);
    assert!(changed.contains("scripts.dev changed to \"bash scripts/b.sh\". Run `proa dev` again"));
    assert_eq!(changed.matches("WRAPPER-a").count(), 1);
    assert!(!changed.contains("WRAPPER-b"));

    fs::rename(&target, artifacts.path().join("moved-target")).unwrap();
    let rebuilt = wait(ready, 7);
    assert!(output.join("manifest.json").is_file());
    assert_eq!(rebuilt.matches("scripts.dev changed to").count(), 1);
    assert_eq!(rebuilt.matches(build_started).count(), 5, "{rebuilt}");

    let source = root.join("src/main.rs");
    let valid_source = fs::read(&source).unwrap();
    let config_builds = cargo_calls();
    fs::write(
        &source,
        b"compile_error!(\"broken while output is missing\");",
    )
    .unwrap();
    let failed = "linked dev rebuild failed";
    let broken = wait(failed, 1);
    assert!(
        broken.contains("broken while output is missing"),
        "{broken}"
    );
    assert_eq!(
        cargo_calls(),
        config_builds,
        "an application edit outside the config inputs should reuse the config executable"
    );
    fs::remove_file(output.join("manifest.json")).unwrap();

    // Watch past the missing-catalog retry interval while the files are idle.
    std::thread::sleep(Duration::from_secs(3));
    let idle = fs::read_to_string(&log).unwrap();
    assert_eq!(idle.matches(build_started).count(), 6, "{idle}");
    assert_eq!(idle.matches(failed).count(), 1, "{idle}");
    assert!(!output.join("manifest.json").exists(), "{idle}");

    fs::write(&source, valid_source).unwrap();
    let recovered = wait(ready, 8);
    assert_eq!(recovered.matches(build_started).count(), 7, "{recovered}");
    assert!(output.join("manifest.json").is_file(), "{recovered}");

    fs::write(root.join("extra-input.txt"), "two").unwrap();
    let watched = wait(ready, 9);
    assert_eq!(watched.matches(build_started).count(), 8, "{watched}");
}

#[cfg(unix)]
#[test]
fn configured_scripts_own_the_entrypoint_and_can_delegate_to_the_linked_pipeline() {
    let temp = fixture();
    let root = temp.path();
    fs::create_dir(root.join("scripts")).unwrap();
    fs::write(
        root.join("proa.config.rs"),
        PUBLIC_CONFIG
            .replace("cargo run", "bash scripts/dev.sh")
            .replace("cargo build --release", "bash scripts/build.sh"),
    )
    .unwrap();
    for phase in ["dev", "build"] {
        fs::write(
            root.join(format!("scripts/{phase}.sh")),
            format!(
                "echo SCRIPT:{phase}\nexec \"$PROA_SCRIPT_EXECUTABLE\" {phase} --linked \"$@\"\n"
            ),
        )
        .unwrap();
    }
    let cli = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_cargo-proa"))
            .current_dir(root.parent().unwrap())
            .env("CARGO_TARGET_DIR", root.join("custom-target"))
            .env_remove(OUTPUT_ENV)
            .args(arguments)
            .output()
            .unwrap()
    };
    let relative_root = root.file_name().unwrap().to_str().unwrap();
    let dev = success(&cli(&[
        "dev",
        "--root",
        relative_root,
        "--no-restart",
        "--offline",
    ]));
    assert_eq!(dev.matches("SCRIPT:dev").count(), 1, "{dev}");
    assert!(dev.contains("MAIN||"), "{dev}");
    assert!(published(&dev).join("manifest.json").is_file());

    let artifact = "custom-target/exported shop";
    let build = success(&cli(&[
        "build",
        &format!("--root={relative_root}"),
        "--profile",
        "dev",
        "--no-check",
        "--offline",
        "--artifact-out",
        artifact,
    ]));
    assert_eq!(build.matches("SCRIPT:build").count(), 1, "{build}");
    assert!(root.join(artifact).is_file());

    fs::write(
        root.join("scripts/dev.sh"),
        "echo SCRIPT:dev\nexec \"$PROA_SCRIPT_EXECUTABLE\" dev \"$@\"\n",
    )
    .unwrap();
    let recursive = cli(&["dev", "--root", relative_root, "--offline"]);
    assert!(!recursive.status.success());
    assert_eq!(
        String::from_utf8_lossy(&recursive.stdout)
            .matches("SCRIPT:dev")
            .count(),
        1
    );
    assert!(String::from_utf8_lossy(&recursive.stderr).contains("use `proa dev --linked`"));
}

#[test]
fn rsjs_minification_follows_the_config_across_reused_builds() {
    let temp = fixture();
    let root = temp.path();
    fs::write(
        root.join("src/main.rs"),
        r#"
fn main() {
    assert!(std::env::var_os("PROA_RSJS_MINIFY").is_none());
    println!("{}", env!("PROA_RSJS_MINIFY"));
}
"#,
    )
    .unwrap();
    let export = root
        .join("custom-target")
        .join(format!("probe{}", std::env::consts::EXE_SUFFIX));
    let build = || {
        success(&cli(
            root,
            &[
                "build",
                "--profile",
                "dev",
                "--offline",
                "--no-check",
                "--artifact-out",
                export.to_str().unwrap(),
            ],
        ))
    };
    let run = || {
        success(
            &Command::new(&export)
                .env_remove("PROA_RSJS_MINIFY")
                .output()
                .unwrap(),
        )
    };

    fs::remove_file(root.join("proa.config.rs")).unwrap();
    build();
    assert_eq!(run().trim(), "1");
    let warm = build();
    assert!(
        warm.contains("descriptor 1/1 Cargo artifacts reused; final 1/1 reused"),
        "{warm}"
    );
    assert_eq!(run().trim(), "1");

    fs::write(
        root.join("proa.config.rs"),
        r#"
use proa_config::{BuildConfig, Config, Result, ScriptsConfig};

pub async fn config() -> Result<Config> {
    Ok(Config {
        scripts: ScriptsConfig {
            dev: "cargo run",
            build: "cargo build --release",
            ..Default::default()
        },
        build: BuildConfig {
            minify_rsjs: false,
            ..Default::default()
        },
        ..Config::default()
    })
}
"#,
    )
    .unwrap();
    build();
    assert_eq!(run().trim(), "0");
}

#[test]
fn build_then_run_refreshes_assets_without_rebinding_the_binary_and_keeps_failures_local() {
    let temp = fixture();
    let root = temp.path();
    let built = success(&cli(
        root,
        &["build", "--profile", "dev", "--offline", "--no-check"],
    ));
    assert!(!built.contains("MAIN|"));
    let output = published(&built);
    assert!(output.starts_with(fs::canonicalize(root.join("custom-target/proa-output")).unwrap()));
    let old = AssetCatalog::load(&output).unwrap();
    let old_object = old.object_path("logo.svg").unwrap();

    let run = success(&cli(root, &["run", "--offline", "--", "--probe"]));
    assert_eq!(published(&run), output);
    assert_eq!(&main_manifest(&run), old.manifest());
    assert!(
        run.contains("descriptor 1/1 Cargo artifacts reused; final 1/1 reused"),
        "{run}"
    );

    fs::write(root.join("public/logo.svg"), b"other").unwrap();
    let edited = success(&cli(root, &["run", "--offline", "--", "--probe"]));
    let current = main_manifest(&edited);
    assert_eq!(published(&edited), output);
    assert_ne!(
        current.assets["logo.svg"].public_path,
        old.manifest().assets["logo.svg"].public_path
    );
    assert!(
        edited.contains("descriptor 1/1 Cargo artifacts reused; final 1/1 reused"),
        "{edited}"
    );
    assert_eq!(fs::read(&old_object).unwrap(), b"first");
    let loaded = AssetCatalog::load(&output).unwrap();
    assert_eq!(
        fs::read(loaded.object_path("logo.svg").unwrap()).unwrap(),
        b"other"
    );
    drop(old);
    drop(loaded);
    success(&cli(
        root,
        &["build", "--profile", "dev", "--offline", "--no-check"],
    ));
    assert!(!old_object.exists());

    let pointer = fs::read(output.join("manifest.json")).unwrap();
    fs::write(root.join("proa.config.rs"), "invalid Rust config").unwrap();
    let invalid_config = cli(root, &["run", "--offline", "--", "--probe"]);
    assert!(!invalid_config.status.success());
    assert!(String::from_utf8_lossy(&invalid_config.stderr).contains("proa.config.rs"));
    assert!(!String::from_utf8_lossy(&invalid_config.stdout).contains("MAIN|"));
    assert_eq!(fs::read(output.join("manifest.json")).unwrap(), pointer);
    fs::write(root.join("proa.config.rs"), PUBLIC_CONFIG).unwrap();
    fs::remove_file(root.join("asset-root.txt")).unwrap();
    let config_error = cli(root, &["run", "--offline", "--", "--probe"]);
    assert!(!config_error.status.success());
    assert!(String::from_utf8_lossy(&config_error.stderr).contains("read asset-root.txt"));
    assert!(!String::from_utf8_lossy(&config_error.stdout).contains("MAIN|"));
    assert_eq!(fs::read(output.join("manifest.json")).unwrap(), pointer);
    fs::write(root.join("asset-root.txt"), "public").unwrap();
    fs::write(root.join("public/logo.svg"), b"third").unwrap();
    fs::write(root.join("src/main.rs"), "this is invalid Rust").unwrap();
    let broken = cli(root, &["run", "--offline", "--", "--probe"]);
    assert!(!broken.status.success());
    assert!(!String::from_utf8_lossy(&broken.stdout).contains("MAIN|"));
    assert_eq!(fs::read(output.join("manifest.json")).unwrap(), pointer);
    fs::write(
        root.join("src/main.rs"),
        "fn main() { panic!(\"must not launch\"); }",
    )
    .unwrap();
    fs::remove_dir_all(root.join("public")).unwrap();
    let missing = cli(root, &["run", "--offline"]);
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("publish configured public directory")
    );
    assert!(!String::from_utf8_lossy(&missing.stderr).contains("must not launch"));
    assert_eq!(fs::read(output.join("manifest.json")).unwrap(), pointer);
}

#[test]
fn public_files_used_by_rust_or_build_scripts_remain_compiler_inputs() {
    let temp = fixture();
    let root = temp.path();
    fs::write(root.join("public/build-input.txt"), "build-first").unwrap();
    fs::write(
        root.join("build.rs"),
        r#"
fn main() {
    println!("cargo:rerun-if-changed=public/build-input.txt");
    let value = std::fs::read_to_string("public/build-input.txt").unwrap();
    println!("cargo:rustc-env=PUBLIC_BUILD_INPUT={value}");
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        r#"
fn main() {
    println!("COMPILED|{}|{}", include_str!("../public/logo.svg"), env!("PUBLIC_BUILD_INPUT"));
}
"#,
    )
    .unwrap();
    let run = success(&cli(root, &["run", "--offline"]));
    assert!(run.lines().any(|line| line == "COMPILED|first|build-first"));
    let output = published(&run);

    fs::write(root.join("public/logo.svg"), "included-second").unwrap();
    let included = success(&cli(root, &["run", "--offline"]));
    assert!(included
        .lines()
        .any(|line| line == "COMPILED|included-second|build-first"));
    assert_eq!(published(&included), output);

    fs::write(root.join("public/build-input.txt"), "build-second").unwrap();
    let generated = success(&cli(root, &["run", "--offline"]));
    assert!(generated
        .lines()
        .any(|line| line == "COMPILED|included-second|build-second"));
    assert_eq!(published(&generated), output);
    let catalog = AssetCatalog::load(output).unwrap();
    assert_eq!(
        fs::read(catalog.object_path("logo.svg").unwrap()).unwrap(),
        b"included-second"
    );
    assert_eq!(
        fs::read(catalog.object_path("build-input.txt").unwrap()).unwrap(),
        b"build-second"
    );
}

#[test]
fn dev_finalizes_before_launch_and_no_assets_skips_publication() {
    let temp = fixture();
    let root = temp.path();
    let skipped = success(&cli(
        root,
        &[
            "build",
            "--profile",
            "dev",
            "--offline",
            "--no-check",
            "--no-assets",
        ],
    ));
    assert!(!skipped.contains("published "));
    assert!(!root.join("custom-target/proa-output").exists());
    let dev = success(&cli(
        root,
        &[
            "dev",
            "--no-restart",
            "--",
            "cargo",
            "run",
            "--offline",
            "--",
            "--probe",
        ],
    ));
    let manifest = main_manifest(&dev);
    let catalog = AssetCatalog::load(published(&dev)).unwrap();
    assert_eq!(&manifest, catalog.manifest());
    assert!(dev.contains("final cache: 1/1 reused"), "{dev}");
}
