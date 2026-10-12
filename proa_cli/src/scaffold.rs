// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
pub enum DryRunFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
pub enum ServerIntegration {
    Axum,
    None,
}

impl std::fmt::Display for ServerIntegration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Axum => f.write_str("axum"),
            Self::None => f.write_str("none"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
pub enum DocsIntegration {
    None,
    #[value(name = "proa_docs")]
    ProaDocs,
}

impl std::fmt::Display for DocsIntegration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::ProaDocs => f.write_str("proa_docs"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
pub enum DeployTarget {
    None,
    Coolify,
}

impl std::fmt::Display for DeployTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Coolify => f.write_str("coolify"),
        }
    }
}

pub struct NewSiteArgs {
    pub name: String,
    pub template: String,
    pub destination: Option<PathBuf>,
    pub copy: Option<String>,
    pub empty: bool,
    pub server: ServerIntegration,
    pub tailwind: bool,
    pub no_tailwind: bool,
    pub docs: DocsIntegration,
    pub blog: bool,
    pub no_blog: bool,
    pub deploy: DeployTarget,
    pub skip_install: bool,
    pub no_git: bool,
    pub agents_md: bool,
    pub no_agents_md: bool,
    pub dry_run: Option<DryRunFormat>,
    pub force: bool,
    pub yes: bool,
}

pub struct NewSiteOptions {
    pub name: String,
    pub root: PathBuf,
    pub template: String,
    pub empty: bool,
    pub server: ServerIntegration,
    pub tailwind: bool,
    pub docs: DocsIntegration,
    pub blog: bool,
    pub deploy: DeployTarget,
    pub skip_install: bool,
    pub no_git: bool,
    pub agents_md: bool,
    pub dry_run: Option<DryRunFormat>,
    pub force: bool,
    pub yes: bool,
}

impl NewSiteOptions {
    pub fn from_args(args: NewSiteArgs) -> Result<Self> {
        if args.copy.is_some() {
            bail!("--copy is reserved for template copying but is not implemented yet");
        }
        if args.tailwind && args.no_tailwind {
            bail!("--tailwind and --no-tailwind cannot be combined");
        }
        if args.blog && args.no_blog {
            bail!("--blog and --no-blog cannot be combined");
        }
        if args.agents_md && args.no_agents_md {
            bail!("--agents-md and --no-agents-md cannot be combined");
        }
        if args.deploy == DeployTarget::Coolify && args.server != ServerIntegration::Axum {
            bail!("--deploy coolify requires --server axum");
        }
        if args.empty && args.deploy != DeployTarget::None {
            bail!("--empty cannot be combined with --deploy");
        }

        let destination = args.destination.unwrap_or(env::current_dir()?);
        let name_path = PathBuf::from(&args.name);
        let root = if name_path.is_absolute() {
            name_path
        } else {
            destination.join(name_path)
        };

        let mut docs = args.docs;
        let mut blog = args.blog;
        let mut tailwind = !args.no_tailwind;
        let mut agents_md = !args.no_agents_md;

        if args.empty {
            docs = DocsIntegration::None;
            blog = false;
            tailwind = false;
            agents_md = false;
        }

        Ok(Self {
            name: args.name,
            root,
            template: args.template,
            empty: args.empty,
            server: args.server,
            tailwind,
            docs,
            blog,
            deploy: args.deploy,
            skip_install: args.skip_install,
            no_git: args.no_git,
            agents_md,
            dry_run: args.dry_run,
            force: args.force,
            yes: args.yes,
        })
    }
}

pub struct GenerateComponentOptions {
    pub name: String,
    pub root: Option<PathBuf>,
    pub folder: bool,
    pub recipe: bool,
    pub children: bool,
    pub asynchronous: bool,
    pub markdown: bool,
    pub no_test: bool,
    pub dry_run: Option<DryRunFormat>,
    pub force: bool,
}

pub struct GenerateLayoutOptions {
    pub name: String,
    pub root: Option<PathBuf>,
    pub document: bool,
    pub section: bool,
    pub replace_root: bool,
    pub dry_run: Option<DryRunFormat>,
    pub force: bool,
}

