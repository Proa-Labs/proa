// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn worker_recovers_from_invalid_requests_and_exits_on_eof() {
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
    let response = request(r#"{"id":1,"overlays":[]}"#);
    assert_eq!(response["id"], 1);
    assert!(request("not json")["error"].is_string());
    let malformed = request(r#"{"id":2,"overlays":false}"#);
    assert_eq!(malformed["id"], 2);
    assert!(malformed["error"].is_string());
    let fixed = request(r#"{"id":3,"overlays":[]}"#);
    assert_eq!(fixed["id"], 3);
    assert_eq!(fixed["diagnostics"], serde_json::json!([]));
    drop(stdin); // EOF shuts down a persistent worker cleanly.
    assert!(worker.wait().unwrap().success());
}
