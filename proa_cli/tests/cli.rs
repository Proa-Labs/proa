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
