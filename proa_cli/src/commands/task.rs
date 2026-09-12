// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Structured project tasks executed without a shell.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use clap::Args;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{config, util::resolve_config_root};

#[derive(Debug, Args)]
pub(crate) struct TaskArgs {
    /// Configured task name.
    name: Option<String>,
    #[arg(long)]
    root: Option<PathBuf>,
    /// List configured tasks and their lifecycle use.
    #[arg(long)]
    list: bool,
    /// Bypass the successful-input cache.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskDefinition {
    #[serde(default)]
    description: String,
    command: Vec<String>,
    #[serde(default = "dot")]
    cwd: PathBuf,
    #[serde(default, alias = "dependencies")]
    depends_on: Vec<String>,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    outputs: Vec<PathBuf>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn dot() -> PathBuf {
    PathBuf::from(".")
}

pub(crate) fn run(args: TaskArgs) -> Result<()> {
    let root = resolve_config_root(args.root)?;
    let project = config::load_project_config(&root)?;
    let tasks: BTreeMap<String, TaskDefinition> = project
        .tasks
        .iter()
        .map(|(name, value)| serde_json::from_value(value.clone()).map(|task| (name.clone(), task)))
        .collect::<std::result::Result<_, _>>()
        .context("parse configured tasks")?;
    if args.list {
        for (name, task) in &tasks {
            let lifecycle = lifecycle_usage(&project, name);
            println!("{name:<24} {:<28} {}", task.description, lifecycle);
        }
        return Ok(());
    }
    let name = args.name.context("provide a task name or use --list")?;
    let order = dependency_order(&tasks, &name)?;
    for task_name in order {
        execute(
            &root,
            &task_name,
            &tasks[&task_name],
            args.force,
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
    let tasks: BTreeMap<String, TaskDefinition> = project
        .tasks
        .iter()
        .map(|(name, value)| serde_json::from_value(value.clone()).map(|task| (name.clone(), task)))
        .collect::<std::result::Result<_, _>>()
        .context("parse configured tasks")?;
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    for name in names {
        visit_dependency(name, &tasks, &mut visiting, &mut visited, &mut order)?;
    }
    for task_name in order {
        execute(root, &task_name, &tasks[&task_name], false, environment)?;
    }
    Ok(())
}

fn lifecycle_usage(project: &config::ProjectConfig, name: &str) -> String {
    let mut phases = Vec::new();
    if project.dev.before.iter().any(|task| task == name) {
        phases.push("dev");
    }
    if project.build.before.iter().any(|task| task == name) {
        phases.push("build");
    }
    if project
        .test
        .as_ref()
        .and_then(|test| test.get("before"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|tasks| tasks.iter().any(|task| task.as_str() == Some(name)))
    {
        phases.push("test");
    }
    if phases.is_empty() {
        "manual".to_string()
    } else {
        phases.join(",")
    }
}

fn dependency_order(tasks: &BTreeMap<String, TaskDefinition>, root: &str) -> Result<Vec<String>> {
    let mut output = Vec::new();
    visit_dependency(
        root,
        tasks,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
        &mut output,
    )?;
    Ok(output)
}

fn visit_dependency(
    name: &str,
    tasks: &BTreeMap<String, TaskDefinition>,
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
    task: &TaskDefinition,
    force: bool,
    environment: &BTreeMap<OsString, OsString>,
) -> Result<()> {
    let executable = task
        .command
        .first()
        .with_context(|| format!("task `{name}` has an empty command"))?;
    let cwd = root.join(&task.cwd);
    let key = task_cache_key(root, task)?;
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
    let status = Command::new(executable)
        .args(&task.command[1..])
        .current_dir(&cwd)
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

fn task_cache_key(root: &Path, task: &TaskDefinition) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(env!("CARGO_PKG_VERSION"));
    for argument in &task.command {
        digest.update(argument.as_bytes());
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
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(dependencies: &[&str]) -> TaskDefinition {
        TaskDefinition {
            description: String::new(),
            command: vec!["true".into()],
            cwd: dot(),
            depends_on: dependencies.iter().map(|value| (*value).into()).collect(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn detects_cycles_before_running() {
        let tasks = BTreeMap::from([("a".into(), task(&["b"])), ("b".into(), task(&["a"]))]);
        assert!(dependency_order(&tasks, "a")
            .unwrap_err()
            .to_string()
            .contains("cycle"));
    }
}
