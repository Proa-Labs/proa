// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ORIGIN routing-atlas template: the complete ORIGIN application emitted as
//! a scaffold.
//!
//! The files under `templates/origin/` are the single source of truth for the
//! ORIGIN site. `examples/origin-routing-atlas` in this repository is the same
//! tree rendered with the test-only workspace manifest flavor; the
//! `example_matches_the_origin_template` test keeps the two byte-identical,
//! so `proa new site --template origin` produces exactly the site the example
//! demonstrates.

use anyhow::Result;

use crate::capabilities::CapabilitySelection;

use super::super::plan::SitePlan;

/// Where the emitted manifest resolves its Proa dependencies from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestFlavor {
    /// crates.io dependencies for standalone scaffolds.
    Standalone,
    /// Path dependencies for the in-repository example.
    #[cfg(test)]
    Workspace,
}

const CRATE_IDENT_PLACEHOLDER: &str = "__CRATE_IDENT__";

/// Every non-manifest file of the ORIGIN application, verbatim from
/// `templates/origin/`. `__CRATE_IDENT__` marks the spots that name the crate.
pub const SOURCE_FILES: &[(&str, &str)] = &[
    ("build.rs", include_str!("origin/build.rs")),
    ("proa.lock.json", include_str!("origin/proa.lock.json")),
    (
        "public/favicon.svg",
        include_str!("origin/public/favicon.svg"),
    ),
    ("public/origin.js", include_str!("origin/public/origin.js")),
    (
        "public/proa-logo.svg",
        include_str!("origin/public/proa-logo.svg"),
    ),
    (
        "public/proa/proa-ui/proa-free.css",
        include_str!("origin/public/proa/proa-ui/proa-free.css"),
    ),
    (
        "public/proa/proa-ui/proa-free.js",
        include_str!("origin/public/proa/proa-ui/proa-free.js"),
    ),
    (
        "public/styles.css",
        include_str!("origin/public/styles.css"),
    ),
    ("src/api.rs", include_str!("origin/src/api.rs")),
    ("src/app.rs", include_str!("origin/src/app.rs")),
    ("src/assets.rs", include_str!("origin/src/assets.rs")),
    (
        "src/components/_proa/anatomy.rs",
        include_str!("origin/src/components/_proa/anatomy.rs"),
    ),
    (
        "src/components/_proa/collection.rs",
        include_str!("origin/src/components/_proa/collection.rs"),
    ),
    (
        "src/components/_proa/component_macros.rs",
        include_str!("origin/src/components/_proa/component_macros.rs"),
    ),
    (
        "src/components/_proa/component_tree.rs",
        include_str!("origin/src/components/_proa/component_tree.rs"),
    ),
    (
        "src/components/_proa/form.rs",
        include_str!("origin/src/components/_proa/form.rs"),
    ),
    (
        "src/components/_proa/free_web.rs",
        include_str!("origin/src/components/_proa/free_web.rs"),
    ),
    (
        "src/components/_proa/md_support.rs",
        include_str!("origin/src/components/_proa/md_support.rs"),
    ),
    (
        "src/components/_proa/mod.rs",
        include_str!("origin/src/components/_proa/mod.rs"),
    ),
    (
        "src/components/_proa/overlay.rs",
        include_str!("origin/src/components/_proa/overlay.rs"),
    ),
    (
        "src/components/_proa/recipe.rs",
        include_str!("origin/src/components/_proa/recipe.rs"),
    ),
    (
        "src/components/_proa/slots.rs",
        include_str!("origin/src/components/_proa/slots.rs"),
    ),
    (
        "src/components/_proa/state.rs",
        include_str!("origin/src/components/_proa/state.rs"),
    ),
    (
        "src/components/_proa/theme.rs",
        include_str!("origin/src/components/_proa/theme.rs"),
    ),
    (
        "src/components/_proa/tokens.rs",
        include_str!("origin/src/components/_proa/tokens.rs"),
    ),
    (
        "src/components/_proa/traits.rs",
        include_str!("origin/src/components/_proa/traits.rs"),
    ),
    (
        "src/components/_proa/variant_spec.rs",
        include_str!("origin/src/components/_proa/variant_spec.rs"),
    ),
    (
        "src/components/agent_surface/agent_surface.rs",
        include_str!("origin/src/components/agent_surface/agent_surface.rs"),
    ),
    (
        "src/components/agent_surface/md.rs",
        include_str!("origin/src/components/agent_surface/md.rs"),
    ),
    (
        "src/components/agent_surface/mod.rs",
        include_str!("origin/src/components/agent_surface/mod.rs"),
    ),
    (
        "src/components/agent_surface/recipe.rs",
        include_str!("origin/src/components/agent_surface/recipe.rs"),
    ),
    (
        "src/components/atlas_page.rs",
        include_str!("origin/src/components/atlas_page.rs"),
    ),
    (
        "src/components/badge/badge.rs",
        include_str!("origin/src/components/badge/badge.rs"),
    ),
    (
        "src/components/badge/md.rs",
        include_str!("origin/src/components/badge/md.rs"),
    ),
    (
        "src/components/badge/mod.rs",
        include_str!("origin/src/components/badge/mod.rs"),
    ),
    (
        "src/components/badge/recipe.rs",
        include_str!("origin/src/components/badge/recipe.rs"),
    ),
    (
        "src/components/button/button.rs",
        include_str!("origin/src/components/button/button.rs"),
    ),
    (
        "src/components/button/md.rs",
        include_str!("origin/src/components/button/md.rs"),
    ),
    (
        "src/components/button/mod.rs",
        include_str!("origin/src/components/button/mod.rs"),
    ),
    (
        "src/components/button/recipe.rs",
        include_str!("origin/src/components/button/recipe.rs"),
    ),
    (
        "src/components/card/card.rs",
        include_str!("origin/src/components/card/card.rs"),
    ),
    (
        "src/components/card/md.rs",
        include_str!("origin/src/components/card/md.rs"),
    ),
    (
        "src/components/card/mod.rs",
        include_str!("origin/src/components/card/mod.rs"),
    ),
    (
        "src/components/card/recipe.rs",
        include_str!("origin/src/components/card/recipe.rs"),
    ),
    (
        "src/components/dialog/dialog.rs",
        include_str!("origin/src/components/dialog/dialog.rs"),
    ),
    (
        "src/components/dialog/md.rs",
        include_str!("origin/src/components/dialog/md.rs"),
    ),
    (
        "src/components/dialog/mod.rs",
        include_str!("origin/src/components/dialog/mod.rs"),
    ),
    (
        "src/components/dialog/recipe.rs",
        include_str!("origin/src/components/dialog/recipe.rs"),
    ),
    (
        "src/components/dossier/dossier.rs",
        include_str!("origin/src/components/dossier/dossier.rs"),
    ),
    (
        "src/components/dossier/md.rs",
        include_str!("origin/src/components/dossier/md.rs"),
    ),
    (
        "src/components/dossier/mod.rs",
        include_str!("origin/src/components/dossier/mod.rs"),
    ),
    (
        "src/components/dossier/recipe.rs",
        include_str!("origin/src/components/dossier/recipe.rs"),
    ),
    (
        "src/components/flood_lab/flood_lab.rs",
        include_str!("origin/src/components/flood_lab/flood_lab.rs"),
    ),
    (
        "src/components/flood_lab/md.rs",
        include_str!("origin/src/components/flood_lab/md.rs"),
    ),
    (
        "src/components/flood_lab/mod.rs",
        include_str!("origin/src/components/flood_lab/mod.rs"),
    ),
    (
        "src/components/flood_lab/recipe.rs",
        include_str!("origin/src/components/flood_lab/recipe.rs"),
    ),
    (
        "src/components/hero/hero.rs",
        include_str!("origin/src/components/hero/hero.rs"),
    ),
    (
        "src/components/hero/md.rs",
        include_str!("origin/src/components/hero/md.rs"),
    ),
    (
        "src/components/hero/mod.rs",
        include_str!("origin/src/components/hero/mod.rs"),
    ),
    (
        "src/components/hero/recipe.rs",
        include_str!("origin/src/components/hero/recipe.rs"),
    ),
    (
        "src/components/input/input.rs",
        include_str!("origin/src/components/input/input.rs"),
    ),
    (
        "src/components/input/md.rs",
        include_str!("origin/src/components/input/md.rs"),
    ),
    (
        "src/components/input/mod.rs",
        include_str!("origin/src/components/input/mod.rs"),
    ),
    (
        "src/components/input/recipe.rs",
        include_str!("origin/src/components/input/recipe.rs"),
    ),
    (
        "src/components/live_wire/live_wire.rs",
        include_str!("origin/src/components/live_wire/live_wire.rs"),
    ),
    (
        "src/components/live_wire/md.rs",
        include_str!("origin/src/components/live_wire/md.rs"),
    ),
    (
        "src/components/live_wire/mod.rs",
        include_str!("origin/src/components/live_wire/mod.rs"),
    ),
    (
        "src/components/live_wire/recipe.rs",
        include_str!("origin/src/components/live_wire/recipe.rs"),
    ),
    (
        "src/components/mod.rs",
        include_str!("origin/src/components/mod.rs"),
    ),
    (
        "src/components/receipt/md.rs",
        include_str!("origin/src/components/receipt/md.rs"),
    ),
    (
        "src/components/receipt/mod.rs",
        include_str!("origin/src/components/receipt/mod.rs"),
    ),
    (
        "src/components/receipt/receipt.rs",
        include_str!("origin/src/components/receipt/receipt.rs"),
    ),
    (
        "src/components/receipt/recipe.rs",
        include_str!("origin/src/components/receipt/recipe.rs"),
    ),
    (
        "src/components/separator/md.rs",
        include_str!("origin/src/components/separator/md.rs"),
    ),
    (
        "src/components/separator/mod.rs",
        include_str!("origin/src/components/separator/mod.rs"),
    ),
    (
        "src/components/separator/recipe.rs",
        include_str!("origin/src/components/separator/recipe.rs"),
    ),
    (
        "src/components/separator/separator.rs",
        include_str!("origin/src/components/separator/separator.rs"),
    ),
    (
        "src/components/site_header/md.rs",
        include_str!("origin/src/components/site_header/md.rs"),
    ),
    (
        "src/components/site_header/mod.rs",
        include_str!("origin/src/components/site_header/mod.rs"),
    ),
    (
        "src/components/site_header/recipe.rs",
        include_str!("origin/src/components/site_header/recipe.rs"),
    ),
    (
        "src/components/site_header/site_header.rs",
        include_str!("origin/src/components/site_header/site_header.rs"),
    ),
    (
        "src/components/spinner/md.rs",
        include_str!("origin/src/components/spinner/md.rs"),
    ),
    (
        "src/components/spinner/mod.rs",
        include_str!("origin/src/components/spinner/mod.rs"),
    ),
    (
        "src/components/spinner/recipe.rs",
        include_str!("origin/src/components/spinner/recipe.rs"),
    ),
    (
        "src/components/spinner/spinner.rs",
        include_str!("origin/src/components/spinner/spinner.rs"),
    ),
    (
        "src/components/theme_toggle/md.rs",
        include_str!("origin/src/components/theme_toggle/md.rs"),
    ),
    (
        "src/components/theme_toggle/mod.rs",
        include_str!("origin/src/components/theme_toggle/mod.rs"),
    ),
    (
        "src/components/theme_toggle/recipe.rs",
        include_str!("origin/src/components/theme_toggle/recipe.rs"),
    ),
    (
        "src/components/theme_toggle/theme_toggle.rs",
        include_str!("origin/src/components/theme_toggle/theme_toggle.rs"),
    ),
    (
        "src/components/tooltip/md.rs",
        include_str!("origin/src/components/tooltip/md.rs"),
    ),
    (
        "src/components/tooltip/mod.rs",
        include_str!("origin/src/components/tooltip/mod.rs"),
    ),
    (
        "src/components/tooltip/recipe.rs",
        include_str!("origin/src/components/tooltip/recipe.rs"),
    ),
    (
        "src/components/tooltip/tooltip.rs",
        include_str!("origin/src/components/tooltip/tooltip.rs"),
    ),
    (
        "src/layouts/mod.rs",
        include_str!("origin/src/layouts/mod.rs"),
    ),
    (
        "src/layouts/root.rs",
        include_str!("origin/src/layouts/root.rs"),
    ),
    ("src/lib.rs", include_str!("origin/src/lib.rs")),
    ("src/loader.rs", include_str!("origin/src/loader.rs")),
    ("src/main.rs", include_str!("origin/src/main.rs")),
    ("src/model.rs", include_str!("origin/src/model.rs")),
    ("styles/input.css", include_str!("origin/styles/input.css")),
    (
        "tests/atlas/dataloader.rs",
        include_str!("origin/tests/atlas/dataloader.rs"),
    ),
    (
        "tests/atlas/golden.rs",
        include_str!("origin/tests/atlas/golden.rs"),
    ),
    (
        "tests/atlas/main.rs",
        include_str!("origin/tests/atlas/main.rs"),
    ),
    (
        "tests/atlas/origin.rs",
        include_str!("origin/tests/atlas/origin.rs"),
    ),
    (
        "tests/golden/home.html",
        include_str!("origin/tests/golden/home.html"),
    ),
];

