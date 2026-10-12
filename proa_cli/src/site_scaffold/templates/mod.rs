// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod app;
pub(crate) mod origin;
mod ui;

use std::collections::BTreeMap;

use anyhow::Result;
use quote::ToTokens;
use serde_json::{json, Map};
use sha2::{Digest, Sha256};

use crate::capabilities::{
    CapabilitySelection, DatabaseChoice, DeployChoice, StarterChoice, StylingChoice, UiChoice,
};

use super::plan::SitePlan;

const CLI_DISTRIBUTION_VERSION: &str = match option_env!("PROA_CLI_DISTRIBUTION_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub fn populate(
    plan: &mut SitePlan,
    package: &str,
    title: &str,
    selection: CapabilitySelection,
    vscode: bool,
    agents_md: bool,
) -> Result<()> {
    if selection.template == crate::capabilities::SiteTemplate::Origin {
        return origin::populate(plan, package, title, selection, vscode, agents_md);
    }
    let has_database = selection.database == DatabaseChoice::Sqlite;
    let has_ui = selection.ui == UiChoice::Proa;
    let blank = selection.starter == StarterChoice::Blank;

    plan.write("Cargo.toml", cargo_toml(package, has_database))?;
    plan.write("rust-toolchain.toml", rust_toolchain())?;
    plan.write(".cargo/config.toml", cargo_config())?;
    plan.write(".env.example", env_example(package, has_database))?;
    plan.write(".gitignore", gitignore())?;
    plan.write(".gitattributes", "*.sql text eol=lf\n")?;
    plan.write("README.md", readme(package, title, selection))?;
    plan.write("build.rs", build_rs(has_database))?;
    plan.write("proa.config.json", project_config(package, selection)?)?;

    plan.write("src/main.rs", app::main_rs(has_database))?;
    plan.write("src/app.rs", app::app_state(has_database))?;
    plan.write("src/config.rs", app::config_rs(package, has_database))?;
    plan.write("src/error.rs", app::error_rs())?;
    plan.write("src/routes.rs", app::routes_rs(selection.markdown))?;
    plan.write("src/static_assets.rs", app::static_assets_rs())?;
    plan.write(
        "src/layouts/mod.rs",
        "mod root;\n\npub use root::RootDocument;\n",
    )?;
    plan.write("src/layouts/root.rs", app::root_layout())?;
    plan.write("src/pages/mod.rs", "pub mod home;\n")?;
    plan.write(
        "src/pages/home.rs",
        format!(
            "{}\n{}",
            app::home_rs(has_database, has_ui, selection.markdown, blank),
            app::home_handler(has_database, selection.markdown, title)
        ),
    )?;
    plan.write("src/endpoints/mod.rs", "pub mod health;\n")?;
    plan.write("src/endpoints/health.rs", app::health_rs(has_database))?;
    plan.write(".github/workflows/ci.yml", github_ci(package))?;
    plan.write("public/favicon.svg", favicon())?;
    plan.write("public/styles.css", css())?;

    if selection.styling == StylingChoice::Tailwind {
        plan.write("styles/input.css", tailwind_input())?;
        plan.write("styles/starter.css", css())?;
    }
    if selection.template == crate::capabilities::SiteTemplate::Docs {
        plan.write(
            "src/docs/index.md",
            "# Introduction\n\nWelcome to your Proa documentation.\n",
        )?;
        plan.write(
            "src/docs/getting-started.md",
            "# Getting started\n\nEdit this collection and run `proa dev`.\n",
        )?;
    }
    if has_database {
        plan.write("src/db.rs", app::db_rs())?;
        plan.write("src/loader.rs", app::loader_rs())?;
        plan.write("data/.gitkeep", "")?;
        plan.write("migrations/.gitkeep", "")?;
        plan.write("src/components/database_badge.rs", app::database_badge_rs())?;
        plan.write("src/components/database_card.rs", app::database_card_rs())?;
    }

    let mut component_sources = BTreeMap::new();
    if has_ui {
        add_component(
            plan,
            &mut component_sources,
            "badge",
            [
                ("mod.rs", ui::badge_mod_rs()),
                ("badge.rs", ui::badge_rs()),
                ("md.rs", ui::badge_md_rs()),
                ("recipe.rs", ui::badge_recipe_rs()),
            ],
        )?;
        add_component(
            plan,
            &mut component_sources,
            "button",
            [
                ("mod.rs", ui::button_mod_rs()),
                ("button.rs", ui::button_rs()),
                ("md.rs", ui::button_md_rs()),
                ("recipe.rs", ui::button_recipe_rs()),
            ],
        )?;
        add_component(
            plan,
            &mut component_sources,
            "card",
            [
                ("mod.rs", ui::card_mod_rs()),
                ("card.rs", ui::card_rs()),
                ("md.rs", ui::card_md_rs()),
                ("recipe.rs", ui::card_recipe_rs()),
            ],
        )?;
    }
    plan.write(
        "src/components/mod.rs",
        components_mod(has_database, has_ui),
    )?;
    plan.write("proa.lock.json", lock_file(selection, component_sources)?)?;

    plan.write("tests/smoke.rs", smoke_test())?;
    if has_database {
        plan.write("tests/dataloader.rs", dataloader_test())?;
    }
    if selection.markdown {
        plan.write(
            "tests/representations.rs",
            representation_test(has_database),
        )?;
    }

    if vscode {
        plan.write(".vscode/extensions.json", vscode_extensions())?;
        plan.write(".vscode/settings.json", vscode_settings())?;
        plan.write(".vscode/tasks.json", vscode_tasks(has_database))?;
    }
    if agents_md {
        plan.write("AGENTS.md", agents())?;
    }
    add_deploy(plan, package, selection.deploy, has_database)?;
    Ok(())
}

