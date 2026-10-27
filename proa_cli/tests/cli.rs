// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests for the `proa` binary: exit codes, output formats, and
//! the fmt/lint/check/config command surface.

use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::process::Stdio;
use std::process::{Command, Output};

fn proa(args: &[&str], dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_proa"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run proa binary")
}

#[cfg(unix)]
fn proa_without_tailwind(args: &[&str], dir: &Path) -> Output {
    proa_with_path(args, dir, "/usr/bin:/bin")
}

#[cfg(unix)]
fn proa_with_path(args: &[&str], dir: &Path, path: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_proa"))
        .args(args)
        .current_dir(dir)
        .env("PATH", path)
        .stdin(Stdio::null())
        .output()
        .expect("run proa binary without tailwindcss on PATH")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const CANONICAL_SOURCE: &str = r#"fn render() {
    html_sync! {
        <div class="card">"hi"</div>
    }
}
"#;

const NONCANONICAL_SOURCE: &str = r#"fn render() {
    html_sync! { <div   class="card">"hi"</div> }
}
"#;

#[cfg(unix)]
#[test]
fn new_site_noninteractive_uses_the_bundled_prebuilt_stylesheet() {
    let temp = tempfile::tempdir().unwrap();
    let output = proa_without_tailwind(&["new", "site", "demo", "--no-git"], temp.path());

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stderr(&output).contains("Tailwind CSS CLI v4 was not found on PATH"));
    assert!(temp.path().join("demo/styles/input.css").exists());
    assert!(temp.path().join("demo/public/styles.css").exists());
}

#[cfg(unix)]
#[test]
fn new_site_dry_run_never_checks_or_installs_tailwind() {
    let temp = tempfile::tempdir().unwrap();
    let output = proa_without_tailwind(
        &["new", "site", "demo", "--dry-run", "--json", "--no-git"],
        temp.path(),
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stderr(&output).contains("Tailwind CSS CLI v4 was not found"));
    assert!(stdout(&output).contains("\"operationId\": \"site.create\""));
    assert!(stdout(&output).contains("\"plannedFilesystemOperations\""));
    assert!(!temp.path().join("demo").exists());
}

#[test]
fn fmt_check_exits_nonzero_and_prints_diff_on_drift() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("page.rs"), NONCANONICAL_SOURCE).unwrap();

    let output = proa(&["fmt", "--check", "page.rs"], temp.path());
    assert!(!output.status.success());
    assert!(
        stdout(&output).contains("-    html_sync! { <div   class=\"card\">\"hi\"</div> }"),
        "expected a unified diff on stdout:\n{}",
        stdout(&output)
    );
    assert!(stderr(&output).contains("Proa macro formatting differs"));
}

#[test]
fn fmt_rewrites_then_check_passes() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("page.rs"), NONCANONICAL_SOURCE).unwrap();

    let rewrite = proa(&["fmt", "page.rs"], temp.path());
    assert!(rewrite.status.success(), "{}", stderr(&rewrite));

    let check = proa(&["fmt", "--check", "page.rs"], temp.path());
    assert!(check.status.success(), "{}", stderr(&check));
}

#[test]
fn fmt_check_passes_on_canonical_source() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("page.rs"), CANONICAL_SOURCE).unwrap();

    let output = proa(&["fmt", "--check", "page.rs"], temp.path());
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn lint_json_emits_valid_json_array() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("page.rs"), CANONICAL_SOURCE).unwrap();

    let output = proa(&["lint", "page.rs", "--json"], temp.path());
    assert!(output.status.success(), "{}", stderr(&output));
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert!(parsed.is_array());
}

#[test]
fn lint_strategy_cache_discovers_virtual_workspace_root() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(temp.path().join("page.rs"), CANONICAL_SOURCE).unwrap();

    let output = proa(
        &["lint", "page.rs", "--use-rsjs-strategy-cache"],
        temp.path(),
    );
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn lint_reports_perf_finding_with_location_in_json() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("page.rs"),
        r#"impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Card {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let label = format!("{}", self.title);
        label.render(cx)
    }
}
"#,
    )
    .unwrap();

    let output = proa(&["lint", "page.rs", "--json"], temp.path());
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    let findings = parsed.as_array().unwrap();
    assert!(
        findings.iter().any(|finding| {
            finding["code"]
                .as_str()
                .unwrap_or_default()
                .starts_with("perf/")
                && finding["line"].as_u64().is_some()
        }),
        "expected a perf finding: {findings:?}"
    );
}

