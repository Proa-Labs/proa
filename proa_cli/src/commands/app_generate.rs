// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Application-aware generators that own route and migration conventions.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::{config, util::resolve_config_root};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateReport {
    operation_id: &'static str,
    command_version: u16,
    project_root: PathBuf,
    planned_filesystem_operations: Vec<GenerateOperation>,
    configuration_changes: Vec<String>,
    warnings: Vec<String>,
    result: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateOperation {
    operation: &'static str,
    path: PathBuf,
    bytes: usize,
}

pub fn migration(
    raw_name: String,
    root: Option<PathBuf>,
    simple: bool,
    open: bool,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let root = resolve_config_root(root)?;
    require_sqlite(&root)?;
    let migrations = root.join("migrations");
    fs::create_dir_all(&migrations)?;
    let _lock = DirectoryLock::acquire(&migrations)?;
    let name = snake_name(&raw_name)?;
    let version = next_migration_version(&migrations)?;
    let base = format!("{version:04}_{name}");
    let files = if simple {
        vec![(migrations.join(format!("{base}.sql")), simple_sql(&name))]
    } else {
        vec![
            (migrations.join(format!("{base}.up.sql")), up_sql(&name)),
            (migrations.join(format!("{base}.down.sql")), down_sql(&name)),
        ]
    };
    for (path, _) in &files {
        if path.exists() {
            bail!("migration already exists: {}", path.display());
        }
    }
    let report = GenerateReport {
        operation_id: "generate.migration",
        command_version: 1,
        project_root: root.clone(),
        planned_filesystem_operations: files
            .iter()
            .map(|(path, source)| GenerateOperation {
                operation: "create",
                path: path.strip_prefix(&root).unwrap_or(path).to_path_buf(),
                bytes: source.len(),
            })
            .collect(),
        configuration_changes: Vec::new(),
        warnings: Vec::new(),
        result: if dry_run { "planned" } else { "created" },
    };
    if dry_run {
        print_report(&report, json)?;
        return Ok(());
    }
    commit_new_files(&files)?;
    print_report(&report, json)?;
    if open {
        open_files(files.iter().map(|(path, _)| path))?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn page(
    raw_path: String,
    root: Option<PathBuf>,
    _layout: Option<String>,
    force_markdown: bool,
    no_markdown: bool,
    no_test: bool,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let root = resolve_config_root(root)?;
    let route = route_name(&raw_path)?;
    let project = config::load_project_config(&root)?;
    let configured_markdown = project
        .representations
        .as_ref()
        .and_then(|value| value.pointer("/markdown/enabled"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let markdown = force_markdown || (!no_markdown && configured_markdown);
    if force_markdown && !configured_markdown {
        bail!("Markdown is not installed for this project; run `proa add representation markdown` first");
    }
    let module_path = root.join("src/pages").join(format!("{}.rs", route.module));
    if module_path.exists() {
        bail!("page already exists: {}", module_path.display());
    }
    let mut modules = fs::read_to_string(root.join("src/pages/mod.rs"))?;
    add_module(&mut modules, &route.module);
    let routes_path = root.join("src/routes.rs");
    let mut routes = fs::read_to_string(&routes_path)?;
    add_route_spec(
        &mut routes,
        &format!(
            "        Route::endpoint(\"{}\", get(pages::{}::handler)),",
            route.canonical, route.module
        ),
    )?;
    if markdown {
        add_explicit_route(
            &mut routes,
            &format!(
                "        .route(\"{}\", get(pages::{}::markdown))",
                route.markdown_alias, route.module
            ),
        )?;
    }
    let mut files = vec![
        (
            PathBuf::from(format!("src/pages/{}.rs", route.module)),
            page_source(&route, markdown),
        ),
        (PathBuf::from("src/pages/mod.rs"), modules),
        (PathBuf::from("src/routes.rs"), routes),
    ];
    if !no_test {
        files.push((
            PathBuf::from(format!("tests/pages/{}.rs", route.module)),
            format!(
                "#[test]\nfn route_contract() {{\n    let source = include_str!(\"../../src/pages/{}.rs\");\n    assert!(source.contains(\"data-page-{}\"));\n}}\n",
                route.module, route.module
            ),
        ));
    }
    finish_generated(&root, "generate.page", files, dry_run, json)
}

#[allow(clippy::too_many_arguments)]
pub fn endpoint(
    raw_path: String,
    root: Option<PathBuf>,
    method: &'static str,
    response: &'static str,
    no_test: bool,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let root = resolve_config_root(root)?;
    let route = route_name(&raw_path)?;
    let module_path = root
        .join("src/endpoints")
        .join(format!("{}.rs", route.module));
    if module_path.exists() {
        bail!("endpoint already exists: {}", module_path.display());
    }
    let mut modules = fs::read_to_string(root.join("src/endpoints/mod.rs"))?;
    add_module(&mut modules, &route.module);
    let routes_path = root.join("src/routes.rs");
    let mut routes = fs::read_to_string(&routes_path)?;
    let registration = format!(
        "        Route::endpoint(\"{}\", axum::routing::{}(endpoints::{}::handler)),",
        route.canonical, method, route.module
    );
    if routes.contains(&format!("\"{}\"", route.canonical)) {
        bail!("route path is already registered: {}", route.canonical);
    }
    add_route_spec(&mut routes, &registration)?;
    let mut files = vec![
        (
            PathBuf::from(format!("src/endpoints/{}.rs", route.module)),
            endpoint_source(response),
        ),
        (PathBuf::from("src/endpoints/mod.rs"), modules),
        (PathBuf::from("src/routes.rs"), routes),
    ];
    if !no_test {
        files.push((
            PathBuf::from(format!("tests/endpoints/{}.rs", route.module)),
            format!(
                "#[test]\nfn endpoint_contract() {{\n    let source = include_str!(\"../../src/endpoints/{}.rs\");\n    assert!(source.contains(\"pub async fn handler\"));\n}}\n",
                route.module
            ),
        ));
    }
    finish_generated(&root, "generate.endpoint", files, dry_run, json)
}

pub fn model(
    raw_name: String,
    raw_fields: Vec<String>,
    root: Option<PathBuf>,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let root = resolve_config_root(root)?;
    require_sqlite(&root)?;
    let name = snake_name(&raw_name)?;
    let struct_name = rust_type_name(&name);
    let fields = raw_fields
        .iter()
        .map(|field| parse_model_field(field))
        .collect::<Result<Vec<_>>>()?;
    let model_path = root.join("src/models").join(format!("{name}.rs"));
    if model_path.exists() {
        bail!("model already exists: {}", model_path.display());
    }
    let migrations = root.join("migrations");
    fs::create_dir_all(&migrations)?;
    let _lock = DirectoryLock::acquire(&migrations)?;
    let version = next_migration_version(&migrations)?;
    let table = format!("{name}s");
    let mut modules = fs::read_to_string(root.join("src/models/mod.rs")).unwrap_or_default();
    add_module(&mut modules, &name);
    let mut files = vec![
        (
            PathBuf::from(format!("src/models/{name}.rs")),
            model_source(&struct_name, &fields),
        ),
        (PathBuf::from("src/models/mod.rs"), modules),
        (
            PathBuf::from(format!("migrations/{version:04}_create_{table}.up.sql")),
            model_up_sql(&table, &fields),
        ),
        (
            PathBuf::from(format!("migrations/{version:04}_create_{table}.down.sql")),
            format!("DROP TABLE {table};\n"),
        ),
    ];
    files.push((
        PathBuf::from(format!("tests/models/{name}.rs")),
        format!(
            "#[test]\nfn model_is_an_ordinary_sqlx_row() {{\n    let source = include_str!(\"../../src/models/{name}.rs\");\n    assert!(source.contains(\"sqlx::FromRow\"));\n}}\n"
        ),
    ));
    finish_generated(&root, "generate.model", files, dry_run, json)
}

#[derive(Debug)]
struct RouteName {
    canonical: String,
    markdown_alias: String,
    module: String,
    rust_type: String,
}

fn route_name(raw: &str) -> Result<RouteName> {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty()
        || trimmed
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("route path must contain safe, nonempty segments");
    }
    let mut module_parts = Vec::new();
    for part in trimmed.split('/') {
        let part = part.trim_start_matches(':');
        let normalized = snake_name(part)?;
        module_parts.push(normalized);
    }
    let module = module_parts.join("_");
    let canonical = format!("/{trimmed}");
    Ok(RouteName {
        markdown_alias: format!("{canonical}.md"),
        rust_type: format!("{}Page", rust_type_name(&module)),
        canonical,
        module,
    })
}

fn rust_type_name(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .map(|first| format!("{}{}", first.to_ascii_uppercase(), characters.as_str()))
                .unwrap_or_default()
        })
        .collect()
}

fn add_module(source: &mut String, module: &str) {
    let declaration = format!("pub mod {module};");
    if !source.lines().any(|line| line.trim() == declaration) {
        if !source.ends_with('\n') {
            source.push('\n');
        }
        source.push_str(&declaration);
        source.push('\n');
    }
}

fn add_route_spec(source: &mut String, line: &str) -> Result<()> {
    insert_before_marker(source, "        // proa:route-specs:end", line)
}

fn add_explicit_route(source: &mut String, line: &str) -> Result<()> {
    insert_before_marker(source, "        // proa:explicit-routes:end", line)
}

fn insert_before_marker(source: &mut String, marker: &str, line: &str) -> Result<()> {
    if source.contains(line) {
        return Ok(());
    }
    let offset = source.find(marker).with_context(|| {
        format!("generated ownership marker `{marker}` is missing; no arbitrary Rust was changed")
    })?;
    source.insert_str(offset, &format!("{line}\n"));
    Ok(())
}

fn page_source(route: &RouteName, markdown: bool) -> String {
    let imports = if markdown {
        "use axum::{http::HeaderMap, response::IntoResponse};\nuse proa_core::{md::MdContext, DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};\nuse proa_framework_axum::{explicit_markdown, negotiate, MarkdownAlternate, RouteResponse};\nuse proa_macros::{html_sync, md_sync};"
    } else {
        "use axum::response::IntoResponse;\nuse proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};\nuse proa_framework_axum::RouteResponse;\nuse proa_macros::html_sync;"
    };
    let markdown_impl = if markdown {
        format!(
            r####"
impl<B: WriteBuf> MdRenderSync<B> for {ty} {{
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {{
        md_sync! {{ r###"
            # {ty}

            Canonical route: `{path}`
            "###
        }}.render_md(cx)
    }}
}}

pub async fn handler(headers: HeaderMap) -> Result<impl IntoResponse, AppError> {{
    let html = RouteResponse::ssr_async(RootDocument {{ title: "{ty}", content: proa_core::to_async({ty}) }});
    Ok(negotiate(&headers, html, {ty}, MarkdownAlternate::new("{alias}"))?)
}}

pub async fn markdown() -> Result<impl IntoResponse, AppError> {{
    Ok(explicit_markdown({ty})?)
}}
"####,
            ty = route.rust_type,
            path = route.canonical,
            alias = route.markdown_alias
        )
    } else {
        format!(
            "\npub async fn handler() -> Result<impl IntoResponse, AppError> {{\n    Ok(RouteResponse::ssr_async(RootDocument {{ title: \"{ty}\", content: proa_core::to_async({ty}) }}))\n}}\n",
            ty = route.rust_type
        )
    };
    format!(
        r#"{imports}

use crate::{{error::AppError, layouts::RootDocument}};

#[derive(Debug, Clone, Copy)]
pub struct {ty};

impl<L: DataLoader> WebRenderSync<L> for {ty} {{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {{
        html_sync! {{ <main data-page-{module}><h1>"{ty}"</h1></main> }}.render(cx)
    }}
}}
{markdown_impl}
"#,
        imports = imports,
        ty = route.rust_type,
        module = route.module,
        markdown_impl = markdown_impl
    )
}

fn endpoint_source(response: &str) -> String {
    match response {
        "json" => "use axum::Json;\nuse serde::Serialize;\n\n#[derive(Serialize)]\npub struct Response { pub ok: bool }\n\npub async fn handler() -> Json<Response> { Json(Response { ok: true }) }\n".to_string(),
        "redirect" => "use axum::response::Redirect;\n\npub async fn handler() -> Redirect { Redirect::to(\"/\") }\n".to_string(),
        "empty" => "use axum::http::StatusCode;\n\npub async fn handler() -> StatusCode { StatusCode::NO_CONTENT }\n".to_string(),
        "html" => "use axum::response::Html;\n\npub async fn handler() -> Html<&'static str> { Html(\"<section data-endpoint-fragment></section>\") }\n".to_string(),
        _ => unreachable!("clap validates endpoint response values"),
    }
}

#[derive(Debug)]
struct ModelField {
    name: String,
    rust_type: &'static str,
    sql_type: &'static str,
    optional: bool,
    unique: bool,
    default: Option<String>,
    reference: Option<String>,
}

fn parse_model_field(raw: &str) -> Result<ModelField> {
    let mut parts = raw.split(':');
    let name = snake_name(parts.next().unwrap_or_default())?;
    let kind = parts
        .next()
        .context("model fields use name:type[:modifier...]")?;
    let (rust_type, sql_type) = match kind {
        "text" | "string" => ("String", "TEXT"),
        "integer" | "bigint" => ("i64", "INTEGER"),
        "real" => ("f64", "REAL"),
        "boolean" => ("bool", "INTEGER"),
        "blob" => ("Vec<u8>", "BLOB"),
        "datetime" => ("String", "TEXT"),
        "uuid" => ("String", "TEXT"),
        other => bail!("unsupported model field type `{other}`"),
    };
    let mut field = ModelField {
        name,
        rust_type,
        sql_type,
        optional: false,
        unique: false,
        default: None,
        reference: None,
    };
    for modifier in parts {
        if modifier == "optional" {
            field.optional = true;
        } else if modifier == "unique" {
            field.unique = true;
        } else if modifier == "index" { /* index generation is reserved in the migration body */
        } else if let Some(value) = modifier.strip_prefix("default=") {
            field.default = Some(value.to_string());
        } else if let Some(value) = modifier.strip_prefix("references=") {
            field.reference = Some(snake_name(value)?);
        } else {
            bail!("unsupported field modifier `{modifier}`");
        }
    }
    Ok(field)
}

fn model_source(struct_name: &str, fields: &[ModelField]) -> String {
    let fields = fields
        .iter()
        .map(|field| {
            format!(
                "    pub {}: {}{},",
                field.name,
                if field.optional { "Option<" } else { "" },
                if field.optional {
                    format!("{}>", field.rust_type)
                } else {
                    field.rust_type.to_string()
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("#[derive(Debug, Clone, sqlx::FromRow)]\npub struct {struct_name} {{\n    pub id: i64,\n{fields}\n    pub created_at: String,\n    pub updated_at: String,\n}}\n")
}

fn model_up_sql(table: &str, fields: &[ModelField]) -> String {
    let mut columns = vec!["    id INTEGER PRIMARY KEY".to_string()];
    for field in fields {
        let mut column = format!("    {} {}", field.name, field.sql_type);
        if !field.optional {
            column.push_str(" NOT NULL");
        }
        if field.unique {
            column.push_str(" UNIQUE");
        }
        if let Some(default) = &field.default {
            column.push_str(" DEFAULT ");
            column.push_str(default);
        }
        if let Some(reference) = &field.reference {
            column.push_str(" REFERENCES ");
            column.push_str(reference);
            column.push_str("s(id)");
        }
        columns.push(column);
    }
    columns.push("    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP".to_string());
    columns.push("    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP".to_string());
    format!("CREATE TABLE {table} (\n{}\n);\n", columns.join(",\n"))
}

fn finish_generated(
    root: &Path,
    operation_id: &'static str,
    files: Vec<(PathBuf, String)>,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let report = GenerateReport {
        operation_id,
        command_version: 1,
        project_root: root.to_path_buf(),
        planned_filesystem_operations: files
            .iter()
            .map(|(path, source)| GenerateOperation {
                operation: if root.join(path).exists() {
                    "update"
                } else {
                    "create"
                },
                path: path.clone(),
                bytes: source.len(),
            })
            .collect(),
        configuration_changes: Vec::new(),
        warnings: Vec::new(),
        result: if dry_run { "planned" } else { "created" },
    };
    if !dry_run {
        commit_mutations(root, &files)?;
    }
    print_report(&report, json)
}

fn commit_mutations(root: &Path, files: &[(PathBuf, String)]) -> Result<()> {
    let mut staged = Vec::with_capacity(files.len());
    for (relative, source) in files {
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let name = target
            .file_name()
            .and_then(|value| value.to_str())
            .context("generated filename must be UTF-8")?;
        let temporary = target.with_file_name(format!(".{name}.proa-{}.tmp", std::process::id()));
        fs::write(&temporary, source)?;
        staged.push((temporary, target));
    }
    for (temporary, target) in staged {
        fs::rename(temporary, target)?;
    }
    Ok(())
}

fn require_sqlite(root: &Path) -> Result<()> {
    let project = config::load_project_config(root)?;
    if project
        .database
        .as_ref()
        .and_then(|database| database.get("driver"))
        .and_then(serde_json::Value::as_str)
        != Some("sqlite")
    {
        bail!("No database capability is configured.\nAdd one with: proa add database sqlite");
    }
    Ok(())
}

fn snake_name(raw: &str) -> Result<String> {
    let mut name = String::with_capacity(raw.len());
    let mut previous_separator = false;
    for character in raw.trim().chars() {
        if character.is_ascii_alphanumeric() {
            if character.is_ascii_uppercase() && !name.is_empty() && !previous_separator {
                name.push('_');
            }
            name.push(character.to_ascii_lowercase());
            previous_separator = false;
        } else if !name.is_empty() && !previous_separator {
            name.push('_');
            previous_separator = true;
        }
    }
    while name.ends_with('_') {
        name.pop();
    }
    if name.is_empty() {
        bail!("migration name must contain a letter or number");
    }
    Ok(name)
}

fn next_migration_version(directory: &Path) -> Result<u32> {
    let mut maximum = 0;
    for entry in fs::read_dir(directory)? {
        let name = entry?.file_name();
        let name = name.to_string_lossy();
        let prefix: String = name.chars().take_while(char::is_ascii_digit).collect();
        if !prefix.is_empty() {
            maximum = maximum.max(prefix.parse::<u32>().unwrap_or(0));
        }
    }
    maximum.checked_add(1).context("migration version overflow")
}

fn up_sql(name: &str) -> String {
    format!(
        "-- {name}\n-- Write the forward schema change here.\n\n-- Example:\n-- CREATE TABLE {name} (id INTEGER PRIMARY KEY);\n"
    )
}

fn down_sql(name: &str) -> String {
    format!(
        "-- Revert {name}\n-- Write the exact inverse operation here; Proa does not guess destructive SQL.\n"
    )
}

fn simple_sql(name: &str) -> String {
    format!("-- {name}\n-- Irreversible migration.\n")
}

fn commit_new_files(files: &[(PathBuf, String)]) -> Result<()> {
    let mut staged = Vec::with_capacity(files.len());
    for (path, source) in files {
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .context("generated path must have a UTF-8 filename")?;
        let temporary =
            path.with_file_name(format!(".{file_name}.proa-{}.tmp", std::process::id()));
        let mut handle = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .with_context(|| format!("stage {}", path.display()))?;
        handle.write_all(source.as_bytes())?;
        handle.sync_all()?;
        staged.push((temporary, path));
    }
    for (temporary, path) in &staged {
        fs::rename(temporary, path).with_context(|| format!("commit {}", path.display()))?;
    }
    Ok(())
}

fn print_report(report: &GenerateReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
    } else {
        println!(
            "{}",
            if report.result == "planned" {
                "Plan"
            } else {
                "Created"
            }
        );
        for operation in &report.planned_filesystem_operations {
            println!("  {:<8} {}", operation.operation, operation.path.display());
        }
    }
    Ok(())
}

fn open_files<'a>(files: impl IntoIterator<Item = &'a PathBuf>) -> Result<()> {
    let editor = std::env::var_os("VISUAL").or_else(|| std::env::var_os("EDITOR"));
    let Some(editor) = editor else {
        bail!("--open requires VISUAL or EDITOR");
    };
    let status = Command::new(editor).args(files).status()?;
    if !status.success() {
        bail!("editor exited with {status}");
    }
    Ok(())
}

struct DirectoryLock {
    path: PathBuf,
}

impl DirectoryLock {
    fn acquire(directory: &Path) -> Result<Self> {
        let path = directory.join(".proa-generate.lock");
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("migration directory is locked: {}", directory.display()))?;
        Ok(Self { path })
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_migration_names() {
        assert_eq!(snake_name("CreatePosts").unwrap(), "create_posts");
        assert_eq!(snake_name("add posts-slug").unwrap(), "add_posts_slug");
    }
}
