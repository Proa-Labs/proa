// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Interactive `proa dev` output: a short header, then one line per change
//! and one per outcome. Piped output and `--verbose` keep the detailed log,
//! which is what scripts, CI, and bug reports read.

use std::{
    env,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

static PRETTY: AtomicBool = AtomicBool::new(false);
static CYCLE: Mutex<Option<Cycle>> = Mutex::new(None);
static STAGE: Mutex<&str> = Mutex::new("Building");
static LOCAL: Mutex<String> = Mutex::new(String::new());

const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
pub(super) const SPIN_INTERVAL: Duration = Duration::from_millis(80);

struct Cycle {
    started: Instant,
    outcome: &'static str,
}

/// Print a detailed-log line to stdout. Silent in the interactive view.
macro_rules! detail {
    ($($argument:tt)*) => {
        if !$crate::lifecycle::dev_ui::pretty() {
            println!($($argument)*);
        }
    };
}
pub(super) use detail;

/// Choose the view once, before anything is printed.
pub(super) fn enable(verbose: bool) {
    let interactive = !verbose && io::stdout().is_terminal() && io::stderr().is_terminal();
    PRETTY.store(interactive, Ordering::Relaxed);
    if interactive {
        // Cargo still reports warnings and errors. Only its per-crate log and
        // progress bar, which the spinner replaces, are dropped.
        env::set_var("CARGO_TERM_QUIET", "true");
        begin("Ready");
    }
}

pub(super) fn pretty() -> bool {
    PRETTY.load(Ordering::Relaxed)
}

fn paint(code: &str, text: &str) -> String {
    if env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
        text.to_owned()
    } else {
        format!("\x1b[{code}m{text}\x1b[0m")
    }
}

fn elapsed(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else if seconds >= 1 {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn changed_label(paths: &[PathBuf]) -> String {
    match paths {
        [] => "build output".to_owned(),
        [path] => path.display().to_string(),
        [path, rest @ ..] => format!("{} (+{} more)", path.display(), rest.len()),
    }
}

fn begin(outcome: &'static str) {
    *CYCLE.lock().unwrap() = Some(Cycle {
        started: Instant::now(),
        outcome,
    });
}

pub(super) fn header(local: &str) {
    if !pretty() {
        return;
    }
    println!();
    println!(
        "  ⛵ {}",
        paint("1", concat!("Proa ", env!("CARGO_PKG_VERSION")))
    );
    println!("  - Local:   {local}");
    local.clone_into(&mut LOCAL.lock().unwrap());
}

/// Last header line.
pub(super) fn reload(state: &str) {
    if pretty() {
        println!("  - Reload:  {state}");
        println!();
    }
}

/// A watched change started a cycle.
pub(super) fn changed(paths: &[PathBuf]) {
    if pretty() {
        println!();
        println!("  {} {}", paint("2", "○"), changed_label(paths));
        begin("Rebuilt");
    }
}

/// Name what this cycle turned out to be when it is not a rebuild.
pub(super) fn outcome(outcome: &'static str) {
    if !pretty() {
        return;
    }
    let mut cycle = CYCLE.lock().unwrap();
    match cycle.as_mut() {
        Some(cycle) => cycle.outcome = outcome,
        None => {
            *cycle = Some(Cycle {
                started: Instant::now(),
                outcome,
            })
        }
    }
}

/// The cycle succeeded.
pub(super) fn finish() {
    if let Some(cycle) = CYCLE.lock().unwrap().take().filter(|_| pretty()) {
        println!(
            "  {} {} in {}",
            paint("32", "✓"),
            cycle.outcome,
            elapsed(cycle.started.elapsed())
        );
    }
}

/// The server answered. Name its URL only when it is not the announced one.
pub(super) fn ready(url: &str) {
    if !pretty() {
        return;
    }
    let moved = *LOCAL.lock().unwrap() != url;
    finish();
    if moved {
        println!("    {url}");
        url.clone_into(&mut LOCAL.lock().unwrap());
    }
}

/// The cycle failed and the previous server keeps running.
pub(super) fn fail(title: &str, detail: Option<&str>) {
    CYCLE.lock().unwrap().take();
    eprintln!("  {} {title}", paint("31", "✗"));
    for line in detail.into_iter().flat_map(str::lines) {
        eprintln!("    {line}");
    }
    eprintln!("    {}", paint("2", "Still serving the last good build."));
}

pub(super) fn stage(stage: &'static str) {
    *STAGE.lock().unwrap() = stage;
}

/// Redraw the one-line build spinner. The cursor returns to column zero so
/// compiler output that arrives next starts on a clean line.
pub(super) fn spin(started: Instant) {
    let since = started.elapsed();
    let frame = FRAMES[(since.as_millis() / SPIN_INTERVAL.as_millis()) as usize % FRAMES.len()];
    let line = format!("{} {}s", *STAGE.lock().unwrap(), since.as_secs());
    let mut stderr = io::stderr().lock();
    let _ = write!(
        stderr,
        "\r\x1b[2K  {} {}\r",
        paint("36", &frame.to_string()),
        paint("2", &line)
    );
    let _ = stderr.flush();
}

pub(super) fn clear_spinner() {
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\r\x1b[2K");
    let _ = stderr.flush();
    stage("Building");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_at_a_glance() {
        assert_eq!(elapsed(Duration::from_millis(40)), "40ms");
        assert_eq!(elapsed(Duration::from_millis(1740)), "1.7s");
        assert_eq!(elapsed(Duration::from_secs(125)), "2m 5s");
    }

    #[test]
    fn a_change_names_its_first_path_and_counts_the_rest() {
        let paths = ["src/a.rs", "src/b.rs", "src/c.rs"].map(PathBuf::from);
        assert_eq!(changed_label(&[]), "build output");
        assert_eq!(changed_label(&paths[..1]), "src/a.rs");
        assert_eq!(changed_label(&paths), "src/a.rs (+2 more)");
    }
}
