// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::SystemTime,
};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use super::ProjectConfig;

const MODEL: &str = include_str!("model.rs");
const MANIFEST: &str = include_str!("runner/Cargo.toml.in");
const LOCKFILE: &str = include_str!("runner/Cargo.lock.in");
const ENTRY: &str = r#"
extern crate self as proa_config;

mod model;
pub use model::*;
pub use serde_json as json;
pub use tokio;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[path = __CONFIG_PATH__]
mod authored;

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let config: Config<_> = runtime.block_on(authored::config())?;
    let output = std::env::args_os().nth(1).expect("missing config output path");
    serde_json::to_writer(std::fs::File::create(output)?, &config)?;
    Ok(())
}
"#;

#[derive(Default)]
pub(crate) struct DevConfigRunner {
    cached: Option<CachedRunner>,
}

type InputStamps = BTreeMap<PathBuf, Option<(SystemTime, u64)>>;

struct CachedRunner {
    executable: PathBuf,
    lock: PathBuf,
    inputs: InputStamps,
}

fn stamp(path: &Path) -> io::Result<Option<(SystemTime, u64)>> {
    match fs::metadata(path) {
        Ok(file) => Ok(Some((file.modified()?, file.len()))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

impl CachedRunner {
    fn capture(
        mut inputs: InputStamps,
        runner: &Path,
        executable: &Path,
        build_started: SystemTime,
    ) -> Option<Self> {
        // Use rustc's dependency list: even an SVG can be an include_bytes!
        // input. If Cargo retained several variants, let Cargo disambiguate.
        let deps = executable.parent()?.join("deps");
        let mut listings = fs::read_dir(deps).ok()?.filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            (name.starts_with("proa_config_runner-") && name.ends_with(".d")).then_some(path)
        });
        let listing = listings.next()?;
        if listings.next().is_some() {
            return None;
        }
        let paths = proa_build::build::rustc_source_dependencies(&fs::read(&listing).ok()?).ok()?;
        for input in paths {
            let path = runner.join(input);
            let state = stamp(&path).ok()??;
            if state.0 > build_started {
                return None;
            }
            inputs.insert(path, Some(state));
        }
        for file in [
            listing,
            executable.to_owned(),
            runner.join("Cargo.toml"),
            runner.join("Cargo.lock"),
        ] {
            let state = stamp(&file).ok()??;
            inputs.insert(file, Some(state));
        }
        Some(Self {
            executable: executable.to_owned(),
            lock: runner.join(".lock"),
            inputs,
        })
    }

    // Capture configuration before Cargo starts. A settings change during
    // compilation must invalidate the next asset-only evaluation too.
    fn settings(root: &Path) -> Option<InputStamps> {
        let mut inputs = InputStamps::new();
        let mut settings = Vec::new();
        let mut cargo_configs = Vec::new();
        for dir in root.ancestors() {
            for name in ["Cargo.toml", "rust-toolchain", "rust-toolchain.toml"] {
                settings.push(dir.join(name));
            }
            cargo_configs.extend([dir.join(".cargo/config"), dir.join(".cargo/config.toml")]);
        }
        if let Some(home) = env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| env::home_dir().map(|dir| dir.join(".cargo")))
        {
            cargo_configs.extend([home.join("config"), home.join("config.toml")]);
        }
        // Path overrides, source replacement, and included Cargo configs can
        // introduce local dependencies outside the runner's rustc dep-info.
        for path in cargo_configs {
            let state = stamp(&path).ok()?;
            if state.is_some() {
                let config = fs::read_to_string(&path)
                    .ok()?
                    .parse::<toml::Table>()
                    .ok()?;
                if ["include", "patch", "paths", "source"]
                    .iter()
                    .any(|key| config.contains_key(*key))
                {
                    return None;
                }
            }
            inputs.insert(path, state);
        }
        if let Some(rustup) = env::var_os("RUSTUP_HOME")
            .map(PathBuf::from)
            .or_else(|| env::home_dir().map(|home| home.join(".rustup")))
        {
            settings.push(rustup.join("settings.toml"));
        }
        settings.extend(
            ["CARGO", "RUSTC"]
                .iter()
                .filter_map(env::var_os)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute()),
        );
        for path in settings {
            let state = stamp(&path).ok()?;
            inputs.insert(path, state);
        }
        Some(inputs)
    }

    fn evaluate_if_fresh(&self, root: &Path, path: &Path) -> Option<Result<ProjectConfig>> {
        // Serialize with other CLI processes replacing the runner in this target.
        let lock = fs::File::open(&self.lock).ok()?;
        lock.lock_shared().ok()?;
        for (path, expected) in &self.inputs {
            if stamp(path).ok()? != *expected {
                return None;
            }
        }
        Some(evaluate(root, path, &self.executable))
    }
}

