// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Application-first Proa site planning and transactional creation.

mod plan;
mod templates;
mod transaction;

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::{
    capabilities::{CapabilityOverrides, CapabilitySelection, SiteTemplate},
    scaffold::DryRunFormat,
};

pub use plan::{PlannedFile, SitePlan};

#[derive(Debug, Clone)]
pub struct NewSiteOptions {
    pub path: PathBuf,
    pub destination: Option<PathBuf>,
    pub title: Option<String>,
    pub template: String,
    pub overrides: CapabilityOverrides,
    pub vscode: bool,
    pub agents_md: bool,
    pub no_git: bool,
    pub verify: bool,
    pub start: bool,
    pub open: bool,
    pub offline: bool,
    pub dry_run: Option<DryRunFormat>,
    pub json: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationReport<'a> {
    operation_id: &'static str,
    command_version: u16,
    project_root: &'a Path,
    planned_filesystem_operations: Vec<FilesystemOperation<'a>>,
    configuration_changes: CapabilitySelection,
    warnings: Vec<&'static str>,
    result: OperationResult<'a>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FilesystemOperation<'a> {
    operation: &'static str,
    path: &'a Path,
    bytes: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationResult<'a> {
    status: &'a str,
    destination: &'a Path,
    package: &'a str,
    title: &'a str,
    selection: CapabilitySelection,
    files: usize,
    next: String,
}

pub fn new_site(options: NewSiteOptions) -> Result<()> {
    let template = SiteTemplate::parse_compat(&options.template)?;
    let selection = CapabilitySelection::resolve(template, options.overrides)?;
    let root = resolve_destination(&options.path, options.destination.as_deref())?;
    transaction::ensure_destination_available(&root)?;
    let package = package_name(&root)?;
    let title = options.title.unwrap_or_else(|| display_title(&package));
    let capabilities = selection
        .capabilities()
        .into_iter()
        .map(|capability| capability.id.to_string())
        .collect();
    let mut plan = SitePlan::new(root.clone(), capabilities);
    templates::populate(
        &mut plan,
        &package,
        &title,
        selection,
        options.vscode,
        options.agents_md,
    )?;
    plan.validate()?;

    if let Some(format) = options.dry_run {
        print_plan(&plan, &package, &title, selection, format)?;
        return Ok(());
    }

    transaction::commit(&plan)?;
    if !options.no_git {
        transaction::initialize_git(&root)?;
    }
    print_success(&plan, &package, &title, selection, options.json)?;

    if options.verify {
        run_child(
            &root,
            &["check", "--all", "--root", root.to_string_lossy().as_ref()],
        )
        .context(
            "project was created, but verification failed; repair it and run `proa check --all`",
        )?;
    }
    if options.start || options.open {
        let root_arg = root.to_string_lossy().into_owned();
        let mut args = vec!["dev", "--root", root_arg.as_str()];
        if options.open {
            args.push("--open");
        }
        if options.offline {
            args.push("--offline");
        }
        run_child(&root, &args)?;
    }
    Ok(())
}

fn resolve_destination(path: &Path, parent: Option<&Path>) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("site path cannot be empty");
    }
    if path.is_absolute() {
        if parent.is_some() {
            bail!("--destination cannot be combined with an absolute site path");
        }
        Ok(path.to_path_buf())
    } else {
        Ok(parent
            .map(Path::to_path_buf)
            .unwrap_or(env::current_dir()?)
            .join(path))
    }
}

fn package_name(root: &Path) -> Result<String> {
    let raw = root
        .file_name()
        .and_then(|value| value.to_str())
        .context("site path must end in a UTF-8 directory name")?;
    let mut package = String::with_capacity(raw.len());
    for character in raw.chars() {
        if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
            package.push(character.to_ascii_lowercase());
        } else if character.is_whitespace() {
            package.push('-');
        } else {
            package.push('_');
        }
    }
    while package.starts_with(|character: char| !character.is_ascii_alphabetic()) {
        package.remove(0);
    }
    if package.is_empty() {
        bail!("site path does not contain a valid package name");
    }
    Ok(package)
}

