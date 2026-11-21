// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ProjectConfig;

const STAMP_CACHE_SCHEMA_VERSION: u16 = 1;
const STAMP_CACHE_FILE: &str = "stamp-cache.json";
static STAMP_CACHE_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedStampCache {
    schema_version: u16,
    executable: PathBuf,
    inputs: Vec<PersistedInputStamp>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedInputStamp {
    path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified_nanos: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    len: Option<u64>,
}

struct CachedRunner {
    executable: PathBuf,
    lock: PathBuf,
    inputs: InputStamps,
}

fn system_time_to_unix(time: SystemTime) -> Option<(u64, u32)> {
    let duration = time.duration_since(UNIX_EPOCH).ok()?;
    Some((duration.as_secs(), duration.subsec_nanos()))
}

fn unix_to_system_time(secs: u64, nanos: u32) -> SystemTime {
    UNIX_EPOCH + Duration::new(secs, nanos)
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

    fn is_fresh(&self) -> bool {
        for (path, expected) in &self.inputs {
            match stamp(path) {
                Ok(actual) if actual == *expected => {}
                _ => return false,
            }
        }
        true
    }

    fn evaluate_if_fresh(&self, root: &Path, path: &Path) -> Option<Result<ProjectConfig>> {
        // Serialize with other CLI processes replacing the runner in this target.
        let lock = fs::File::open(&self.lock).ok()?;
        lock.lock_shared().ok()?;
        if !self.is_fresh() {
            return None;
        }
        Some(evaluate(root, path, &self.executable))
    }

    fn save(&self, runner: &Path) {
        let Some(inputs) = self
            .inputs
            .iter()
            .map(|(path, state)| match state {
                None => Some(PersistedInputStamp {
                    path: path.clone(),
                    modified_secs: None,
                    modified_nanos: None,
                    len: None,
                }),
                Some((modified, len)) => {
                    let (secs, nanos) = system_time_to_unix(*modified)?;
                    Some(PersistedInputStamp {
                        path: path.clone(),
                        modified_secs: Some(secs),
                        modified_nanos: Some(nanos),
                        len: Some(*len),
                    })
                }
            })
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let payload = PersistedStampCache {
            schema_version: STAMP_CACHE_SCHEMA_VERSION,
            executable: self.executable.clone(),
            inputs,
        };
        let Ok(bytes) = serde_json::to_vec(&payload) else {
            return;
        };
        let seq = STAMP_CACHE_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = runner.join(format!(
            ".{STAMP_CACHE_FILE}.proa-{}-{seq}.tmp",
            std::process::id()
        ));
        let dest = runner.join(STAMP_CACHE_FILE);
        if fs::write(&tmp, bytes).is_ok() {
            #[cfg(windows)]
            let _ = fs::remove_file(&dest);
            if fs::rename(&tmp, &dest).is_err() {
                let _ = fs::remove_file(&tmp);
            }
        }
    }

    fn load_persisted(root: &Path, runner: &Path, expected_executable: &Path) -> Option<Self> {
        // Re-check Cargo config safety rules (e.g., `patch`/`paths`/`source`/`include`)
        // before trusting any persisted stamp set.
        let current_settings = Self::settings(root)?;
        let bytes = fs::read(runner.join(STAMP_CACHE_FILE)).ok()?;
        let persisted: PersistedStampCache = serde_json::from_slice(&bytes).ok()?;
        if persisted.schema_version != STAMP_CACHE_SCHEMA_VERSION
            || persisted.executable != expected_executable
        {
            return None;
        }
        let mut inputs = InputStamps::new();
        for item in persisted.inputs {
            let state = match (item.modified_secs, item.modified_nanos, item.len) {
                (Some(secs), Some(nanos), Some(len)) => {
                    Some((unix_to_system_time(secs, nanos), len))
                }
                (None, None, None) => None,
                _ => return None,
            };
            inputs.insert(item.path, state);
        }
        // Every current Cargo/toolchain settings path must be accounted for and unchanged.
        for (path, expected) in &current_settings {
            if inputs.get(path) != Some(expected) {
                return None;
            }
        }
        let cached = Self {
            executable: persisted.executable,
            lock: runner.join(".lock"),
            inputs,
        };
        cached.is_fresh().then_some(cached)
    }
}

pub(super) fn load(
    root: &Path,
    path: &Path,
    mut cache: Option<&mut DevConfigRunner>,
) -> Result<ProjectConfig> {
    if let Some(result) = cache
        .as_ref()
        .and_then(|cache| cache.cached.as_ref())
        .and_then(|runner| runner.evaluate_if_fresh(root, path))
    {
        return result;
    }
    let root = root.canonicalize()?;
    let path = root.join(path.file_name().context("missing config filename")?);
    let config_path = path.to_str().context("Rust config path must be UTF-8")?;
    let entry = ENTRY.replace("__CONFIG_PATH__", &format!("{config_path:?}"));
    let (host, rustc_version) = host_target_and_version(&root)?;
    let mut key = Sha256::new();
    for input in [MODEL, MANIFEST, LOCKFILE, &entry, &host, &rustc_version] {
        key.update((input.len() as u64).to_le_bytes());
        key.update(input.as_bytes());
    }
    let key = format!("{:x}", base16ct::HexDisplay(&key.finalize()));
    // Put persistent helper artifacts in Cargo's resolved target directory.
    // Config inspection also works in a standalone directory before Cargo init.
    let scratch;
    let persistent = root.join("Cargo.toml").is_file();
    let runner = if persistent {
        cargo_target_directory(&root)?.join("proa-config").join(key)
    } else {
        scratch = tempfile::tempdir()?;
        scratch.path().to_owned()
    };
    let executable = runner
        .join("target")
        .join(&host)
        .join("debug")
        .join(format!("proa-config-runner{}", env::consts::EXE_SUFFIX));

    // Fast path across fresh CLI processes: validate persisted rustc dep-info +
    // Cargo settings under a shared lock and skip invoking `cargo build`.
    if persistent {
        if let Ok(shared_lock) = fs::File::open(runner.join(".lock")) {
            if shared_lock.lock_shared().is_ok() {
                if let Some(persisted) = CachedRunner::load_persisted(&root, &runner, &executable) {
                    let result = evaluate(&root, &path, &persisted.executable);
                    if let Some(cache) = cache.as_deref_mut() {
                        cache.cached = Some(persisted);
                    }
                    return result;
                }
            }
        }
    }

    let settings = CachedRunner::settings(&root);
    fs::create_dir_all(runner.join("src"))?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(runner.join(".lock"))?;
    lock.lock()?;
    if persistent {
        if let Some(persisted) = CachedRunner::load_persisted(&root, &runner, &executable) {
            let result = evaluate(&root, &path, &persisted.executable);
            if let Some(cache) = cache.as_deref_mut() {
                cache.cached = Some(persisted);
            }
            return result;
        }
    }

    write_changed(&runner.join("Cargo.toml"), MANIFEST)?;
    write_changed(&runner.join("Cargo.lock"), LOCKFILE)?;
    write_changed(&runner.join("src/model.rs"), MODEL)?;
    write_changed(&runner.join("src/main.rs"), &entry)?;

    if !executable.is_file() {
        eprintln!(
            "compiling {}...",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("proa.config.rs")
        );
    }
    let _ = fs::remove_file(runner.join(STAMP_CACHE_FILE));
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
    if let Some(captured) = settings
        .and_then(|inputs| CachedRunner::capture(inputs, &runner, &executable, build_started))
    {
        if persistent {
            captured.save(&runner);
        }
        if let Some(cache) = cache {
            cache.cached = Some(captured);
        }
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

fn host_target_and_version(root: &Path) -> Result<(String, String)> {
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
    let version = String::from_utf8(output.stdout)?;
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .context("rustc did not report its host target")?;
    Ok((host, version))
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
        "use proa::config::{Config, Result};\n\npub async fn config() -> Result<Config> {\n    Ok(Config {\n",
    );
    let defaults = ProjectConfig::default();
    macro_rules! section {
        ($($field:ident),+ $(,)?) => {
            $(if config.$field != defaults.$field {
                writeln!(source, "        {}: {},", stringify!($field), config.$field.rust_expr(8, TextSource::Literal)?)?;
            })+
        };
    }
    section!(application, database, deploy);
    writeln!(
        source,
        "        scripts: {},",
        config.scripts.rust_expr(8, TextSource::Literal)?
    )?;
    section!(paths);
    if config.registries == super::RegistriesConfig::public_only() {
        writeln!(
            source,
            "        registries: proa::config::RegistriesConfig::public_only(),"
        )?;
    } else {
        section!(registries);
    }
    section!(build);
    source.push_str("        ..Config::default()\n    })\n}\n");
    Ok(source)
}

fn path_literal(path: &Path) -> Result<String> {
    Ok(format!(
        "{:?}",
        path.to_str().context("config path must be UTF-8")?
    ))
}

#[derive(Clone, Copy)]
enum TextSource {
    Literal,
    Convert,
}

impl TextSource {
    fn render(self, mut literal: String) -> String {
        if matches!(self, Self::Convert) {
            // proa-perf-ok: CLI config source codegen, not an SSR render path
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

impl ConfigSource for bool {
    fn rust_expr(&self, _: usize, _: TextSource) -> Result<String> {
        Ok(self.to_string())
    }
}

impl<T: ConfigSource> ConfigSource for Vec<T> {
    fn rust_expr(&self, indent: usize, text: TextSource) -> Result<String> {
        let items = self
            .iter()
            .map(|value| value.rust_expr(indent + 4, text))
            .collect::<Result<Vec<_>>>()?;
        Ok(collection_source("vec![", "]", &items, indent))
    }
}

impl<T: ConfigSource> ConfigSource for BTreeMap<String, T> {
    fn rust_expr(&self, indent: usize, text: TextSource) -> Result<String> {
        let items = self
            .iter()
            .map(|(key, value)| {
                Ok(format!(
                    "({}, {})",
                    key.rust_expr(indent + 4, text)?,
                    value.rust_expr(indent + 4, text)?
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(collection_source(
            "std::collections::BTreeMap::from([",
            "])",
            &items,
            indent,
        ))
    }
}

fn collection_source(open: &str, close: &str, items: &[String], indent: usize) -> String {
    if items.iter().any(|item| item.contains('\n')) {
        let padding = " ".repeat(indent + 4);
        let items = items.join(&format!(",\n{padding}"));
        format!("{open}\n{padding}{items},\n{:indent$}{close}", "")
    } else {
        format!("{open}{}{close}", items.join(", "))
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
    ($($name:ident $([$($explicit:ident),+])? { $($field:ident),+ $(,)? }),+ $(,)?) => {
        $(impl ConfigSource for super::$name {
            fn rust_expr(&self, indent: usize, text: TextSource) -> Result<String> {
                use std::fmt::Write as _;
                let default = Self::default();
                let mut fields = String::new();
                let padding = indent + 4;
                $($(
                    writeln!(
                        fields, "{:padding$}{}: {},", "", stringify!($explicit),
                        self.$explicit.rust_expr(padding, text)?,
                    )?;
                )+)?
                $(if self.$field != default.$field {
                    writeln!(
                        fields, "{:padding$}{}: {},", "", stringify!($field),
                        self.$field.rust_expr(padding, text)?,
                    )?;
                })+
                let name = concat!("proa::config::", stringify!($name));
                if fields.is_empty() {
                    return Ok(format!("{name}::default()"));
                }
                Ok(format!("{name} {{\n{fields}{:padding$}..Default::default()\n{:indent$}}}", "", ""))
            }
        })+
    };
}

config_sections! {
    RegistriesConfig { proa, proa_pro },
    ApplicationConfig { binary },
    DatabaseConfig { driver, migrations, integration },
    DeployConfig { provider, cdn },
    ScriptsConfig [dev, build] { css, before, named },
    ScriptHooks { dev, build, test },
    ScriptConfig { description, command, cwd, depends_on, inputs, outputs, env },
    ProjectPaths { components, assets, public },
    BuildConfig { minify_rsjs, assets },
    AssetConfig { kind, command, input, output, config },
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