pub struct GenerateDataOptions {
    pub name: String,
    pub root: Option<PathBuf>,
    pub dry_run: Option<DryRunFormat>,
    pub force: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct ScaffoldPlan {
    pub root: PathBuf,
    pub operations: Vec<PlanOperation>,
    pub warnings: Vec<String>,
    pub follow_up: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PlanOperation {
    WriteFile {
        path: PathBuf,
        description: String,
        bytes: usize,
        #[serde(skip)]
        content: String,
    },
    AppendLine {
        path: PathBuf,
        description: String,
        line: String,
    },
}

#[derive(Debug, Serialize)]
pub struct SiteCheckReport {
    pub root: PathBuf,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RouteInfo {
    pub method: &'static str,
    pub path: String,
    pub kind: &'static str,
    pub representations: String,
    pub file: PathBuf,
}

pub fn new_site(options: NewSiteOptions) -> Result<()> {
    new_site_with_tailwind_ready(options, false)
}

pub(crate) fn new_site_with_tailwind_ready(
    options: NewSiteOptions,
    tailwind_ready: bool,
) -> Result<()> {
    let mut plan = ScaffoldPlan::new(options.root.clone());
    let project_name = package_name(
        options
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&options.name),
    );
    let title = title_case(&project_name);
    let has_docs = options.docs == DocsIntegration::ProaDocs;
    let has_blog = options.blog;
    let has_axum = options.server == ServerIntegration::Axum;
    // Font wiring needs the generated crate root (src/main.rs) to include the
    // build-generated module, so it is tied to the Axum integration.
    let has_fonts = !options.empty && has_axum;
    let has_coolify = options.deploy == DeployTarget::Coolify;

    if options.template != "basic" && options.template != "marketing" && options.template != "docs"
    {
        plan.warnings.push(format!(
            "unknown template `{}`; using the basic site shape",
            options.template
        ));
    }

    plan.write_file(
        options.root.join("Cargo.toml"),
        cargo_toml(&project_name, &options),
    );
    plan.write_file(
        options.root.join(crate::config::CONFIG_FILE),
        crate::config::generated_project_config(options.tailwind)?,
    );
    plan.write_file(
        options.root.join(crate::config::LOCK_FILE),
        crate::config::generated_component_lock()?,
    );
    plan.write_file(options.root.join(".cargo/config.toml"), cargo_config_toml());
    plan.write_file(options.root.join("scripts/dev.sh"), dev_script());
    plan.write_file(options.root.join("scripts/build.sh"), build_script());
    plan.write_file(
        options.root.join("scripts/watch-lock.sh"),
        watch_lock_script(),
    );
    plan.write_file(
        options.root.join("README.md"),
        readme(&project_name, options.deploy),
    );
    plan.write_file(options.root.join(".gitignore"), gitignore());
    if options.agents_md {
        plan.write_file(options.root.join("AGENTS.md"), agents_md());
    }

    plan.write_file(options.root.join("src/components/mod.rs"), components_mod());
    plan.write_file(
        options.root.join("src/layouts/mod.rs"),
        "pub mod root;\n".to_string(),
    );
    plan.write_file(options.root.join("src/layouts/root.rs"), root_layout());
    plan.write_file(
        options.root.join("src/data/mod.rs"),
        "pub mod nav;\npub mod routes;\n".to_string(),
    );
    plan.write_file(options.root.join("src/data/nav.rs"), nav_data());
    plan.write_file(
        options.root.join("src/data/routes.rs"),
        routes_data(has_docs, has_blog),
    );
    plan.write_file(
        options.root.join("src/pages/mod.rs"),
        pages_mod(has_docs, has_blog),
    );
    plan.write_file(
        options.root.join("src/pages/home.rs"),
        home_page(&title, has_axum),
    );
    plan.write_file(options.root.join("public/favicon.svg"), favicon_svg());

    if has_axum {
        plan.write_file(
            options.root.join("src/main.rs"),
            main_rs(has_fonts, has_docs, has_blog),
        );
        plan.write_file(
            options.root.join("src/static_assets.rs"),
            static_assets_rs(has_fonts),
        );
    } else {
        plan.follow_up
            .push("wire src/pages::home::render() into your custom server integration".to_string());
    }

    if options.tailwind {
        plan.write_file(options.root.join("styles/input.css"), tailwind_input_css());
        plan.write_file(
            options.root.join("public/styles.css"),
            generated_styles_placeholder(),
        );
        if !tailwind_ready {
            plan.follow_up.push(
                "install Tailwind CSS CLI v4 with the Proa installer (`--tailwind-only`) or Homebrew (`brew install tailwindcss`) before building assets".to_string(),
            );
        }
        plan.follow_up
            .push("run bash scripts/dev.sh to build assets and start the server".to_string());
    } else {
        plan.write_file(options.root.join("public/styles.css"), plain_css());
    }

    if has_docs {
        plan.write_file(options.root.join("src/docs/index.mdx"), docs_index());
        plan.write_file(
            options.root.join("src/docs/getting-started.mdx"),
            docs_getting_started(),
        );
        plan.write_file(options.root.join("src/pages/docs.rs"), docs_page(has_axum));
    }

    if has_blog {
        plan.write_file(
            options.root.join("src/blog/first-post.mdx"),
            blog_first_post(),
        );
        plan.write_file(options.root.join("src/pages/blog.rs"), blog_page(has_axum));
    }

    if has_fonts || has_docs || has_blog {
        plan.write_file(
            options.root.join("build.rs"),
            build_rs(has_fonts, has_docs, has_blog),
        );
    }

    if has_coolify {
        plan.write_file(options.root.join("Dockerfile"), dockerfile(&project_name));
        plan.write_file(options.root.join(".dockerignore"), dockerignore());
        plan.write_file(
            options.root.join("coolify.env.example"),
            coolify_env_example(),
        );
        plan.write_file(
            options.root.join("COOLIFY.md"),
            coolify_readme(&project_name),
        );
        plan.follow_up.push(
            "configure Coolify with Dockerfile build, port 3000, and /healthz health check"
                .to_string(),
        );
        plan.follow_up.push(
            "set up wildcard DNS and Coolify Preview Deployments for pull request URLs".to_string(),
        );
    }

    if !options.skip_install {
        plan.follow_up
            .push("run proa build before deploying".to_string());
    }
    if options.no_git {
        plan.warnings
            .push("git initialization skipped by --no-git".to_string());
    }

    finish_plan(plan, options.dry_run, options.force)
}

pub fn generate_component(options: GenerateComponentOptions) -> Result<()> {
    let root = resolve_root(options.root)?;
    let target = ModuleTarget::new(&root, "components", &options.name)?;
    let struct_name = pascal_case(&target.leaf);
    let data_attr = format!("data-{}", kebab_case(&target.leaf));
    let mut plan = ScaffoldPlan::new(root.clone());

    target.plan_mod_updates(&mut plan);

    if options.folder || options.recipe {
        let dir = target.file_path.with_extension("");
        plan.write_file(
            dir.join("mod.rs"),
            if options.recipe {
                format!(
                    "mod {};\nmod recipe;\n\npub use {}::{};\npub use recipe::{}Recipe;\n",
                    target.leaf, target.leaf, struct_name, struct_name
                )
            } else {
                format!(
                    "mod {};\n\npub use {}::{};\n",
                    target.leaf, target.leaf, struct_name
                )
            },
        );
        plan.write_file(
            dir.join(format!("{}.rs", target.leaf)),
            component_source(
                &struct_name,
                &data_attr,
                options.children,
                options.asynchronous,
                options.markdown,
                options.recipe,
                !options.no_test,
            ),
        );
        if options.recipe {
            plan.write_file(dir.join("recipe.rs"), component_recipe_source(&struct_name));
        }
    } else {
        plan.write_file(
            target.file_path,
            component_source(
                &struct_name,
                &data_attr,
                options.children,
                options.asynchronous,
                options.markdown,
                false,
                !options.no_test,
            ),
        );
    }

    finish_plan(plan, options.dry_run, options.force)
}

pub fn generate_layout(options: GenerateLayoutOptions) -> Result<()> {
    if options.document && options.section {
        bail!("--document and --section cannot be combined");
    }

    let root = resolve_root(options.root)?;
    let target = ModuleTarget::new(&root, "layouts", &options.name)?;
    let struct_name = format!("{}Layout", pascal_case(&target.leaf));
    let mut plan = ScaffoldPlan::new(root.clone());

    if options.document {
        let mut current_roots = Vec::new();
        let layouts = root.join("src/layouts");
        if layouts.is_dir() {
            for path in rust_files(&layouts)? {
                if path != target.file_path
                    && fs::read_to_string(&path)
                        .unwrap_or_default()
                        .contains("// proa:document-root")
                {
                    current_roots.push(path);
                }
            }
        }
        if !current_roots.is_empty() && !options.replace_root {
            bail!(
                "a document root is already marked at {}; rerun with --replace-root",
                display_rel(&root, &current_roots[0])
            );
        }
        for path in current_roots {
            let source = fs::read_to_string(&path)?
                .replace("// proa:document-root", "// proa:document-layout");
            plan.write_file(path, source);
        }
    }

    target.plan_mod_updates(&mut plan);
    let source = if options.document {
        document_layout_source(&struct_name)
    } else {
        section_layout_source(&struct_name)
    };
    plan.write_file(target.file_path, source);

    finish_plan(plan, options.dry_run, options.force || options.replace_root)
}

pub fn generate_data(options: GenerateDataOptions) -> Result<()> {
    let root = resolve_root(options.root)?;
    let target = ModuleTarget::new(&root, "static_data", &options.name)?;
    let mut plan = ScaffoldPlan::new(root);

    target.plan_mod_updates(&mut plan);
    plan.write_file(target.file_path, data_source(&target.leaf));

    finish_plan(plan, options.dry_run, options.force)
}

pub fn site_check(root: Option<PathBuf>) -> Result<SiteCheckReport> {
    let root = resolve_root(root)?;
    let mut report = SiteCheckReport {
        root: root.clone(),
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    let config_value = fs::read_to_string(root.join(crate::config::CONFIG_FILE))
        .ok()
        .and_then(|source| serde_json::from_str::<serde_json::Value>(&source).ok());
    let application_first = config_value
        .as_ref()
        .and_then(|value| value.get("application"))
        .is_some();
    let template = config_value
        .as_ref()
        .and_then(|value| value.pointer("/application/template"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let origin_template = template == "origin";
    let required: &[&str] = if origin_template {
        &[
            "src/lib.rs",
            "src/main.rs",
            "src/components/mod.rs",
            "src/layouts/mod.rs",
            "public/styles.css",
        ]
    } else if application_first {
        &[
            "src/pages/mod.rs",
            "src/components/mod.rs",
            "src/layouts/mod.rs",
            "src/endpoints/mod.rs",
            "src/routes.rs",
            "src/app.rs",
            "src/config.rs",
            "src/error.rs",
            "src/static_assets.rs",
        ]
    } else {
        &[
            "src/pages/mod.rs",
            "src/components/mod.rs",
            "src/layouts/mod.rs",
            "src/data/mod.rs",
        ]
    };
    for rel in required {
        if !root.join(rel).exists() {
            report.errors.push(format!("missing {rel}"));
        }
    }

    if root.join("src/islands").exists() {
        report
            .errors
            .push("src/islands should not be generated for this site convention".to_string());
    }

    if application_first {
        // Capability checks follow application code after users replace starter pages.
        let sources = rust_files(&root.join("src"))?
            .into_iter()
            .map(fs::read_to_string)
            .collect::<std::io::Result<Vec<_>>>()?;
        let has_source = |needle: &str| sources.iter().any(|source| source.contains(needle));
        if domain_root_is_empty(&root.join("src/models"))
            || domain_root_is_empty(&root.join("src/services"))
        {
            report.errors.push(
                "empty domain roots must not be generated; create models/services on demand"
                    .to_string(),
            );
        }
        let database = config_value
            .as_ref()
            .and_then(|value| value.pointer("/database/driver"))
            .and_then(serde_json::Value::as_str);
        if database == Some("sqlite") {
            let manual = config_value
                .as_ref()
                .and_then(|value| value.pointer("/database/integration"))
                .and_then(serde_json::Value::as_str)
                == Some("manual");
            let required_database_files: &[&str] = if manual {
                &["src/db.rs", "data/.gitkeep", "migrations/.gitkeep"]
            } else {
                &[
                    "src/db.rs",
                    "src/loader.rs",
                    "data/.gitkeep",
                    "migrations/.gitkeep",
                ]
            };
            for rel in required_database_files {
                if !root.join(rel).exists() {
                    report
                        .errors
                        .push(format!("SQLite capability is missing {rel}"));
                }
            }
            if manual {
                report.warnings.push(
                    "SQLite is installed with manual integration pending; add `mod db;`, connect the pool, and place it in application state"
                        .to_string(),
                );
            } else {
                if !has_source("DataResolver::request_scope") {
                    report.errors.push(
                        "SQLite application does not create a request-scoped DataResolver"
                            .to_string(),
                    );
                }
            }
        }
        let markdown = config_value
            .as_ref()
            .and_then(|value| value.pointer("/representations/markdown/enabled"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if markdown {
            if origin_template {
                let library = fs::read_to_string(root.join("src/lib.rs")).unwrap_or_default();
                for contract in [
                    "MarkdownAlternate::new(\"/index.md\")",
                    "explicit_markdown(",
                ] {
                    if !library.contains(contract) {
                        report.errors.push(format!(
                            "Markdown representation contract is missing `{contract}`"
                        ));
                    }
                }
                let page = fs::read_to_string(root.join("src/components/atlas_page.rs"))
                    .unwrap_or_default();
                if !page.contains("MdRenderSync") {
                    report.errors.push(
                        "Markdown representation contract is missing `MdRenderSync`".to_string(),
                    );
                }
            } else {
                for contract in ["MdRenderSync", "explicit_markdown("] {
                    let has_contract = has_source(contract)
                        || match contract {
                            "MdRenderSync" => has_source("MdRender<"),
                            "explicit_markdown(" => {
                                has_source("explicit_markdown_async_with_loader(")
                            }
                            _ => false,
                        };
                    if !has_contract {
                        report.errors.push(format!(
                            "Markdown representation contract is missing `{contract}`"
                        ));
                    }
                }
            }
            let layout = fs::read_to_string(root.join("src/layouts/root.rs")).unwrap_or_default();
            if layout.contains("MdRender") {
                report
                    .errors
                    .push("RootDocument must remain HTML-only".to_string());
            }
        }
        if let Ok(validation) = crate::config::validate_project_config(&root) {
            report.errors.extend(validation.errors);
            report.warnings.extend(validation.warnings);
        }
        for rel in [
            ".vscode/extensions.json",
            ".vscode/settings.json",
            ".vscode/tasks.json",
        ] {
            let path = root.join(rel);
            if path.exists()
                && fs::read_to_string(&path)
                    .ok()
                    .and_then(|source| serde_json::from_str::<serde_json::Value>(&source).ok())
                    .is_none()
            {
                report.errors.push(format!("{rel} is not valid JSON"));
            }
        }
        validate_component_hashes(&root, &mut report)?;
        validate_migration_pairs(&root, &mut report)?;
    }

    let pages_dir = root.join("src/pages");
    if pages_dir.exists() {
        for file in rust_files(&pages_dir)? {
            if file.file_name().and_then(|name| name.to_str()) == Some("mod.rs") {
                continue;
            }
            let source =
                fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?;
            if !source.contains("fn handler") && !source.contains("fn render") {
                // proa-perf-ok: CLI validation builds diagnostic messages outside SSR render paths
                report.errors.push(format!(
                    "{} does not expose a handler or render function",
                    display_rel(&root, &file)
                ));
            }
        }
    }

    Ok(report)
}

fn domain_root_is_empty(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .all(|entry| entry.file_name() == "mod.rs")
}

fn validate_component_hashes(root: &Path, report: &mut SiteCheckReport) -> Result<()> {
    let lock_path = root.join(crate::config::LOCK_FILE);
    if !lock_path.is_file() {
        return Ok(());
    }
    let lock: serde_json::Value = serde_json::from_str(&fs::read_to_string(&lock_path)?)?;
    let Some(components) = lock
        .get("components")
        .and_then(serde_json::Value::as_object)
    else {
        return Ok(());
    };
    for (component, value) in components {
        let Some(files) = value.get("files").and_then(serde_json::Value::as_object) else {
            continue;
        };
        for (relative, expected) in files {
            let path = root.join(relative);
            let actual = fs::read(&path)
                .ok()
                .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)));
            if actual.as_deref() != expected.as_str() {
                report.errors.push(format!(
                    "component {component} has local drift in {relative}"
                ));
            }
        }
    }
    Ok(())
}

fn validate_migration_pairs(root: &Path, report: &mut SiteCheckReport) -> Result<()> {
    let directory = root.join("migrations");
    if !directory.is_dir() {
        return Ok(());
    }
    let mut up = std::collections::BTreeSet::new();
    let mut down = std::collections::BTreeSet::new();
    for entry in fs::read_dir(directory)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if let Some(base) = name.strip_suffix(".up.sql") {
            up.insert(base.to_string());
        } else if let Some(base) = name.strip_suffix(".down.sql") {
            down.insert(base.to_string());
        }
    }
    for missing in up.difference(&down) {
        report
            .errors
            .push(format!("migration {missing} is missing its .down.sql pair"));
    }
    for missing in down.difference(&up) {
        report
            .errors
            .push(format!("migration {missing} is missing its .up.sql pair"));
    }
    Ok(())
}

pub fn print_site_check(report: &SiteCheckReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    if report.errors.is_empty() {
        println!("Proa site check passed for {}", report.root.display());
    } else {
        for error in &report.errors {
            eprintln!("error: {error}");
        }
    }
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(())
}

pub fn site_routes(root: Option<PathBuf>) -> Result<Vec<RouteInfo>> {
    let root = resolve_root(root)?;
    let config = fs::read_to_string(root.join(crate::config::CONFIG_FILE))
        .ok()
        .and_then(|source| serde_json::from_str::<serde_json::Value>(&source).ok());
    if config
        .as_ref()
        .and_then(|value| value.get("application"))
        .is_some()
    {
        let markdown = config
            .as_ref()
            .and_then(|value| value.pointer("/representations/markdown/enabled"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let mut routes = vec![RouteInfo {
            method: "GET",
            path: "/".to_string(),
            kind: "page",
            representations: if markdown {
                "html · md:/index.md".to_string()
            } else {
                "html".to_string()
            },
            file: PathBuf::from("src/pages/home.rs"),
        }];
        if markdown {
            routes.push(RouteInfo {
                method: "GET",
                path: "/index.md".to_string(),
                kind: "representation",
                representations: "markdown".to_string(),
                file: PathBuf::from("src/pages/home.rs"),
            });
        }
        for path in ["/healthz", "/readyz"] {
            routes.push(RouteInfo {
                method: "GET",
                path: path.to_string(),
                kind: "endpoint",
                representations: "json".to_string(),
                file: PathBuf::from("src/endpoints/health.rs"),
            });
        }
        let generated_routes = fs::read_to_string(root.join("src/routes.rs")).unwrap_or_default();
        for line in generated_routes.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("Route::endpoint(\"") {
                continue;
            }
            let Some(route_path) = trimmed
                .strip_prefix("Route::endpoint(\"")
                .and_then(|rest| rest.split('\"').next())
            else {
                continue;
            };
            if route_path == "/" || routes.iter().any(|route| route.path == route_path) {
                continue;
            }
            let (kind, module, method) = if let Some(rest) = trimmed.split("pages::").nth(1) {
                ("page", rest.split("::").next().unwrap_or("unknown"), "GET")
            } else if let Some(rest) = trimmed.split("endpoints::").nth(1) {
                let method = ["get", "post", "put", "patch", "delete"]
                    .into_iter()
                    .find(|method| trimmed.contains(&format!("routing::{method}(")))
                    .unwrap_or("get");
                (
                    "endpoint",
                    rest.split("::").next().unwrap_or("unknown"),
                    match method {
                        "post" => "POST",
                        "put" => "PUT",
                        "patch" => "PATCH",
                        "delete" => "DELETE",
                        _ => "GET",
                    },
                )
            } else {
                continue;
            };
            let markdown_alias = format!("{route_path}.md");
            let has_markdown = kind == "page"
                && generated_routes.contains(&format!(".route(\"{markdown_alias}\""));
            routes.push(RouteInfo {
                method,
                path: route_path.to_string(),
                kind,
                representations: if has_markdown {
                    format!("html · md:{markdown_alias}")
                } else if kind == "page" {
                    "html".to_string()
                } else {
                    "response".to_string()
                },
                file: PathBuf::from(format!("src/{}s/{module}.rs", kind)),
            });
            if has_markdown {
                routes.push(RouteInfo {
                    method: "GET",
                    path: markdown_alias,
                    kind: "representation",
                    representations: "markdown".to_string(),
                    file: PathBuf::from(format!("src/pages/{module}.rs")),
                });
            }
        }
        routes.sort_by(|left, right| left.path.cmp(&right.path));
        return Ok(routes);
    }
    let pages_dir = root.join("src/pages");
    let mut routes = Vec::new();
    if !pages_dir.exists() {
        return Ok(routes);
    }

    for file in rust_files(&pages_dir)? {
        if file.file_name().and_then(|name| name.to_str()) == Some("mod.rs") {
            continue;
        }
        let rel = file.strip_prefix(&pages_dir).unwrap_or(&file);
        routes.push(RouteInfo {
            method: "GET",
            path: route_path_for_page(rel),
            kind: "page",
            representations: "html".to_string(),
            file: display_rel_path(&root, &file),
        });
    }
    routes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(routes)
}

pub fn print_routes(routes: &[RouteInfo], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(routes)?);
        return Ok(());
    }

    println!("METHOD  PATH                             KIND            REPRESENTATIONS             SOURCE");
    for route in routes {
        println!(
            "{:<7} {:<32} {:<15} {:<27} {}",
            route.method,
            route.path,
            route.kind,
            route.representations,
            route.file.display()
        );
    }
    Ok(())
}

impl ScaffoldPlan {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            operations: Vec::new(),
            warnings: Vec::new(),
            follow_up: Vec::new(),
        }
    }

