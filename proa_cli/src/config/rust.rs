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
extern crate self as proa;

mod model;

pub mod config {
    pub use crate::model::*;
    pub use tokio;

    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}

#[path = __CONFIG_PATH__]
mod authored;

fn main() -> config::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let config: config::Config<_, _> = runtime.block_on(authored::config())?;
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
        "use proa::config::{Config, ProjectPaths, Result};\n\npub async fn config() -> Result<Config> {\n    Ok(Config {\n",
    );
    macro_rules! section {
        ($($field:ident),+ $(,)?) => {
            $(if let Some(value) = &config.$field {
                writeln!(source, "        {}: Some({}),", stringify!($field), value.rust_expr(8, TextSource::Literal)?)?;
            })+
        };
    }
    section!(application, database, deploy);
    writeln!(source, "        scripts: proa::config::ScriptsConfig {{")?;
    writeln!(source, "            dev: {:?},", config.scripts.dev)?;
    writeln!(source, "            build: {:?},", config.scripts.build)?;
    if !config.scripts.css.is_empty() {
        writeln!(source, "            css: {:?},", config.scripts.css)?;
    }
    let before = &config.scripts.before;
    if !before.is_empty() {
        writeln!(source, "            before: proa::config::ScriptHooks {{")?;
        for (phase, tasks) in [
            ("dev", &before.dev),
            ("build", &before.build),
            ("test", &before.test),
        ] {
            if !tasks.is_empty() {
                writeln!(
                    source,
                    "                {phase}: {},",
                    string_vec(tasks, TextSource::Literal)
                )?;
            }
        }
        writeln!(
            source,
            "                ..Default::default()\n            }},"
        )?;
    }
    if !config.scripts.named.is_empty() {
        writeln!(
            source,
            "            named: std::collections::BTreeMap::from(["
        )?;
        for (name, script) in &config.scripts.named {
            let description = &script.description;
            let command = string_vec(&script.command, TextSource::Literal);
            let cwd = path_literal(&script.cwd)?;
            let depends_on = string_vec(&script.depends_on, TextSource::Literal);
            let inputs = string_vec(&script.inputs, TextSource::Literal);
            writeln!(
                source,
                "                ({name:?}, proa::config::ScriptConfig {{
                    description: {description:?},
                    command: {command},
                    cwd: {cwd},
                    depends_on: {depends_on},
                    inputs: {inputs},
                    outputs: vec!["
            )?;
            for path in &script.outputs {
                writeln!(source, "                        {},", path_literal(path)?)?;
            }
            writeln!(source, "                    ],")?;
            writeln!(
                source,
                "                    env: std::collections::BTreeMap::from(["
            )?;
            for (key, value) in &script.env {
                writeln!(source, "                        ({key:?}, {value:?}),")?;
            }
            writeln!(source, "                    ]),\n                }}),")?;
        }
        writeln!(source, "            ]),")?;
    }
    writeln!(source, "            ..Default::default()")?;
    writeln!(source, "        }},")?;
    if config.registries == super::RegistriesConfig::public_only() {
        writeln!(
            source,
            "        registries: proa::config::RegistriesConfig::public_only(),"
        )?;
    } else if config.registries != ProjectConfig::default().registries {
        writeln!(
            source,
            "        registries: {},",
            config.registries.rust_expr(8, TextSource::Literal)?,
        )?;
    }
    writeln!(source, "        paths: ProjectPaths {{")?;
    let default_paths = super::ProjectPaths::default();
    for (field, path, default) in [
        (
            "components",
            &config.paths.components,
            &default_paths.components,
        ),
        ("assets", &config.paths.assets, &default_paths.assets),
    ] {
        if path != default {
            writeln!(source, "            {field}: {},", path_literal(path)?)?;
        }
    }
    if let Some(path) = &config.paths.public {
        writeln!(source, "            public: Some({}),", path_literal(path)?)?;
    }
    writeln!(source, "            ..Default::default()\n        }},")?;
    if !config.build.minify_rsjs || config.build.assets.is_some() {
        writeln!(source, "        build: proa::config::BuildConfig {{")?;
        writeln!(
            source,
            "            minify_rsjs: {},",
            config.build.minify_rsjs
        )?;
        match &config.build.assets {
            Some(assets) if assets.is_empty() => {
                writeln!(source, "            assets: Some(vec![]),")?
            }
            Some(assets) => {
                writeln!(source, "            assets: Some(vec![")?;
                for asset in assets {
                    writeln!(source, "                proa::config::AssetConfig {{")?;
                    writeln!(source, "                    kind: {:?},", asset.kind)?;
                    writeln!(
                        source,
                        "                    command: {},",
                        string_vec(&asset.command, TextSource::Literal)
                    )?;
                    writeln!(
                        source,
                        "                    input: {},",
                        path_literal(&asset.input)?
                    )?;
                    writeln!(
                        source,
                        "                    output: {},",
                        path_literal(&asset.output)?
                    )?;
                    if let Some(path) = &asset.config {
                        writeln!(
                            source,
                            "                    config: Some({}),",
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
        source.push_str("        },\n");
    }
    source.push_str("        ..Config::default()\n    })\n}\n");
    Ok(source)
}

fn path_literal(path: &Path) -> Result<String> {
    Ok(format!(
        "{:?}",
        path.to_str().context("config path must be UTF-8")?
    ))
}

fn string_vec(values: &[String], text: TextSource) -> String {
    let values = values
        .iter()
        .map(|value| text.render(format!("{value:?}")))
        .collect::<Vec<_>>();
    format!("vec![{}]", values.join(", "))
}

#[derive(Clone, Copy)]
enum TextSource {
    Literal,
    Convert,
}

impl TextSource {
    fn render(self, mut literal: String) -> String {
        if matches!(self, Self::Convert) {
            literal.push_str(".into()");
        }
        literal
    }
}

// Render typed values back into source for scaffolding and capability edits.
trait ConfigSource {
    fn rust_expr(&self, indent: usize, text: TextSource) -> Result<String>;
}

impl ConfigSource for String {
    fn rust_expr(&self, _: usize, text: TextSource) -> Result<String> {
        Ok(text.render(format!("{self:?}")))
    }
}

impl ConfigSource for std::path::PathBuf {
    fn rust_expr(&self, _: usize, text: TextSource) -> Result<String> {
        Ok(text.render(path_literal(self)?))
    }
}

impl ConfigSource for Vec<String> {
    fn rust_expr(&self, _: usize, text: TextSource) -> Result<String> {
        Ok(string_vec(self, text))
    }
}

impl<T: ConfigSource> ConfigSource for Option<T> {
    fn rust_expr(&self, indent: usize, text: TextSource) -> Result<String> {
        match self {
            Some(value) => Ok(format!("Some({})", value.rust_expr(indent, text)?)),
            None => Ok("None".into()),
        }
    }
}

macro_rules! config_sections {
    ($($name:ident { $($field:ident),+ $(,)? }),+ $(,)?) => {
        $(impl ConfigSource for super::$name {
            fn rust_expr(&self, indent: usize, text: TextSource) -> Result<String> {
                use std::fmt::Write as _;
                let default = Self::default();
                if *self == default {
                    return Ok(concat!("proa::config::", stringify!($name), "::default()").into());
                }
                let mut result = String::from(concat!("proa::config::", stringify!($name), " {\n"));
                let padding = indent + 4;
                $(if self.$field != default.$field {
                    writeln!(
                        result, "{:padding$}{}: {},", "", stringify!($field),
                        self.$field.rust_expr(padding, text)?,
                    )?;
                })+
                write!(result, "{:padding$}..Default::default()\n{:indent$}}}", "", "")?;
                Ok(result)
            }
        })+
    };
}

config_sections! {
    RegistriesConfig { proa, proa_pro },
    ApplicationConfig { binary },
    DatabaseConfig { driver, migrations, integration },
    DeployConfig { provider, cdn },
}

impl ConfigSource for super::DatabaseDriver {
    fn rust_expr(&self, _: usize, _: TextSource) -> Result<String> {
        Ok(match self {
            Self::None => "proa::config::DatabaseDriver::None",
            Self::Sqlite => "proa::config::DatabaseDriver::Sqlite",
        }
        .into())
    }
}

impl ConfigSource for super::CdnConfig {
    fn rust_expr(&self, indent: usize, text: TextSource) -> Result<String> {
        let padding = indent + 4;
        match self {
            Self::Cloudflare { zone_id, hostname } => Ok(format!(
                "proa::config::CdnConfig::Cloudflare {{\n{:padding$}zone_id: {},\n{:padding$}hostname: {},\n{:indent$}}}",
                "", zone_id.rust_expr(padding, text)?, "", hostname.rust_expr(padding, text)?, "",
            )),
            Self::Command { command } => Ok(format!(
                "proa::config::CdnConfig::Command {{\n{:padding$}command: {},\n{:indent$}}}",
                "", command.rust_expr(padding, text)?, "",
            )),
        }
    }
}

pub(super) fn with_capability_changes(
    source: &str,
    before: &ProjectConfig,
    after: &ProjectConfig,
) -> Result<String> {
    use quote::ToTokens as _;
    use std::fmt::Write as _;

    let mut changes = String::new();
    macro_rules! replace_section {
        ($field:ident) => {
            if before.$field != after.$field {
                writeln!(
                    changes,
                    "    configured.{} = {};",
                    stringify!($field),
                    after.$field.rust_expr(4, TextSource::Convert)?
                )?;
            }
        };
    }
    macro_rules! replace_field {
        ($section:ident, $field:ident) => {
            let previous = before.$section.as_ref().map(|section| &section.$field);
            if let Some(value) = after.$section.as_ref().map(|section| &section.$field) {
                if previous != Some(value) {
                    writeln!(
                        changes,
                        "    configured.{}.get_or_insert_with(Default::default).{} = {};",
                        stringify!($section),
                        stringify!($field),
                        value.rust_expr(4, TextSource::Convert)?,
                    )?;
                }
            }
        };
    }
    replace_section!(database);
    replace_field!(deploy, provider);
    if changes.is_empty() {
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
    patched.push_str(&changes);
    patched.push_str("    Ok(configured)\n");
    patched.push_str(&source[end..]);
    Ok(patched)
}
