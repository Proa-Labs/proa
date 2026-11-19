// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Supervise a script's process group while it owns the foreground terminal.

use std::{
    io,
    os::unix::process::{CommandExt, ExitStatusExt},
    process::{Command, ExitStatus},
    sync::atomic::{AtomicI32, Ordering::SeqCst},
};

static CHILD_GROUP: AtomicI32 = AtomicI32::new(0);
static STOP_SIGNAL: AtomicI32 = AtomicI32::new(0);

pub(super) fn run(command: &mut Command) -> io::Result<i32> {
    let _signals = SignalHandlers::install()?;
    let mut terminal = Terminal::find();
    let child = command.process_group(0).spawn()?;
    let group = child.id() as libc::pid_t;
    CHILD_GROUP.store(group, SeqCst);
    if STOP_SIGNAL.load(SeqCst) != 0 {
        terminate_group(group);
    }
    if let Some(terminal) = &mut terminal {
        terminal.give_to(group);
    }
    let status = wait_for_script(group, terminal.as_mut());
    CHILD_GROUP.store(0, SeqCst);
    let status = status?;
    match (STOP_SIGNAL.load(SeqCst), status.signal()) {
        (signal, _) if signal != 0 => Ok(128 + signal),
        (_, Some(signal)) => Ok(128 + signal),
        _ => Ok(status.code().unwrap_or(1)),
    }
}

fn wait_for_script(
    group: libc::pid_t,
    mut terminal: Option<&mut Terminal>,
) -> io::Result<ExitStatus> {
    let options = if terminal.is_some() {
        libc::WUNTRACED
    } else {
        0
    };
    loop {
        let mut status = 0;
        if unsafe { libc::waitpid(group, &mut status, options) } == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if !libc::WIFSTOPPED(status) {
            return Ok(ExitStatus::from_raw(status));
        }
        if STOP_SIGNAL.load(SeqCst) != 0 {
            terminate_group(group);
            continue;
        }
        if let Some(terminal) = terminal.as_deref_mut() {
            // I/O before the initial foreground handoff can briefly stop the
            // shell. It can resume as soon as its group owns the terminal.
            let early_io = matches!(libc::WSTOPSIG(status), libc::SIGTTIN | libc::SIGTTOU)
                && terminal.foreground() == group;
            if !early_io {
                // Let the invoking shell see the stopped job. On fg/bg,
                // resume the script with the resulting foreground status.
                terminal.restore();
                unsafe { libc::raise(libc::SIGSTOP) };
                terminal.give_to(group);
            }
        }
        unsafe { libc::kill(-group, libc::SIGCONT) };
    }
}

fn terminate_group(group: libc::pid_t) {
    if group > 0 {
        unsafe {
            libc::kill(-group, libc::SIGTERM);
            libc::kill(-group, libc::SIGCONT);
        }
    }
}

extern "C" fn relay_signal(signal: libc::c_int) {
    STOP_SIGNAL.store(signal, SeqCst);
    terminate_group(CHILD_GROUP.load(SeqCst));
}

struct SignalHandlers(Vec<(libc::c_int, libc::sigaction)>);

impl SignalHandlers {
    fn install() -> io::Result<Self> {
        let mut handlers = Self(Vec::new());
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            unsafe {
                let mut previous: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(signal, std::ptr::null(), &mut previous) == -1 {
                    return Err(io::Error::last_os_error());
                }
                if previous.sa_sigaction == libc::SIG_IGN {
                    continue;
                }
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction =
                    relay_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
                libc::sigemptyset(&mut action.sa_mask);
                if libc::sigaction(signal, &action, std::ptr::null_mut()) == -1 {
                    return Err(io::Error::last_os_error());
                }
                handlers.0.push((signal, previous));
            }
        }
        Ok(handlers)
    }
}

impl Drop for SignalHandlers {
    fn drop(&mut self) {
        for (signal, previous) in &self.0 {
            unsafe { libc::sigaction(*signal, previous, std::ptr::null_mut()) };
        }
        STOP_SIGNAL.store(0, SeqCst);
    }
}

struct Terminal {
    fd: libc::c_int,
    owner: libc::pid_t,
    active: Option<libc::pid_t>,
}

impl Terminal {
    fn find() -> Option<Self> {
        let fd = [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO]
            .into_iter()
            .find(|&fd| unsafe { libc::tcgetpgrp(fd) } > 0)?;
        Some(Self {
            fd,
            owner: unsafe { libc::getpgrp() },
            active: None,
        })
    }

    fn foreground(&self) -> libc::pid_t {
        unsafe { libc::tcgetpgrp(self.fd) }
    }

    fn give_to(&mut self, group: libc::pid_t) {
        if self.foreground() == self.owner && self.set_foreground(group) {
            self.active = Some(group);
        }
    }

    fn restore(&mut self) {
        if self
            .active
            .take()
            .is_some_and(|group| self.foreground() == group)
        {
            self.set_foreground(self.owner);
        }
    }

    fn set_foreground(&self, group: libc::pid_t) -> bool {
        // At restoration the CLI is in a background group. Block SIGTTOU
        // around tcsetpgrp so the terminal cannot suspend the supervisor.
        unsafe {
            let mut blocked = std::mem::zeroed();
            let mut previous = std::mem::zeroed();
            libc::sigemptyset(&mut blocked);
            libc::sigaddset(&mut blocked, libc::SIGTTOU);
            if libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous) != 0 {
                return false;
            }
            let success = libc::tcsetpgrp(self.fd, group) == 0;
            libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
            success
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.restore();
    }
}