pub(super) fn load(
    root: &Path,
    path: &Path,
    cache: Option<&mut DevConfigRunner>,
) -> Result<ProjectConfig> {
    if let Some(result) = cache
        .as_ref()
        .and_then(|cache| cache.cached.as_ref())
        .and_then(|runner| runner.evaluate_if_fresh(root, path))
    {
        return result;
    }
    let root = root.canonicalize()?;
    let settings = cache.as_ref().and_then(|_| CachedRunner::settings(&root));
    let path = root.join(path.file_name().context("missing config filename")?);
    let config_path = path.to_str().context("Rust config path must be UTF-8")?;
    let entry = ENTRY.replace("__CONFIG_PATH__", &format!("{config_path:?}"));
    let host = host_target(&root)?;
    let mut key = Sha256::new();
    for input in [MODEL, MANIFEST, LOCKFILE, &entry, &host] {
        key.update((input.len() as u64).to_le_bytes());
        key.update(input.as_bytes());
    }
    let key = format!("{:x}", base16ct::HexDisplay(&key.finalize()));
    // Put persistent helper artifacts in Cargo's resolved target directory.
    // Config inspection also works in a standalone directory before Cargo init.
    let scratch;
    let runner = if root.join("Cargo.toml").is_file() {
        cargo_target_directory(&root)?.join("proa-config").join(key)
    } else {
        scratch = tempfile::tempdir()?;
        scratch.path().to_owned()
    };
    fs::create_dir_all(runner.join("src"))?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(runner.join(".lock"))?;
    lock.lock()?;
    write_changed(&runner.join("Cargo.toml"), MANIFEST)?;
    write_changed(&runner.join("Cargo.lock"), LOCKFILE)?;
    write_changed(&runner.join("src/model.rs"), MODEL)?;
    write_changed(&runner.join("src/main.rs"), &entry)?;

    let mut build = cargo(&root);
    build
        .args(["build", "--quiet", "--locked", "--target", &host])
        .arg("--manifest-path")
        .arg(runner.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(runner.join("target"))
        .env("CARGO_ENCODED_RUSTFLAGS", "")
        .stdout(Stdio::null());
    if requested_offline() {
        build.arg("--offline");
    }
    let build_started = SystemTime::now();
    if !build
        .status()
        .context("start Cargo for Rust config")?
        .success()
    {
        bail!("could not compile {}", path.display());
    }
    let executable = runner
        .join("target")
        .join(host)
        .join("debug")
        .join(format!("proa-config-runner{}", env::consts::EXE_SUFFIX));
    if let Some(cache) = cache {
        cache.cached = settings
            .and_then(|inputs| CachedRunner::capture(inputs, &runner, &executable, build_started));
    }
    evaluate(&root, &path, &executable)
}

fn evaluate(root: &Path, path: &Path, executable: &Path) -> Result<ProjectConfig> {
    let output_directory = tempfile::tempdir()?;
    let output_path = output_directory.path().join("config.json");
    let output = Command::new(executable)
        .current_dir(root)
        .arg(&output_path)
        .output()
        .context("run the host Rust config")?;
    // Author diagnostics stay visible without contaminating `config show --json`.
    io::stderr().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    if !output.status.success() {
        bail!("{} failed with {}", path.display(), output.status);
    }
    serde_json::from_slice(&fs::read(output_path)?).context("read evaluated Rust config")
}

fn requested_offline() -> bool {
    let offline = |arg: std::ffi::OsString| arg == "--offline" || arg == "--frozen";
    let mut args = env::args_os();
    args.by_ref().take_while(|arg| arg != "--").any(offline)
        || (args.next().is_some_and(|arg| arg == "cargo")
            && args.take_while(|arg| arg != "--").any(offline))
}

fn cargo(root: &Path) -> Command {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(root);
    command
        .env("RUSTC_WRAPPER", "")
        .env("RUSTC_WORKSPACE_WRAPPER", "");
    command
}

fn cargo_target_directory(root: &Path) -> Result<PathBuf> {
    #[derive(serde::Deserialize)]
    struct Metadata {
        target_directory: PathBuf,
    }
    let output = cargo(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .output()
        .context("query Cargo target directory for Rust config")?;
    if !output.status.success() {
        bail!(
            "Cargo metadata for Rust config failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(serde_json::from_slice::<Metadata>(&output.stdout)?.target_directory)
}

fn host_target(root: &Path) -> Result<String> {
    let output = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .current_dir(root)
        .arg("-vV")
        .output()
        .context("query host rustc for Rust config")?;
    if !output.status.success() {
        bail!(
            "rustc -vV for Rust config failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout)?
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .context("rustc did not report its host target")
}

fn write_changed(path: &Path, source: &str) -> Result<()> {
    match fs::read(path) {
        Ok(current) if current == source.as_bytes() => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut file = tempfile::NamedTempFile::new_in(path.parent().context("runner directory")?)?;
    file.write_all(source.as_bytes())?;
    file.persist(path)?;
    Ok(())
}

pub(super) fn source(config: &ProjectConfig) -> Result<String> {
    use std::fmt::Write as _;

    let mut source = String::from(
        "use proa_config::{BuildConfig, Config, DevConfig, ProjectPaths, Result};\n\npub async fn config() -> Result<Config> {\n    Ok(Config {\n",
    );
    for (field, value) in [
        ("application", &config.application),
        ("database", &config.database),
        ("ui", &config.ui),
        ("representations", &config.representations),
        ("styling", &config.styling),
        ("deploy", &config.deploy),
        ("content", &config.content),
        ("test", &config.test),
    ] {
        if let Some(value) = value {
            writeln!(source, "        {field}: Some({}),", value_expr(value))?;
        }
    }
    writeln!(source, "        scripts: proa_config::ScriptsConfig {{")?;
    writeln!(source, "            dev: {:?},", config.scripts.dev)?;
    writeln!(source, "            build: {:?},", config.scripts.build)?;
    if config.scripts.named.is_empty() {
        writeln!(source, "            ..Default::default()")?;
    } else {
        writeln!(
            source,
            "            named: std::collections::BTreeMap::from(["
        )?;
        for (name, script) in &config.scripts.named {
            let description = &script.description;
            let command = string_vec(&script.command);
            let cwd = path_literal(&script.cwd)?;
            let depends_on = string_vec(&script.depends_on);
            let inputs = string_vec(&script.inputs);
            writeln!(
                source,
                "                ({name:?}.into(), proa_config::ScriptConfig {{
                    description: {description:?}.into(),
                    command: {command},
                    cwd: {cwd}.into(),
                    depends_on: {depends_on},
                    inputs: {inputs},
                    outputs: vec!["
            )?;
            for path in &script.outputs {
                writeln!(
                    source,
                    "                        {}.into(),",
                    path_literal(path)?
                )?;
            }
            writeln!(source, "                    ],")?;
            writeln!(
                source,
                "                    env: std::collections::BTreeMap::from(["
            )?;
            for (key, value) in &script.env {
                writeln!(
                    source,
                    "                        ({key:?}.into(), {value:?}.into()),"
                )?;
            }
            writeln!(source, "                    ]),\n                }}),")?;
        }
        writeln!(source, "            ]),")?;
    }
    writeln!(source, "        }},")?;
    if config.registries != ProjectConfig::default().registries {
        writeln!(
            source,
            "        registries: std::collections::BTreeMap::from(["
        )?;
        for (name, url) in &config.registries {
            writeln!(source, "            ({name:?}.into(), {url:?}.into()),")?;
        }
        writeln!(source, "        ]),")?;
    }
    writeln!(source, "        paths: ProjectPaths {{")?;
    for (field, path) in [
        ("components", config.paths.components.as_path()),
        ("assets", config.paths.assets.as_path()),
    ] {
        writeln!(
            source,
            "            {field}: {}.into(),",
            path_literal(path)?
        )?;
    }
    for (field, path) in [
        ("pages", &config.paths.pages),
        ("endpoints", &config.paths.endpoints),
        ("layouts", &config.paths.layouts),
        ("models", &config.paths.models),
        ("public", &config.paths.public),
        ("runtime_data", &config.paths.runtime_data),
    ] {
        if let Some(path) = path {
            writeln!(
                source,
                "            {field}: Some({}.into()),",
                path_literal(path)?
            )?;
        }
    }
    writeln!(source, "            ..Default::default()\n        }},")?;
    writeln!(source, "        dev: DevConfig {{")?;
    writeln!(source, "            host: {:?}.into(),", config.dev.host)?;
    writeln!(source, "            port: {},", config.dev.port)?;
    writeln!(
        source,
        "            before: {},",
        string_vec(&config.dev.before)
    )?;
    let watch = config
        .dev
        .watch
        .iter()
        .map(|path| Ok(format!("{}.into()", path_literal(path)?)))
        .collect::<Result<Vec<_>>>()?
        .join(", ");
    writeln!(source, "            watch: vec![{watch}],\n        }},")?;
    writeln!(source, "        build: BuildConfig {{")?;
    writeln!(
        source,
        "            minify_rsjs: {},",
        config.build.minify_rsjs
    )?;
    writeln!(
        source,
        "            before: {},",
        string_vec(&config.build.before)
    )?;
    match &config.build.assets {
        Some(assets) if assets.is_empty() => writeln!(source, "            assets: Some(vec![]),")?,
        Some(assets) => {
            writeln!(source, "            assets: Some(vec![")?;
            for asset in assets {
                writeln!(source, "                proa_config::AssetConfig {{")?;
                writeln!(source, "                    kind: {:?}.into(),", asset.kind)?;
                writeln!(
                    source,
                    "                    command: {},",
                    string_vec(&asset.command)
                )?;
                writeln!(
                    source,
                    "                    input: {}.into(),",
                    path_literal(&asset.input)?
                )?;
                writeln!(
                    source,
                    "                    output: {}.into(),",
                    path_literal(&asset.output)?
                )?;
                if let Some(path) = &asset.config {
                    writeln!(
                        source,
                        "                    config: Some({}.into()),",
                        path_literal(path)?
                    )?;
                } else {
                    writeln!(source, "                    config: None,")?;
                }
                writeln!(source, "                }},")?;
            }
            writeln!(source, "            ]),")?;
        }
        None => writeln!(source, "            assets: None,")?,
    }
    source.push_str("        },\n        ..Config::default()\n    })\n}\n");
    Ok(source)
}

fn path_literal(path: &Path) -> Result<String> {
    Ok(format!(
        "{:?}",
        path.to_str().context("config path must be UTF-8")?
    ))
}

fn string_vec(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|value| format!("{value:?}.into()"))
        .collect::<Vec<_>>();
    format!("vec![{}]", values.join(", "))
}

fn value_expr(value: &serde_json::Value) -> String {
    fn literal(value: &serde_json::Value) -> String {
        use serde_json::Value::*;
        match value {
            Null => "null".into(),
            Bool(value) => value.to_string(),
            Number(value) => {
                let suffix = if value.is_i64() {
                    "i64"
                } else if value.is_u64() {
                    "u64"
                } else {
                    "f64"
                };
                format!("{value}{suffix}")
            }
            String(value) => format!("{value:?}"),
            Array(values) => format!(
                "[{}]",
                values.iter().map(literal).collect::<Vec<_>>().join(", ")
            ),
            Object(values) => {
                let fields = values
                    .iter()
                    .map(|(name, value)| format!("{name:?}: {}", literal(value)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{ {fields} }}")
            }
        }
    }
    format!("proa_config::json::json!({})", literal(value))
}

pub(super) fn with_capability_changes(
    source: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
) -> Result<String> {
    use quote::ToTokens as _;
    use std::fmt::Write as _;

    if before == after {
        return Ok(source.to_owned());
    }
    let parsed = syn::parse_file(source).context("parse proa.config.rs for capability addition")?;
    let function = parsed
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "config" => Some(function),
            _ => None,
        })
        .context("expected a top-level config() function to update")?;
    let open = function.block.brace_token.span.open().start();
    let close = function.block.brace_token.span.close().start();
    let map = crate::source::SourceMap::new(source);
    let start = map
        .byte_for_line_column(open.line, open.column)
        .context("config opening brace")?;
    let end = map
        .byte_for_line_column(close.line, close.column)
        .context("config closing brace")?;
    let mut patched = format!(
        "{}{{\n    let mut configured = (async || {} {{{}\n    }})().await?;\n",
        &source[..start],
        function.sig.output.to_token_stream(),
        &source[start + 1..end],
    );
    for field in ["database", "ui", "deploy"] {
        if before.get(field) != after.get(field) {
            writeln!(
                patched,
                "    configured.{field} = Some({});",
                value_expr(&after[field])
            )?;
        }
    }
    for field in ["representations", "content"] {
        if let Some(entries) = after[field].as_object() {
            for (name, value) in entries {
                if before[field].get(name) != Some(value) {
                    writeln!(patched, "    configured.{field}\n        .get_or_insert_with(|| proa_config::json::json!({{}}))\n        .as_object_mut().expect(\"{field} config must be an object\")\n        .insert({name:?}.into(), {});", value_expr(value))?;
                }
            }
        }
    }
    patched.push_str("    Ok(configured)\n");
    patched.push_str(&source[end..]);
    Ok(patched)
}