    fn write_file(&mut self, path: PathBuf, content: String) {
        self.operations.push(PlanOperation::WriteFile {
            description: "write file".to_string(),
            path,
            bytes: content.len(),
            content,
        });
    }

    fn append_line(&mut self, path: PathBuf, line: String, description: impl Into<String>) {
        self.operations.push(PlanOperation::AppendLine {
            path,
            line,
            description: description.into(),
        });
    }
}

fn finish_plan(plan: ScaffoldPlan, dry_run: Option<DryRunFormat>, force: bool) -> Result<()> {
    if let Some(format) = dry_run {
        match format {
            DryRunFormat::Text => print_plan(&plan),
            DryRunFormat::Json => println!("{}", serde_json::to_string_pretty(&plan)?),
        }
        return Ok(());
    }

    preflight(&plan, force)?;
    apply_plan(&plan, force)?;
    print_plan(&plan);
    Ok(())
}

fn print_plan(plan: &ScaffoldPlan) {
    for operation in &plan.operations {
        match operation {
            PlanOperation::WriteFile { path, .. } => println!("create {}", path.display()),
            PlanOperation::AppendLine { path, .. } => println!("update {}", path.display()),
        }
    }
    for warning in &plan.warnings {
        eprintln!("warning: {warning}");
    }
    if !plan.follow_up.is_empty() {
        println!("\nNext steps:");
        for step in &plan.follow_up {
            println!("- {step}");
        }
    }
}

fn preflight(plan: &ScaffoldPlan, force: bool) -> Result<()> {
    if force {
        return Ok(());
    }

    for operation in &plan.operations {
        if let PlanOperation::WriteFile { path, .. } = operation {
            if path.exists() {
                bail!(
                    "{} already exists (use --force to overwrite)",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

fn apply_plan(plan: &ScaffoldPlan, force: bool) -> Result<()> {
    // Journal each file's prior state so a mid-plan failure rolls everything
    // back instead of leaving a half-written project.
    let mut journal: Vec<(PathBuf, Option<String>)> = Vec::new();
    let outcome = apply_operations(plan, force, &mut journal);
    if outcome.is_err() {
        rollback(&journal);
    }
    outcome
}

fn apply_operations(
    plan: &ScaffoldPlan,
    force: bool,
    journal: &mut Vec<(PathBuf, Option<String>)>,
) -> Result<()> {
    for operation in &plan.operations {
        match operation {
            PlanOperation::WriteFile { path, content, .. } => {
                if path.exists() && !force {
                    bail!(
                        "{} already exists (use --force to overwrite)",
                        path.display()
                    );
                }
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
                journal.push((path.clone(), fs::read_to_string(path).ok()));
                fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
            }
            PlanOperation::AppendLine { path, line, .. } => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
                let mut current = fs::read_to_string(path).unwrap_or_default();
                if !current
                    .lines()
                    .any(|existing| existing.trim() == line.trim())
                {
                    journal.push((path.clone(), fs::read_to_string(path).ok()));
                    if !current.is_empty() && !current.ends_with('\n') {
                        current.push('\n');
                    }
                    current.push_str(line);
                    current.push('\n');
                    fs::write(path, current)
                        .with_context(|| format!("update {}", path.display()))?;
                }
            }
        }
    }
    Ok(())
}

fn rollback(journal: &[(PathBuf, Option<String>)]) {
    for (path, prior) in journal.iter().rev() {
        let restored = match prior {
            Some(content) => fs::write(path, content),
            None => fs::remove_file(path),
        };
        if restored.is_err() {
            eprintln!(
                "warning: could not roll back {} after a failed scaffold",
                path.display()
            );
        }
    }
}

struct ModuleTarget {
    leaf: String,
    file_path: PathBuf,
    mod_updates: Vec<(PathBuf, String)>,
}

impl ModuleTarget {
    fn new(root: &Path, bucket: &str, raw_name: &str) -> Result<Self> {
        let parts: Vec<String> = raw_name
            .split('/')
            .filter(|part| !part.is_empty())
            .map(snake_case)
            .collect();
        if parts.is_empty() {
            bail!("name cannot be empty");
        }
        let leaf = parts.last().cloned().unwrap();
        let base = root.join("src").join(bucket);
        let mut dir = base.clone();
        let mut mod_updates = Vec::new();

        for (index, part) in parts.iter().enumerate() {
            let mod_path = if index == 0 {
                base.join("mod.rs")
            } else {
                base.join(parts[..index].join("/")).join("mod.rs")
            };
            mod_updates.push((mod_path, format!("pub mod {part};")));
            if index + 1 < parts.len() {
                dir.push(part);
            }
        }

        Ok(Self {
            leaf,
            file_path: dir.join(format!("{}.rs", parts.last().unwrap())),
            mod_updates,
        })
    }

    fn plan_mod_updates(&self, plan: &mut ScaffoldPlan) {
        for (path, line) in &self.mod_updates {
            plan.append_line(path.clone(), line.clone(), "add module export");
        }
    }
}

pub fn resolve_root(root: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(root) = root {
        return Ok(root);
    }

    let mut current = env::current_dir()?;
    loop {
        if current.join("Cargo.toml").exists() && current.join("src").exists() {
            return Ok(current);
        }
        if !current.pop() {
            bail!("could not find a Proa project root (pass --root)");
        }
    }
}

fn rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files)?;
    Ok(files)
}

fn collect_rust_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn route_path_for_page(rel: &Path) -> String {
    let mut parts: Vec<String> = rel
        .with_extension("")
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_string))
        .collect();
    if parts.last().is_some_and(|part| part == "index") {
        parts.pop();
    }
    if parts.len() == 1 && parts[0] == "home" {
        return "/".to_string();
    }
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/").replace('_', "-"))
    }
}