#[test]
fn lint_accepts_rsjs_event_attrs_inside_rsjs_items() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("island.rs"),
        r#"#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let increment = rsjs::event_handler(|_| count.update(|n| n + 1));
        html_sync! {
            <button type="button" on_click={increment()}>{count.get()}</button>
        }.render(cx)
    }
}
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("plain.rs"),
        r#"fn render() {
    html_sync! {
        <button on_click={handler}>"Save"</button>
    };
}
"#,
    )
    .unwrap();

    let island = proa(&["lint", "island.rs", "--deny-warnings"], temp.path());
    assert!(island.status.success(), "{}", stderr(&island));

    let plain = proa(&["lint", "plain.rs", "--deny-warnings"], temp.path());
    assert!(!plain.status.success());
    assert!(stderr(&plain).contains("attr/event-name"));
}

#[test]
fn lint_errors_on_nonexistent_path() {
    let temp = tempfile::tempdir().unwrap();
    let output = proa(&["lint", "does/not/exist.rs"], temp.path());
    assert!(!output.status.success());
}

#[test]
fn check_fails_on_warning_unless_allowed() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("page.rs"),
        r#"impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Card {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let label = format!("{}", self.title);
        label.render(cx)
    }
}
"#,
    )
    .unwrap();

    let strict = proa(&["check", "page.rs"], temp.path());
    assert!(!strict.status.success());

    let allowed = proa(&["check", "page.rs", "--allow-warnings"], temp.path());
    assert!(allowed.status.success(), "{}", stderr(&allowed));
}

#[test]
fn config_validate_reports_typos_and_fails_on_bad_kind() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[package]\nname=\"demo\"\n").unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("proa.config.json"),
        r#"{
            "style": "default",
            "dev": { "prot": 3001 },
            "build": { "assets": [{ "kind": "webpack", "input": "a", "output": "b" }] }
        }"#,
    )
    .unwrap();

    let output = proa(&["config", "validate"], temp.path());
    assert!(!output.status.success());
    let text = stdout(&output);
    assert!(text.contains("dev.prot"), "{text}");
    assert!(text.contains("webpack"), "{text}");
}

#[test]
fn config_validate_json_is_machine_readable() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[package]\nname=\"demo\"\n").unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();

    let output = proa(&["config", "validate", "--json"], temp.path());
    assert!(output.status.success(), "{}", stderr(&output));
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert!(parsed["warnings"].is_array());
    assert!(parsed["errors"].is_array());
}

#[test]
fn completions_generate_for_zsh() {
    let temp = tempfile::tempdir().unwrap();
    let output = proa(&["completions", "zsh"], temp.path());
    assert!(output.status.success());
    assert!(stdout(&output).contains("#compdef proa"));
}

#[test]
fn lint_changed_errors_outside_a_git_repository() {
    let temp = tempfile::tempdir().unwrap();
    let output = proa(&["lint", "--changed"], temp.path());
    assert!(
        !output.status.success(),
        "git selection outside a repo must not report a false clean"
    );
}

