// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use base16ct::HexDisplay;
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

pub struct GenerateComponentOptions {
    pub name: String,
    pub root: Option<PathBuf>,
    pub folder: bool,
    pub recipe: bool,
    pub children: bool,
    pub asynchronous: bool,
    pub markdown: bool,
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
                .map(|bytes| format!("sha256:{:x}", HexDisplay(&Sha256::digest(bytes))));
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

fn fill(mut template: String, replacements: &[(&str, &str)]) -> String {
    for (needle, value) in replacements {
        template = template.replace(needle, value);
    }
    template
}

fn nav_data() -> String {
    "pub struct NavItem {\n    pub label: &'static str,\n    pub href: &'static str,\n}\n\npub const NAV_ITEMS: &[NavItem] = &[\n    NavItem { label: \"Home\", href: \"/\" },\n];\n".to_string()
}

// proa-perf-ok(scope): trusted scaffold emitter assembles Rust source, not response HTML

fn component_source(
    struct_name: &str,
    data_attr: &str,
    children: bool,
    asynchronous: bool,
    markdown: bool,
    recipe: bool,
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

    fill(
        format!("{template}{markdown_source}"),
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
        );

        assert!(source.contains("impl<L, C> WebRenderSync<L> for MarketingCard<C>"));
        assert!(source.contains("C: WebRenderSync<L>"));
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
}
