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

pub(crate) fn deployment_dockerfile(has_database: bool) -> String {
    templates::current_deployment_dockerfile(has_database)
}

/// The `src/db.rs` that `proa new site` and `proa add database sqlite` both write.
pub(crate) fn database_module() -> &'static str {
    templates::database_module()
}

/// The managed-head `src/layouts/root.rs` that `proa new site` writes.
#[cfg(test)]
pub(crate) fn root_layout() -> &'static str {
    templates::root_layout()
}

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

pub(crate) fn scaffold_version(package: &str) -> String {
    let versions: std::collections::BTreeMap<String, String> = toml::from_str(include_str!(
        concat!(env!("OUT_DIR"), "/scaffold-versions.toml")
    ))
    .expect("bundled scaffold versions are valid TOML");
    let version = versions
        .get(package)
        .expect("scaffold dependency is listed");
    if package == "proa" {
        return proa_scaffold_requirement(version);
    }
    version.clone()
}

fn proa_scaffold_requirement(requirement: &str) -> String {
    // Cargo treats `0.4` and `0.4.0` alike, but `0.0` admits more than `0.0.0`.
    if let Ok(version) = semver::Version::parse(requirement) {
        if version.patch == 0
            && version.pre.is_empty()
            && version.build.is_empty()
            && (version.major != 0 || version.minor != 0)
        {
            return format!("{}.{}", version.major, version.minor);
        }
    }
    requirement.to_owned()
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
        let mut args = vec!["run", "--root", root_arg.as_str(), "dev"];
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
    println!("\nNext:\n  cd {} && proa run dev --open", root.display());
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
            next: format!("cd {} && proa run dev --open", plan.destination.display()),
        },
    }
}