#[test]
fn update_preview_does_not_install_and_invalid_versions_fail_before_network_access() {
    let temp = tempfile::tempdir().unwrap();
    let preview = proa(&["update", "--check", "--version", "9.8.7"], temp.path());
    assert!(preview.status.success(), "{}", stderr(&preview));
    assert!(stdout(&preview).contains("Requested: 9.8.7"));
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    let invalid = proa(&["update", "--version", "../../other"], temp.path());
    assert!(!invalid.status.success());
    assert!(stderr(&invalid).contains("invalid release version"));
    let snapshot = proa(&["update", "--version", "0.1.4-main.abc123"], temp.path());
    assert!(!snapshot.status.success());
    assert!(stderr(&snapshot).contains("stable releases only"));
    assert!(!stdout(&snapshot).contains("Downloading"));
    assert!(!stdout(&snapshot).contains("Building"));
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn standalone_bundles_switch_together_while_the_previous_binary_is_running() {
    let temp = tempfile::Builder::new()
        .prefix("proa bundles with spaces ")
        .tempdir()
        .unwrap();
    let public = temp.path().join("bin");
    let fixture = temp.path().join("fixture.rs");
    fs::write(
        &fixture,
        r#"fn main() {
        if std::env::args().any(|arg| arg == "--hold") {
            std::thread::sleep(std::time::Duration::from_secs(120));
        }
        println!("proa {}", env!("FIXTURE_VERSION"));
    }"#,
    )
    .unwrap();
    let names =
        ["proa", "cargo-proa"].map(|name| format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let mut sources = Vec::new();
    for version in ["1.2.3", "1.3.0"] {
        let source = temp.path().join(version);
        fs::create_dir(&source).unwrap();
        let output = Command::new("rustc")
            .arg(&fixture)
            .arg("-o")
            .arg(source.join(&names[0]))
            .env("FIXTURE_VERSION", version)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        fs::copy(source.join(&names[0]), source.join(&names[1])).unwrap();
        sources.push(source);
    }
    let install = |source: &Path, downgrade: bool| {
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("sh");
            command
                .args(["-c", "umask 077; exec \"$@\"", "--"])
                .arg(env!("CARGO_BIN_EXE_proa"));
            command
        };
        #[cfg(not(unix))]
        let mut command = Command::new(env!("CARGO_BIN_EXE_proa"));
        command
            .args(["install-self", "--source"])
            .arg(source)
            .arg("--bin-dir")
            .arg(&public);
        if downgrade {
            command.arg("--allow-downgrade");
        }
        command.current_dir(temp.path()).output().unwrap()
    };
    // Reinstalling the same version must still migrate a legacy flat layout.
    fs::create_dir(&public).unwrap();
    for name in &names {
        fs::copy(sources[0].join(name), public.join(name)).unwrap();
    }
    // Retry a migration into an existing shared root under a restrictive umask.
    // Access to retained executables must not depend on the updater's umask.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let root = public.with_file_name("bin.proa");
        fs::create_dir_all(root.join("releases")).unwrap();
        for path in [&root, &root.join("releases")] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let legacy_path = public.join(&names[0]).canonicalize().unwrap();
    let output = install(&sources[0], false);
    assert!(output.status.success(), "{}", stderr(&output));
    let previous = public.join(&names[0]).canonicalize().unwrap();
    assert_ne!(previous, legacy_path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let legacy = fs::read_dir(public.with_file_name("bin.proa"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("legacy-")
            })
            .unwrap();
        // Shared installations must retain access for users already running
        // the old CLI, which reopens its renamed executable during builds.
        assert_eq!(
            fs::metadata(&legacy).unwrap().permissions().mode() & 0o111,
            0o111
        );
        for name in &names {
            let output = Command::new(legacy.join(name))
                .arg("--version")
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(stdout(&output).trim(), "proa 1.2.3");
        }
    }
    struct Running(std::process::Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut running = Running(Command::new(&previous).arg("--hold").spawn().unwrap());
    let output = install(&sources[1], false);
    assert!(output.status.success(), "{}", stderr(&output));
    let selected = public.join(&names[0]).canonicalize().unwrap();
    assert_ne!(previous, selected);
    assert!(previous.exists());
    assert!(running.0.try_wait().unwrap().is_none());
    for name in &names {
        let output = Command::new(public.join(name))
            .arg("--version")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(stdout(&output).trim(), "proa 1.3.0");
    }
    // Main archives have a distinct distribution identity without changing
    // the compiled package version. Refreshing one must still switch bundles.
    let snapshot = format!("1.3.0-main.{}", "0".repeat(40));
    let output = Command::new(env!("CARGO_BIN_EXE_proa"))
        .args(["install-self", "--source"])
        .arg(&sources[1])
        .arg("--bin-dir")
        .arg(&public)
        .args([
            "--expected-version",
            "1.3.0",
            "--distribution-version",
            &snapshot,
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let snapshot_selected = public.join(&names[0]).canonicalize().unwrap();
    assert_ne!(snapshot_selected, selected);
    assert_eq!(
        fs::read_to_string(
            snapshot_selected
                .parent()
                .unwrap()
                .join("distribution-version")
        )
        .unwrap(),
        snapshot
    );
    let selected = snapshot_selected;
    // A stale automatic update must not undo the newer installation.
    let output = install(&sources[0], false);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(public.join(&names[0]).canonicalize().unwrap(), selected);
    // A mismatched pair fails before changing the selected bundle.
    fs::copy(sources[0].join(&names[1]), sources[1].join(&names[1])).unwrap();
    assert!(!install(&sources[1], false).status.success());
    assert_eq!(public.join(&names[0]).canonicalize().unwrap(), selected);
    // Explicit downgrades still switch both binaries through the same path.
    let output = install(&sources[0], true);
    assert!(output.status.success(), "{}", stderr(&output));
    for name in &names {
        let output = Command::new(public.join(name))
            .arg("--version")
            .output()
            .unwrap();
        assert_eq!(stdout(&output).trim(), "proa 1.2.3");
    }

    // Equivalent Unix destination paths must not bypass the managed-install
    // guard and fall back to copying old binaries through the public links.
    #[cfg(unix)]
    {
        let installer =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/install.sh"))
                .unwrap();
        let definitions = installer.rsplit_once("\nmain \"$@\"").unwrap().0;
        let script = format!(
            "{definitions}\nset_defaults\nINSTALL_DIR=\"$1\"\nresolve_bin_dir\nVERSION=1.2.3\nTMP_DIR=\"$2\"\nOS_NAME=Linux\nif install_bundle \"$3\"; then exit 0; else exit 0; fi\n"
        );
        let selected = public.join(&names[0]).canonicalize().unwrap();
        for suffix in ["", "/.", "//"] {
            let output = Command::new("sh")
                .args(["-c", &script, "--"])
                .arg(format!("{}{suffix}", public.display()))
                .arg(temp.path())
                .arg(&sources[0])
                .env_remove("PROA_INSTALL_HELPER")
                .current_dir(temp.path())
                .output()
                .unwrap();
            assert!(!output.status.success(), "legacy copy accepted {suffix:?}");
            assert!(stderr(&output).contains("current managed CLI"));
            assert_eq!(public.join(&names[0]).canonicalize().unwrap(), selected);
        }
    }
}

#[cfg(windows)]
#[test]
fn standalone_bundles_bootstrap_from_powershell_with_equivalent_paths() {
    let temp = tempfile::Builder::new()
        .prefix("proa installer with spaces ")
        .tempdir()
        .unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    for name in ["proa.exe", "cargo-proa.exe"] {
        fs::copy(env!("CARGO_BIN_EXE_proa"), source.join(name)).unwrap();
    }
    let harness = temp.path().join("bootstrap.ps1");
    fs::write(
        &harness,
        r#"param($Installer, $Source, $Location, $BinDir, $RequestedVersion)
$ErrorActionPreference = 'Stop'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($Installer, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
# Load the real installer without its network, toolchain, or PATH setup.
$entry = $ast.EndBlock.Statements[-1]
Invoke-Expression ([IO.File]::ReadAllText($Installer).Substring(0, $entry.Extent.StartOffset))
Set-StrictMode -Version Latest
$script:BundleAllowDowngrade = $false
Set-Location -LiteralPath $Location
if (-not (Publish-ProaBundle $Source $BinDir $RequestedVersion)) { throw 'unexpected legacy fallback' }
"#,
    )
    .unwrap();
    let installer = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/install.ps1");
    for (index, (input, leaf)) in [
        (r".\bin\", "bin"),
        ("./bin/", "bin"),
        (r".\bin\.", "bin"),
        (".", ""),
    ]
    .into_iter()
    .enumerate()
    {
        let location = temp.path().join(format!("shell location {index}"));
        fs::create_dir(&location).unwrap();
        let public = location.join(leaf);
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&harness)
            .arg(&installer)
            .arg(&source)
            .arg(&location)
            .arg(input)
            .arg(env!("CARGO_PKG_VERSION"))
            .env_remove("PROA_INSTALL_HELPER")
            // PowerShell's Set-Location must win over the process directory.
            .current_dir(temp.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            stdout(&output),
            stderr(&output)
        );
        for name in ["proa.exe", "cargo-proa.exe"] {
            let installed = Command::new(public.join(name))
                .arg("--version")
                .output()
                .unwrap();
            assert!(installed.status.success());
            assert_eq!(
                stdout(&installed).trim(),
                concat!("proa ", env!("CARGO_PKG_VERSION"))
            );
        }
        assert!(!public.join(".proa").exists());
        let selected = public.join("proa.exe").canonicalize().unwrap();
        assert_eq!(
            selected
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .file_name()
                .unwrap(),
            "releases"
        );
    }
}

#[test]
fn dev_exposes_live_reload_controls_and_rejects_an_unhonourable_debounce() {
    let temp = tempfile::tempdir().unwrap();
    let help = proa(&["dev", "--help"], temp.path());
    assert!(help.status.success(), "{}", stderr(&help));
    let help = stdout(&help);
    for flag in [
        "--no-reload",
        "--no-css-hot-swap",
        "--reload-port",
        "--debounce-ms",
    ] {
        assert!(help.contains(flag), "`proa dev --help` is missing {flag}");
    }

    // The watcher polls every 100ms, so a shorter window promises a precision
    // it cannot deliver. Rejecting it must cost nothing: no build, no bound
    // port, no spawned server.
    let rejected = proa(
        &[
            "dev",
            "--root",
            temp.path().to_str().unwrap(),
            "--debounce-ms",
            "20",
            "--no-restart",
        ],
        temp.path(),
    );
    assert!(!rejected.status.success());
    assert!(
        stderr(&rejected).contains("below the 100ms file-watch poll interval"),
        "{}",
        stderr(&rejected)
    );
}