fn add_component<const N: usize>(
    plan: &mut SitePlan,
    sources: &mut BTreeMap<String, BTreeMap<String, String>>,
    name: &str,
    files: [(&str, &str); N],
) -> Result<()> {
    let mut hashes = BTreeMap::new();
    for (file, source) in files {
        let path = format!("src/components/{name}/{file}");
        hashes.insert(path.clone(), sha256(source));
        plan.write(path, source)?;
    }
    sources.insert(name.to_string(), hashes);
    Ok(())
}

fn sha256(source: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(source.as_bytes()))
}

fn rust_string_literal(value: &str) -> String {
    syn::LitStr::new(value, proc_macro2::Span::call_site())
        .into_token_stream()
        .to_string()
}

fn cargo_toml(package: &str, has_database: bool) -> String {
    let database = if has_database {
        "sqlx = { version = \"0.8\", features = [\"runtime-tokio\", \"sqlite\", \"migrate\"] }\n"
    } else {
        ""
    };
    format!(
        r#"[package]
name = "{package}"
version = "0.1.0"
edition = "2021"
rust-version = "1.93"

[dependencies]
anyhow = "1"
axum = "0.7.9"
dotenvy = "0.15"
proa_core = {{ version = "0.3.0" }}
proa_framework_axum = {{ version = "0.3.0" }}
proa_macros = {{ version = "0.3.0" }}
proa_stream = {{ version = "0.3.0" }}
serde = {{ version = "1", features = ["derive"] }}
{database}thiserror = "2"
tokio = {{ version = "1", features = ["macros", "rt-multi-thread", "signal"] }}
tower-http = {{ version = "0.6", features = ["fs", "trace"] }}
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}

[dev-dependencies]
http-body-util = "0.1"
tower = {{ version = "0.5", features = ["util"] }}

[profile.release]
lto = "thin"
codegen-units = 1
"#
    )
}

fn rust_toolchain() -> &'static str {
    "[toolchain]\nchannel = \"1.93.0\"\nprofile = \"minimal\"\ncomponents = [\"rustfmt\", \"clippy\"]\n"
}

fn cargo_config() -> &'static str {
    "[net]\ngit-fetch-with-cli = true\n"
}

fn env_example(package: &str, has_database: bool) -> String {
    let database = if has_database {
        format!("DATABASE_URL=sqlite://data/{package}.db\n")
    } else {
        String::new()
    };
    format!("{database}PROA_HOST=127.0.0.1\nPORT=3000\nRUST_LOG={package}=debug,tower_http=info\n")
}

fn gitignore() -> &'static str {
    ".env\n/target/\n/data/*.db\n/data/*.db-shm\n/data/*.db-wal\n"
}

fn build_rs(has_database: bool) -> &'static str {
    if has_database {
        "fn main() {\n    println!(\"cargo:rerun-if-changed=migrations\");\n    println!(\"cargo:rerun-if-changed=public\");\n}\n"
    } else {
        "fn main() {\n    println!(\"cargo:rerun-if-changed=public\");\n}\n"
    }
}

fn project_config(package: &str, selection: CapabilitySelection) -> Result<String> {
    let database = match selection.database {
        DatabaseChoice::Sqlite => json!({
            "driver": "sqlite",
            "migrations": "migrations",
            "runMigrationsOnStartup": true
        }),
        DatabaseChoice::None => json!({ "driver": "none" }),
    };
    let markdown = if selection.markdown {
        json!({
            "enabled": true,
            "negotiateOnCanonicalPath": true,
            "explicitSuffix": ".md",
            "rootPath": "/index.md"
        })
    } else {
        json!({ "enabled": false })
    };
    let styling = match selection.styling {
        StylingChoice::Tailwind => json!({
            "kind": "tailwind", "input": "styles/input.css", "output": "public/styles.css"
        }),
        StylingChoice::Css => json!({ "kind": "css", "output": "public/styles.css" }),
        StylingChoice::Docs => json!({ "kind": "docs", "output": "public/styles.css" }),
    };
    let assets = if selection.styling == StylingChoice::Tailwind {
        json!([{ "kind": "tailwind", "input": "styles/input.css", "output": "public/styles.css" }])
    } else {
        json!([])
    };
    let value = json!({
        "$schema": crate::config::CONFIG_SCHEMA_URL,
        "application": {
            "kind": "site", "package": package, "binary": package,
            "template": selection.template.as_str(), "starter": selection.starter.as_str()
        },
        "database": database,
        "ui": { "provider": selection.ui.as_str() },
        "representations": { "html": { "enabled": true }, "markdown": markdown },
        "styling": styling,
        "paths": {
            "pages": "src/pages", "endpoints": "src/endpoints",
            "components": "src/components", "layouts": "src/layouts",
            "models": "src/models", "public": "public", "runtimeData": "data"
        },
        "deploy": { "provider": selection.deploy.as_str() },
        "tasks": {},
        "dev": {
            "command": ["cargo", "run"], "before": [],
            "watch": ["src", "styles", "migrations", "Cargo.toml", "build.rs", "proa.config.json", "proa.lock.json"]
        },
        "build": { "command": ["cargo", "build", "--release"], "before": [], "assets": assets },
        "test": { "before": [] },
        "registries": { "@proa": "https://ui.proa.so/r/{name}.json" }
    });
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}

