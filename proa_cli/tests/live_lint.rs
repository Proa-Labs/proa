// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn persistent_worker_checks_unsaved_views_and_survives_invalid_requests() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    let path = root.path().join("src/lib.rs");
    let source = "use proa_core::{WebRenderSync,MdRenderSync}; use proa_macros::{html_sync,md_sync}; struct Product { title:String } impl WebRenderSync for Product { fn render(self,cx:&mut Cx)->Result {html_sync! {{self.title}}.render(cx)} } impl MdRenderSync for Product {fn render_md(self,cx:&mut Cx)->Result {md_sync! {r###\"{self.title}\"###}.render_md(cx)}}";
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='worker_fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    std::fs::write(&path, source).unwrap();
    let mut worker = Command::new(env!("CARGO_BIN_EXE_proa"))
        .args(["lint", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = worker.stdin.take().unwrap();
    let mut stdout = BufReader::new(worker.stdout.take().unwrap());
    let mut request = |value: &str| {
        writeln!(stdin, "{value}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        assert!(stdout.read_line(&mut line).unwrap() > 0);
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    };
    let missing = source.replace("r###\"{self.title}\"###", "r###\"\"###");
    let response = request(
        &serde_json::json!({"id":1,"overlays":[{"path":path,"source":missing}]}).to_string(),
    );
    assert_eq!(response["id"], 1);
    assert!(response["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "render/content-coverage"));
    assert!(request("not json")["error"].is_string());
    let malformed = request(r#"{"id":2,"overlays":false}"#);
    assert_eq!(malformed["id"], 2);
    assert!(malformed["error"].is_string());
    let fixed = request(
        &serde_json::json!({"id":3,"overlays":[{"path":path,"source":source}]}).to_string(),
    );
    assert_eq!(fixed["id"], 3);
    assert!(!fixed["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "render/content-coverage"));
    drop(stdin); // EOF shuts down a persistent worker cleanly.
    assert!(worker.wait().unwrap().success());
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}
