// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Structured project tasks executed without a shell.

use base16ct::HexDisplay;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::config::{self, ScriptConfig};

pub(super) fn list(project: &config::ProjectConfig) {
    let scripts = &project.scripts;
    for (name, source) in super::script::shell_entries(scripts) {
        println!("{name:<24} {source}");
    }
    for (name, task) in &scripts.named {
        if super::script::shell_entries(scripts).any(|(script, _)| script == name) {
            continue;
        }
        let lifecycle = lifecycle_usage(project, name);
        println!("{name:<24} {:<28} {}", task.description, lifecycle);
    }
}

pub(super) fn run_named(
    root: &Path,
    project: &config::ProjectConfig,
    name: &str,
    force: bool,
    arguments: &[OsString],
) -> Result<()> {
    let scripts = &project.scripts.named;
    let order = dependency_order(scripts, [name])?;
    for task_name in order {
        execute(
            root,
            &task_name,
            &scripts[&task_name],
            force,
            if task_name == name { arguments } else { &[] },
            &BTreeMap::new(),
        )?;
    }
    Ok(())
}

/// Run lifecycle-owned task roots with one dependency traversal and an exact
/// child-process environment overlay. The overlay is intentionally applied to
/// children only; test database setup must never mutate the CLI process or the
/// project's `.env` files.
pub(crate) fn run_lifecycle_tasks(
    root: &Path,
    project: &config::ProjectConfig,
    names: &[String],
    environment: &BTreeMap<OsString, OsString>,
) -> Result<()> {
    let scripts = &project.scripts.named;
    for task_name in dependency_order(scripts, names.iter().map(String::as_str))? {
        execute(
            root,
            &task_name,
            &scripts[&task_name],
            false,
            &[],
            environment,
        )?;
    }
    Ok(())
}

pub(crate) fn validate_lifecycle_tasks(
    project: &config::ProjectConfig,
    names: &[String],
) -> Result<()> {
    let _ = dependency_order(&project.scripts.named, names.iter().map(String::as_str))?;
    Ok(())
}

pub(crate) fn lifecycle_output_paths<'a>(
    project: &'a config::ProjectConfig,
    names: &[String],
) -> Result<Vec<&'a Path>> {
    let scripts = &project.scripts.named;
    let order = dependency_order(scripts, names.iter().map(String::as_str))?;
    Ok(order
        .iter()
        .flat_map(|name| scripts[name].outputs.iter().map(PathBuf::as_path))
        .collect())
}

fn dependency_order<'a>(
    scripts: &BTreeMap<String, ScriptConfig>,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<String>> {
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    for name in names {
        visit_dependency(name, scripts, &mut visiting, &mut visited, &mut order)?;
    }
    Ok(order)
}

fn lifecycle_usage(project: &config::ProjectConfig, name: &str) -> String {
    let before = &project.scripts.before;
    let phases: Vec<_> = [
        ("dev", &before.dev),
        ("build", &before.build),
        ("test", &before.test),
    ]
    .into_iter()
    .filter_map(|(phase, tasks)| tasks.iter().any(|task| task == name).then_some(phase))
    .collect();
    if phases.is_empty() {
        "manual".to_string()
    } else {
        phases.join(",")
    }
}

fn visit_dependency(
    name: &str,
    tasks: &BTreeMap<String, ScriptConfig>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
    output: &mut Vec<String>,
) -> Result<()> {
    if visited.contains(name) {
        return Ok(());
    }
    if !visiting.insert(name.to_string()) {
        bail!("task dependency cycle includes `{name}`");
    }
    let task = tasks
        .get(name)
        .with_context(|| format!("unknown task `{name}`"))?;
    for dependency in &task.depends_on {
        visit_dependency(dependency, tasks, visiting, visited, output)?;
    }
    visiting.remove(name);
    visited.insert(name.to_string());
    output.push(name.to_string());
    Ok(())
}

fn execute(
    root: &Path,
    name: &str,
    task: &ScriptConfig,
    force: bool,
    arguments: &[OsString],
    environment: &BTreeMap<OsString, OsString>,
) -> Result<()> {
    let executable = task
        .command
        .first()
        .with_context(|| format!("task `{name}` has an empty command"))?;
    let cwd = root.join(&task.cwd);
    let key = task_cache_key(root, task, arguments)?;
    let cache = root
        .join("target/proa/tasks")
        .join(format!("{name}.sha256"));
    let outputs_ready =
        !task.outputs.is_empty() && task.outputs.iter().all(|output| root.join(output).exists());
    if !force && outputs_ready && fs::read_to_string(&cache).ok().as_deref() == Some(&key) {
        println!("Task {name} · cached");
        return Ok(());
    }
    println!("Task {name}");
    let current = crate::util::current_executable()?;
    let program = if executable == "proa" {
        current.as_os_str()
    } else {
        std::ffi::OsStr::new(executable)
    };
    let status = Command::new(program)
        .args(&task.command[1..])
        .args(arguments)
        .current_dir(&cwd)
        .env(
            "PROA_SCRIPT_EXECUTABLE",
            crate::util::external_path(&current),
        )
        .envs(environment)
        .envs(&task.env)
        .status()
        .with_context(|| format!("run task `{name}`"))?;
    if !status.success() {
        bail!("task `{name}` failed with {status}");
    }
    if !task.outputs.is_empty() {
        if let Some(parent) = cache.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&cache, key)?;
    }
    Ok(())
}

fn task_cache_key(root: &Path, task: &ScriptConfig, arguments: &[OsString]) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(env!("CARGO_PKG_VERSION"));
    for argument in &task.command {
        digest.update(argument.as_bytes());
        digest.update([0]);
    }
    for argument in arguments {
        digest.update(argument.as_encoded_bytes());
        digest.update([0]);
    }
    for pattern in &task.inputs {
        let absolute = root.join(pattern);
        let pattern = absolute.to_string_lossy();
        let mut matches = glob::glob(&pattern)?.collect::<std::result::Result<Vec<_>, _>>()?;
        matches.sort();
        for path in matches {
            if path.is_file() {
                digest.update(path.to_string_lossy().as_bytes());
                digest.update(fs::read(path)?);
            }
        }
    }
    Ok(format!("{:x}", HexDisplay(&digest.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(dependencies: &[&str]) -> ScriptConfig {
        ScriptConfig {
            command: vec!["true".into()],
            depends_on: dependencies.iter().map(|value| (*value).into()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn detects_cycles_before_running() {
        let tasks = BTreeMap::from([("a".into(), task(&["b"])), ("b".into(), task(&["a"]))]);
        assert!(dependency_order(&tasks, ["a"])
            .unwrap_err()
            .to_string()
            .contains("cycle"));
    }
}