fn lock_file(
    selection: CapabilitySelection,
    sources: BTreeMap<String, BTreeMap<String, String>>,
) -> Result<String> {
    let mut components = Map::new();
    for (name, files) in sources {
        let installed_files = files.keys().cloned().collect::<Vec<_>>();
        let aggregate_hash = sha256(
            &files
                .iter()
                .map(|(path, hash)| format!("{path}:{hash}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        components.insert(
            format!("@proa/{name}"),
            json!({
                "sourceVersion": env!("CARGO_PKG_VERSION"),
                "version": env!("CARGO_PKG_VERSION"),
                "registry": "@proa",
                "mode": "source",
                "sourceHash": aggregate_hash.clone(),
                "localHashAtInstall": aggregate_hash,
                "installedFiles": installed_files,
                "dependencies": [],
                "rustModules": [],
                "cargoDependencies": [],
                "files": files
            }),
        );
    }
    let value = json!({
        "schemaVersion": 1,
        "scaffold": {
            "cliVersion": CLI_DISTRIBUTION_VERSION,
            "release": CLI_DISTRIBUTION_VERSION,
            "capabilities": selection.capabilities()
        },
        "components": components
    });
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}

fn components_mod(has_database: bool, has_ui: bool) -> String {
    let mut source = String::new();
    if has_ui {
        source.push_str("pub mod badge;\npub mod button;\npub mod card;\n\npub use badge::Badge;\npub use button::Button;\npub use card::Card;\n");
    }
    if has_database {
        source.push_str("\nmod database_badge;\nmod database_card;\n\npub use database_badge::DatabaseBadge;\npub use database_card::DatabaseCard;\n");
    }
    source
}

fn readme(package: &str, title: &str, selection: CapabilitySelection) -> String {
    format!(
        "# {title}\n\nA production-shaped Proa `{}` site.\n\n```console\ncp .env.example .env\nproa dev --open\n```\n\nProa dependencies resolve from crates.io. No GitHub credentials or Cargo registry token is required to build the generated project.\n\nThe first dependency-resolving command creates the real `Cargo.lock`; commit it before deployment. Use `proa build` for a linked release build and `proa start` to run that exported release.\n\nThe HTML page is available at `/`{}. Prefer keyed loading through `cx` when several components may request the same resource, the work should deduplicate, or sections can render independently. Keep a one-owner query in the handler when rendering cannot begin without it.\n\nPackage: `{package}`.\n",
        selection.template.as_str(),
        if selection.markdown { " and the same typed resource has semantic Markdown at `/index.md`" } else { "" }
    )
}

fn agents() -> &'static str {
    r#"# Proa application instructions

- Use `WebRenderSync<L>` unless a component awaits; then use `WebRender<L>` and `html!`.
- Put request-shared keyed reads behind `cx.load_typed(...)` and create one `DataResolver::request_scope(...)` per HTML response.
- Keep `RootDocument` HTML-only. Put semantic non-browser output on the page with `MdRenderSync`.
- Static classes stay in constants and hot render paths must not allocate.
- Every component root has a stable `data-*` marker.
- Run `cargo fmt`, `cargo test`, `proa check`, and the Proa performance lint before merging render changes.
"#
}

fn favicon() -> &'static str {
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="#0b0b0c"/><path d="M13 46V18h22c10 0 16 5 16 14s-6 14-16 14H25v-8h10c3 0 5-2 5-6s-2-6-5-6H24v20z" fill="#3de1e8"/></svg>"##
}

