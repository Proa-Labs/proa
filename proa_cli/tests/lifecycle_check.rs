// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[derive(Debug, PartialEq, Eq)]
struct CacheReuse {
    descriptor_reused: usize,
    descriptor_total: usize,
    final_reused: usize,
    final_total: usize,
}

fn run_check(root: &Path, target: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-proa"))
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target)
        .args([
            "check",
            "--root",
            root.to_str().unwrap(),
            "-p",
            "proa_linked_check_fixture",
            "--bin",
            "proa_linked_check_fixture",
            "--offline",
            "--allow-warnings",
        ])
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(output),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parse_count(value: &str) -> (usize, usize) {
    let (reused, total) = value
        .split_once('/')
        .unwrap_or_else(|| panic!("invalid cache count `{value}`"));
    (reused.parse().unwrap(), total.parse().unwrap())
}

fn cache_reuse(output: &Output) -> CacheReuse {
    let output = stdout(output);
    let line = output
        .lines()
        .find_map(|line| line.strip_prefix("linked RSJS check cache: "))
        .unwrap_or_else(|| panic!("missing linked check cache counters in:\n{output}"));
    let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
    assert_eq!(fields.len(), 8, "unexpected cache counter shape: {line}");
    assert_eq!(fields[0], "descriptor", "unexpected cache counter: {line}");
    assert_eq!(fields[2], "Cargo", "unexpected cache counter: {line}");
    assert_eq!(fields[3], "artifacts", "unexpected cache counter: {line}");
    assert_eq!(fields[4], "reused;", "unexpected cache counter: {line}");
    assert_eq!(fields[5], "final", "unexpected cache counter: {line}");
    assert_eq!(fields[7], "reused", "unexpected cache counter: {line}");
    let (descriptor_reused, descriptor_total) = parse_count(fields[1]);
    let (final_reused, final_total) = parse_count(fields[6]);
    CacheReuse {
        descriptor_reused,
        descriptor_total,
        final_reused,
        final_total,
    }
}

fn linked_plan(output: &Output) -> String {
    stdout(output)
        .lines()
        .find_map(|line| line.strip_prefix("linked RSJS plan "))
        .and_then(|line| line.split_once(" for package ").map(|(plan, _)| plan))
        .unwrap_or_else(|| panic!("missing linked plan in:\n{}", stdout(output)))
        .to_string()
}

#[test]
fn cargo_proa_check_links_a_real_component_and_unstaged_builds_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    let proa_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = \"proa_linked_check_fixture\"\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\n\
             [workspace]\n\n\
             [dependencies]\n\
             proa_core = {{ path = {:?} }}\n\
             proa_macros = {{ path = {:?} }}\n\
             rsjs = {{ path = {:?} }}\n",
            proa_root.join("proa_core"),
            proa_root.join("proa_macros"),
            proa_root.join("rsjs"),
        ),
    )
    .unwrap();
    let source = temp.path().join("src/main.rs");
    let initial_source = r#"
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;
use rsjs::web::dom;

struct StatusBadge {
    label: &'static str,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StatusBadge {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = rsjs::signal(String::new());
        let update = rsjs::event_handler(|event: rsjs::InputEvent<dom::HtmlInputElement>| {
            let input = event.current_target();
            let next = input.value();
            value.set(next);
        });
        proa_macros::html_sync! {
            <section>
                <input on_input={update()} />
                <strong>{self.label}{value.get()}</strong>
            </section>
        }
        .render(cx)
    }
}

