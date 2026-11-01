// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{fs, process::Command};

#[test]
fn file_scoped_commands_reject_invalid_markdown_without_rewriting_it() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("invalid.rs");
    let source = "fn main() { let _ = md_sync! { r###\"{if false {A} elsse {B}}\"### }; }";
    fs::write(&file, source).unwrap();
    for args in [
        vec!["fmt", "--verify"],
        vec!["lint", "--deny", "warnings"],
        vec!["check"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_proa"))
            .args(&args)
            .arg(&file)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "{args:?} accepted invalid Markdown"
        );
        let diagnostics = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            diagnostics.contains("parse/template"),
            "{args:?}: {diagnostics}"
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), source);
    }
}

#[test]
fn named_argument_templates_format_and_pass_file_scoped_checks() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("valid.rs");
    fs::write(
        &file,
        r####"use proa_macros::md_sync;
fn main() {
    let _ = md_sync! { r###"
        {if true {
        A 6" screen.
        if false {hidden}
        {title}
        }}
        "###, title = "Example"
    };
}
"####,
    )
    .unwrap();
    for args in [
        vec!["fmt", "--verify"],
        vec!["fmt", "--check", "--verify"],
        vec!["lint", "--deny", "warnings"],
        vec!["check"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_proa"))
            .args(&args)
            .arg(&file)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(fs::read_to_string(&file)
        .unwrap()
        .contains("            {title}\n"));
}