fn css() -> &'static str {
    r#":root{color-scheme:dark;--ink:#0b0b0c;--bone:#e5e1d6;--cyan:#3de1e8;--red:#ff3b30}*{box-sizing:border-box}body{margin:0;background:var(--ink);color:var(--bone);font-family:ui-monospace,SFMono-Regular,Menlo,monospace}a{color:inherit}.topbar{height:58px;border-bottom:1px solid #ffffff2b;display:flex;align-items:center;justify-content:space-between;padding:0 4vw;font-size:11px;letter-spacing:.15em}.wordmark{font-size:22px;font-weight:900;color:var(--cyan);text-decoration:none}.hero{min-height:62vh;padding:8vh 4vw 6vh;display:grid;align-content:center;background-image:linear-gradient(#ffffff0a 1px,transparent 1px),linear-gradient(90deg,#ffffff0a 1px,transparent 1px);background-size:48px 48px}.kicker,.metric-label,.card-eyebrow{color:var(--cyan);font-size:11px;letter-spacing:.18em}.hero h1{max-width:1050px;margin:.4em 0 .25em;font-family:Arial,sans-serif;font-size:clamp(54px,8vw,124px);line-height:.84;text-transform:uppercase;letter-spacing:-.065em}.lede{max-width:760px;color:#aaa69e;font-family:Arial,sans-serif;font-size:20px;line-height:1.5}.badges{display:flex;gap:10px;flex-wrap:wrap;margin-top:24px}.status-pill{display:inline-flex;align-items:center;gap:8px;border:1px solid #ffffff45;padding:8px 10px;font-size:10px;letter-spacing:.12em}.status-dot{width:7px;height:7px;border-radius:50%;background:var(--cyan);box-shadow:0 0 12px var(--cyan)}.hero-action{margin-top:28px}.metrics{display:grid;grid-template-columns:repeat(3,1fr);border-top:1px solid #ffffff2b}.metric-card{min-height:210px;padding:28px;border-right:1px solid #ffffff2b}.metric-card h2,.metric-value{font-family:Arial,sans-serif;font-size:40px;font-weight:800;margin:20px 0}.metric-note,.metric-card>p:last-child{color:#96938c;line-height:1.5}.receipt{position:sticky;bottom:0;display:flex;gap:28px;justify-content:flex-end;background:var(--bone);color:var(--ink);padding:12px 4vw;font-size:10px;letter-spacing:.08em}.receipt strong{color:#b44e00}@media(max-width:760px){.metrics{grid-template-columns:1fr}.hero h1{font-size:52px}.receipt{position:static;flex-wrap:wrap;justify-content:flex-start}}
"#
}

/// A distilled CI pipeline for scaffolded sites, carrying over what the
/// framework repository's own pipeline learned the hard way: compiler
/// caching via sccache for plain cargo steps, incremental compilation and
/// debuginfo off in CI, and the linked `proa` steps running without a
/// compiler wrapper because cargo-proa rejects `RUSTC_WRAPPER` by design
/// (descriptor/final cache identity must never be bypassed by a compiler
/// cache).
fn github_ci(package: &str) -> String {
    github_ci_for_version(package, CLI_DISTRIBUTION_VERSION)
}

fn github_ci_for_version(package: &str, cli_version: &str) -> String {
    let cli_download_base = cli_download_base(cli_version);
    format!(
        r#"name: CI

on:
  push:
    branches: [main]
  pull_request:
  workflow_dispatch:

permissions:
  contents: read

# A superseded run spends nothing: force-pushes and rapid PR updates cancel
# the previous run instead of queueing behind it.
concurrency:
  group: ci-${{{{ github.event.pull_request.number || github.ref }}}}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always
  # The scaffold generator pins the exact CLI release it was built from.
  PROA_CLI_VERSION: "{cli_version}"
  # Incremental artifacts and debuginfo only slow CI down.
  CARGO_INCREMENTAL: "0"
  CARGO_PROFILE_DEV_DEBUG: "0"
  CARGO_PROFILE_TEST_DEBUG: "0"
  RUST_TEST_THREADS: "2"

jobs:
  ci:
    name: fmt, lint, build, test
    runs-on: ubuntu-latest
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4.3.1
        with:
          persist-credentials: false

      # Download immutable release artifacts and verify them before execution.
      # rust-toolchain.toml separately pins the Rust toolchain.
      - name: Install the Proa CLI and Tailwind
        shell: bash
        run: |
          set -euo pipefail
          install_dir="$RUNNER_TEMP/proa-bin"
          temp_dir="$RUNNER_TEMP/proa-install"
          target="x86_64-unknown-linux-gnu"
          archive="proa-cli-$PROA_CLI_VERSION-$target.tar.gz"
          sums="proa-cli-$PROA_CLI_VERSION-SHA256SUMS"
          base="{cli_download_base}"
          tailwind_version="4.3.3"
          tailwind_asset="tailwindcss-linux-x64"
          tailwind_sha256="dc61b3ac6b8c9ca874c0cc4c57b2409791a64c5540404ca5f5367360babc313a"

          mkdir -p "$install_dir" "$temp_dir"
          curl --fail --location --proto '=https' --proto-redir '=https' \
            --silent --show-error --tlsv1.2 \
            --output "$temp_dir/$archive" "$base/$archive"
          curl --fail --location --proto '=https' --proto-redir '=https' \
            --silent --show-error --tlsv1.2 \
            --output "$temp_dir/$sums" "$base/$sums"
          awk -v archive="$archive" '
            ($2 == archive || $2 == "*" archive) &&
            length($1) == 64 && $1 !~ /[^0-9a-fA-F]/ {{
              print tolower($1) "  " archive
              found = 1
            }}
            END {{ exit found ? 0 : 1 }}
          ' "$temp_dir/$sums" > "$temp_dir/selected-SHA256SUMS"
          (cd "$temp_dir" && sha256sum --check --strict selected-SHA256SUMS)
          tar -xzf "$temp_dir/$archive" -C "$temp_dir"
          release_dir="$temp_dir/proa-cli-$PROA_CLI_VERSION-$target"
          install -m 0755 "$release_dir/proa" "$install_dir/proa"
          install -m 0755 "$release_dir/cargo-proa" "$install_dir/cargo-proa"

          curl --fail --location --proto '=https' --proto-redir '=https' \
            --silent --show-error --tlsv1.2 \
            --output "$temp_dir/$tailwind_asset" \
            "https://github.com/tailwindlabs/tailwindcss/releases/download/v$tailwind_version/$tailwind_asset"
          printf '%s  %s\n' "$tailwind_sha256" "$tailwind_asset" \
            > "$temp_dir/tailwind-SHA256SUMS"
          (cd "$temp_dir" && sha256sum --check --strict tailwind-SHA256SUMS)
          install -m 0755 "$temp_dir/$tailwind_asset" "$install_dir/tailwindcss"

          echo "$install_dir" >> "$GITHUB_PATH"

      - name: Format
        run: |
          cargo fmt --all -- --check
          proa fmt --check

      - name: Lint
        run: proa lint

      # cargo-proa refuses RUSTC_WRAPPER by design (a compiler cache must
      # never bypass its descriptor/final compilation), so any wrapper the
      # runner environment sets is dropped here.
      - name: Test ({package}, linked pipeline)
        run: env -u RUSTC_WRAPPER proa test

      - name: Release build
        run: env -u RUSTC_WRAPPER proa build
"#
    )
}