pub fn populate(
    plan: &mut SitePlan,
    package: &str,
    title: &str,
    selection: CapabilitySelection,
    vscode: bool,
    agents_md: bool,
) -> Result<()> {
    let ident = crate_ident(package);

    plan.write("Cargo.toml", manifest(package, ManifestFlavor::Standalone))?;
    plan.write("Cargo.lock", "# Generated by Proa.\nversion = 4\n")?;
    plan.write("rust-toolchain.toml", super::rust_toolchain())?;
    plan.write(".cargo/config.toml", super::cargo_config())?;
    plan.write(".env.example", env_example(&ident))?;
    plan.write(".gitignore", super::gitignore())?;
    plan.write(".gitattributes", "*.sql text eol=lf\n")?;
    plan.write("README.md", format!("{}\n## Dependency access\n\nProa dependencies resolve from crates.io. No GitHub credentials or Cargo registry token is required to build the generated project.\n", readme(package, title)))?;
    plan.write(".github/workflows/ci.yml", super::github_ci(package))?;
    plan.write(
        "proa.config.json",
        super::project_config(package, selection)?,
    )?;

    for (path, source) in SOURCE_FILES {
        plan.write(*path, render_source(source, &ident))?;
    }

    if vscode {
        plan.write(".vscode/extensions.json", super::vscode_extensions())?;
        plan.write(".vscode/settings.json", super::vscode_settings())?;
        plan.write(".vscode/tasks.json", super::vscode_tasks(false))?;
    }
    if agents_md {
        plan.write("AGENTS.md", super::agents())?;
    }
    super::add_deploy(plan, package, selection.deploy, false)?;
    Ok(())
}