fn display_rel(root: &Path, path: &Path) -> String {
    display_rel_path(root, path).display().to_string()
}

fn display_rel_path(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn package_name(value: &str) -> String {
    kebab_case(value)
}

fn snake_case(value: &str) -> String {
    normalize_case(value, '_')
}

fn kebab_case(value: &str) -> String {
    normalize_case(value, '-')
}

fn normalize_case(value: &str, separator: char) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep && !out.is_empty() {
            out.push(separator);
            last_was_sep = true;
        }
    }
    while out.ends_with(separator) {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("generated");
    }
    if out.starts_with(|ch: char| ch.is_ascii_digit()) {
        out.insert(0, separator);
    }
    out
}

fn pascal_case(value: &str) -> String {
    let mut out = String::new();
    for part in value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
    {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            for ch in chars {
                out.push(ch.to_ascii_lowercase());
            }
        }
    }
    if out.is_empty() {
        out.push_str("Generated");
    }
    if out.starts_with(|ch: char| ch.is_ascii_digit()) {
        out.insert_str(0, "Generated");
    }
    out
}

fn title_case(value: &str) -> String {
    value
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn fill(mut template: String, replacements: &[(&str, &str)]) -> String {
    for (needle, value) in replacements {
        template = template.replace(needle, value);
    }
    template
}

fn cargo_toml(project_name: &str, options: &NewSiteOptions) -> String {
    // Mirrors `new_site`: font wiring is tied to the generated Axum crate
    // root, since the include! boilerplate lives in src/main.rs.
    let has_fonts = !options.empty && options.server == ServerIntegration::Axum;
    let mut deps = vec![
        "proa_core = \"0.3.0\"".to_string(),
        "proa_macros = \"0.3.0\"".to_string(),
    ];
    if has_fonts {
        deps.extend([
            "proa_fonts = \"0.1.0\"".to_string(),
            "proa_fonts_macros = \"0.1.0\"".to_string(),
        ]);
    }
    if options.server == ServerIntegration::Axum {
        deps.extend([
            "proa_framework_axum = \"0.3.0\"".to_string(),
            "anyhow = \"1\"".to_string(),
            "axum = \"0.7\"".to_string(),
            "tokio = { version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }".to_string(),
            "tower = \"0.5\"".to_string(),
            "tower-http = { version = \"0.6\", features = [\"fs\", \"set-header\", \"trace\"] }"
                .to_string(),
            "tracing = \"0.1\"".to_string(),
            "tracing-subscriber = \"0.3\"".to_string(),
        ]);
    }
    if options.docs == DocsIntegration::ProaDocs || options.blog {
        deps.push("proa_docs = { version = \"0.2.0\", features = [\"axum\"] }".to_string());
    }

    let has_docs_build = options.docs == DocsIntegration::ProaDocs || options.blog;
    let mut build_deps = String::new();
    if has_fonts || has_docs_build {
        build_deps.push_str("\n[build-dependencies]\n");
        if has_fonts {
            build_deps.push_str("proa_fonts_build = \"0.1.0\"\n");
        }
        if has_docs_build {
            build_deps.push_str("proa_docs_build = \"0.2.0\"\n");
        }
    }

    format!(
        "[package]\nname = \"{project_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{}\n{build_deps}\n[profile.release]\nlto = \"thin\"\ncodegen-units = 1\nstrip = true\n",
        deps.join("\n")
    )
}

fn cargo_config_toml() -> String {
    "# Fetch Git dependencies using your Git credential helpers and proxy settings.\n[net]\ngit-fetch-with-cli = true\n".to_string()
}

fn dev_script() -> String {
    include_str!("../templates/dev.sh").to_string()
}

fn build_script() -> String {
    include_str!("../templates/build.sh").to_string()
}

fn watch_lock_script() -> String {
    include_str!("../templates/watch-lock.sh").to_string()
}

fn readme(project_name: &str, deploy: DeployTarget) -> String {
    let deploy_section = if deploy == DeployTarget::Coolify {
        "\n## Deploy\n\nThis project includes a Coolify deployment preset. Read `COOLIFY.md`, then deploy with the generated `Dockerfile`.\n"
    } else {
        ""
    };
    format!(
        "# {}\n\nGenerated with `proa new site`.\n\nDevelop with a watching server:\n\n```bash\nbash scripts/dev.sh\n```\n\nBuild the release binary:\n\n```bash\nbash scripts/build.sh\n```\n{}",
        title_case(project_name),
        deploy_section
    )
}

fn gitignore() -> String {
    "/target\n/public/styles.css\n.env\n".to_string()
}

fn agents_md() -> String {
    "# Agent instructions\n\nFollow the Proa site structure: reusable render components in `src/components/`, route surfaces in `src/pages/`, shared shells in `src/layouts/`, and route/nav data in `src/data/`.\n\nAuthor ordinary synchronous components with explicit `impl<L: DataLoader> WebRenderSync<L>` blocks. Their `fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError>` implementations should end in `html_sync! { ... }.render(cx)`. Add `C: WebRenderSync<L>` for generic child renderables. `WebContext<L>` owns the default `RenderBuffer`; only lower-level custom-writer integrations need the second `B` parameter. When a component actually awaits, implement the canonical `WebRender<L>` signature directly and return `RenderOutcome::pending(async move { ... })`; bridge synchronous children only at that async composition boundary.\n\nOpaque SSR components use no RSJS attribute. Put `#[rsjs(component)]` on a real render-trait impl only when it must participate in analyzed composition, and use `#[rsjs(component, client)]` when that component originates signals, event handlers, queries, mutations, shared-state acquisition, node refs, or browser work. Do not use inherent render shorthand, `async fn render` sugar, or component `clients(...)` allowlists; annotate each portable browser helper once with `#[rsjs::client]`.\n\nInside an analyzed component, a direct inbound read such as `self.label` is eligible to remain live when a parent supplies a live value. A `signal(self.value)` initializer samples that inbound value once for the child-owned signal. Use `rsjs::server_only(...)` or `rsjs::server_attr(...)` only when the contract intentionally takes a per-render server snapshot.\n\nRun `proa site check` after structural changes.\n".to_string()
}

fn components_mod() -> String {
    "// Reusable Proa render components live here.\n".to_string()
}

fn pages_mod(has_docs: bool, has_blog: bool) -> String {
    let mut out = "pub mod home;\n".to_string();
    if has_docs {
        out.push_str("pub mod docs;\n");
    }
    if has_blog {
        out.push_str("pub mod blog;\n");
    }
    out
}

fn routes_data(has_docs: bool, has_blog: bool) -> String {
    let mut routes = vec!["\"/\""];
    if has_docs {
        routes.push("\"/docs\"");
        routes.push("\"/docs/*path\"");
    }
    if has_blog {
        routes.push("\"/blog\"");
        routes.push("\"/blog/*path\"");
    }
    format!("pub const ROUTES: &[&str] = &[{}];\n", routes.join(", "))
}

fn nav_data() -> String {
    "pub struct NavItem {\n    pub label: &'static str,\n    pub href: &'static str,\n}\n\npub const NAV_ITEMS: &[NavItem] = &[\n    NavItem { label: \"Home\", href: \"/\" },\n];\n".to_string()
}

fn root_layout() -> String {
    r#"use crate::data::nav::NAV_ITEMS;
use proa_core::{raw_text_ok, DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

pub struct PageMeta {
    pub title: &'static str,
    pub description: &'static str,
    pub path: &'static str,
}

pub struct RootDocument<C> {
    pub meta: PageMeta,
    pub content: C,
}

struct SiteNav;

impl<L: DataLoader> WebRenderSync<L> for SiteNav {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            {for item in NAV_ITEMS {
                html_sync! {
                    <a class="text-sm text-zinc-600 hover:text-zinc-900" href={item.href}>
                        {item.label}
                    </a>
                }
            }}
        }
        .render(cx)
    }
}

impl<L, C> WebRenderSync<L> for RootDocument<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <title>{raw_text_ok(self.meta.title)}</title>
                    <meta name="description" content={self.meta.description} />
                    <meta name="proa-route" content={self.meta.path} />
                    <link rel="icon" href="/static/favicon.svg" type="image/svg+xml" />
                    <link rel="stylesheet" href="/static/styles.css" />
                </head>
                <body>
                    <header class="mx-auto max-w-4xl px-6 pt-8">
                        <nav class="flex items-center gap-6">{SiteNav}</nav>
                    </header>
                    {self.content}
                </body>
            </html>
        }
        .render(cx)
    }
}
"#
    .to_string()
}