fn tailwind_input() -> &'static str {
    "@layer theme, base, proa-compat, components, utilities;\n@import \"tailwindcss\";\n@import \"./starter.css\" layer(components);\n@source \"../src/**/*.rs\";\n"
}

fn smoke_test() -> &'static str {
    r##"#[test]
fn generated_project_has_no_client_root() {
    let home = include_str!("../src/pages/home.rs");
    assert!(!home.contains("#[rsjs(component, client)]"));
    assert!(!home.contains("with_islands"));
}
"##
}

fn dataloader_test() -> &'static str {
    r#"#![allow(dead_code)]

use std::{future::Future, sync::{Arc, atomic::{AtomicUsize, Ordering}}};

use proa_core::{DataLoader, LoadError, LoadRequest, LoadValue, MdRender, WebContext, WebRender};
use proa_stream::DataResolver;

#[path = "../src/app.rs"] mod app;
#[path = "../src/components/mod.rs"] mod components;
#[path = "../src/error.rs"] mod error;
#[path = "../src/layouts/mod.rs"] mod layouts;
#[path = "../src/loader.rs"] mod loader;
#[path = "../src/pages/mod.rs"] mod pages;

struct InstrumentedLoader {
    origin_loads: Arc<AtomicUsize>,
}

impl DataLoader for InstrumentedLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        assert_eq!(request.key().to_string(), "system:database-status");
        self.origin_loads.fetch_add(1, Ordering::SeqCst);
        async {
            Ok(LoadValue::typed(loader::DatabaseStatus {
                driver: "SQLite",
                version: "9.9-test".to_string(),
            }))
        }
    }
}

async fn render_request(loader: Arc<InstrumentedLoader>) -> String {
    let resolver = DataResolver::request_scope(loader);
    let mut cx = WebContext::with_loader(resolver);
    pages::home::HomePage
        .render(&mut cx)
        .resolve()
        .await
        .unwrap();
    cx.into_output().into_string()
}

