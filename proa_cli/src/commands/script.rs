// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{ffi::OsString, path::Path, process::Command};

use anyhow::{ensure, Context, Result};

pub(super) fn run(root: &Path, name: &str, source: &str, args: &[OsString]) -> Result<()> {
    let mut process = command(root, name, source, args)?;
    #[cfg(unix)]
    {
        use std::{
            io::IsTerminal,
            os::unix::process::{CommandExt, ExitStatusExt},
        };

        // A foreground shell keeps the terminal's process group and job
        // control. A supervisor can signal only the CLI PID, so give a
        // noninteractive script its own group and relay signals to the group.
        if std::io::stdin().is_terminal() {
            return Err(process.exec()).with_context(|| format!("run script `{name}`"));
        }
        crate::lifecycle::install_child_group_signal_handler();
        let mut child = process
            .process_group(0)
            .spawn()
            .with_context(|| format!("run script `{name}`"))?;
        crate::lifecycle::set_active_child_group(&child);
        let status = child.wait();
        crate::lifecycle::clear_active_child_group();
        let status = status.with_context(|| format!("wait for script `{name}`"))?;
        if let Some(signal) = status.signal() {
            std::process::exit(128 + signal);
        }
        exit_with_status(status)
    }
    #[cfg(not(unix))]
    exit_with_status(
        process
            .status()
            .with_context(|| format!("run script `{name}`"))?,
    )
}

fn exit_with_status(status: std::process::ExitStatus) -> Result<()> {
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

pub(crate) fn command(root: &Path, name: &str, source: &str, args: &[OsString]) -> Result<Command> {
    let source = source.trim();
    ensure!(!source.is_empty(), "scripts.{name} must specify a command");
    #[cfg(windows)]
    let mut command = shell(source, args)?;
    #[cfg(not(windows))]
    let mut command = shell(source, args);
    let executable = crate::util::external_path(&crate::util::current_executable()?);
    command
        .current_dir(root)
        .env("PROA_SCRIPT_NAME", name)
        .env("PROA_SCRIPT_EXECUTABLE", executable);
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
    let mut script = String::from("proa() { \"$PROA_SCRIPT_EXECUTABLE\" \"$@\"; }\n");
    let end = if args.is_empty() {
        source.len()
    } else {
        argument_position(source)
    };
    script.push_str(&source[..end]);
    if !args.is_empty() && end != 0 {
        script.push_str(" \"$@\"");
    }
    script.push_str(&source[end..]);
    let mut command = Command::new("sh");
    command.args(["-c", &script, "proa run"]).args(args);
    command
}

#[cfg(not(windows))]
fn argument_position(source: &str) -> usize {
    // Keep trailing separators and shell comments after the forwarded args.
    // The shell still parses and executes the full source.
    let mut end = 0;
    let mut quote = None;
    let mut word = false;
    let mut chars = source.char_indices();
    while let Some((index, character)) = chars.next() {
        match (quote, character) {
            (None, '#') if !word => {
                for (_, next) in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
                continue;
            }
            (None, ' ' | '\t' | '\n' | ';' | '&') => {
                word = false;
                continue;
            }
            (None, '\'' | '"') => quote = Some(character),
            (Some(open), close) if open == close => quote = None,
            (None | Some('"'), '\\') => {
                if let Some((index, escaped)) = chars.next() {
                    end = index + escaped.len_utf8();
                    word = true;
                    continue;
                }
            }
            _ => {}
        }
        end = index + character.len_utf8();
        word = true;
    }
    end
}

#[cfg(windows)]
fn shell(source: &str, args: &[OsString]) -> Result<Command> {
    use std::os::windows::process::CommandExt;

    let mut script = match source.strip_prefix("proa") {
        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
            format!("\"%PROA_SCRIPT_EXECUTABLE%\"{rest}")
        }
        _ => source.to_owned(),
    };
    let mut command = Command::new("cmd.exe");
    if !args.is_empty() {
        command.env("PROA_SCRIPT_ARGS", windows_arguments(args)?);
        script.push_str(" %PROA_SCRIPT_ARGS%");
    }
    command
        .args(["/d", "/s", "/v:off", "/c"])
        .raw_arg(format!("\"{script}\""));
    Ok(command)
}

#[cfg(windows)]
fn windows_arguments(args: &[OsString]) -> Result<OsString> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    const BACKSLASH: u16 = b'\\' as u16;
    const QUOTE: u16 = b'"' as u16;
    let mut quoted = Vec::new();
    for arg in args {
        ensure!(
            !arg.as_encoded_bytes()
                .iter()
                .any(|&b| matches!(b, b'\r' | b'\n')),
            "Windows command scripts cannot forward arguments containing line breaks"
        );
        if !quoted.is_empty() {
            quoted.push(u16::from(b' '));
        }
        quoted.push(QUOTE);
        let mut backslashes = 0;
        for unit in arg.encode_wide() {
            match unit {
                BACKSLASH => backslashes += 1,
                QUOTE => {
                    // Two adjacent quotes keep shell operators quoted and
                    // become one literal quote in a Windows program's argv.
                    quoted.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
                    quoted.extend([QUOTE, QUOTE]);
                    backslashes = 0;
                }
                _ => {
                    quoted.extend(std::iter::repeat_n(BACKSLASH, backslashes));
                    quoted.push(unit);
                    backslashes = 0;
                }
            }
        }
        quoted.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
        quoted.push(QUOTE);
    }
    Ok(OsString::from_wide(&quoted))
}