fn display_title(package: &str) -> String {
    package
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), characters.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn print_plan(
    plan: &SitePlan,
    package: &str,
    title: &str,
    selection: CapabilitySelection,
    format: DryRunFormat,
) -> Result<()> {
    match format {
        DryRunFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&operation_report(
                plan, package, title, selection, "planned"
            ))?
        ),
        DryRunFormat::Text => {
            println!("Plan {}", plan.destination.display());
            for capability in &plan.capabilities {
                println!("  capability  {capability}");
            }
            for file in &plan.files {
                println!("  create      {} ({} B)", file.path.display(), file.bytes);
            }
        }
    }
    Ok(())
}

fn print_success(
    plan: &SitePlan,
    package: &str,
    title: &str,
    selection: CapabilitySelection,
    json: bool,
) -> Result<()> {
    let root = &plan.destination;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&operation_report(
                plan, package, title, selection, "created"
            ))?
        );
        return Ok(());
    }
    println!("Created {package}\n");
    println!("  App         Proa + Axum");
    println!(
        "  Database    {}",
        if selection.database == crate::capabilities::DatabaseChoice::Sqlite {
            "SQLite + SQLx"
        } else {
            "none"
        }
    );
    if selection.database == crate::capabilities::DatabaseChoice::Sqlite {
        println!("  Data        keyed loading through cx");
    }
    println!(
        "  UI          {}",
        match selection.ui {
            crate::capabilities::UiChoice::Proa => "proa-ui · 3 components vendored",
            crate::capabilities::UiChoice::ProaDocs => "proa-docs",
            crate::capabilities::UiChoice::None => "none",
        }
    );
    println!(
        "  Markdown    {}",
        if selection.markdown {
            "/index.md · Accept: text/markdown"
        } else {
            "disabled"
        }
    );
    println!(
        "  Client      {}",
        if selection.template == crate::capabilities::SiteTemplate::Origin {
            "rsjs flood-the-table island on /"
        } else {
            "no client root on /"
        }
    );
    println!("  Health      /healthz · /readyz");
    println!("\nNext:\n  cd {} && proa dev --open", root.display());
    Ok(())
}

fn operation_report<'a>(
    plan: &'a SitePlan,
    package: &'a str,
    title: &'a str,
    selection: CapabilitySelection,
    status: &'a str,
) -> OperationReport<'a> {
    OperationReport {
        operation_id: "site.create",
        command_version: 1,
        project_root: &plan.destination,
        planned_filesystem_operations: plan
            .files
            .iter()
            .map(|file| FilesystemOperation {
                operation: "create",
                path: &file.path,
                bytes: file.bytes,
            })
            .collect(),
        configuration_changes: selection,
        warnings: Vec::new(),
        result: OperationResult {
            status,
            destination: &plan.destination,
            package,
            title,
            selection,
            files: plan.files.len(),
            next: format!("cd {} && proa dev --open", plan.destination.display()),
        },
    }
}