// proa-perf-ok(scope): trusted scaffold emitter assembles Rust source, not response HTML
fn home_page(title: &str, axum: bool) -> String {
    let response = if axum {
        "use axum::response::Html;\n\npub async fn handler() -> Html<String> {\n    // proa-perf-ok: top-level Axum response finalization owns the HTML String\n    Html(render())\n}\n\n"
    } else {
        ""
    };

    fill(
        format!(
            r#"{}use proa_core::{{
    render_to_string, DataLoader, WebContext, WebRenderSync, WriteError,
}};
use proa_macros::html_sync;

use crate::layouts::root::{{PageMeta, RootDocument}};

pub fn render() -> String {{
    // proa-perf-ok: generated scaffold owns the top-level HTML String at the HTTP boundary
    render_to_string(RootDocument {{
        meta: PageMeta {{
            title: "__TITLE__",
            description: "A Proa site.",
            path: "/",
        }},
        content: HomePage,
    }})
}}

struct HomePage;

impl<L: DataLoader> WebRenderSync<L> for HomePage {{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {{
        html_sync! {{
            <main class="mx-auto max-w-4xl px-6 py-16">
                <p class="font-mono text-sm uppercase tracking-[0.18em] text-zinc-500">"Proa"</p>
                <h1 class="mt-4 text-5xl font-semibold tracking-[-0.04em]">"__TITLE__"</h1>
                <p class="mt-4 text-lg leading-8 text-zinc-600">
                    "Edit src/pages/home.rs to start building."
                </p>
            </main>
        }}
        .render(cx)
    }}
}}

#[cfg(test)]
mod tests {{
    #[test]
    fn home_page_renders() {{
        let html = super::render();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("__TITLE__"));
        assert!(crate::data::routes::ROUTES.contains(&"/"));
    }}
}}
"#,
            response
        ),
        &[("__TITLE__", title)],
    )
}

fn docs_page(axum: bool) -> String {
    docs_like_page("DOCS", "docs", "/docs", axum)
}

fn blog_page(axum: bool) -> String {
    docs_like_page("BLOG", "blog", "/blog", axum)
}

fn docs_like_page(const_name: &str, dir: &str, base_path: &str, axum: bool) -> String {
    let template = if axum {
        r#"use axum::{extract::Path, response::Response};
use proa_docs::Docs;

static __CONST__: Docs = proa_docs::include_docs!("__DIR__");

pub async fn handler() -> Response {
    proa_docs::adapter::axum::response(&__CONST__, "__BASE__")
}

pub async fn path_handler(Path(path): Path<String>) -> Response {
    let path = if path.trim_matches('/').is_empty() {
        "__BASE__".to_string()
    } else {
        format!("__BASE__/{}", path.trim_matches('/'))
    };
    proa_docs::adapter::axum::response(&__CONST__, &path)
}
"#
    } else {
        r#"use proa_docs::{Docs, DocsResponse};

static __CONST__: Docs = proa_docs::include_docs!("__DIR__");

pub fn render(path: &str) -> DocsResponse {
    proa_docs::render(&__CONST__, path)
}
"#
    };
    fill(
        template.to_string(),
        &[
            ("__CONST__", const_name),
            ("__DIR__", dir),
            ("__BASE__", base_path),
        ],
    )
}

fn main_rs(has_fonts: bool, has_docs: bool, has_blog: bool) -> String {
    let fonts_mod = if has_fonts {
        "\n// Generated font consts for google_font!/local_font! declarations.\nmod __proa_fonts_generated { include!(concat!(env!(\"OUT_DIR\"), \"/proa_fonts.rs\")); }\n"
    } else {
        ""
    };
    let fonts_route = if has_fonts {
        "        .nest_service(\"/fonts\", static_assets::fonts_service())\n"
    } else {
        ""
    };
    let mut routes = "        .route(\"/\", get(pages::home::handler))\n        .route(\"/healthz\", get(healthz))\n".to_string();
    if has_docs {
        routes.push_str("        .route(\"/docs\", get(pages::docs::handler))\n");
        routes.push_str("        .route(\"/docs/*path\", get(pages::docs::path_handler))\n");
    }
    if has_blog {
        routes.push_str("        .route(\"/blog\", get(pages::blog::handler))\n");
        routes.push_str("        .route(\"/blog/*path\", get(pages::blog::path_handler))\n");
    }

    fill(
        r#"use axum::{
    http::header::{HeaderValue, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS},
    routing::get,
    Router,
};
use std::time::Duration;
use tower_http::{set_header::SetResponseHeaderLayer, trace::TraceLayer};

mod components;
mod data;
mod layouts;
mod pages;
mod static_assets;
__FONTS_MOD__
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

async fn healthz() -> &'static str {
    "ok"
}

async fn robots_txt() -> &'static str {
    "User-agent: *\nAllow: /\n"
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(3000);

    let app = Router::new()
__ROUTES__        .route("/robots.txt", get(robots_txt))
__FONTS_ROUTE__        .nest_service("/static", static_assets::service())
        .layer(TraceLayer::new_for_http())
        .layer(SetResponseHeaderLayer::if_not_present(
            X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ));

    let listener = tokio::net::TcpListener::bind((host.as_str(), port)).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    tracing::info!(routes = ?data::routes::ROUTES, "registered page routes");
    proa_framework_axum::serve(listener, app, SHUTDOWN_DRAIN_TIMEOUT).await?;
    Ok(())
}
"#
        .to_string(),
        &[
            ("__FONTS_MOD__", fonts_mod),
            ("__FONTS_ROUTE__", fonts_route),
            ("__ROUTES__", &routes),
        ],
    )
}

fn static_assets_rs(has_fonts: bool) -> String {
    let fonts_service = if has_fonts {
        r#"
/// Vendored font files are content-hashed, so they can be cached forever.
pub fn fonts_service() -> Router {
    serve_dir(
        static_root().join("fonts"),
        "public, max-age=31536000, immutable",
    )
}
"#
    } else {
        ""
    };
    fill(
        r#"use std::{env, path::PathBuf};

use axum::{
    http::header::{HeaderValue, CACHE_CONTROL, X_CONTENT_TYPE_OPTIONS},
    Router,
};
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

fn static_root() -> PathBuf {
    env::var_os("STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("public"))
}

fn serve_dir(dir: PathBuf, cache_control: &'static str) -> Router {
    Router::new().nest_service(
        "/",
        ServiceBuilder::new()
            .layer(SetResponseHeaderLayer::if_not_present(
                CACHE_CONTROL,
                HeaderValue::from_static(cache_control),
            ))
            .layer(SetResponseHeaderLayer::if_not_present(
                X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ))
            .service(ServeDir::new(dir)),
    )
}

pub fn service() -> Router {
    // Debug builds revalidate on every load so `proa dev` asset rebuilds show
    // up on refresh; release builds keep the shared one-hour cache.
    let cache_control = if cfg!(debug_assertions) {
        "no-cache"
    } else {
        "public, max-age=3600"
    };
    serve_dir(static_root(), cache_control)
}
__FONTS_SERVICE__"#
            .to_string(),
        &[("__FONTS_SERVICE__", fonts_service)],
    )
}

fn dockerfile(project_name: &str) -> String {
    format!(
        r#"# syntax=docker/dockerfile:1.7

FROM rust:1.93-bookworm AS build
WORKDIR /app

COPY . .

# Public Proa dependencies resolve from crates.io without credentials.
RUN cargo build --release --bin {project_name}

FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/target/release/{project_name} /usr/local/bin/{project_name}
COPY --from=build /app/public /app/public

ENV HOST=0.0.0.0
ENV PORT=3000
ENV STATIC_DIR=/app/public
ENV RUST_LOG=info

EXPOSE 3000
CMD ["{project_name}"]
"#
    )
}

fn dockerignore() -> String {
    r#".git
target
.env
.env.*
!.env.example
node_modules
.DS_Store
"#
    .to_string()
}

fn coolify_env_example() -> String {
    r#"# Runtime variables for Coolify.
HOST=0.0.0.0
PORT=3000
STATIC_DIR=/app/public
RUST_LOG=info

"#
    .to_string()
}

fn coolify_readme(project_name: &str) -> String {
    format!(
        r#"# Coolify deployment

This Proa app is ready for Coolify's Dockerfile deployment path.

## Local image check

```bash
docker build -t {project_name} .

docker run --rm -p 3000:3000 -e PORT=3000 {project_name}
curl -fsS http://127.0.0.1:3000/healthz
```

## Coolify settings

- Deployment type: Dockerfile
- Exposed port: `3000`
- Health check path: `/healthz`
- Runtime variables: copy from `coolify.env.example`

Proa dependencies resolve from crates.io at build time. No registry or GitHub
credentials are required.

## Pull request previews

Coolify can create pull request preview deployments with unique URLs. Use the
GitHub App integration, add a wildcard DNS record such as
`*.preview.example.com`, then enable Preview Deployments with a URL template
such as `{{{{pr_id}}}}.preview.example.com`.
"#
    )
}

fn build_rs(has_fonts: bool, has_docs: bool, has_blog: bool) -> String {
    let has_docs_build = has_docs || has_blog;
    let mut out = String::new();
    if has_docs_build {
        out.push_str("use std::{env, path::PathBuf};\n\n");
    }
    out.push_str("fn main() {\n");
    if has_fonts {
        out.push_str(
            r#"    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=proa-fonts.lock");
    let fonts = proa_fonts_build::collect::CollectOptions::from_build_env()
        .expect("cargo build env");
    if let Err(error) = proa_fonts_build::collect::run(&fonts) {
        panic!("proa fonts: {error}");
    }
"#,
        );
    }
    if has_docs_build {
        if has_fonts {
            out.push('\n');
        }
        out.push_str(
            r#"    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", manifest_dir.join("src/docs").display());
    println!("cargo:rerun-if-changed={}", manifest_dir.join("src/blog").display());

    let config = proa_docs_build::Config::new()
"#,
        );
        if has_docs {
            out.push_str(
                r#"        .source(
            proa_docs_build::DocsSource::new(manifest_dir.join("src/docs"))
                .out_dir_name("docs")
                .base_path("/docs")
                .site_name("Docs"),
        )
"#,
            );
        }
        if has_blog {
            out.push_str(
                r#"        .source(
            proa_docs_build::DocsSource::new(manifest_dir.join("src/blog"))
                .out_dir_name("blog")
                .base_path("/blog")
                .site_name("Blog"),
        )
"#,
            );
        }
        out.push_str(";\n\n    proa_docs_build::compile(config);\n");
    }
    out.push_str("}\n");
    out
}

fn docs_index() -> String {
    "---\ntitle: Introduction\ndescription: Start here.\nnav_order: 1\n---\n\n# Introduction\n\nWelcome to your Proa docs.\n".to_string()
}

