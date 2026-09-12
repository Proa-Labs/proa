// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[rsjs::client(class)]
pub mod toggle_class {
    pub struct Toggle {
        enabled: bool,
    }

    impl Toggle {
        pub fn new(enabled: bool) -> Self {
            Self { enabled }
        }

        pub fn toggle(&mut self) {
            let next = !self.enabled;
            self.enabled = next;
        }

        pub fn enabled(&self) -> bool {
            self.enabled
        }

        pub fn toggle_twice(&mut self) {
            self.toggle();
            self.toggle();
        }
    }
}

#[rsjs::client(class)]
pub mod checked_counter_class {
    pub struct Counter {
        value: i32,
    }

    impl Counter {
        pub fn new(value: i32) -> Self {
            Self { value }
        }

        pub fn increment(&mut self) {
            self.value += 1_i32;
        }

        pub fn value(&self) -> i32 {
            self.value
        }
    }
}

#[test]
fn class_module_is_registered_as_a_real_esm_export() {
    let manifest = rsjs::manifest();
    let class = manifest
        .client_classes
        .iter()
        .copied()
        .find(|entry| entry.module_path.ends_with("::toggle_class"))
        .expect("the client class module should be link-time registered");

    assert_eq!(class.export_names, &["Toggle"]);
    assert!(class.js_source.contains("export class Toggle"));
    assert!(class.js_source.contains("#__rsjs_state"));
    assert!(class.js_source.contains("toggle()"));
    assert!(class.js_source.contains("enabled()"));
    assert!(class.js_source.contains("toggle_twice()"));
    assert!(
        rsjs::client_class_filename(class.module_path).starts_with("client_class_toggle_class_")
    );
}

#[test]
fn class_module_preserves_new_methods_identity_and_private_state_in_node() {
    let node = std::env::var_os("NODE").unwrap_or_else(|| OsString::from("node"));
    if let Err(error) = Command::new(&node).arg("--version").output() {
        assert!(
            std::env::var_os("CI").is_none(),
            "Node is required for generated client-class execution tests in CI: {error}"
        );
        return;
    }
    let class = rsjs::manifest()
        .client_classes
        .iter()
        .copied()
        .find(|entry| entry.module_path.ends_with("::toggle_class"))
        .expect("the client class module should be link-time registered");
    let mut script = class.js_source.to_owned();
    script.push_str(
        r#"
const toggle = new Toggle(false);
if (!(toggle instanceof Toggle)) throw new Error("instanceof failed");
if (toggle.enabled() !== false) throw new Error("constructor/read failed");
toggle.toggle();
if (toggle.enabled() !== true) throw new Error("mutation did not persist");
toggle.toggle_twice();
if (toggle.enabled() !== true) throw new Error("nested method calls drifted");
if (Object.keys(toggle).length !== 0) throw new Error("private state leaked");
"#,
    );
    let output = Command::new(node)
        .args(["--input-type=module", "--eval", &script])
        .output()
        .expect("Node should execute the generated class module");
    assert!(
        output.status.success(),
        "Node rejected generated class JS:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn class_runtime_imports_can_be_rewritten_for_a_nonroot_mount() {
    let class = rsjs::manifest()
        .client_classes
        .iter()
        .copied()
        .find(|entry| entry.module_path.ends_with("::checked_counter_class"))
        .expect("the checked counter class should be registered");
    assert!(class.js_source.contains(r#"from "/rsjs-runtime.js""#));

    let rewritten = rsjs::rewrite_runtime_import_specifiers(
        class.js_source,
        "/app/rsjs-runtime.1234.js",
        "/app/rsjs-runtime-query.5678.js",
    );
    assert!(rewritten.contains(r#"from "/app/rsjs-runtime.1234.js""#));
    assert!(!rewritten.contains(r#"from "/rsjs-runtime.js""#));
}

#[test]
fn manifest_asset_writer_materializes_the_class_module() {
    let class = rsjs::manifest()
        .client_classes
        .iter()
        .copied()
        .find(|entry| entry.module_path.ends_with("::toggle_class"))
        .expect("the client class module should be link-time registered");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should follow the Unix epoch")
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("rsjs-client-class-{}-{unique}", std::process::id()));
    rsjs::write_manifest_assets(&directory).expect("class assets should materialize");
    let path = directory.join(rsjs::client_class_filename(class.module_path));
    let written = std::fs::read_to_string(&path).expect("class module file should exist");
    assert_eq!(written, class.js_source);
    std::fs::remove_dir_all(&directory).expect("temporary class asset directory should clean up");
}
