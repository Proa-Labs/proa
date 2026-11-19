// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{ffi::OsString, path::Path, process::Command};

use anyhow::{ensure, Context, Result};

pub(super) fn run(root: &Path, name: &str, source: &str, args: &[OsString]) -> Result<()> {
    let mut process = command(root, name, source, args)?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(process.exec()).with_context(|| format!("run script `{name}`"))
    }
    #[cfg(not(unix))]
    {
        let status = process
            .status()
            .with_context(|| format!("run script `{name}`"))?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }
}

pub(crate) fn command(root: &Path, name: &str, source: &str, args: &[OsString]) -> Result<Command> {
    let source = source.trim();
    ensure!(!source.is_empty(), "scripts.{name} must specify a command");
    let mut command = shell(source, args);
    command
        .current_dir(root)
        .env("PROA_SCRIPT_NAME", name)
        .env("PROA_SCRIPT_EXECUTABLE", crate::util::current_executable()?);
    Ok(command)
}

pub(crate) fn validate(name: &str, source: &str) -> Result<()> {
    ensure!(
        !source.trim().is_empty(),
        "scripts.{name} must specify a command"
    );
    #[cfg(not(windows))]
    {
        let output = Command::new("sh")
            .args(["-n", "-c", source])
            .output()
            .with_context(|| format!("check shell syntax for scripts.{name}"))?;
        ensure!(
            output.status.success(),
            "scripts.{name} has invalid shell syntax: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub(crate) fn hint(name: &str, source: &str, default: &str) {
    if source.trim() != default
        && std::env::var_os("PROA_SCRIPT_NAME").as_deref() != Some(std::ffi::OsStr::new(name))
    {
        eprintln!(
            "Using the built-in {name} workflow. To run scripts.{name}, use `proa run {name}`."
        );
    }
}

#[cfg(not(windows))]
fn shell(source: &str, args: &[OsString]) -> Command {
    let mut script = format!("proa() {{ \"$PROA_SCRIPT_EXECUTABLE\" \"$@\"; }}\n{source}");
    if !args.is_empty() {
        script.push_str(" \"$@\"");
    }
    let mut command = Command::new("sh");
    command.args(["-c", &script, "proa run"]).args(args);
    command
}

#[cfg(windows)]
fn shell(source: &str, args: &[OsString]) -> Command {
    use std::os::windows::process::CommandExt;

    let executable = if args.is_empty() {
        "%PROA_SCRIPT_EXECUTABLE%"
    } else {
        "!PROA_SCRIPT_EXECUTABLE!"
    };
    let mut script = match source.strip_prefix("proa") {
        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
            format!("\"{executable}\"{rest}")
        }
        _ => source.to_owned(),
    };
    let mut command = Command::new("cmd.exe");
    command.args(["/d", "/s"]);
    if args.is_empty() {
        command.arg("/v:off");
    } else {
        // Expand forwarded arguments after cmd's operator parsing. Their
        // contents are then interpreted only by the child process.
        command
            .args(["/v:on"])
            .env("PROA_SCRIPT_ARGS", windows_arguments(args));
        script.push_str(" !PROA_SCRIPT_ARGS!");
    }
    command.arg("/c").raw_arg(format!("\"{script}\""));
    command
}

#[cfg(windows)]
fn windows_arguments(args: &[OsString]) -> OsString {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let mut quoted = Vec::new();
    for arg in args {
        if !quoted.is_empty() {
            quoted.push(u16::from(b' '));
        }
        quoted.push(u16::from(b'"'));
        let mut backslashes = 0;
        for unit in arg.encode_wide() {
            match unit {
                92 => backslashes += 1,
                _ => {
                    let escaped = if unit == u16::from(b'"') {
                        backslashes * 2 + 1
                    } else {
                        backslashes
                    };
                    quoted.extend(std::iter::repeat_n(u16::from(b'\\'), escaped));
                    quoted.push(unit);
                    backslashes = 0;
                }
            }
        }
        quoted.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes * 2));
        quoted.push(u16::from(b'"'));
    }
    OsString::from_wide(&quoted)
}