fn docs_getting_started() -> String {
    "---\ntitle: Getting Started\ndescription: Your first Proa page.\nnav_order: 2\n---\n\n# Getting Started\n\nEdit `src/docs/getting-started.mdx`.\n".to_string()
}

fn blog_first_post() -> String {
    "---\ntitle: First Post\ndescription: Your first Proa blog post.\n---\n\n# First Post\n\nEdit `src/blog/first-post.mdx`.\n".to_string()
}

fn tailwind_input_css() -> String {
    // The layer declaration has to precede `@import "tailwindcss"`. Tailwind
    // otherwise declares `@layer theme, base, components, utilities` itself,
    // whichever declaration the browser sees first wins, and `proa-compat`
    // lands last — where a component stylesheet's hand-written colour
    // fallbacks outrank the real generated utilities and buttons come out
    // without their padding or radius.
    //
    // `proa-compat` is the layer Proa UI's runtime CSS wraps those fallbacks
    // in. Declaring it here is inert for a site that never installs a
    // component, and correct in advance for one that does.
    "@layer theme, base, proa-compat, components, utilities;\n\n\
     @import \"tailwindcss\";\n\n@source \"../src/**/*.rs\";\n\nbody {\n  margin: 0;\n}\n"
        .to_string()
}

fn generated_styles_placeholder() -> String {
    "/* Generated CSS placeholder. Run the Tailwind build command from the CLI output. */\n"
        .to_string()
}

fn plain_css() -> String {
    ":root {\n  color: #18181b;\n  background: #ffffff;\n}\n\n* {\n  box-sizing: border-box;\n}\n\nbody {\n  margin: 0;\n  font-family: system-ui, sans-serif;\n}\n".to_string()
}

fn favicon_svg() -> String {
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><path d="M13 36.5C17.6 25.7 25.5 18 36.2 13L34.8 33.5L13 36.5Z" fill="#dd7627"/><path d="M10 39.5C16.2 39.8 23.4 39.2 31.3 37.9C41.2 36.3 49 33.7 54 30.2C50.9 39.8 42.3 46.5 30 47.8C18.7 49 11.5 46 10 39.5Z" fill="#18181b"/></svg>
"##
    .to_string()
}

fn component_source(
    struct_name: &str,
    data_attr: &str,
    children: bool,
    asynchronous: bool,
    markdown: bool,
    recipe: bool,
    test: bool,
) -> String {
    let base = if recipe {
        "use super::recipe::BASE_CLASS;\n"
    } else {
        "const BASE_CLASS: &str = \"rounded-lg border p-4\";\n"
    };
    let template = match (children, asynchronous) {
        (true, false) => {
            r#"use proa_core::{ClassList, DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

__BASE__
pub struct __STRUCT__<C = ()> {
    pub class: Option<&'static str>,
    pub children: C,
}

impl<C> __STRUCT__<C> {
    pub const fn new(children: C) -> Self {
        Self { class: None, children }
    }

    pub const fn class(mut self, class: &'static str) -> Self {
        self.class = Some(class);
        self
    }
}

impl<L, C> WebRenderSync<L> for __STRUCT__<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let classes = ClassList::<2>::new().add(BASE_CLASS).add_opt(self.class);
        html_sync! {
            <div __DATA__ class={classes}>{self.children}</div>
        }
        .render(cx)
    }
}
"#
        }
        (false, false) => {
            r#"use proa_core::{ClassList, DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

__BASE__
#[derive(Debug, Clone, Copy, Default)]
pub struct __STRUCT__ {
    pub class: Option<&'static str>,
}

impl __STRUCT__ {
    pub const fn new() -> Self {
        Self { class: None }
    }

    pub const fn class(mut self, class: &'static str) -> Self {
        self.class = Some(class);
        self
    }
}

impl<L: DataLoader> WebRenderSync<L> for __STRUCT__ {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let classes = ClassList::<2>::new().add(BASE_CLASS).add_opt(self.class);
        html_sync! {
            <div __DATA__ class={classes}>"__STRUCT__"</div>
        }
        .render(cx)
    }
}
"#
        }
        (true, true) => {
            r#"use std::future::Future;

use proa_core::{ClassList, DataLoader, RenderOutcome, WebContext, WebRender, WriteError};
use proa_macros::html;

__BASE__
pub struct __STRUCT__<C = ()> {
    pub class: Option<&'static str>,
    pub children: C,
}

impl<C> __STRUCT__<C> {
    pub const fn new(children: C) -> Self {
        Self { class: None, children }
    }

    pub const fn class(mut self, class: &'static str) -> Self {
        self.class = Some(class);
        self
    }
}

impl<L, C> WebRender<L> for __STRUCT__<C>
where
    L: DataLoader,
    C: WebRender<L> + Send + 'static,
{
    fn render(self, cx: &mut WebContext<L>) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let classes = ClassList::<2>::new().add(BASE_CLASS).add_opt(self.class);
            html! {
                <div __DATA__ class={classes}>{self.children}</div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
"#
        }
        (false, true) => {
            r#"use std::future::Future;

use proa_core::{ClassList, DataLoader, RenderOutcome, WebContext, WebRender, WriteError};
use proa_macros::html;

__BASE__
#[derive(Debug, Clone, Copy, Default)]
pub struct __STRUCT__ {
    pub class: Option<&'static str>,
}

impl __STRUCT__ {
    pub const fn new() -> Self {
        Self { class: None }
    }

    pub const fn class(mut self, class: &'static str) -> Self {
        self.class = Some(class);
        self
    }
}

impl<L: DataLoader> WebRender<L> for __STRUCT__ {
    fn render(self, cx: &mut WebContext<L>) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let classes = ClassList::<2>::new().add(BASE_CLASS).add_opt(self.class);
            let content = proa_core::await_component(async { proa_core::text::text("__STRUCT__") });
            html! {
                <div __DATA__ class={classes}>{content}</div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
"#
        }
    };

    let markdown_source = if markdown {
        if children {
            r####"
use proa_core::{MdRenderSync, WriteBuf};
use proa_macros::md_sync;

impl<L: proa_core::DataLoader, B, C> MdRenderSync<L, B> for __STRUCT__<C>
where
    B: WriteBuf,
    C: MdRenderSync<L, B>,
{
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r###"
### __STRUCT__

{self.children}
"###
        }
        .render_md(cx)
    }
}
"####
        } else {
            r####"
use proa_core::{MdRenderSync, WriteBuf};
use proa_macros::md_sync;

impl<L: proa_core::DataLoader, B: WriteBuf> MdRenderSync<L, B> for __STRUCT__ {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r###"__STRUCT__"### }.render_md(cx)
    }
}
"####
        }
    } else {
        ""
    };

    let test_source = if test {
        match (children, asynchronous) {
            (true, false) => {
                r#"
#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::{text::text, WebContext, WebRenderSync};

    #[test]
    fn renders_dynamic_children_and_the_component_contract() {
        let mut cx = WebContext::new();
        __STRUCT__::new(text("dynamic child")).render(&mut cx).unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains("__DATA__"));
        assert!(html.contains("dynamic child"));
    }
}
"#
            }
            (false, false) => {
                r#"
#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::{WebContext, WebRenderSync};

    #[test]
    fn renders_the_component_contract() {
        let mut cx = WebContext::new();
        __STRUCT__::new().render(&mut cx).unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains("__DATA__"));
    }
}
"#
            }
            (true, true) => {
                r#"
#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::{text::text, WebContext, WebRender};

    #[tokio::test]
    async fn renders_dynamic_children_and_the_component_contract() {
        let mut cx = WebContext::new();
        __STRUCT__::new(proa_core::to_async(text("dynamic child")))
            .render(&mut cx)
            .resolve()
            .await
            .unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains("__DATA__"));
        assert!(html.contains("dynamic child"));
    }
}
"#
            }
            (false, true) => {
                r#"
#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::{WebContext, WebRender};

    #[tokio::test]
    async fn renders_the_component_contract() {
        let mut cx = WebContext::new();
        __STRUCT__::new()
            .render(&mut cx)
            .resolve()
            .await
            .unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains("__DATA__"));
    }
}
"#
            }
        }
    } else {
        ""
    };

    fill(
        format!("{template}{markdown_source}{test_source}"),
        &[
            ("__BASE__", base),
            ("__STRUCT__", struct_name),
            ("__DATA__", data_attr),
        ],
    )
}

fn component_recipe_source(struct_name: &str) -> String {
    fill(
        r#"pub const BASE_CLASS: &str = "rounded-lg border p-4";

pub struct __STRUCT__Recipe {
    pub class: &'static str,
}

impl Default for __STRUCT__Recipe {
    fn default() -> Self {
        Self { class: BASE_CLASS }
    }
}
"#
        .to_string(),
        &[("__STRUCT__", struct_name)],
    )
}

fn section_layout_source(struct_name: &str) -> String {
    fill(
        r#"use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

pub struct __STRUCT__<C> {
    pub content: C,
}

impl<C> __STRUCT__<C> {
    pub const fn new(content: C) -> Self {
        Self { content }
    }
}

impl<L, C> WebRenderSync<L> for __STRUCT__<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <section class="mx-auto max-w-5xl px-6 py-12">
                {self.content}
            </section>
        }
        .render(cx)
    }
}
"#
        .to_string(),
        &[("__STRUCT__", struct_name)],
    )
}

fn document_layout_source(struct_name: &str) -> String {
    fill(
        r#"// proa:document-root
use proa_core::{raw_text_ok, DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

pub struct __STRUCT__<C> {
    pub title: &'static str,
    pub description: &'static str,
    pub content: C,
}

impl<L, C> WebRenderSync<L> for __STRUCT__<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <title>{raw_text_ok(self.title)}</title>
                    <meta name="description" content={self.description} />
                </head>
                <body>{self.content}</body>
            </html>
        }
        .render(cx)
    }
}
"#
        .to_string(),
        &[("__STRUCT__", struct_name)],
    )
}

