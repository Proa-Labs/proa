// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Inline-const assertions require code generation; `cargo check`/trybuild's
//! compile-fail path alone cannot exercise their monomorphization errors.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn source(surface: &str, role: &str, has_hook: bool) -> String {
    let (render_trait, context, hook_name, operation, method, dispatch, sync) = match surface {
        "local" => (
            "WebRender",
            "WebContext",
            "render_head",
            "HeadOperation",
            "render",
            "let _ = head_dispatch::begin(Example, &mut cx);",
            false,
        ),
        "send" => (
            "WebRenderSend",
            "SendWebContext",
            "render_head_send",
            "SendHeadOperation",
            "render_send",
            "let _ = head_dispatch::send_begin(Example, &mut cx);",
            false,
        ),
        "sync" => (
            "WebRenderSync",
            "WebContext",
            "render_head",
            "SyncHeadOperation",
            "render",
            "let _ = head_dispatch::sync(Example, &mut cx);",
            true,
        ),
        "send_sync" => (
            "WebRenderSendSync",
            "SendWebContext",
            "render_head_send_sync",
            "SyncHeadOperation",
            "render_send_sync",
            "let _ = head_dispatch::send_sync(Example, &mut cx);",
            true,
        ),
        _ => unreachable!(),
    };
    let role = if role.is_empty() {
        String::new()
    } else {
        format!("const ROLE: ComponentRole = ComponentRole::{role};")
    };
    let hook = if !has_hook {
        String::new()
    } else if sync {
        format!("fn {hook_name}(&self, cx: &mut {context}) -> impl {operation} {{ cx.head_mut().title(\"Hook\") }}")
    } else {
        format!("fn {hook_name}(&self, cx: &mut {context}) -> impl {operation} {{ async move {{ cx.head_mut().title(\"Hook\") }} }}")
    };
    let body = if sync {
        format!("fn {method}(self, _: &mut {context}) -> Result<(), WriteError> {{ Ok(()) }}")
    } else {
        let send = if surface == "send" { " + Send" } else { "" };
        format!("fn {method}(self, _: &mut {context}) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>{send}> {{ RenderOutcome::<std::future::Ready<_>>::Done(Ok(())) }}")
    };
    format!("use proa_core::*;\nstruct Example;\nimpl {render_trait} for Example {{\n{role}\n{hook}\n{body}\n}}\nfn main() {{ let mut cx = {context}::new(); {dispatch} }}\n")
}

#[test]
fn head_hooks_require_explicit_roles_on_every_render_surface() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("proa-head-role-{}-{stamp}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let fixture = Fixture(dir);
    fs::create_dir(fixture.0.join("src")).unwrap();
    let core_path = serde_json::to_string(&workspace.join("proa_core").to_str().unwrap()).unwrap();
    fs::write(fixture.0.join("Cargo.toml"), format!("[workspace]\n[package]\nname = \"proa-head-role-ui\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[dependencies]\nproa_core = {{ path = {core_path} }}\n")).unwrap();
    fs::copy(workspace.join("Cargo.lock"), fixture.0.join("Cargo.lock")).unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let version = Command::new(&cargo).arg("--version").output().unwrap();
    assert!(version.status.success());
    let version = String::from_utf8(version.stdout).unwrap();
    let version = version.split_whitespace().nth(1).unwrap();
    // Stable and MSRV tests can run concurrently without rebuilding each
    // other's dependency artifacts on every fixture.
    let target = workspace.join("target/head-role-ui").join(version);
    for surface in ["local", "send", "sync", "send_sync"] {
        // NoHead remains legal for ordinary components and inherited-only Pages.
        // Both explicit metadata roles accept actual hooks.
        for (role, has_hook, valid) in [
            ("", false, true),
            ("PAGE", false, true),
            ("LAYOUT", true, true),
            ("PAGE", true, true),
            ("", true, false),
            ("COMPONENT", true, false),
        ] {
            fs::write(
                fixture.0.join("src/main.rs"),
                source(surface, role, has_hook),
            )
            .unwrap();
            let output = Command::new(&cargo)
                .args(["build", "--offline", "--quiet", "--manifest-path"])
                .arg(fixture.0.join("Cargo.toml"))
                .arg("--target-dir")
                .arg(&target)
                .env("CARGO_TERM_COLOR", "never")
                .output()
                .unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.success(),
                valid,
                "surface={surface}, role={role:?}, hook={has_hook}:\n{stderr}"
            );
            if !valid {
                assert!(stderr.contains("E0080"), "wrong failure:\n{stderr}");
                assert!(
                    stderr.contains("render_head requires an explicit role"),
                    "wrong failure:\n{stderr}"
                );
                assert!(
                    stderr.contains("ComponentRole::LAYOUT or ComponentRole::PAGE"),
                    "missing guidance:\n{stderr}"
                );
            }
        }
    }
}