fn main() {
    let _ = StatusBadge { label: "ready" };
}
"#;
    fs::write(&source, initial_source).unwrap();

    // This standalone fixture resolves its own dependency graph. Prepare it
    // before the offline checks instead of relying on the CLI test runner's
    // registry cache to already contain the framework's dependencies.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let fetched = Command::new(cargo)
        .current_dir(temp.path())
        .arg("fetch")
        .output()
        .unwrap();
    assert_success(&fetched);

    let cargo_proa = env!("CARGO_BIN_EXE_cargo-proa");
    let formatted = Command::new(cargo_proa)
        .current_dir(temp.path())
        .args(["fmt", "src/main.rs"])
        .output()
        .unwrap();
    assert!(
        formatted.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&formatted.stdout),
        String::from_utf8_lossy(&formatted.stderr)
    );

    let target = temp.path().join("outer-target");
    let checked = run_check(temp.path(), &target);
    assert_success(&checked);
    let first_cache = cache_reuse(&checked);
    assert!(first_cache.descriptor_total > 0, "{first_cache:?}");
    assert!(first_cache.final_total > 0, "{first_cache:?}");
    assert_eq!(first_cache.descriptor_reused, 0, "{first_cache:?}");
    assert_eq!(first_cache.final_reused, 0, "{first_cache:?}");
    let first_plan = linked_plan(&checked);
    let stdout = stdout(&checked);
    assert!(
        stdout.contains("running linked RSJS Cargo check:"),
        "{stdout}"
    );
    assert!(stdout.contains("linked RSJS plan"), "{stdout}");
    assert!(stdout.contains("proa_linked_check_fixture"), "{stdout}");
    assert!(stdout.contains("final Cargo check artifacts:"), "{stdout}");

    let unchanged = run_check(temp.path(), &target);
    assert_success(&unchanged);
    let unchanged_cache = cache_reuse(&unchanged);
    assert_eq!(
        unchanged_cache.descriptor_reused, unchanged_cache.descriptor_total,
        "{unchanged_cache:?}"
    );
    assert_eq!(
        unchanged_cache.final_reused, unchanged_cache.final_total,
        "{unchanged_cache:?}"
    );
    assert_eq!(linked_plan(&unchanged), first_plan);

    let plans = target
        .join(proa_cli::rsjs::build::TOOL_NAMESPACE_DIR)
        .join(if cfg!(windows) { "p" } else { "plans" });
    let plan_paths = fs::read_dir(&plans)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".semantic-plan.json"))
        })
        .collect::<Vec<_>>();
    assert_eq!(plan_paths.len(), 1, "{plan_paths:?}");
    let plan: serde_json::Value =
        serde_json::from_slice(&fs::read(&plan_paths[0]).unwrap()).unwrap();
    assert_eq!(
        plan["format"],
        serde_json::Value::String("proa.rsjs.semantic_link_plan".to_string())
    );
    assert_eq!(plan["components"].as_array().unwrap().len(), 1);

    let changed_source = initial_source
        .replace("strong", "em")
        .replace("label: \"ready\"", "label: \"changed\"");
    fs::write(&source, changed_source).unwrap();
    let source_changed = run_check(temp.path(), &target);
    assert_success(&source_changed);
    let changed_cache = cache_reuse(&source_changed);
    assert!(
        changed_cache.descriptor_reused < changed_cache.descriptor_total,
        "{changed_cache:?}"
    );
    assert!(
        changed_cache.final_reused < changed_cache.final_total,
        "{changed_cache:?}"
    );
    assert_ne!(linked_plan(&source_changed), first_plan);

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let plain_check_target = temp.path().join("plain-check-target");
    let plain_check = Command::new(&cargo)
        .current_dir(temp.path())
        .args([
            "check",
            "--offline",
            "--target-dir",
            plain_check_target.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        plain_check.status.success(),
        "plain Cargo check must retain local/provisional type checking:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&plain_check.stdout),
        String::from_utf8_lossy(&plain_check.stderr)
    );

    let plain_build = Command::new(&cargo)
        .current_dir(temp.path())
        .args([
            "build",
            "--offline",
            "--target-dir",
            plain_check_target.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !plain_build.status.success(),
        "plain Cargo build must not silently link a provisional RSJS binary"
    );
    let stderr = String::from_utf8_lossy(&plain_build.stderr);
    assert!(
        stderr.contains("RSJS_ERROR_plain_cargo_build_is_not_deployable_use_cargo_proa_build"),
        "{stderr}"
    );

    let plain_target = temp.path().join("plain-final-target");
    let plain_final = Command::new(cargo)
        .current_dir(temp.path())
        .env("RSJS_FINAL_PHASE", "1")
        .env("RSJS_FINAL_CARGO_UNIT_STATUS", "mapped-v1")
        .env("RSJS_CARGO_UNIT_ID", "11".repeat(32))
        .args([
            "build",
            "--offline",
            "--target-dir",
            plain_target.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !plain_final.status.success(),
        "plain final-mode Cargo must not produce a deployable artifact without an exact plan"
    );
    let stderr = String::from_utf8_lossy(&plain_final.stderr);
    assert!(stderr.contains("RSJS_PLAN_ID"), "{stderr}");
    assert!(stderr.contains("cargo proa build"), "{stderr}");
}