fn run_child(root: &Path, args: &[&str]) -> Result<()> {
    let executable = crate::util::current_executable().context("locate the running Proa CLI")?;
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
    use crate::capabilities::{DatabaseChoice, StarterChoice, StylingChoice, UiChoice};

    #[test]
    fn proa_requirement_shortens_only_equivalent_versions() {
        for (declared, expected) in [
            ("0.4.0", "0.4"),
            ("0.5.0", "0.5"),
            ("1.0.0", "1.0"),
            ("0.4.1", "0.4.1"),
            ("0.0.0", "0.0.0"),
            ("0.5.0-rc.1", "0.5.0-rc.1"),
            ("=0.4.0", "=0.4.0"),
        ] {
            assert_eq!(proa_scaffold_requirement(declared), expected);
        }
    }

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
            "src/lib.rs",
            "src/db.rs",
            "src/loader.rs",
            "src/components/database_badge.rs",
            "src/components/database_card.rs",
            "src/components/badge/md.rs",
            "migrations/.gitkeep",
            "data/.gitkeep",
            "tests/dataloader.rs",
            "tests/database.rs",
            "tests/representations.rs",
        ] {
            assert!(root.join(path).exists(), "missing {path}");
        }
        // Tailwind owns the generated public output; its input must include
        // the starter design so the first asset build retains that design.
        let config = std::fs::read_to_string(root.join(crate::config::CONFIG_FILE)).unwrap();
        assert!(!config.contains("build: proa::config::BuildConfig"));
        assert!(!config.contains("paths: proa::config::ProjectPaths"));
        let config = crate::config::load_project_config(&root).unwrap();
        assert_eq!(config.paths.public.as_deref(), Some(Path::new("public")));
        let [asset] = config.build.assets() else {
            panic!("expected one inferred Tailwind asset");
        };
        assert_eq!(asset.kind, "tailwind");
        assert_eq!(asset.input, Path::new("styles/input.css"));
        assert_eq!(asset.output, Path::new("public/styles.css"));
        let input = std::fs::read_to_string(root.join("styles/input.css")).unwrap();
        assert!(input.contains("@import \"./starter.css\" layer(components);"));
        let starter = std::fs::read_to_string(root.join("styles/starter.css")).unwrap();
        assert_eq!(
            starter,
            std::fs::read_to_string(root.join("public/styles.css")).unwrap()
        );
        assert!(!root.join("scripts").exists());
        assert!(!root.join("src/islands").exists());
        assert!(!root.join("rust-toolchain.toml").exists());
        let cargo_toml = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(cargo_toml.contains("rust-version = \"1.98\""));
        let home = std::fs::read_to_string(root.join("src/pages/home.rs")).unwrap();
        let parsed_home = syn::parse_file(&home).unwrap();
        let mut saw_non_use_item = false;
        for item in &parsed_home.items {
            if matches!(item, syn::Item::Use(_)) {
                assert!(
                    !saw_non_use_item,
                    "src/pages/home.rs should keep all `use` imports at the top of the file"
                );
            } else {
                saw_non_use_item = true;
            }
        }
        assert!(home.contains("const ROLE: ComponentRole = ComponentRole::PAGE;"));
        assert!(home.contains("async fn render_head("));
        assert!(!home.contains("with_head"));
        let layout = std::fs::read_to_string(root.join("src/layouts/root.rs")).unwrap();
        assert!(layout.contains("const ROLE: ComponentRole = ComponentRole::LAYOUT;"));
        assert!(layout.contains("{cx.head()}"));
        assert!(home.contains("DataResolver::request_scope"));
        assert!(home.contains("MdRender<L> for HomePage"));
        assert!(home.contains("# {HEADLINE}"));
        assert!(!home.contains("Rust\\."));
        assert!(!home.contains("multi\\-target"));
        let routes = std::fs::read_to_string(root.join("src/routes.rs")).unwrap();
        assert!(routes.contains("PublicAssets::configured()"));
        assert!(routes.contains(".with_head(proa::HeadConfig::default())"));
    }

    #[test]
    fn every_preset_has_canonical_macros_and_clean_component_hashes() {
        let temp = tempfile::tempdir().unwrap();
        for template in ["minimal", "app", "marketing", "docs", "origin"] {
            let root = temp.path().join(template);
            let mut value = options(root.clone());
            value.template = template.to_string();
            new_site(value).unwrap();
            assert!(!root.join(".cargo/config.toml").exists(), "{template}");
            assert!(!root.join("rust-toolchain.toml").exists(), "{template}");
            let config = std::fs::read_to_string(root.join(crate::config::CONFIG_FILE)).unwrap();
            assert!(!config.contains("template:"), "{template}");
            let formatting =
                crate::fmt::format_paths_with_options(std::slice::from_ref(&root), true, true)
                    .unwrap();
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
            let report = crate::scaffold::site_check(Some(root.clone())).unwrap();
            assert!(report.errors.is_empty(), "{template}: {:?}", report.errors);
            let routes = crate::scaffold::site_routes(Some(root)).unwrap();
            let markdown = routes.iter().find(|route| route.path == "/index.md");
            match template {
                "minimal" => assert!(markdown.is_none()),
                "origin" => assert_eq!(markdown.unwrap().file, Path::new("src/lib.rs")),
                _ => assert_eq!(markdown.unwrap().file, Path::new("src/pages/home.rs")),
            }
        }
    }

    #[test]
    fn any_title_scaffolds_rust_that_parses_and_keeps_the_title() {
        // Quotes, backslashes, braces, raw-string terminators, a newline,
        // non-ASCII, and the home page template's own placeholder names.
        let title =
            "Sam's \"Quote\" \\ {x}} \"# \"## # caf\u{e9} \u{65e5}\u{672c} \u{1f680}\nline2 \
            __IMPORTS__ __CONSTANTS__ __RENDER_IMPL__ __MARKDOWN_IMPL__ __ARGS__ \
            __HTML_RESPONSE__ __RESPONSE__ __MD_HANDLER__ __HEAD__ __BODY__";
        let temp = tempfile::tempdir().unwrap();
        for (template, starter) in [
            ("app", None),
            ("app", Some(StarterChoice::Blank)),
            ("marketing", None),
            ("docs", None),
            ("minimal", None),
            ("origin", None),
        ] {
            let root = temp
                .path()
                .join(format!("{template}-{}", starter.is_some()));
            let mut value = options(root.clone());
            value.template = template.to_string();
            value.overrides.starter = starter;
            value.title = Some(title.to_string());
            new_site(value).unwrap();
            for entry in walkdir::WalkDir::new(&root) {
                let path = entry.unwrap().into_path();
                if path.extension().is_some_and(|extension| extension == "rs") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    if let Err(error) = syn::parse_file(&source) {
                        panic!("{template}: {} does not parse: {error}", path.display());
                    }
                }
            }
            if template == "origin" {
                continue;
            }
            let home =
                syn::parse_file(&std::fs::read_to_string(root.join("src/pages/home.rs")).unwrap())
                    .unwrap();
            let constant = home
                .items
                .iter()
                .find_map(|item| match item {
                    syn::Item::Const(constant) if constant.ident == "TITLE" => Some(constant),
                    _ => None,
                })
                .expect("home page declares TITLE");
            let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(literal),
                ..
            }) = constant.expr.as_ref()
            else {
                panic!("TITLE is a string literal");
            };
            assert_eq!(literal.value(), title, "{template}");
        }
    }

    /// Every capability combination `proa new site` accepts for the presets
    /// that share the application templates.
    fn application_variants() -> Vec<(&'static str, CapabilityOverrides)> {
        let with = |database: Option<DatabaseChoice>, starter: Option<StarterChoice>| {
            CapabilityOverrides {
                database,
                starter,
                ..Default::default()
            }
        };
        let mut variants = Vec::new();
        for template in ["app", "marketing", "docs", "minimal"] {
            for database in [DatabaseChoice::Sqlite, DatabaseChoice::None] {
                for starter in [StarterChoice::Guided, StarterChoice::Blank] {
                    variants.push((template, with(Some(database), Some(starter))));
                }
            }
        }
        variants
    }

    fn module_names(source: &str) -> Vec<String> {
        syn::parse_file(source)
            .unwrap()
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Mod(module) => Some(module.ident.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn sites_are_library_crates_that_the_binary_and_tests_import() {
        let temp = tempfile::tempdir().unwrap();
        for (index, (template, overrides)) in application_variants().into_iter().enumerate() {
            let label = format!("{template} {overrides:?}");
            // A hyphenated package name is imported with underscores.
            let root = temp.path().join(format!("site-{index}"));
            let mut value = options(root.clone());
            value.template = template.to_string();
            value.overrides = overrides;
            new_site(value).unwrap();
            let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();

            // The library declares every module under `src/`, and nothing else does.
            let mut sources = std::fs::read_dir(root.join("src"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "rs") || path.is_dir())
                .filter(|path| !path.is_dir() || path.join("mod.rs").is_file())
                .map(|path| path.file_stem().unwrap().to_str().unwrap().to_string())
                .filter(|name| name != "lib" && name != "main")
                .collect::<Vec<_>>();
            sources.sort();
            assert_eq!(module_names(&read("src/lib.rs")), sources, "{label}");
            let main = read("src/main.rs");
            assert!(module_names(&main).is_empty(), "{label}: {main}");
            assert!(
                main.contains(&format!("use site_{index}::{{")),
                "{label}: {main}"
            );

            let tests = root.join("tests");
            for entry in std::fs::read_dir(&tests).into_iter().flatten() {
                let path = entry.unwrap().path();
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(!source.contains("#[path"), "{label}: {}", path.display());
                assert!(
                    module_names(&source).is_empty(),
                    "{label}: {}",
                    path.display()
                );
                assert!(
                    source.contains(&format!("use site_{index}::")),
                    "{label}: {}",
                    path.display()
                );
            }
            // The page each test renders exists in this variant.
            let home = read("src/pages/home.rs");
            if tests.join("dataloader.rs").exists() {
                let dataloader = read("tests/dataloader.rs");
                assert!(home.contains("{DatabaseBadge}"), "{label}");
                assert_eq!(
                    dataloader.contains("render_md"),
                    home.contains("fn render_md"),
                    "{label}"
                );
            } else {
                assert!(!home.contains("{DatabaseBadge}"), "{label}");
            }
            assert_eq!(
                tests.join("database.rs").exists(),
                root.join("src/db.rs").exists(),
                "{label}"
            );
        }
    }

    #[test]
    fn a_site_named_after_one_of_its_modules_still_imports_its_library() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["app", "db", "routes"] {
            let root = temp.path().join(name);
            new_site(options(root.clone())).unwrap();
            // `use app::{app, ...}` would import a module over the crate name.
            let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
            assert!(
                main.contains(&format!(
                    "use ::{name}::{{app, config, db, loader, routes}};"
                )),
                "{main}"
            );
            for test in ["database", "dataloader", "representations"] {
                let source =
                    std::fs::read_to_string(root.join(format!("tests/{test}.rs"))).unwrap();
                assert!(source.contains(&format!("use ::{name}::")), "{source}");
            }
        }
    }

    #[test]
    fn site_library_imports_avoid_keywords_and_dependency_names() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["async", "test", "proa", "tower-http"] {
            for template in ["app", "origin"] {
                let root = temp.path().join(template).join(name);
                let mut value = options(root.clone());
                value.template = template.to_string();
                new_site(value).unwrap();
                let manifest: toml::Table =
                    toml::from_str(&std::fs::read_to_string(root.join("Cargo.toml")).unwrap())
                        .unwrap();
                assert_eq!(manifest["lib"]["name"].as_str(), Some("proa_site_app"));
                let env = std::fs::read_to_string(root.join(".env.example")).unwrap();
                assert!(
                    env.lines()
                        .any(|line| line.starts_with("RUST_LOG=proa_site_app=")),
                    "{env}"
                );
                if template == "app" {
                    assert!(
                        env.lines()
                            .any(|line| line == format!("DATABASE_URL=sqlite://data/{name}.db")),
                        "{env}"
                    );
                }
                for path in [
                    "src/main.rs",
                    if template == "app" {
                        "tests/database.rs"
                    } else {
                        "tests/atlas/origin.rs"
                    },
                ] {
                    assert!(
                        std::fs::read_to_string(root.join(path))
                            .unwrap()
                            .contains("proa_site_app"),
                        "{root:?}/{path}"
                    );
                }
            }
        }
    }

    #[test]
    fn agents_md_states_the_head_role_and_island_contracts() {
        let temp = tempfile::tempdir().unwrap();
        for template in ["app", "origin"] {
            let root = temp.path().join(template);
            let mut value = options(root.clone());
            value.template = template.to_string();
            new_site(value).unwrap();
            let agents = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
            for contract in [
                "const ROLE: ComponentRole = ComponentRole::PAGE;",
                "ComponentRole::LAYOUT",
                "render_head",
                // Where modules are declared, and which command runs which tests.
                "Declare each new top-level module in `src/lib.rs`",
                "`cargo test --lib`",
                // What an island body may contain, and where to read more.
                "#[rsjs(component, client)]",
                "use proa::rsjs::{self, event_handler, rsjs, signal};",
                "exactly one top-level element",
                "#[rsjs::client]",
                "unsupported_shapes",
                "https://proa.so/docs/llms.txt",
                "Accept: text/markdown",
            ] {
                assert!(agents.contains(contract), "{template}: missing {contract}");
            }
            assert!(root.join("src/lib.rs").is_file(), "{template}");
        }
    }

    /// A site as proa-cli 0.2.2 generated it: the binary declares the modules.
    fn binary_only_site(root: &Path) {
        new_site(options(root.to_path_buf())).unwrap();
        let library = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
        let modules = module_names(&library)
            .iter()
            .map(|module| format!("mod {module};\n"))
            .collect::<String>();
        let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
        let import = main
            .lines()
            .find(|line| line.starts_with("use earlier::"))
            .unwrap()
            .to_string();
        std::fs::write(root.join("src/main.rs"), main.replace(&import, &modules)).unwrap();
        std::fs::remove_file(root.join("src/lib.rs")).unwrap();
    }

    #[test]
    fn a_binary_only_site_from_an_earlier_release_still_checks() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("earlier");
        binary_only_site(&root);
        let report = crate::scaffold::site_check(Some(root.clone())).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let routes = crate::scaffold::site_routes(Some(root.clone())).unwrap();
        assert!(routes.iter().any(|route| route.path == "/index.md"));
        assert_eq!(
            crate::scaffold::module_declaration(&root, "db"),
            "`mod db;` to `src/main.rs`"
        );

        // Its first island is the point where `proa test` stops working.
        let counter = root.join("src/components/counter.rs");
        std::fs::write(
            &counter,
            "// #[rsjs(component, client)]\nconst EXAMPLE: &str = \"#[rsjs(component)]\";",
        )
        .unwrap();
        assert!(crate::scaffold::site_check(Some(root.clone()))
            .unwrap()
            .warnings
            .is_empty());
        for attr in [
            "#[rsjs(component, client)]",
            "#[rsjs::rsjs (component, client)]",
        ] {
            std::fs::write(&counter, format!("mod nested {{ {attr}\nimpl<L: DataLoader> WebRenderSync<L> for Counter {{}} }}")).unwrap();
            let report = crate::scaffold::site_check(Some(root.clone())).unwrap();
            assert!(report.errors.is_empty(), "{:?}", report.errors);
            let [warning] = report.warnings.as_slice() else {
                panic!("expected one warning: {:?}", report.warnings);
            };
            assert!(warning.contains("`src/lib.rs`"), "{warning}");
            assert!(warning.contains("use earlier::"), "{warning}");
        }

        // The same island in a library-crate site needs no warning.
        let current = temp.path().join("current");
        new_site(options(current.clone())).unwrap();
        std::fs::copy(
            root.join("src/components/counter.rs"),
            current.join("src/components/counter.rs"),
        )
        .unwrap();
        let report = crate::scaffold::site_check(Some(current.clone())).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(
            crate::scaffold::module_declaration(&current, "db"),
            "`pub mod db;` to `src/lib.rs`"
        );
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
        let config = std::fs::read_to_string(root.join(crate::config::CONFIG_FILE)).unwrap();
        assert!(config.contains("assets: Some(vec![])"));
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