#[tokio::test]
async fn real_guided_page_deduplicates_two_components_per_request() {
    let origin_loads = Arc::new(AtomicUsize::new(0));
    let loader = Arc::new(InstrumentedLoader {
        origin_loads: Arc::clone(&origin_loads),
    });

    let first = render_request(Arc::clone(&loader)).await;
    assert!(first.contains("data-database-badge"));
    assert!(first.contains("data-database-card"));
    assert!(first.contains("9.9-test"));
    assert_eq!(origin_loads.load(Ordering::SeqCst), 1);

    let second = render_request(loader).await;
    assert!(second.contains("data-database-badge"));
    assert_eq!(origin_loads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn markdown_exposes_the_same_database_value_and_deduplicates_its_children() {
    let origin_loads = Arc::new(AtomicUsize::new(0));
    let loader = Arc::new(InstrumentedLoader { origin_loads: Arc::clone(&origin_loads) });
    let resolver = DataResolver::request_scope(loader);
    let mut cx = WebContext::with_loader(resolver);
    pages::home::HomePage.render_md(&mut cx).resolve().await.unwrap();
    let markdown = cx.into_output().into_string();
    assert!(markdown.contains(r"9\.9\-test"), "{markdown}");
    assert!(!markdown.contains("<html"));
    assert_eq!(origin_loads.load(Ordering::SeqCst), 1);
}
"#
}

fn representation_test(has_database: bool) -> String {
    let arc_import = if has_database {
        "use std::sync::Arc;\n\n"
    } else {
        ""
    };
    let database_modules = if has_database {
        "#[path = \"../src/db.rs\"] mod db;\n#[path = \"../src/loader.rs\"] mod loader;\n"
    } else {
        ""
    };
    let state = if has_database {
        r#"let database_url = std::env::var("DATABASE_URL").expect("proa test supplies DATABASE_URL");
    let pool = db::connect(&database_url).await.unwrap();
    let loader = Arc::new(loader::AppLoader::new(pool.clone()));
    let app = routes::router(app::AppState { db: pool.clone(), loader });"#
    } else {
        "let app = routes::router(app::AppState);"
    };
    let close = if has_database {
        "pool.close().await;"
    } else {
        ""
    };
    r#"__ARC_IMPORT__use axum::{body::Body, http::{header, Request, StatusCode}, response::Response};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[path = "../src/app.rs"] mod app;
#[path = "../src/components/mod.rs"] mod components;
__DATABASE_MODULES__#[path = "../src/endpoints/mod.rs"] mod endpoints;
#[path = "../src/error.rs"] mod error;
#[path = "../src/layouts/mod.rs"] mod layouts;
#[path = "../src/pages/mod.rs"] mod pages;
#[path = "../src/routes.rs"] mod routes;
#[path = "../src/static_assets.rs"] mod static_assets;

async fn body(response: Response) -> Vec<u8> {
    response.into_body().collect().await.unwrap().to_bytes().to_vec()
}

#[tokio::test(flavor = "multi_thread")]
async fn canonical_and_explicit_markdown_share_one_page_policy() {
    __STATE__

    let html = app.clone().oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(html.status(), StatusCode::OK);
    assert!(html.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));
    assert_eq!(html.headers()[header::VARY], "Accept");
    assert!(html.headers()[header::LINK].to_str().unwrap().contains("/index.md"));

    let refused_markdown = app.clone().oneshot(
        Request::builder().uri("/").header(header::ACCEPT, "text/markdown;q=0, text/html").body(Body::empty()).unwrap()
    ).await.unwrap();
    assert!(refused_markdown.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));

    let negotiated = app.clone().oneshot(
        Request::builder().uri("/").header(header::ACCEPT, "text/markdown").body(Body::empty()).unwrap()
    ).await.unwrap();
    assert!(negotiated.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/markdown"));
    let negotiated = body(negotiated).await;

    let explicit = app.oneshot(Request::builder().uri("/index.md").body(Body::empty()).unwrap()).await.unwrap();
    assert!(explicit.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/markdown"));
    let explicit = body(explicit).await;
    assert_eq!(negotiated, explicit);
    let markdown = String::from_utf8(explicit).unwrap();
    assert!(!markdown.contains("<html"));
    assert!(!markdown.contains("/static/"));

    __CLOSE__
}
"#
    .replace("__ARC_IMPORT__", arc_import)
    .replace("__DATABASE_MODULES__", database_modules)
    .replace("__STATE__", state)
    .replace("__CLOSE__", close)
}

fn vscode_extensions() -> &'static str {
    "{\n  \"recommendations\": [\n    \"rust-lang.rust-analyzer\",\n    \"proa-labs.proa\",\n    \"tamasfe.even-better-toml\",\n    \"bradlc.vscode-tailwindcss\"\n  ]\n}\n"
}

fn vscode_settings() -> &'static str {
    "{\n  \"rust-analyzer.check.command\": \"clippy\",\n  \"editor.formatOnSave\": true,\n  \"files.exclude\": { \"target\": true, \"data/*.db*\": true }\n}\n"
}

fn vscode_tasks(has_database: bool) -> String {
    let mut tasks = vec![
        json!({ "label": "proa: dev", "type": "process", "command": "proa", "args": ["dev", "--open"], "problemMatcher": [] }),
        json!({ "label": "proa: check", "type": "process", "command": "proa", "args": ["check", "--all"], "problemMatcher": ["$rustc"] }),
    ];
    if has_database {
        tasks.push(json!({ "label": "proa: db status", "type": "process", "command": "proa", "args": ["db", "status"], "problemMatcher": [] }));
    }
    format!(
        "{}\n",
        serde_json::to_string_pretty(&json!({ "version": "2.0.0", "tasks": tasks })).unwrap()
    )
}