fn run_child(root: &Path, args: &[&str]) -> Result<()> {
    let executable = env::current_exe().context("locate the running Proa CLI")?;
    let status = Command::new(executable)
        .args(args)
        .current_dir(root)
        .status()
        .context("start Proa child command")?;
    if !status.success() {
        bail!("Proa child command failed with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{DatabaseChoice, StylingChoice, UiChoice};

    fn options(path: PathBuf) -> NewSiteOptions {
        NewSiteOptions {
            path,
            destination: None,
            title: None,
            template: "app".to_string(),
            overrides: CapabilityOverrides::default(),
            vscode: true,
            agents_md: true,
            no_git: true,
            verify: false,
            start: false,
            open: false,
            offline: true,
            dry_run: None,
            json: false,
        }
    }

    #[test]
    fn default_tree_has_application_first_contract() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("journal");
        new_site(options(root.clone())).unwrap();
        for path in [
            "src/db.rs",
            "src/loader.rs",
            "src/components/database_badge.rs",
            "src/components/database_card.rs",
            "src/components/badge/md.rs",
            "migrations/.gitkeep",
            "data/.gitkeep",
            "tests/dataloader.rs",
            "tests/representations.rs",
        ] {
            assert!(root.join(path).exists(), "missing {path}");
        }
        // Tailwind owns the generated public output; its input must include
        // the starter design so the first asset build retains that design.
        let input = std::fs::read_to_string(root.join("styles/input.css")).unwrap();
        assert!(input.contains("@import \"./starter.css\" layer(components);"));
        let starter = std::fs::read_to_string(root.join("styles/starter.css")).unwrap();
        assert_eq!(
            starter,
            std::fs::read_to_string(root.join("public/styles.css")).unwrap()
        );
        assert!(!root.join("scripts").exists());
        assert!(!root.join("src/islands").exists());
        let home = std::fs::read_to_string(root.join("src/pages/home.rs")).unwrap();
        syn::parse_file(&home).unwrap();
        assert!(home.contains("DataResolver::request_scope"));
        assert!(home.contains("MdRender<L> for HomePage"));
    }

    #[test]
    fn every_preset_has_canonical_macros_and_clean_component_hashes() {
        let temp = tempfile::tempdir().unwrap();
        for template in ["minimal", "app", "marketing", "docs", "origin"] {
            let root = temp.path().join(template);
            let mut value = options(root.clone());
            value.template = template.to_string();
            new_site(value).unwrap();
            for path in crate::source::find_rust_files(&[root.join("src")]).unwrap() {
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(
                    !source.contains("content_part!"),
                    "{template}: {}",
                    path.display()
                );
            }
            let formatting =
                crate::fmt::format_paths_with_options(&[root.clone()], true, true).unwrap();
            assert!(
                formatting.changed.is_empty() && formatting.diagnostics.is_empty(),
                "{template}: {:?}",
                formatting.diagnostics
            );
            let diagnostics =
                crate::lint::lint_paths(&[root.join("src")], crate::lint::LintOptions::default())
                    .unwrap();
            assert!(
                diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.severity.rank()
                        < crate::diagnostic::Severity::Warning.rank()),
                "{template}: {diagnostics:?}"
            );
            let report = crate::scaffold::site_check(Some(root)).unwrap();
            assert!(report.errors.is_empty(), "{template}: {:?}", report.errors);
        }
    }

    #[test]
    fn customized_pages_preserve_capability_checks() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("customized");
        new_site(options(root.clone())).unwrap();
        let home_path = root.join("src/pages/home.rs");
        let source = std::fs::read_to_string(&home_path)
            .unwrap()
            .replace("HomePage", "IssuePage")
            .replace("/index.md", "/issues.md");
        let issue_path = root.join("src/pages/issues.rs");
        std::fs::write(&issue_path, &source).unwrap();
        std::fs::write(
            &home_path,
            "pub async fn handler() -> &'static str { \"Welcome\" }\n",
        )
        .unwrap();
        let report = crate::scaffold::site_check(Some(root.clone())).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);

        // Removing a capability still fails after moving the real page.
        std::fs::write(
            &issue_path,
            source
                .replace("DataResolver::request_scope", "DataResolver::new")
                .replace("explicit_markdown_async_with_loader(", "other_response("),
        )
        .unwrap();
        let report = crate::scaffold::site_check(Some(root)).unwrap();
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("request-scoped DataResolver")));
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("explicit_markdown(")));
    }

    #[test]
    fn capability_deltas_remove_owned_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("bare");
        let mut value = options(root.clone());
        value.overrides.database = Some(DatabaseChoice::None);
        value.overrides.ui = Some(UiChoice::None);
        value.overrides.styling = Some(StylingChoice::Css);
        value.vscode = false;
        new_site(value).unwrap();
        assert!(!root.join("src/db.rs").exists());
        assert!(!root.join("src/components/badge").exists());
        assert!(!root.join("styles").exists());
        assert!(!root.join(".vscode").exists());
    }

    #[test]
    fn dry_run_and_nonempty_refusal_do_not_write() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("dry");
        let mut value = options(root.clone());
        value.dry_run = Some(DryRunFormat::Json);
        new_site(value).unwrap();
        assert!(!root.exists());

        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("keep"), "mine").unwrap();
        let error = new_site(options(root.clone())).unwrap_err();
        assert!(error.to_string().contains("not empty"));
        assert_eq!(std::fs::read_to_string(root.join("keep")).unwrap(), "mine");
    }
}
