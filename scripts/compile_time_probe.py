#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Measure warm consumer-app builds without inheriting Proa workspace profiles.

Creates a standalone app in a new temporary directory. Dependencies point at
this checkout; the app and all Cargo targets are outside the checkout. Results
and logs stay under the fixture's target directory (excluded from RSJS inputs).
"""

import argparse
import json
import os
from pathlib import Path
import re
import shlex
import statistics
import subprocess
import tempfile
import time


def create_fixture(root, repo, pages, split_islands=False):
    (root / "src").mkdir()
    dependencies = {
        "proa_core": "proa_core",
        "proa_macros": "proa_macros",
        "proa_framework_axum": "proa_framework/proa_framework_axum",
        "rsjs": "rsjs",
    }
    manifest = '''[package]
name = "proa_compile_probe"
version = "0.0.0"
edition = "2021"

[workspace]

[features]
islands = []

[dependencies]
axum = "0.7.9"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net"] }
'''
    for name, relative in dependencies.items():
        manifest += f'{name} = {{ path = {json.dumps(str(repo / relative))} }}\n'
    manifest += '''
[profile.nodebug]
inherits = "dev"
debug = 0

[profile.macroopt]
inherits = "nodebug"

[profile.macroopt.build-override]
opt-level = 3
'''
    (root / "Cargo.toml").write_text(manifest)
    (root / "rust-toolchain.toml").write_text((repo / "rust-toolchain.toml").read_text())
    (root / "src/counter.rs").write_text('''use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
use rsjs::{event_handler, rsjs, signal};

pub struct Counter { pub initial: i64 }

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Counter {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let count = signal::<i64>(self.initial);
        let increment = event_handler(|_: rsjs::Event| count.update(|c| c + 1));
        html_sync! {
            <div data-counter>
                <button type="button" on_click={increment()}>"Increment"</button>
                <span>{count.get()}</span>
            </div>
        }.render(cx)
    }
}
''')
    main = '''use axum::{response::Html, routing::get, Router};
use proa_core::{WebContext, WebRenderSync};
#[cfg(feature = "islands")]
mod counter;
'''
    for index in range(pages):
        main += f"mod page_{index};\n"
        cards = '\n'.join(
            f'<article data-card><h2>"Card {card}"</h2><p>{{self.title}}</p><span>{{self.value + {card}}}</span></article>'
            for card in range(8)
        )
        source = f'''use proa_core::{{DataLoader, WebContext, WebRenderSync, WriteError}};
use proa_macros::html_sync;

pub struct Page<'a> {{ pub title: &'a str, pub value: i64 }}

impl<L: DataLoader> WebRenderSync<L> for Page<'_> {{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {{
        html_sync! {{
            <main data-page><h1>"EDIT_MARKER_A"</h1>{cards}</main>
        }}.render(cx)?;
        #[cfg(feature = "islands")]
        crate::counter::Counter {{ initial: self.value }}.render(cx)?;
        Ok(())
    }}
}}
'''
        (root / f"src/page_{index}.rs").write_text(source)
    main += '''
#[tokio::main]
async fn main() {
    let mut app = Router::new();
'''
    for index in range(pages):
        main += f'''    app = app.route("/page/{index}", get(|| async {{
        let mut cx = WebContext::new();
        page_{index}::Page {{ title: "dynamic <input>", value: {index} }}.render(&mut cx).unwrap();
        Html(cx.into_output().into_string())
    }}));
'''
    main += '''    if let (Ok(port), Ok(token)) = (std::env::var("PROA_DEV_RELOAD_PORT"), std::env::var("PROA_DEV_RELOAD_TOKEN")) {
        app = app.layer(proa_framework_axum::DevReloadLayer::new(port.parse().unwrap(), &token));
    }
    if std::env::args().any(|arg| arg == "--probe") { return; }
    let port = std::env::var("PORT").unwrap_or_else(|_| "39147".into());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
'''
    (root / "src/main.rs").write_text(main)
    if split_islands:
        library = root / "stable_island"
        (library / "src").mkdir(parents=True)
        counter = (root / "src/counter.rs").read_text()
        (library / "src/lib.rs").write_text(counter)
        (library / "Cargo.toml").write_text('[package]\nname = "probe_stable_island"\nversion = "0.0.0"\nedition = "2021"\n\n[dependencies]\n' + ''.join(
            f'{name} = {{ path = {json.dumps(str(repo / relative))} }}\n'
            for name, relative in dependencies.items() if name != 'proa_framework_axum'))
        (root / "src/counter.rs").write_text('pub use probe_stable_island::Counter;\n')
        manifest_path = root / "Cargo.toml"
        text = manifest_path.read_text().replace('islands = []', 'islands = ["dep:probe_stable_island"]')
        text = text.replace('[dependencies]\n', '[dependencies]\nprobe_stable_island = { path = "stable_island", optional = true }\n', 1)
        manifest_path.write_text(text)



def run(root, command, label, environment):
    start = time.perf_counter()
    result = subprocess.run(command, cwd=root, env=environment, capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    log = root / "target" / "probe-logs" / f"{label}.txt"
    log.parent.mkdir(parents=True, exist_ok=True)
    log.write_text(result.stdout + "\n" + result.stderr)
    if result.returncode:
        raise RuntimeError(f"{label} failed: {log}\n{result.stderr[-5000:]}")
    cargo_seconds = [float(value) for value in re.findall(r"Finished .* in ([0-9.]+)s", result.stderr)]
    record = {"label": label, "seconds": elapsed, "cargo_seconds": cargo_seconds,
              "log": str(log), "cache": [line for line in result.stdout.splitlines() if "cache:" in line]}
    print(json.dumps(record), flush=True)
    with (root / "target/probe-records.jsonl").open("a") as history:
        history.write(json.dumps(record) + "\n")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True, help="Current built cargo-proa executable")
    parser.add_argument("--fixture", type=Path, help="Reuse a previous probe's standalone directory and Cargo targets")
    parser.add_argument("--split-islands", action="store_true", help="Put the unchanged RSJS island in a separate consumer crate")
    parser.add_argument("--pages", type=int, default=24)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--profiles", nargs="+", default=["dev", "nodebug", "macroopt"])
    parser.add_argument("--cases", nargs="+", default=["direct-ssr", "linked-ssr", "linked-islands"])
    parser.add_argument("--toolchain", help="Explicit rustup toolchain for Cargo and rustc")
    parser.add_argument("--rustflags", help="Explicit compiler flags; parsed as words, never executed")
    parser.add_argument("--configuration", default="default", help="Label and isolated target namespace")
    args = parser.parse_args()
    if args.samples < 1 or args.pages < 1:
        parser.error("--samples and --pages must be positive")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", args.configuration):
        parser.error("--configuration must be a single alphanumeric label")
    repo = Path(__file__).resolve().parents[1]
    root = args.fixture.resolve() if args.fixture else Path(tempfile.mkdtemp(prefix="proa-compile-probe-")).resolve()
    if not args.fixture:
        create_fixture(root, repo, args.pages, args.split_islands)
    print(f"Fixture: {root}", flush=True)
    environment = dict(os.environ)
    for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_INCREMENTAL"):
        environment.pop(key, None)
    environment["CARGO_TARGET_DIR"] = str(root / "target" if args.configuration == "default" else root / "target" / args.configuration)
    if args.toolchain:
        environment["RUSTUP_TOOLCHAIN"] = args.toolchain
    if args.rustflags is not None:
        environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(shlex.split(args.rustflags))
    metadata = {"toolchain": args.toolchain, "rustflags": args.rustflags,
                "configuration": args.configuration,
                "rustc": subprocess.check_output(["rustc", "-vV"], cwd=root, env=environment, text=True)}
    (root / "target").mkdir(exist_ok=True)
    (root / "target" / f"{args.configuration}-metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    records = []
    page = root / "src/page_0.rs"
    marker = "A" if "EDIT_MARKER_A" in page.read_text() else "B"
    for profile in args.profiles:
        for case in args.cases:
            cargo = ["cargo", "run", "--offline", "--profile", profile]
            if case.endswith("islands"):
                cargo += ["--features", "islands"]
            cargo += ["--", "--probe"]
            command = cargo if case.startswith("direct") else [str(args.cli.resolve()), "dev", "--root", str(root), "--no-assets", "--no-restart", "--", *cargo]
            label = f"{args.configuration}-{profile}-{case}"
            records.append(run(root, command, f"{label}-cold", environment))
            records.append(run(root, command, f"{label}-noop", environment))
            edits = []
            for sample in range(args.samples):
                next_marker = "B" if marker == "A" else "A"
                page.write_text(page.read_text().replace(f"EDIT_MARKER_{marker}", f"EDIT_MARKER_{next_marker}"))
                marker = next_marker
                record = run(root, command, f"{label}-edit-{sample}", environment)
                records.append(record)
                edits.append(record["seconds"])
            print(f"MEDIAN {profile} {case}: {statistics.median(edits):.3f}s", flush=True)
            if case.endswith("islands"):
                counter = root / ("stable_island/src/lib.rs" if (root / "stable_island").is_dir() else "src/counter.rs")
                counter.write_text(counter.read_text().replace("c + 1", "c + 2"))
                records.append(run(root, command, f"{label}-handler-edit", environment))
                counter.write_text(counter.read_text().replace("c + 2", "c + 1"))
    (root / "target/results.json").write_text(json.dumps(records, indent=2) + "\n")
    (root / "target" / f"{args.configuration}-results.json").write_text(json.dumps(records, indent=2) + "\n")
    print(f"Results: {root / 'target/results.json'}", flush=True)


if __name__ == "__main__":
    main()