fn add_deploy(
    plan: &mut SitePlan,
    package: &str,
    deploy: DeployChoice,
    has_database: bool,
) -> Result<()> {
    if deploy == DeployChoice::None {
        return Ok(());
    }
    let migration_copy = if has_database {
        "COPY --from=build /app/migrations ./migrations\n"
    } else {
        ""
    };
    plan.write(
        "Dockerfile",
        deployment_dockerfile(migration_copy, CLI_DISTRIBUTION_VERSION),
    )?;
    plan.write(".dockerignore", "target\n.git\n.env\ndata/*.db*\n")?;
    let mut deployment = "# Deployment\n\nBuild the generated Dockerfile. Proa dependencies resolve from crates.io without credentials:\n\n```bash\ndocker build .\n```\n\nUse `/readyz` for readiness and `/healthz` for liveness.\n".to_string();
    if has_database {
        deployment.push_str("\nMount persistent storage at `/app/data`, run one replica, and configure tested backups.\n");
    }
    plan.write("DEPLOYMENT.md", deployment)?;
    if deploy == DeployChoice::Railway {
        plan.write(
            "railway.json",
            "{\n  \"$schema\": \"https://railway.com/railway.schema.json\",\n  \"deploy\": {\n    \"healthcheckPath\": \"/readyz\",\n    \"healthcheckTimeout\": 120,\n    \"restartPolicyType\": \"ON_FAILURE\"\n  }\n}\n",
        )?;
        plan.write(
            "RAILWAY.md",
            if has_database {
                "# Railway\n\nRun `railway up`, attach a volume at `/app/data`, use one replica, then verify `/readyz`.\n"
            } else {
                "# Railway\n\nRun `railway up`, then verify `/readyz`.\n"
            },
        )?;
    }
    if deploy == DeployChoice::Coolify {
        plan.write(
            "coolify.env.example",
            if has_database {
                format!("PORT=3000\nPROA_HOST=0.0.0.0\nDATABASE_URL=sqlite://data/{package}.db\n")
            } else {
                "PORT=3000\nPROA_HOST=0.0.0.0\n".to_string()
            },
        )?;
        plan.write(
            "COOLIFY.md",
            if has_database {
                "# Coolify\n\nBuild the Dockerfile, persist `/app/data`, use one replica, and configure `/readyz` as the readiness check.\n"
            } else {
                "# Coolify\n\nBuild the Dockerfile and configure `/readyz` as the readiness check.\n"
            },
        )?;
    }
    Ok(())
}

fn cli_download_base(version: &str) -> String {
    let snapshot = version
        .split_once('-')
        .is_some_and(|(_, prerelease)| prerelease.starts_with("main."));
    if snapshot {
        format!("https://proa.so/downloads/cli/main/{version}")
    } else {
        format!("https://proa.so/downloads/cli/{version}")
    }
}

