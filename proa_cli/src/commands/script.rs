// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{ffi::OsString, path::Path, process::Command};

use anyhow::{ensure, Context, Result};

use crate::config::ScriptsConfig;

#[cfg(unix)]
mod unix;

pub(crate) fn shell_entries(scripts: &ScriptsConfig) -> impl Iterator<Item = (&'static str, &str)> {
    [
        ("dev", scripts.dev.as_str()),
        ("build", scripts.build.as_str()),
    ]
    .into_iter()
    .chain((!scripts.css.trim().is_empty()).then_some(("css", scripts.css.as_str())))
}

pub(super) fn run(root: &Path, name: &str, source: &str, args: &[OsString]) -> Result<()> {
    let mut process = command(root, name, source, args)?;
    #[cfg(unix)]
    let code = unix::run(&mut process).with_context(|| format!("run script `{name}`"))?;
    #[cfg(not(unix))]
    let code = process
        .status()
        .with_context(|| format!("run script `{name}`"))?
        .code()
        .unwrap_or(1);
    if code != 0 {
        std::process::exit(code);
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

    let mut script = rewrite_windows_proa_invocations(source);
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

#[cfg(any(windows, test))]
fn rewrite_windows_proa_invocations(source: &str) -> String {
    let mut output = String::with_capacity(source.len() + 32);
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut in_quotes = false;
    let mut command_start = true;
    while index < bytes.len() {
        let rest = &source[index..];
        if !in_quotes && command_start {
            if let Some(after) = rest.strip_prefix("proa") {
                if after.is_empty()
                    || after.starts_with(|c: char| {
                        c.is_whitespace() || matches!(c, '&' | '|' | ')' | '<' | '>')
                    })
                {
                    output.push_str("\"%PROA_SCRIPT_EXECUTABLE%\"");
                    index += "proa".len();
                    command_start = false;
                    continue;
                }
            }
        }
        let ch = rest.chars().next().expect("non-empty rest");
        output.push(ch);
        index += ch.len_utf8();
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                command_start = false;
            }
            c if !in_quotes && c.is_whitespace() => {}
            '&' | '|' | '(' if !in_quotes => command_start = true,
            _ if !in_quotes => command_start = false,
            _ => {}
        }
    }
    output
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

#[cfg(test)]
mod tests {
    use super::rewrite_windows_proa_invocations;

    #[test]
    fn windows_rewrites_proa_across_chained_commands() {
        assert_eq!(
            rewrite_windows_proa_invocations("proa dev"),
            "\"%PROA_SCRIPT_EXECUTABLE%\" dev"
        );
        assert_eq!(
            rewrite_windows_proa_invocations("python3 scripts/gen.py && proa dev --watch content"),
            "python3 scripts/gen.py && \"%PROA_SCRIPT_EXECUTABLE%\" dev --watch content"
        );
        assert_eq!(
            rewrite_windows_proa_invocations("echo \"proa dev\" || (proa build)"),
            "echo \"proa dev\" || (\"%PROA_SCRIPT_EXECUTABLE%\" build)"
        );
        assert_eq!(
            rewrite_windows_proa_invocations("proa_helper && echo proa"),
            "proa_helper && echo proa"
        );
    }
}