pub fn render_source(source: &str, ident: &str) -> String {
    source.replace(CRATE_IDENT_PLACEHOLDER, ident)
}

pub fn crate_ident(package: &str) -> String {
    package.replace('-', "_")
}

fn env_example(ident: &str) -> String {
    format!("PROA_HOST=127.0.0.1\nPORT=3000\nRUST_LOG={ident}=info,tower_http=info\n")
}

fn readme(package: &str, title: &str) -> String {
    format!(
        r#"<div align="center">

# {title}

**ORIGIN** — a live routing atlas, scaffolded as the Proa `origin` reference
application. Enter any domain, IP, prefix, or ASN and watch one lookup walk
through DNS, addressing, and BGP on real RIPEstat data.

[Proa docs](https://proa.so/docs) ·
[Component catalog](https://ui.proa.so) ·
[Framework source](https://github.com/Proa-Labs/proa)

</div>

---

## Run it

```console
cp .env.example .env
proa dev --open
```

`proa build` produces the release binary; `proa test` runs the suite through
the linked island pipeline (plain `cargo test` rejects island binaries at
link time by design).

## How this app fits together

| Path | What lives there |
| --- | --- |
| `src/lib.rs` | Routes: the page, `/site/<host>.md`, `/lab/rows`, health |
| `src/loader.rs` | The RIPEstat `DataLoader`: request-scoped dedup + 60s TTL cache |
| `src/api.rs` | RIPEstat client with per-widget timeout budgets |
| `src/model.rs` | Curated dossiers, carrier names, collector locations |
| `src/components/` | One directory per component: `mod.rs`, `recipe.rs`, the component, and `md.rs` |
| `src/components/_proa/` | Vendored proa-ui foundation (owned source, edit like any file) |
| `src/layouts/root.rs` | Document shell: fonts, theme bootstrap, cache-busted assets |
| `styles/input.css` | Tailwind v4 entry: ORIGIN palette tokens, keyframes, responsive layer |
| `public/origin.js` | Progressive engine: RIS Live socket, feeds, render lab |
| `tests/golden/home.html` | Byte-for-byte pin of the server-rendered document |

## Editing guide (read this before changing anything, humans and agents alike)

1. **Every visible component renders twice.** HTML lives in the component
   file (`html_sync!` for sync, `html!` for async); the Markdown twin lives
   next to it in `md.rs` (`md_sync!`). If you change one surface, change the
   other — the golden test and `/site/<host>.md` will catch drift.
2. **Classes belong in `recipe.rs`.** Components use Tailwind utilities via
   recipe constants; run `proa dev` and the pipeline recompiles
   `public/styles.css` from `styles/input.css`. New colors go in the
   `@theme` block as tokens, not as hex literals in markup.
3. **Interactivity is an rsjs island only when it needs a typed browser
   API** (clipboard, signals, dialogs). Everything else is server-rendered
   or lives in `public/origin.js` as progressive enhancement.
4. **Data flows through the loader.** Panes request one shared
   `ripestat:dossier:<query>` key; the resolver collapses a whole render
   into one upstream fan-out and the TTL cache serves reloads. Never fetch
   from a component.
5. **The golden test is the contract.** After an intentional markup change:
   `PROA_UPDATE_GOLDEN=1 proa test --test atlas`, then re-run `proa test`.
6. **Before you finish:** `proa fmt`, `proa lint`, `proa build`. Intentional
   allocations need a `// proa-perf-ok: reason` marker; the lint fails the
   build on unannotated warnings.

## Full documentation for agents

Fetch these as plain text — they are written to be read by tooling:

- `https://proa.so/llms-full.txt` — the complete Proa framework docs
- `https://ui.proa.so/llms-full.txt` — the complete proa-ui component docs

To add a catalog component: `proa ui add <name>` vendors its source into
`src/components/<name>/` where you own it.

---

Package: `{package}` · SSR contract pinned by `tests/golden/home.html`
"#
    )
}

pub fn manifest(package: &str, flavor: ManifestFlavor) -> String {
    match flavor {
        ManifestFlavor::Standalone => format!(
            r#"[package]
name = "{package}"
version = "0.1.0"
edition = "2021"
rust-version = "1.93"
description = "ORIGIN: a live routing atlas built with Proa and RIPE data"

[dependencies]
anyhow = {{ version = "1" }}
axum = {{ version = "0.7.9" }}
const_format = {{ version = "0.2" }}
heapless = {{ version = "0.8" }}
proa_actions = {{ version = "0.3.0" }}
proa_core = {{ version = "0.3.0" }}
proa_fonts = {{ version = "0.1.0" }}
proa_fonts_macros = {{ version = "0.1.0" }}
proa_framework_axum = {{ version = "0.3.0" }}
proa_ir = {{ version = "0.1.2" }}
proa_macros = {{ version = "0.3.0" }}
proa_stream = {{ version = "0.3.0" }}
reqwest = {{ version = "0.12", features = ["json"] }}
rsjs = {{ version = "0.2.0" }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = {{ version = "1" }}
thiserror = {{ version = "2" }}
tokio = {{ version = "1", features = ["macros", "rt-multi-thread", "signal"] }}
tower-http = {{ version = "0.6", features = ["fs", "trace"] }}
tracing = {{ version = "0.1" }}
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}

[build-dependencies]
proa_fonts_build = {{ version = "0.1.0" }}

[dev-dependencies]
http-body-util = {{ version = "0.1" }}
tower = {{ version = "0.5", features = ["util"] }}

[profile.release]
lto = "thin"
codegen-units = 1
"#
        ),
        #[cfg(test)]
        ManifestFlavor::Workspace => format!(
            r#"# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

[package]
name = "{package}"
version = "0.1.0"
edition.workspace = true
publish = false
rust-version.workspace = true
description = "ORIGIN: a live routing atlas built with Proa and RIPE data"
license-file.workspace = true

[dependencies]
anyhow = {{ workspace = true }}
axum = {{ workspace = true }}
const_format = {{ version = "0.2" }}
heapless = {{ workspace = true }}
proa_actions = {{ workspace = true }}
proa_core = {{ workspace = true }}
proa_fonts = {{ workspace = true }}
proa_fonts_macros = {{ workspace = true }}
proa_framework_axum = {{ workspace = true }}
proa_ir = {{ workspace = true }}
proa_macros = {{ workspace = true }}
proa_stream = {{ workspace = true, features = ["rt-tokio"] }}
reqwest = {{ workspace = true, features = ["json"] }}
rsjs = {{ workspace = true }}
serde = {{ workspace = true }}
serde_json = {{ workspace = true }}
thiserror = {{ workspace = true }}
tokio = {{ workspace = true }}
tower-http = {{ workspace = true }}
tracing = {{ workspace = true }}
tracing-subscriber = {{ workspace = true, features = ["env-filter"] }}

[build-dependencies]
proa_fonts_build = {{ workspace = true }}

[dev-dependencies]
http-body-util = {{ workspace = true }}
tower = {{ workspace = true }}
"#
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    /// `examples/origin-routing-atlas` must be exactly this template rendered
    /// with the workspace manifest flavor and the example's crate identity.
    /// When this fails, regenerate the example from the template (or fold the
    /// example's change back into `templates/origin/`), never let them drift.
    #[test]
    fn example_matches_the_origin_template() {
        let example =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/origin-routing-atlas");
        if !example.exists() {
            // Packaged builds of proa-cli do not carry the workspace examples.
            return;
        }
        let ident = "proa_origin_routing_atlas";

        let mut drifted = Vec::new();
        for (path, source) in SOURCE_FILES {
            let rendered = render_source(source, ident);
            let on_disk = fs::read_to_string(example.join(path)).unwrap_or_default();
            if rendered != on_disk {
                drifted.push(*path);
            }
        }
        let manifest_on_disk = fs::read_to_string(example.join("Cargo.toml")).unwrap_or_default();
        if manifest(ident, ManifestFlavor::Workspace) != manifest_on_disk {
            drifted.push("Cargo.toml");
        }
        assert!(
            drifted.is_empty(),
            "examples/origin-routing-atlas drifted from templates/origin for: {drifted:?}"
        );
    }

    #[test]
    fn crate_ident_replaces_hyphens() {
        assert_eq!(crate_ident("origin-demo"), "origin_demo");
        assert_eq!(
            render_source("use __CRATE_IDENT__::app;", "origin_demo"),
            "use origin_demo::app;"
        );
    }
}