fn deployment_dockerfile(migration_copy: &str, cli_version: &str) -> String {
    let cli_download_base = cli_download_base(cli_version);
    format!(
        r#"FROM rust:1.93-bookworm AS rust-toolchain
FROM ubuntu:24.04 AS build
ENV RUSTUP_HOME="/usr/local/rustup" CARGO_HOME="/usr/local/cargo" PATH="/usr/local/cargo/bin:$PATH" DEBIAN_FRONTEND="noninteractive"
COPY --from=rust-toolchain /usr/local/rustup /usr/local/rustup
COPY --from=rust-toolchain /usr/local/cargo /usr/local/cargo
RUN apt-get update && apt-get install -y --no-install-recommends build-essential ca-certificates curl git libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /app
ENV PROA_CLI_VERSION="{cli_version}"
RUN set -eux; \
    install_dir="/usr/local/bin"; \
    temp_dir="$(mktemp -d)"; \
    target="x86_64-unknown-linux-gnu"; \
    archive="proa-cli-$PROA_CLI_VERSION-$target.tar.gz"; \
    sums="proa-cli-$PROA_CLI_VERSION-SHA256SUMS"; \
    base="{cli_download_base}"; \
    curl --fail --location --proto '=https' --proto-redir '=https' --silent --show-error --tlsv1.2 --output "$temp_dir/$archive" "$base/$archive"; \
    curl --fail --location --proto '=https' --proto-redir '=https' --silent --show-error --tlsv1.2 --output "$temp_dir/$sums" "$base/$sums"; \
    awk -v archive="$archive" '($2 == archive || $2 == "*" archive) && length($1) == 64 && $1 !~ /[^0-9a-fA-F]/ {{ print tolower($1) "  " archive; found = 1 }} END {{ exit found ? 0 : 1 }}' "$temp_dir/$sums" > "$temp_dir/selected-SHA256SUMS"; \
    (cd "$temp_dir" && sha256sum --check --strict selected-SHA256SUMS); \
    tar -xzf "$temp_dir/$archive" -C "$temp_dir"; \
    release_dir="$temp_dir/proa-cli-$PROA_CLI_VERSION-$target"; \
    install -m 0755 "$release_dir/proa" "$install_dir/proa"; \
    tailwind_version="4.3.3"; \
    tailwind_asset="tailwindcss-linux-x64"; \
    curl --fail --location --proto '=https' --proto-redir '=https' --silent --show-error --tlsv1.2 --output "$temp_dir/$tailwind_asset" "https://github.com/tailwindlabs/tailwindcss/releases/download/v$tailwind_version/$tailwind_asset"; \
    printf '%s  %s\n' "dc61b3ac6b8c9ca874c0cc4c57b2409791a64c5540404ca5f5367360babc313a" "$tailwind_asset" > "$temp_dir/tailwind-SHA256SUMS"; \
    (cd "$temp_dir" && sha256sum --check --strict tailwind-SHA256SUMS); \
    install -m 0755 "$temp_dir/$tailwind_asset" "$install_dir/tailwindcss"; \
    rm -rf "$temp_dir"
COPY . .
RUN set -eu; \
    if [ -f Cargo.lock ]; then \
        proa build --locked --artifact-out /tmp/proa-app; \
    else \
        proa build --artifact-out /tmp/proa-app; \
    fi
FROM ubuntu:24.04
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3t64 && rm -rf /var/lib/apt/lists/*
COPY --from=build /tmp/proa-app /usr/local/bin/app
COPY --from=build /app/public ./public
{migration_copy}ENV PROA_HOST=0.0.0.0 PORT=3000
EXPOSE 3000
CMD ["/usr/local/bin/app"]
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_manifests_use_crates_io_without_git_credentials() {
        for source in [
            cargo_toml("minimal", false),
            cargo_toml("app", true),
            origin::manifest("atlas", origin::ManifestFlavor::Standalone),
        ] {
            let manifest: toml::Value = source.parse().unwrap();
            for section in ["dependencies", "build-dependencies"] {
                if let Some(dependencies) = manifest.get(section).and_then(toml::Value::as_table) {
                    for (name, dependency) in dependencies {
                        if name.starts_with("proa_") || name == "rsjs" {
                            assert!(dependency
                                .get("version")
                                .and_then(toml::Value::as_str)
                                .is_some());
                            assert!(
                                dependency.get("git").is_none(),
                                "{name} must resolve from crates.io"
                            );
                            assert!(dependency.get("registry").is_none());
                        }
                    }
                }
            }
        }
        let config: toml::Value = cargo_config().parse().unwrap();
        assert!(config.get("registries").is_none());
        assert_eq!(config["net"]["git-fetch-with-cli"].as_bool(), Some(true));
    }

    #[test]
    fn generated_ci_uses_versioned_verified_binaries() {
        let workflow = github_ci("demo");
        assert!(workflow.contains(&format!(
            "PROA_CLI_VERSION: \"{}\"",
            CLI_DISTRIBUTION_VERSION
        )));
        assert!(workflow.contains("sha256sum --check --strict"));
        assert!(
            workflow.contains("dc61b3ac6b8c9ca874c0cc4c57b2409791a64c5540404ca5f5367360babc313a")
        );
        assert!(!workflow.contains("install.sh"));
        assert!(!workflow.contains("/latest"));
    }

    #[test]
    fn generated_ci_preserves_a_main_snapshot_distribution_identity() {
        let version = "0.1.1-main.0123456789abcdef0123456789abcdef01234567";
        let workflow = github_ci_for_version("demo", version);
        assert!(workflow.contains(&format!("PROA_CLI_VERSION: \"{version}\"")));
        assert!(workflow.contains(&format!(
            "base=\"https://proa.so/downloads/cli/main/{version}\""
        )));
        assert!(workflow.contains("proa-cli-$PROA_CLI_VERSION-$target.tar.gz"));
        assert!(!workflow.contains("PROA_GITHUB_TOKEN"));
        assert!(!workflow.contains("PROA_CLI_VERSION: \"0.1.1\""));
        assert_eq!(
            cli_download_base("0.1.1"),
            "https://proa.so/downloads/cli/0.1.1"
        );
    }

    #[test]
    fn deployment_uses_the_verified_linked_build_and_exported_artifact() {
        let version = "0.1.1-main.0123456789abcdef0123456789abcdef01234567";
        let dockerfile = deployment_dockerfile("", version);

        assert!(dockerfile.contains(&format!("ENV PROA_CLI_VERSION=\"{version}\"")));
        assert!(dockerfile.contains("FROM ubuntu:24.04 AS build"));
        assert!(dockerfile.contains(&format!(
            "base=\"https://proa.so/downloads/cli/main/{version}\""
        )));
        assert!(dockerfile.contains("sha256sum --check --strict"));
        assert!(dockerfile.contains("proa build --locked --artifact-out /tmp/proa-app"));
        assert!(dockerfile.contains("proa build --artifact-out /tmp/proa-app"));
        assert!(dockerfile.contains("COPY --from=build /tmp/proa-app /usr/local/bin/app"));
        assert!(!dockerfile.contains("cargo build --release"));
        assert!(!dockerfile.contains("/app/target/release/"));
        assert!(!dockerfile.contains("install.sh"));
    }

    #[test]
    fn rust_string_literals_escape_source_syntax() {
        let value = "Sam\"s\nRouting Atlas";
        let literal = rust_string_literal(value);
        let parsed: syn::LitStr = syn::parse_str(&literal).unwrap();
        assert_eq!(parsed.value(), value);
    }
}
