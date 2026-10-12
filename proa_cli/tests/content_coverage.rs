// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_proa"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
const HTML:&str="use crate::product::{Product,RestockNotice};use proa_core::WebRenderSync;use proa_macros::html_sync;impl WebRenderSync for Product {fn render(self,cx:&mut Cx)->Result{html_sync! { {self.title} }.render(cx)}}";
const MD:&str="use crate::product::Product;use proa_core::MdRenderSync;use proa_macros::md_sync;impl MdRenderSync for Product {fn render_md(self,cx:&mut Cx)->Result{md_sync! {r###\"{self.title}\"###}.render_md(cx)}}";
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    for (path, source) in [
        (
            "Cargo.toml",
            "[package]\nname=\"coverage_fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        ),
        ("src/lib.rs", "mod product;mod html;mod md;"),
        (
            "src/product.rs",
            "pub struct Product {pub title:String,pub restock_at:String}pub struct RestockNotice;",
        ),
        ("src/html.rs", HTML),
        ("src/md.rs", MD),
    ] {
        std::fs::write(root.path().join(path), source).unwrap();
    }
    assert!(cli(root.path(), &["fmt", "src"]).status.success());
    git(root.path(), &["init", "-q"]);
    git(root.path(), &["add", "."]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=Coverage test",
            "-c",
            "user.email=coverage@example.invalid",
            "commit",
            "-qm",
            "initial fixture",
        ],
    );
    root
}
fn diagnostics(output: &Output) -> Vec<serde_json::Value> {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
#[test]
fn changed_html_loads_unchanged_markdown_and_missing_content_fails_by_default() {
    let root = fixture();
    std::fs::write(
        root.path().join("src/html.rs"),
        HTML.replace(
            "{self.title}",
            "{self.title} {RestockNotice::new(self.restock_at)}",
        ),
    )
    .unwrap();
    let output = cli(root.path(), &["lint", "--changed", "--json"]);
    assert!(!output.status.success());
    let diagnostics = diagnostics(&output);
    let missing = diagnostics
        .iter()
        .find(|d| d["code"] == "render/content-coverage")
        .unwrap();
    assert_eq!(missing["severity"], "error");
    assert!(missing["message"]
        .as_str()
        .unwrap()
        .contains("RestockNotice"));
    assert!(missing["labels"][0]["range"]["path"]
        .as_str()
        .unwrap()
        .ends_with("src/md.rs"));
    assert!(missing["labels"][1]["range"]["path"]
        .as_str()
        .unwrap()
        .ends_with("src/html.rs"));
    assert_eq!(
        missing["suggestions"][0]["replacement"],
        "proa_macros::omit!(crate::product::RestockNotice);"
    );
    let text = cli(root.path(), &["lint", "--changed"]);
    assert!(!text.status.success());
    let stderr = String::from_utf8_lossy(&text.stderr);
    assert!(stderr.contains("error[render/content-coverage]"));
    assert!(stderr.contains("-->") && stderr.contains(":::"));
    let github_output = cli(root.path(), &["lint", "--changed", "--github"]);
    assert!(!github_output.status.success());
    let github = String::from_utf8_lossy(&github_output.stdout);
    assert!(github.lines().any(|line| {
        line.starts_with("::error ") && line.contains("title=render/content-coverage::")
    }));
    assert!(github.contains("title=render/content-coverage"));
    assert!(github.contains("src/html.rs"));
    assert!(github.contains("omit!"));
}
#[test]
fn staged_paths_use_working_tree_contents_and_checks_enforce_missing_content() {
    let root = fixture();
    let md = root.path().join("src/md.rs");
    let clean = std::fs::read_to_string(&md).unwrap();
    std::fs::write(&md, clean.replace("{self.title}", "")).unwrap();
    git(root.path(), &["add", "src/md.rs"]);
    std::fs::write(&md, &clean).unwrap();
    let output = cli(root.path(), &["lint", "--staged", "--json"]);
    assert!(output.status.success());
    assert!(!diagnostics(&output)
        .iter()
        .any(|d| d["code"] == "render/content-coverage"));
    std::fs::write(&md, clean.replace("{self.title}", "")).unwrap();
    let output = cli(root.path(), &["lint", "--staged", "--json"]);
    assert!(!output.status.success());
    assert!(diagnostics(&output)
        .iter()
        .any(|d| d["code"] == "render/content-coverage"));
    assert!(cli(root.path(), &["fmt", "src"]).status.success());
    let check = cli(root.path(), &["check", "src", "--source-only"]);
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stderr).contains("error[render/content-coverage]"));
    let allow = cli(
        root.path(),
        &["check", "src", "--source-only", "--allow-warnings"],
    );
    assert!(!allow.status.success());
    let warnings = cli(root.path(), &["lint", "src", "--deny-warnings"]);
    assert!(!warnings.status.success());
}

#[test]
fn unchecked_coverage_is_visible_and_obeys_warning_policy() {
    let root = fixture();
    std::fs::write(
        root.path().join("src/md.rs"),
        MD.replace("{self.title}", "{opaque(self.title)}"),
    )
    .unwrap();
    assert!(cli(root.path(), &["fmt", "src"]).status.success());

    let output = cli(root.path(), &["lint", "src", "--json"]);
    assert!(output.status.success());
    let diagnostics = diagnostics(&output);
    assert!(diagnostics.iter().any(|d| {
        d["code"] == "render/content-coverage-unchecked" && d["severity"] == "warning"
    }));
    assert!(!diagnostics.iter().any(|d| d["severity"] == "error"));

    let text = cli(root.path(), &["lint", "src"]);
    assert!(text.status.success());
    assert!(String::from_utf8_lossy(&text.stderr)
        .contains("warning[render/content-coverage-unchecked]"));
    let github = cli(root.path(), &["lint", "src", "--github"]);
    assert!(github.status.success());
    assert!(String::from_utf8_lossy(&github.stdout).lines().any(|line| {
        line.starts_with("::warning ") && line.contains("title=render/content-coverage-unchecked::")
    }));

    for args in [
        vec!["lint", "src", "--deny-warnings"],
        vec!["lint", "src", "--deny", "warnings"],
        vec!["lint", "src", "--deny", "render/content-coverage-unchecked"],
        vec!["check", "src", "--source-only"],
    ] {
        assert!(!cli(root.path(), &args).status.success(), "{args:?}");
    }
    assert!(cli(
        root.path(),
        &["check", "src", "--source-only", "--allow-warnings"]
    )
    .status
    .success());
}

#[test]
fn intentional_omissions_pass_and_discovery_status_remains_informational() {
    let root = fixture();
    std::fs::write(
        root.path().join("src/md.rs"),
        MD.replace("md_sync!", "proa_macros::omit!(self.title); md_sync!")
            .replace("{self.title}", ""),
    )
    .unwrap();
    assert!(cli(root.path(), &["fmt", "src"]).status.success());
    assert!(cli(root.path(), &["lint", "src", "--deny-warnings"])
        .status
        .success());
    assert!(cli(root.path(), &["check", "src", "--source-only"])
        .status
        .success());

    let output = cli(root.path(), &["lint", "src", "--content-summary", "--json"]);
    assert!(output.status.success());
    let diagnostics = diagnostics(&output);
    assert!(!diagnostics.is_empty());
    assert!(diagnostics.iter().all(|d| d["severity"] == "info"));
}