fn data_source(name: &str) -> String {
    if name == "routes" {
        "pub const ROUTES: &[&str] = &[\"/\"];\n".to_string()
    } else if name == "nav" {
        nav_data()
    } else {
        format!(
            "pub struct {}Item {{\n    pub label: &'static str,\n}}\n\npub const ITEMS: &[{}Item] = &[];\n",
            pascal_case(name),
            pascal_case(name)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_site_dry_run_writes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("demo");
        new_site(NewSiteOptions {
            name: "demo".to_string(),
            root: root.clone(),
            template: "basic".to_string(),
            empty: false,
            server: ServerIntegration::Axum,
            tailwind: true,
            docs: DocsIntegration::None,
            blog: false,
            deploy: DeployTarget::None,
            skip_install: true,
            no_git: true,
            agents_md: true,
            dry_run: Some(DryRunFormat::Text),
            force: false,
            yes: true,
        })
        .unwrap();

        assert!(!root.exists());
    }

    #[test]
    fn new_site_writes_tailwind_sources_under_styles() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("styles-demo");
        new_site(NewSiteOptions {
            name: "styles-demo".to_string(),
            root: root.clone(),
            template: "basic".to_string(),
            empty: false,
            server: ServerIntegration::Axum,
            tailwind: true,
            docs: DocsIntegration::None,
            blog: false,
            deploy: DeployTarget::None,
            skip_install: true,
            no_git: true,
            agents_md: true,
            dry_run: None,
            force: false,
            yes: true,
        })
        .unwrap();

        assert!(root.join("styles/input.css").exists());
        assert!(!root.join("styles/tailwind.config.cjs").exists());
        assert!(root.join("public/styles.css").exists());
        assert!(!root.join("frontend").exists());

        // Layer order is load-bearing and fails silently: get it wrong and the
        // site still builds, but installed components render without their
        // padding or radius. Assert the declaration exists and, critically,
        // that it comes before the Tailwind import.
        let input = fs::read_to_string(root.join("styles/input.css")).unwrap();
        let layers = input
            .find("@layer theme, base, proa-compat, components, utilities;")
            .expect("input.css must declare the cascade layer order");
        let tailwind = input
            .find("@import \"tailwindcss\";")
            .expect("input.css must import Tailwind");
        assert!(
            layers < tailwind,
            "the layer declaration must precede the Tailwind import, or Tailwind's own \
             declaration wins and `proa-compat` is appended last"
        );
    }

    #[test]
    fn apply_plan_rolls_back_files_on_mid_plan_failure() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let mut plan = ScaffoldPlan::new(root.clone());
        plan.write_file(root.join("ok.txt"), "hello".to_string());
        // Writing over a directory fails, aborting the plan midway.
        let blocked = root.join("blocked");
        fs::create_dir_all(&blocked).unwrap();
        plan.write_file(blocked.clone(), "nope".to_string());

        apply_plan(&plan, true).unwrap_err();
        assert!(
            !root.join("ok.txt").exists(),
            "files from the failed plan should be rolled back"
        );
    }

    #[test]
    fn apply_plan_rollback_restores_overwritten_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs::write(root.join("keep.txt"), "original").unwrap();
        let mut plan = ScaffoldPlan::new(root.clone());
        plan.write_file(root.join("keep.txt"), "replaced".to_string());
        let blocked = root.join("blocked");
        fs::create_dir_all(&blocked).unwrap();
        plan.write_file(blocked.clone(), "nope".to_string());

        apply_plan(&plan, true).unwrap_err();
        assert_eq!(
            fs::read_to_string(root.join("keep.txt")).unwrap(),
            "original",
            "overwritten files should be restored on rollback"
        );
    }

    #[test]
    fn generate_component_updates_mod_file() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("src/components")).unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[package]\nname=\"demo\"\n").unwrap();
        fs::write(temp.path().join("src/components/mod.rs"), "").unwrap();

        generate_component(GenerateComponentOptions {
            name: "marketing-card".to_string(),
            root: Some(temp.path().to_path_buf()),
            folder: false,
            recipe: false,
            children: false,
            asynchronous: false,
            markdown: false,
            no_test: true,
            dry_run: None,
            force: false,
        })
        .unwrap();

        let module = fs::read_to_string(temp.path().join("src/components/mod.rs")).unwrap();
        assert!(module.contains("pub mod marketing_card;"));
        assert!(temp
            .path()
            .join("src/components/marketing_card.rs")
            .exists());

        let source =
            fs::read_to_string(temp.path().join("src/components/marketing_card.rs")).unwrap();
        assert!(source.contains(
            "use proa_core::{ClassList, DataLoader, WebContext, WebRenderSync, WriteError};"
        ));
        assert!(source.contains("impl<L: DataLoader> WebRenderSync<L> for MarketingCard"));
        assert!(source.contains("fn render(self, cx: &mut WebContext<L>)"));
        syn::parse_file(&source).expect("generated component must be valid Rust syntax");
    }

    #[test]
    fn generated_component_with_children_uses_default_writer() {
        let source = component_source(
            "MarketingCard",
            "data-marketing-card",
            true,
            false,
            false,
            false,
            true,
        );

        assert!(source.contains("impl<L, C> WebRenderSync<L> for MarketingCard<C>"));
        assert!(source.contains("C: WebRenderSync<L>"));
        assert!(source.contains("WebContext::new()"));
        assert!(source.contains("cx.into_output().into_string()"));
        syn::parse_file(&source).expect("generated child component must be valid Rust syntax");
    }

    #[test]
    fn generated_layouts_use_default_writer() {
        for source in [
            section_layout_source("MarketingLayout"),
            document_layout_source("DocumentLayout"),
        ] {
            assert!(source.contains("impl<L, C> WebRenderSync<L>"));
            assert!(source.contains("C: WebRenderSync<L>"));
            assert!(source.contains("DataLoader"));
            assert!(!source.contains("WriteBuf"));
            syn::parse_file(&source).expect("generated layout must be valid Rust syntax");
        }
    }

    #[test]
    fn new_site_writes_project_config_and_lock() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("config-demo");
        new_site(NewSiteOptions {
            name: "config-demo".to_string(),
            root: root.clone(),
            template: "basic".to_string(),
            empty: false,
            server: ServerIntegration::Axum,
            tailwind: false,
            docs: DocsIntegration::None,
            blog: false,
            deploy: DeployTarget::None,
            skip_install: true,
            no_git: true,
            agents_md: true,
            dry_run: None,
            force: false,
            yes: true,
        })
        .unwrap();

        assert!(root.join(crate::config::CONFIG_FILE).exists());
        assert!(root.join(crate::config::LOCK_FILE).exists());
    }

    #[test]
    fn new_site_writes_hardened_dev_scripts() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("watch-demo");
        new_site(NewSiteOptions {
            name: "watch-demo".to_string(),
            root: root.clone(),
            template: "basic".to_string(),
            empty: false,
            server: ServerIntegration::Axum,
            tailwind: false,
            docs: DocsIntegration::None,
            blog: false,
            deploy: DeployTarget::None,
            skip_install: true,
            no_git: true,
            agents_md: true,
            dry_run: None,
            force: false,
            yes: true,
        })
        .unwrap();

        let dev = fs::read_to_string(root.join("scripts/dev.sh")).unwrap();
        assert_eq!(dev, dev_script());
        assert!(dev.contains("$ROOT/target/watch/dev"));
        assert!(dev.contains("proa_watch_lock_acquire \"dev\" \"$ROOT\""));
        assert!(dev.contains(
            "proa_watch_run proa dev --root \"$ROOT\" ${dev_args[@]+\"${dev_args[@]}\"} \"$@\""
        ));

        // Live-reload settings are shell variables at the top of the script,
        // so they can be edited in place or exported, and each one maps to the
        // `proa dev` flag that implements it.
        for setting in [
            "PROA_DEV_RELOAD=\"${PROA_DEV_RELOAD:-on}\"",
            "PROA_DEV_RELOAD_PORT=\"${PROA_DEV_RELOAD_PORT:-}\"",
            "PROA_DEV_CSS_HOT_SWAP=\"${PROA_DEV_CSS_HOT_SWAP:-on}\"",
            "PROA_DEV_DEBOUNCE_MS=\"${PROA_DEV_DEBOUNCE_MS:-300}\"",
            "dev_args+=(--no-reload)",
            "dev_args+=(--no-css-hot-swap)",
            "dev_args+=(--reload-port \"$PROA_DEV_RELOAD_PORT\")",
            "dev_args+=(--debounce-ms \"$PROA_DEV_DEBOUNCE_MS\")",
            // Turning a setting off must work for the spellings people
            // actually type, not only the literal `0`.
            "0 | false | no | off) return 0 ;;",
        ] {
            assert!(dev.contains(setting), "dev.sh is missing `{setting}`");
        }

        let watch_lock = fs::read_to_string(root.join("scripts/watch-lock.sh")).unwrap();
        assert_eq!(watch_lock, watch_lock_script());
        assert!(watch_lock.contains("if mkdir \"$lock_dir\""));
        assert!(watch_lock.contains("kill -KILL -- \"-$child_pgid\""));

        let build = fs::read_to_string(root.join("scripts/build.sh")).unwrap();
        assert_eq!(build, build_script());
        assert!(build.contains("proa build --root \"$ROOT\" \"$@\""));

        let readme = fs::read_to_string(root.join("README.md")).unwrap();
        assert!(readme.contains("bash scripts/dev.sh"));
        assert!(readme.contains("bash scripts/build.sh"));
    }

    #[test]
    fn new_site_generates_production_server_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("prod-demo");
        new_site(NewSiteOptions {
            name: "prod-demo".to_string(),
            root: root.clone(),
            template: "basic".to_string(),
            empty: false,
            server: ServerIntegration::Axum,
            tailwind: false,
            docs: DocsIntegration::None,
            blog: false,
            deploy: DeployTarget::None,
            skip_install: true,
            no_git: true,
            agents_md: true,
            dry_run: None,
            force: false,
            yes: true,
        })
        .unwrap();

        let main = fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(main.contains("const SHUTDOWN_DRAIN_TIMEOUT"));
        assert!(main.contains("proa_framework_axum::serve(listener, app, SHUTDOWN_DRAIN_TIMEOUT)"));
        assert!(!main.contains("async fn shutdown_signal"));
        assert!(main.contains("/robots.txt"));
        assert!(main.contains("X_CONTENT_TYPE_OPTIONS"));
        assert!(main.contains("X_FRAME_OPTIONS"));
        assert!(main.contains("REFERRER_POLICY"));
        assert!(main.contains("TraceLayer::new_for_http()"));

        let layout = fs::read_to_string(root.join("src/layouts/root.rs")).unwrap();
        // Written into the macro, not through StaticRaw: `html/staticraw-doctype`
        // rejects the wrapped form, so scaffolding it made every fresh project
        // fail the `proa lint --deny warnings` and `proa build` we tell people to run.
        assert!(layout.contains("<!DOCTYPE html>"));
        assert!(!layout.contains("StaticRaw"));
        assert!(!layout.contains("UnsafeRaw"));
        assert!(layout.contains("NAV_ITEMS"));
        assert!(layout.contains("rel=\"icon\""));
        assert!(layout.contains("impl<L, C> WebRenderSync<L> for RootDocument<C>"));
        assert!(layout.contains("C: WebRenderSync<L>"));
        assert!(layout.contains("impl<L: DataLoader> WebRenderSync<L> for SiteNav"));
        syn::parse_file(&layout).expect("generated root layout must be valid Rust syntax");

        let home = fs::read_to_string(root.join("src/pages/home.rs")).unwrap();
        assert!(home.contains("fn home_page_renders"));
        assert!(home.contains("impl<L: DataLoader> WebRenderSync<L> for HomePage"));
        assert!(home.contains("render_to_string(RootDocument"));
        assert!(!home.contains("WebContext::with_buffer(&mut out)"));
        syn::parse_file(&home).expect("generated home page must be valid Rust syntax");

        let agent_instructions = fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert!(agent_instructions.contains("WebRenderSync<L>"));
        assert!(agent_instructions.contains("C: WebRenderSync<L>"));
        assert!(agent_instructions.contains("owns the default `RenderBuffer`"));
        assert!(agent_instructions.contains("#[rsjs(component)]"));
        assert!(agent_instructions.contains("#[rsjs(component, client)]"));
        assert!(agent_instructions.contains("Do not use inherent render shorthand"));
        assert!(agent_instructions.contains("component `clients(...)` allowlists"));
        assert!(agent_instructions.contains("direct inbound read such as `self.label`"));
        assert!(agent_instructions.contains("`signal(self.value)` initializer samples"));

        let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("proa_core = \"0.3.0\""));
        assert!(cargo.contains("proa_macros = \"0.3.0\""));
        assert!(cargo.contains("proa_framework_axum = \"0.3.0\""));
        assert!(!cargo.contains("registry = \"proa\""));
        assert!(!cargo.contains("proa_core = { path ="));
        let cargo_config = fs::read_to_string(root.join(".cargo/config.toml")).unwrap();
        assert!(cargo_config.contains("git-fetch-with-cli = true"));
        assert!(cargo_config
            .parse::<toml::Value>()
            .unwrap()
            .get("registries")
            .is_none());
        assert!(!cargo.contains("proa_macros = { path ="));
        assert!(!cargo.contains("proa_framework_axum = { path ="));
        assert!(!cargo.contains("\"signal\""));
        assert!(cargo.contains("\"trace\""));
        assert!(cargo.contains("[profile.release]"));
    }

    #[test]
    fn new_site_docs_option_creates_docs_route() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("docs-demo");
        new_site(NewSiteOptions {
            name: "docs-demo".to_string(),
            root: root.clone(),
            template: "basic".to_string(),
            empty: false,
            server: ServerIntegration::Axum,
            tailwind: false,
            docs: DocsIntegration::ProaDocs,
            blog: false,
            deploy: DeployTarget::None,
            skip_install: true,
            no_git: true,
            agents_md: true,
            dry_run: None,
            force: false,
            yes: true,
        })
        .unwrap();

        assert!(root.join("src/docs/index.mdx").exists());
        assert!(root.join("src/pages/docs.rs").exists());
        let main = fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(main.contains("/docs/*path"));

        // Docs and fonts share one build.rs: the font collector first, then
        // the docs compilation, with [build-dependencies] emitted only once.
        let build = fs::read_to_string(root.join("build.rs")).unwrap();
        let fonts = build
            .find("proa_fonts_build::collect::run")
            .expect("build.rs must run the font collector");
        let docs = build
            .find("proa_docs_build::compile")
            .expect("build.rs must compile docs");
        assert!(
            fonts < docs,
            "the fonts section must precede docs compilation"
        );
        syn::parse_file(&build).expect("generated build.rs must be valid Rust syntax");

        let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert_eq!(cargo.matches("[build-dependencies]").count(), 1);
        assert!(cargo.contains("proa_fonts_build = \"0.1.0\""));
        assert!(cargo.contains("proa_docs_build = \"0.2.0\""));
    }

    #[test]
    fn new_site_wires_font_collector() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("fonts-demo");
        new_site(NewSiteOptions {
            name: "fonts-demo".to_string(),
            root: root.clone(),
            template: "basic".to_string(),
            empty: false,
            server: ServerIntegration::Axum,
            tailwind: false,
            docs: DocsIntegration::None,
            blog: false,
            deploy: DeployTarget::None,
            skip_install: true,
            no_git: true,
            agents_md: false,
            dry_run: None,
            force: false,
            yes: true,
        })
        .unwrap();

        let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("proa_fonts = \"0.1.0\""));
        assert!(cargo.contains("proa_fonts_macros = \"0.1.0\""));
        assert!(cargo.contains("[build-dependencies]\nproa_fonts_build = \"0.1.0\""));
        assert!(!cargo.contains("proa_docs_build"));

        let build = fs::read_to_string(root.join("build.rs")).unwrap();
        assert!(build.contains("proa_fonts_build::collect::run("));
        assert!(build.contains("CollectOptions::from_build_env()"));
        assert!(build.contains("panic!(\"proa fonts: {error}\")"));
        assert!(build.contains("cargo:rerun-if-changed=src"));
        assert!(build.contains("cargo:rerun-if-changed=proa-fonts.lock"));
        assert!(!build.contains("proa_docs_build"));
        syn::parse_file(&build).expect("generated build.rs must be valid Rust syntax");

        let main = fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(main.contains(
            "mod __proa_fonts_generated { include!(concat!(env!(\"OUT_DIR\"), \"/proa_fonts.rs\")); }"
        ));
        assert!(
            main.contains("// Generated font consts for google_font!/local_font! declarations.")
        );
        // The generated CSS references /fonts, so the router must serve the
        // vendored files there, not only under /static.
        let fonts_route = main
            .find(".nest_service(\"/fonts\", static_assets::fonts_service())")
            .expect("main.rs must mount the fonts service at /fonts");
        let static_route = main
            .find(".nest_service(\"/static\"")
            .expect("main.rs must mount the static service");
        assert!(fonts_route < static_route);
        syn::parse_file(&main).expect("generated main.rs must be valid Rust syntax");

        let assets = fs::read_to_string(root.join("src/static_assets.rs")).unwrap();
        assert!(assets.contains("pub fn fonts_service() -> Router"));
        assert!(assets.contains("static_root().join(\"fonts\")"));
        assert!(assets.contains("public, max-age=31536000, immutable"));
        syn::parse_file(&assets).expect("generated static_assets.rs must be valid Rust syntax");
    }

    #[test]
    fn new_site_empty_skips_font_wiring() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("empty-demo");
        new_site(NewSiteOptions {
            name: "empty-demo".to_string(),
            root: root.clone(),
            template: "basic".to_string(),
            empty: true,
            server: ServerIntegration::Axum,
            tailwind: false,
            docs: DocsIntegration::None,
            blog: false,
            deploy: DeployTarget::None,
            skip_install: true,
            no_git: true,
            agents_md: false,
            dry_run: None,
            force: false,
            yes: true,
        })
        .unwrap();

        let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(!cargo.contains("proa_fonts"));
        assert!(!cargo.contains("[build-dependencies]"));
        assert!(!root.join("build.rs").exists());

        let main = fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(!main.contains("__proa_fonts_generated"));
        assert!(!main.contains("fonts_service"));

        let assets = fs::read_to_string(root.join("src/static_assets.rs")).unwrap();
        assert!(!assets.contains("fonts_service"));
        syn::parse_file(&assets).expect("generated static_assets.rs must be valid Rust syntax");
    }

    #[test]
    fn new_site_server_none_skips_font_wiring() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("headless-demo");
        new_site(NewSiteOptions {
            name: "headless-demo".to_string(),
            root: root.clone(),
            template: "basic".to_string(),
            empty: false,
            server: ServerIntegration::None,
            tailwind: false,
            docs: DocsIntegration::ProaDocs,
            blog: false,
            deploy: DeployTarget::None,
            skip_install: true,
            no_git: true,
            agents_md: false,
            dry_run: None,
            force: false,
            yes: true,
        })
        .unwrap();

        // Without a generated crate root there is nowhere to include the
        // build-generated module, so no font wiring is emitted.
        let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(!cargo.contains("proa_fonts"));
        assert_eq!(cargo.matches("[build-dependencies]").count(), 1);
        assert!(cargo.contains("proa_docs_build = \"0.2.0\""));

        let build = fs::read_to_string(root.join("build.rs")).unwrap();
        assert!(!build.contains("proa_fonts_build"));
        assert!(build.contains("proa_docs_build::compile(config);"));
        syn::parse_file(&build).expect("generated build.rs must be valid Rust syntax");

        assert!(!root.join("src/main.rs").exists());
    }

    #[test]
    fn new_site_coolify_deploy_preset_writes_deploy_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("deploy-demo");
        new_site(NewSiteOptions {
            name: "deploy-demo".to_string(),
            root: root.clone(),
            template: "basic".to_string(),
            empty: false,
            server: ServerIntegration::Axum,
            tailwind: false,
            docs: DocsIntegration::None,
            blog: false,
            deploy: DeployTarget::Coolify,
            skip_install: true,
            no_git: true,
            agents_md: true,
            dry_run: None,
            force: false,
            yes: true,
        })
        .unwrap();

        assert!(root.join("Dockerfile").exists());
        assert!(root.join(".dockerignore").exists());
        assert!(root.join("coolify.env.example").exists());
        assert!(root.join("COOLIFY.md").exists());
        let dockerfile = fs::read_to_string(root.join("Dockerfile")).unwrap();
        assert!(!dockerfile.contains("CARGO_REGISTRIES_PROA_TOKEN"));
        assert!(!dockerfile.contains("GIT_CONFIG"));
        assert!(!dockerfile.contains("github_token"));
        assert!(dockerfile.contains("cargo build --release --bin deploy-demo"));
        assert!(dockerfile.contains("EXPOSE 3000"));
        let main = fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(main.contains("/healthz"));
    }
}
