// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{env, ffi::OsString, path::Path, process::Command};

use anyhow::{ensure, Context, Result};

use super::{is_cargo_program, LinkedCargoAction};
use crate::config::{parse_script_command, DEFAULT_BUILD_COMMAND, DEFAULT_DEV_COMMAND};

/// Launch project scripts from the project root. The Cargo shorthand enters
/// the linked pipeline in the calling process.
pub(super) fn run_script(
    root: &Path,
    action: LinkedCargoAction,
    source: &str,
    arguments: &[OsString],
) -> Result<bool> {
    let phase = action.command_name();
    let field = format!("scripts.{phase}");
    let argv = parse_script_command(&field, source)?;
    if is_linked_command(&argv, action) {
        return Ok(false);
    }

    let active = format!("PROA_{}_SCRIPT_ACTIVE", phase.to_ascii_uppercase());
    ensure!(
        env::var_os(&active).is_none(),
        "{field} called `proa {phase}` again; use `proa {phase} --linked` inside the script"
    );

    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(root)
        .env(active, "1")
        .env("PROA_SCRIPT_EXECUTABLE", crate::util::current_executable()?);
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        // The outer CLI has resolved --root and changed the script's working
        // directory. Forwarding a relative --root would resolve it twice.
        if argument == "--root" {
            arguments.next();
        } else if !argument.as_encoded_bytes().starts_with(b"--root=") {
            command.arg(argument);
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec()).with_context(|| format!("launch {field}"))
    }
    #[cfg(not(unix))]
    {
        let status = command
            .status()
            .with_context(|| format!("launch {field}"))?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(true)
    }
}

pub(super) fn linked_command(action: LinkedCargoAction, source: &str) -> Result<Vec<String>> {
    let field = format!("scripts.{}", action.command_name());
    let argv = parse_script_command(&field, source)?;
    if is_linked_command(&argv, action) {
        return Ok(argv);
    }
    let default = match action {
        LinkedCargoAction::Dev => DEFAULT_DEV_COMMAND,
        LinkedCargoAction::Build => DEFAULT_BUILD_COMMAND,
        _ => unreachable!("entrypoint only supports dev and build"),
    };
    parse_script_command(&field, default)
}

fn is_linked_command(command: &[String], action: LinkedCargoAction) -> bool {
    is_cargo_program(&command[0]) && command.get(1).map(String::as_str) == Some(action.subcommand())
}

/// The watcher can change Cargo arguments. Only a fresh top-level invocation
/// can enter a different project wrapper and inherit its shell environment.
pub(super) struct WatchedDevScript {
    active: String,
    last_notice: Option<String>,
    in_wrapper: bool,
}

impl WatchedDevScript {
    pub(super) fn new(initial: &str, dispatches_scripts: bool) -> Option<Self> {
        let in_wrapper = env::var_os("PROA_DEV_SCRIPT_ACTIVE").is_some();
        (dispatches_scripts || in_wrapper).then(|| Self {
            active: initial.to_owned(),
            last_notice: None,
            in_wrapper,
        })
    }

    pub(super) fn apply(&mut self, configured: &mut String) -> Result<()> {
        let next = parse_script_command("scripts.dev", configured)?;
        if next == parse_script_command("scripts.dev", &self.active)?
            || (!self.in_wrapper && is_linked_command(&next, LinkedCargoAction::Dev))
        {
            self.active.clone_from(configured);
            self.last_notice = None;
        } else {
            if self.last_notice.as_ref() != Some(configured) {
                eprintln!("scripts.dev changed to {configured:?}. Run `proa dev` again to use it.");
                self.last_notice = Some(configured.clone());
            }
            configured.clone_from(&self.active);
        }
        Ok(())
    }
}
