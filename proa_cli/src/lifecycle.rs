// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use rsjs_ir::{
    abi::{AbiVersion, ProducerVersion, SemanticHash},
    build::{
        ArtifactRole, BuildToolchain, DeclaredEnvironmentInput, InvocationSpec, RequestedRootTarget,
    },
};
use rsjs_transport::TRANSPORT_FORMAT_VERSION;
use serde::Deserialize;
use sha2_accelerated::{Digest, Sha256};

mod reload;

use reload::{ReloadEvent, ReloadServer};

#[cfg(not(windows))]
use crate::rsjs::final_compile::snapshot_wrapper_executable;
use crate::{
    commands::{db, task},
    config::{load_project_config, AssetConfig, ProjectConfig},
    diagnostic::Severity,
    rsjs::build::{
        DescriptorCargoRequest, DESCRIPTOR_TARGET_POLICY_VERSION,
        DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION,
    },
    rsjs::cache::DESCRIPTOR_CACHE_SCHEMA_VERSION,
    rsjs::pipeline::{
        run_linked_cargo_cached, run_linked_cargo_cached_with_progress, DescriptorRootSelector,
        LinkedCargoOutput, LinkedCargoRequest, LinkedCargoStage,
    },
    scaffold,
};

static RSJS_BUILD_NONCE: AtomicU64 = AtomicU64::new(0);
static TEST_INTERRUPTED: AtomicBool = AtomicBool::new(false);
const RSJS_COMPILER_ABI: AbiVersion = AbiVersion::new(1, 1);
const RSJS_RUNTIME_PROTOCOL: AbiVersion = AbiVersion::new(1, 0);
const RSJS_COMPILER_PRODUCER: ProducerVersion = ProducerVersion::new(0, 2, 0);
const RSJS_WRAPPER_EXECUTABLE_FINGERPRINT_DOMAIN: &[u8] = b"proa.rsjs.wrapper-executable.v1\0";
const PERSISTENT_DESCRIPTOR_TARGET_LAYOUT_VERSION: u16 = 1;
const DEV_WATCH_POLL: Duration = Duration::from_millis(100);
/// Editors write in bursts (temp file, rename, mtime touch) and formatters
/// rewrite whole trees, so the loop waits for one quiet window before paying
/// for a Cargo cycle. Vite and webpack sit in the same 100-300ms range.
pub const DEV_WATCH_DEBOUNCE: Duration = Duration::from_millis(200);
/// Refuse debounce windows shorter than one poll interval: they would claim a
/// precision the poller cannot deliver.
const DEV_WATCH_MIN_DEBOUNCE: Duration = DEV_WATCH_POLL;
/// How long startup and restart probes wait for the server. A startup timeout
/// leaves the browser closed; a restart timeout still reloads existing tabs.
const DEV_READY_TIMEOUT: Duration = Duration::from_secs(20);

pub struct DevOptions {
    pub root: Option<PathBuf>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub open: bool,
    pub offline: bool,
    pub no_assets: bool,
    pub hot_static: bool,
    pub no_restart: bool,
    pub no_reload: bool,
    pub no_css_hot_swap: bool,
    pub reload_port: Option<u16>,
    pub debounce_ms: Option<u64>,
    pub command: Vec<String>,
}

pub struct BuildOptions {
    pub root: Option<PathBuf>,
    pub profile: Option<String>,
    pub package: Option<String>,
    pub bin: Option<String>,
    pub example: Option<String>,
    pub target: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub locked: bool,
    pub offline: bool,
    pub no_check: bool,
    pub no_assets: bool,
    pub artifact_out: Option<PathBuf>,
    pub command: Vec<String>,
}

pub struct CheckOptions {
    pub root: Option<PathBuf>,
    pub profile: Option<String>,
    pub package: Option<String>,
    pub bin: Option<String>,
    pub target: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub locked: bool,
    pub offline: bool,
}

pub struct RunOptions {
    pub root: Option<PathBuf>,
    pub profile: Option<String>,
    pub package: Option<String>,
    pub bin: Option<String>,
    pub target: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub locked: bool,
    pub offline: bool,
    pub arguments: Vec<String>,
}

#[derive(Clone)]
pub struct TestOptions {
    pub root: Option<PathBuf>,
    pub profile: Option<String>,
    pub package: Option<String>,
    pub bin: Option<String>,
    pub test: Option<String>,
    pub target: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub locked: bool,
    pub offline: bool,
    pub watch: bool,
    pub keep_database: bool,
    pub arguments: Vec<String>,
}

#[derive(Debug)]
struct LinkedDevLaunch {
    executable: PathBuf,
    arguments: Vec<OsString>,
    plan_id: String,
    descriptor_artifacts: usize,
    reused_descriptor_artifacts: usize,
    reused_final_artifacts: usize,
}

struct LinkedDevRebuild {
    config: ProjectConfig,
    watch_paths: Vec<PathBuf>,
    snapshot: WatchSnapshot,
    launch: LinkedDevLaunch,
    hot_state: Option<crate::hot_static::HotState>,
}

pub fn dev(options: DevOptions) -> Result<()> {
    let root = absolute_project_root(scaffold::resolve_root(options.root.clone())?)?;
    let config = load_dev_config(&root, &options)?;
    // Reject an impossible watch window before anything is built, bound, or
    // spawned. A flag error should cost nothing to recover from.
    let debounce = dev_watch_debounce(&options)?;
    install_dev_signal_handler();

    println!("Proa dev root: {}", root.display());
    println!(
        "Proa dev URL: {}",
        dev_url(&config.dev.host, config.dev.port)
    );
    println!("Building your site. The first build downloads dependencies and compiles crates; this can take several minutes.");
    if options.open {
        println!("The browser will open when the server is ready.");
    }
    task::run_lifecycle_tasks(&root, &config, &config.dev.before, &BTreeMap::new())?;

    if options.no_restart {
        if !options.no_assets {
            build_assets(&root, &config)?;
        }
        let hot_state = hot_scan_or_rustc_diagnostics(
            &root,
            &config.dev.command,
            None,
            options
                .hot_static
                .then(|| crate::hot_static::HotState::prepare(&root))
                .transpose(),
        )?;
        let launch = run_linked_dev_cycle(&root, &config.dev.command, hot_state.as_ref())?;
        return run_linked_dev_foreground(
            &root,
            &launch,
            &config.dev.host,
            config.dev.port,
            options.open,
        );
    }

    // Live reload is a watch-mode feature. Without restarts there is nothing
    // to announce, so a one-shot run never pays for the event server.
    let reload = start_reload_server(&config, &options);
    let session = reload
        .as_ref()
        .map(|server| (server.port(), server.token().to_string()));

    if !options.no_assets {
        build_assets(&root, &config)?;
    }
    let mut watch_paths = config.dev.watch.clone();
    let baseline = watch_snapshot(&root, &watch_paths)?;
    let mut hot_state = hot_scan_or_rustc_diagnostics(
        &root,
        &config.dev.command,
        None,
        options
            .hot_static
            .then(|| crate::hot_static::HotState::prepare(&root))
            .transpose(),
    )?;
    let launch = run_linked_dev_cycle(&root, &config.dev.command, hot_state.as_ref())?;
    let mut child = spawn_linked_dev(
        &root,
        &launch,
        &config.dev.host,
        config.dev.port,
        session.as_ref(),
    )?;
    set_active_child_group(&child);
    announce_dev_start(options.open, &config.dev.host, config.dev.port, &mut child);

    let clock = Instant::now();
    let mut debouncer = DevWatchDebouncer::new(baseline, debounce);

    loop {
        if let Some(status) = child.try_wait().context("poll linked dev server")? {
            clear_active_child_group();
            return exit_status_to_result(status, "linked dev server");
        }

        thread::sleep(debouncer.next_poll_delay(clock.elapsed()));
        let current = watch_snapshot(&root, &watch_paths)?;
        let DevWatchDecision::Ready(trigger_snapshot) = debouncer.observe(clock.elapsed(), current)
        else {
            continue;
        };

        // A stylesheet edit changes no compiled input, so rebuilding assets
        // and letting tabs re-fetch their stylesheets is both faster than a
        // Cargo cycle and less destructive: scroll position, form state, and
        // open dialogs all survive. This is Vite's CSS update path.
        let changed = changed_paths(&debouncer.committed, &trigger_snapshot);
        if !options.no_css_hot_swap && !options.no_assets && is_stylesheet_only_change(&changed) {
            println!("stylesheet change stabilized; rebuilding assets without restarting");
            notify(reload.as_ref(), &ReloadEvent::Building);
            let assets = (|| -> Result<ProjectConfig> {
                let next_config = load_dev_config(&root, &options)?;
                task::run_lifecycle_tasks(
                    &root,
                    &next_config,
                    &next_config.dev.before,
                    &BTreeMap::new(),
                )?;
                build_assets(&root, &next_config)?;
                Ok(next_config)
            })();
            match assets {
                Ok(next_config) => {
                    watch_paths = next_config.dev.watch.clone();
                    // Commit the state that triggered this cycle, not one read
                    // after the asset command finished. A stylesheet saved
                    // while Tailwind was running would otherwise be folded in
                    // as already handled and never rebuilt.
                    debouncer.commit(trigger_snapshot);
                    notify(reload.as_ref(), &ReloadEvent::Css);
                }
                Err(error) => {
                    eprintln!("asset rebuild failed; keeping the last built assets:\n{error:#}");
                    notify(reload.as_ref(), &ReloadEvent::Failed(format!("{error:#}")));
                    debouncer.commit(trigger_snapshot);
                }
            }
            continue;
        }

        if let Some(state) = &hot_state {
            match state.try_update(&root, &changed) {
                Ok(true) => {
                    println!(
                        "static template change stabilized; published without Cargo or restart"
                    );
                    // Class edits are the common hot case, and source-scanning
                    // generators (Tailwind) must see them before the reload.
                    let assets = (|| -> Result<ProjectConfig> {
                        let next_config = load_dev_config(&root, &options)?;
                        task::run_lifecycle_tasks(
                            &root,
                            &next_config,
                            &next_config.dev.before,
                            &BTreeMap::new(),
                        )?;
                        if !options.no_assets {
                            build_assets(&root, &next_config)?;
                        }
                        Ok(next_config)
                    })();
                    debouncer.commit(trigger_snapshot);
                    match assets {
                        Ok(next_config) => {
                            watch_paths = next_config.dev.watch.clone();
                            notify(reload.as_ref(), &ReloadEvent::Reload);
                        }
                        Err(error) => {
                            eprintln!(
                                "asset rebuild failed; keeping the last built assets:\n{error:#}"
                            );
                            notify(reload.as_ref(), &ReloadEvent::Failed(format!("{error:#}")));
                        }
                    }
                    continue;
                }
                Ok(false) => {}
                Err(error) => {
                    eprintln!("static update rejected; using the linked rebuild: {error:#}")
                }
            }
        }
        println!("change stabilized; rebuilding linked RSJS dev server");
        notify(reload.as_ref(), &ReloadEvent::Building);
        let rebuild = (|| -> Result<LinkedDevRebuild> {
            let next_config = load_dev_config(&root, &options)?;
            task::run_lifecycle_tasks(
                &root,
                &next_config,
                &next_config.dev.before,
                &BTreeMap::new(),
            )?;
            if !options.no_assets {
                build_assets(&root, &next_config)?;
            }
            let next_watch_paths = next_config.dev.watch.clone();
            // Capture the exact source/config state handed to this cycle after
            // asset generation. A change during Cargo will differ on the next
            // poll and schedule another cycle rather than being swallowed.
            let cycle_snapshot = watch_snapshot(&root, &next_watch_paths)?;
            // Stage after tasks/assets may have generated Rust sources,
            // and after the committed snapshot. An edit during staging or
            // Cargo must differ from that snapshot on the next poll.
            let next_hot_state = hot_scan_or_rustc_diagnostics(
                &root,
                &next_config.dev.command,
                hot_state.as_ref(),
                hot_state
                    .as_ref()
                    .map(|state| state.candidate(&root))
                    .transpose(),
            )?;
            let launch =
                run_linked_dev_cycle(&root, &next_config.dev.command, next_hot_state.as_ref())?;
            Ok(LinkedDevRebuild {
                config: next_config,
                watch_paths: next_watch_paths,
                snapshot: cycle_snapshot,
                launch,
                hot_state: next_hot_state,
            })
        })();

        match rebuild {
            Ok(LinkedDevRebuild {
                config: next_config,
                watch_paths: next_watch_paths,
                snapshot: cycle_snapshot,
                launch: next_launch,
                hot_state: mut next_hot_state,
            }) => {
                println!(
                    "linked dev rebuild succeeded; gracefully stopping the previous server before activating plan {}",
                    next_launch.plan_id
                );
                stop_child(&mut child)?;
                clear_active_child_group();
                if let Some(state) = &mut next_hot_state {
                    state.activate()?;
                }
                child = spawn_linked_dev(
                    &root,
                    &next_launch,
                    &next_config.dev.host,
                    next_config.dev.port,
                    session.as_ref(),
                )?;
                set_active_child_group(&child);
                hot_state = next_hot_state;
                watch_paths = next_watch_paths;
                debouncer = DevWatchDebouncer::new(cycle_snapshot, debounce);
                announce_reload(
                    reload.as_ref(),
                    &next_config.dev.host,
                    next_config.dev.port,
                    &mut child,
                );
            }
            Err(error) => {
                eprintln!(
                    "linked dev rebuild failed; keeping the last valid server running:\n{error:#}"
                );
                notify(reload.as_ref(), &ReloadEvent::Failed(format!("{error:#}")));
                // Do not spin on unchanged broken input. The next source,
                // feature, cfg, or configuration change schedules a new cycle.
                debouncer.commit(trigger_snapshot);
            }
        }
    }
}

fn start_reload_server(config: &ProjectConfig, options: &DevOptions) -> Option<ReloadServer> {
    if options.no_reload {
        println!("Proa dev live reload: off");
        return None;
    }

    // Zero asks the operating system for a free port. A fixed offset from the
    // dev port would quietly claim the port the developer's next site wants,
    // and that site would then fail to bind with an error naming neither live
    // reload nor the process holding it. Tabs do not need the port to be
    // stable: one whose stream endpoint has moved reloads itself and picks up
    // the current one from the page.
    let requested = options.reload_port.unwrap_or_default();
    if requested != 0 && requested == config.dev.port {
        eprintln!(
            "Proa dev live reload: off (--reload-port {requested} is the port the site itself uses)"
        );
        return None;
    }
    match ReloadServer::bind(&config.dev.host, requested) {
        Ok(server) => {
            println!("Proa dev live reload: on (port {})", server.port());
            Some(server)
        }
        Err(error) => {
            // A dev loop that still rebuilds is far more useful than one that
            // refuses to start because a port was unavailable.
            eprintln!("Proa dev live reload: off ({error:#})");
            None
        }
    }
}

fn dev_watch_debounce(options: &DevOptions) -> Result<Duration> {
    let Some(requested) = options.debounce_ms else {
        return Ok(DEV_WATCH_DEBOUNCE);
    };
    let requested = Duration::from_millis(requested);
    if requested < DEV_WATCH_MIN_DEBOUNCE {
        bail!(
            "`--debounce-ms {}` is below the {}ms file-watch poll interval; use {}ms or more",
            requested.as_millis(),
            DEV_WATCH_POLL.as_millis(),
            DEV_WATCH_MIN_DEBOUNCE.as_millis()
        );
    }
    Ok(requested)
}

fn notify(reload: Option<&ReloadServer>, event: &ReloadEvent) {
    if let Some(reload) = reload {
        reload.broadcast(event);
    }
}

fn dev_url(host: &str, port: u16) -> String {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    match host.trim_matches(['[', ']']).parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => {
            let address = if address.is_unspecified() {
                Ipv4Addr::LOCALHOST
            } else {
                address
            };
            format!("http://{address}:{port}")
        }
        Ok(IpAddr::V6(address)) => {
            let address = if address.is_unspecified() {
                Ipv6Addr::LOCALHOST
            } else {
                address
            };
            format!("http://[{address}]:{port}")
        }
        Err(_) => format!("http://{host}:{port}"),
    }
}

fn announce_dev_start(open: bool, host: &str, port: u16, child: &mut Child) {
    let url = dev_url(host, port);
    println!("Waiting for the server to accept connections at {url}...");
    let mut still_running = || matches!(child.try_wait(), Ok(None));
    if reload::wait_until_serving(host, port, DEV_READY_TIMEOUT, &mut still_running) {
        println!("Proa dev ready: {url}");
        if open {
            open_url(&url);
        }
    } else if let Ok(Some(status)) = child.try_wait() {
        eprintln!("Proa dev server exited before accepting connections ({status}).");
    } else {
        eprintln!("Proa dev server did not accept a connection within {}s at {url}; the server process is still running.", DEV_READY_TIMEOUT.as_secs());
        if open {
            eprintln!("The browser was not opened. Open {url} once the server is ready.");
        }
    }
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    };
    if let Err(error) = command.arg(url).spawn() {
        eprintln!("Could not open the browser: {error}. Open {url} manually.");
    }
}

/// Tell tabs to reload, preferably once the replacement server answers.
///
/// Reloading into a connection-refused error page is the failure mode that
/// makes naive live reload feel broken, so readiness is confirmed first.
fn announce_reload(reload: Option<&ReloadServer>, host: &str, port: u16, child: &mut Child) {
    let Some(reload) = reload else { return };

    let mut still_running = || !matches!(child.try_wait(), Ok(Some(_)));
    if reload::wait_until_serving(host, port, DEV_READY_TIMEOUT, &mut still_running) {
        reload.broadcast(&ReloadEvent::Reload);
        return;
    }

    // A server that exits on boot is a result the developer needs to see.
    if let Ok(Some(status)) = child.try_wait() {
        let detail = format!("the rebuilt server exited with {status}");
        eprintln!("live reload: {detail}");
        reload.broadcast(&ReloadEvent::Failed(detail));
        return;
    }

    // The process is alive but the probe never connected. The build did
    // succeed, so this is far more likely to be a readiness check looking at
    // the wrong address than a broken server, and refusing to reload would
    // leave a stale page with no explanation. Reload and say what happened.
    eprintln!(
        "live reload: {host}:{port} did not accept a connection within {}s; reloading anyway",
        DEV_READY_TIMEOUT.as_secs()
    );
    reload.broadcast(&ReloadEvent::Reload);
}

/// Paths whose stamp differs between two snapshots, including deletions.
fn changed_paths(before: &WatchSnapshot, after: &WatchSnapshot) -> Vec<PathBuf> {
    let mut changed: Vec<PathBuf> = after
        .iter()
        .filter(|(path, stamp)| before.get(*path) != Some(*stamp))
        .map(|(path, _)| path.clone())
        .collect();
    changed.extend(
        before
            .keys()
            .filter(|path| !after.contains_key(*path))
            .cloned(),
    );
    changed
}

/// Whether a change set can be served by rebuilding assets alone.
///
/// Only stylesheets qualify. Anything else, including a Tailwind class edited
/// inside a Rust template, changes a compiled input and needs a real rebuild.
fn is_stylesheet_only_change(changed: &[PathBuf]) -> bool {
    !changed.is_empty()
        && changed.iter().all(|path| {
            path.extension()
                .and_then(OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("css"))
        })
}

fn load_dev_config(root: &Path, options: &DevOptions) -> Result<ProjectConfig> {
    let mut config = load_project_config(root)?;
    apply_dev_overrides(&mut config, options);
    Ok(config)
}

// The exact final server runs in its own process group so restarts can stop
// its whole subprocess tree. That also detaches it from the terminal's
// foreground group, so Ctrl-C / SIGTERM on `proa dev` must be forwarded to
// the child group by hand or the server would outlive the CLI and keep the
// port bound.
#[cfg(unix)]
static ACTIVE_CHILD_GROUP: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

fn set_active_child_group(child: &Child) {
    #[cfg(unix)]
    ACTIVE_CHILD_GROUP.store(child.id() as i32, std::sync::atomic::Ordering::SeqCst);
    #[cfg(not(unix))]
    let _ = child;
}

fn clear_active_child_group() {
    #[cfg(unix)]
    ACTIVE_CHILD_GROUP.store(0, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(unix)]
extern "C" fn dev_signal_handler(signal: libc::c_int) {
    let pgid = ACTIVE_CHILD_GROUP.load(std::sync::atomic::Ordering::SeqCst);
    if pgid > 0 {
        unsafe { libc::kill(-pgid, libc::SIGTERM) };
    }
    unsafe { libc::_exit(128 + signal) };
}

fn install_dev_signal_handler() {
    #[cfg(unix)]
    unsafe {
        libc::signal(
            libc::SIGINT,
            dev_signal_handler as extern "C" fn(libc::c_int) as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            dev_signal_handler as extern "C" fn(libc::c_int) as libc::sighandler_t,
        );
    }
}

pub fn build(options: BuildOptions) -> Result<()> {
    let root = absolute_project_root(scaffold::resolve_root(options.root.clone())?)?;
    let config = load_project_config(&root)?;
    let command = build_command(&config, &options);
    // Validate and resolve the exact Cargo root before running configured asset
    // commands. An unsupported deployment command must fail without partially
    // executing a build.
    let prepared = prepare_linked_cargo(&root, &command, LinkedCargoAction::Build)?;
    let artifact_destination = options
        .artifact_out
        .as_deref()
        .map(|destination| resolve_artifact_output(&root, destination));
    if let Some(destination) = artifact_destination.as_deref() {
        warn_if_artifact_output_enters_source_closure(
            destination,
            &prepared.package_source_root,
            &prepared.request.descriptor.source_exclusion_directory,
        );
    }

    task::run_lifecycle_tasks(&root, &config, &config.build.before, &BTreeMap::new())?;
    if !options.no_check {
        run_project_check(&root)?;
    }
    if !options.no_assets {
        build_assets(&root, &config)?;
    }

    println!("running linked RSJS Cargo build: {}", shellish(&command));
    let output = run_linked_cargo_cached(&prepared.request, &prepared.descriptor_cache_directory)
        .context("linked RSJS build failed")?;
    print_linked_cache_reuse(LinkedCargoAction::Build, &output);
    print_linked_cargo_output(LinkedCargoAction::Build, &prepared.request, &output);
    if let Some(destination) = artifact_destination.as_deref() {
        let source =
            select_final_root_executable(&prepared.request, &output, LinkedCargoAction::Build)?;
        copy_executable_atomically(&source, destination).with_context(|| {
            format!(
                "failed to export linked executable `{}` to `{}`",
                source.display(),
                destination.display()
            )
        })?;
        println!(
            "exported linked RSJS executable to {}",
            destination.display()
        );
    }
    Ok(())
}

pub fn check(mut options: CheckOptions) -> Result<()> {
    let root = absolute_project_root(scaffold::resolve_root(options.root.clone())?)?;
    if options.bin.is_none() {
        let config = load_project_config(&root)?;
        options.bin = config
            .application
            .as_ref()
            .and_then(|application| application.get("binary"))
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
    }
    let command = check_command(&options);
    let prepared = prepare_linked_cargo(&root, &command, LinkedCargoAction::Check)?;

    println!("running linked RSJS Cargo check: {}", shellish(&command));
    let output = run_linked_cargo_cached(&prepared.request, &prepared.descriptor_cache_directory)
        .context("linked RSJS check failed")?;
    print_linked_cache_reuse(LinkedCargoAction::Check, &output);
    print_linked_cargo_output(LinkedCargoAction::Check, &prepared.request, &output);
    Ok(())
}

pub fn run(options: RunOptions) -> Result<()> {
    let root = absolute_project_root(scaffold::resolve_root(options.root.clone())?)?;
    let command = run_command(&options);
    run_linked_execute(&root, &command, LinkedCargoAction::Run)
}

pub fn test(mut options: TestOptions) -> Result<()> {
    install_test_signal_handler();
    TEST_INTERRUPTED.store(false, Ordering::SeqCst);
    if options.bin.is_some() && options.test.is_some() {
        bail!(
            "`cargo proa test` requires exactly one target selector: use either `--bin <name>` or `--test <name>`"
        );
    }
    let root = absolute_project_root(scaffold::resolve_root(options.root.clone())?)?;
    let explicit_target = options.bin.is_some() || options.test.is_some();
    if options.bin.is_none() && options.test.is_none() {
        let config = load_project_config(&root)?;
        options.bin = config
            .application
            .as_ref()
            .and_then(|application| application.get("binary"))
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
        if options.bin.is_none() {
            bail!(
                "`cargo proa test` requires exactly one target selector: use either `--bin <name>` or `--test <name>`"
            );
        }
    }
    let mut commands = vec![test_command(&options)];
    if !explicit_target {
        let prepared = prepare_linked_cargo(&root, &commands[0], LinkedCargoAction::Test)?;
        for test in discover_integration_test_targets(&prepared.package_source_root)? {
            let mut integration = options.clone();
            integration.bin = None;
            integration.test = Some(test);
            commands.push(test_command(&integration));
        }
    }
    let config = load_project_config(&root)?;
    if options.watch {
        run_watched_tests(&root, &config, &commands, options.keep_database)
    } else {
        run_test_session(&root, &config, &commands, options.keep_database)
    }
}

fn discover_integration_test_targets(package_source_root: &Path) -> Result<Vec<String>> {
    let directory = package_source_root.join("tests");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", directory.display())),
    };
    let mut targets = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let is_rust_file = path.is_file() && path.extension() == Some(OsStr::new("rs"));
        let is_directory_target = path.is_dir() && path.join("main.rs").is_file();
        if !is_rust_file && !is_directory_target {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(OsStr::to_str)
            .context("integration-test target names must be UTF-8")?;
        targets.push(name.to_string());
    }
    targets.sort();
    Ok(targets)
}

#[derive(Debug)]
struct IsolatedTestDatabase {
    run_directory: PathBuf,
    path: PathBuf,
}

impl IsolatedTestDatabase {
    fn create(root: &Path) -> Result<Self> {
        let run_directory = root.join("target/proa-tests").join(fresh_build_nonce());
        fs::create_dir_all(&run_directory)
            .with_context(|| format!("create {}", run_directory.display()))?;
        let database = Self {
            path: run_directory.join("test.db"),
            run_directory,
        };
        if let Err(error) = db::prepare_test_database(root, &database.path) {
            let _ = db::remove_test_database(&database.path);
            let _ = fs::remove_dir(&database.run_directory);
            return Err(error).context("prepare isolated test database");
        }
        println!("Test database  {}", database.path.display());
        Ok(database)
    }

    fn url(&self) -> OsString {
        OsString::from(format!("sqlite://{}", self.path.display()))
    }

    fn reset(&self, root: &Path) -> Result<()> {
        db::remove_test_database(&self.path)?;
        db::prepare_test_database(root, &self.path)
    }

    fn cleanup(&self) -> Result<()> {
        db::remove_test_database(&self.path)?;
        match fs::remove_dir(&self.run_directory) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("remove {}", self.run_directory.display()))
            }
        }
    }
}

fn project_uses_sqlite(config: &ProjectConfig) -> bool {
    config
        .database
        .as_ref()
        .and_then(|database| database.get("driver"))
        .and_then(serde_json::Value::as_str)
        == Some("sqlite")
}

fn run_test_session(
    root: &Path,
    config: &ProjectConfig,
    commands: &[Vec<String>],
    keep_database: bool,
) -> Result<()> {
    let database = project_uses_sqlite(config)
        .then(|| IsolatedTestDatabase::create(root))
        .transpose()?;
    if database.is_none() && keep_database {
        eprintln!("No SQLite capability is configured; --keep-database is a no-op.");
    }
    let result = (|| {
        let environment = test_environment(root, database.as_ref())?;
        run_test_cycle(root, config, commands, &environment)
    })();
    finish_test_database(database.as_ref(), keep_database && result.is_err())?;
    result
}

fn run_watched_tests(
    root: &Path,
    config: &ProjectConfig,
    commands: &[Vec<String>],
    keep_database: bool,
) -> Result<()> {
    let database = project_uses_sqlite(config)
        .then(|| IsolatedTestDatabase::create(root))
        .transpose()?;
    if database.is_none() && keep_database {
        eprintln!("No SQLite capability is configured; --keep-database is a no-op.");
    }
    let result = watched_test_loop(root, config, commands, database.as_ref());
    let preserve = keep_database
        && match &result {
            Ok(last_failed) => *last_failed,
            Err(_) => true,
        };
    finish_test_database(database.as_ref(), preserve)?;
    result.map(|_| ())
}

fn watched_test_loop(
    root: &Path,
    config: &ProjectConfig,
    commands: &[Vec<String>],
    database: Option<&IsolatedTestDatabase>,
) -> Result<bool> {
    let mut paths = config.dev.watch.clone();
    let migrations = config
        .database
        .as_ref()
        .and_then(|database| database.get("migrations"))
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("migrations"));
    if !paths.contains(&migrations) {
        paths.push(migrations.clone());
    }
    for path in [PathBuf::from(".env"), PathBuf::from(".env.test")] {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let mut baseline = watch_snapshot(root, &paths)?;
    let mut last_failed;

    loop {
        let environment = test_environment(root, database)?;
        match run_test_cycle(root, config, commands, &environment) {
            Ok(()) => last_failed = false,
            Err(error) => {
                last_failed = true;
                eprintln!("test run failed; watching for changes:\n{error:#}");
            }
        }
        if TEST_INTERRUPTED.load(Ordering::SeqCst) {
            break;
        }
        println!("Watching for test input changes · press Ctrl-C to stop");
        let changed = loop {
            thread::sleep(Duration::from_millis(200));
            if TEST_INTERRUPTED.load(Ordering::SeqCst) {
                break Vec::new();
            }
            let snapshot = watch_snapshot(root, &paths)?;
            let changed = changed_paths(&baseline, &snapshot);
            if !changed.is_empty() {
                thread::sleep(DEV_WATCH_DEBOUNCE);
                baseline = watch_snapshot(root, &paths)?;
                break changed;
            }
        };
        if TEST_INTERRUPTED.load(Ordering::SeqCst) {
            break;
        }
        if changed.iter().any(|path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .starts_with(&migrations)
        }) {
            if let Some(database) = &database {
                database.reset(root)?;
                println!("Test database reset after migration change");
            }
        }
    }
    Ok(last_failed)
}

fn run_test_cycle(
    root: &Path,
    config: &ProjectConfig,
    commands: &[Vec<String>],
    environment: &BTreeMap<OsString, OsString>,
) -> Result<()> {
    if TEST_INTERRUPTED.load(Ordering::SeqCst) {
        bail!("test interrupted");
    }
    let before = config
        .test
        .as_ref()
        .and_then(|test| test.get("before"))
        .and_then(serde_json::Value::as_array)
        .map(|tasks| {
            tasks
                .iter()
                .map(|task| {
                    task.as_str()
                        .map(ToString::to_string)
                        .context("test.before entries must be task names")
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    task::run_lifecycle_tasks(root, config, &before, environment)?;
    for command in commands {
        run_linked_execute_with_environment(root, command, LinkedCargoAction::Test, environment)?;
    }
    Ok(())
}

fn finish_test_database(database: Option<&IsolatedTestDatabase>, preserve: bool) -> Result<()> {
    let Some(database) = database else {
        return Ok(());
    };
    if preserve {
        println!(
            "Preserved failing test database at {}",
            database.path.display()
        );
        Ok(())
    } else {
        database.cleanup()
    }
}

fn test_environment(
    root: &Path,
    database: Option<&IsolatedTestDatabase>,
) -> Result<BTreeMap<OsString, OsString>> {
    let mut loaded = BTreeMap::<String, String>::new();
    merge_test_env_file(&root.join(".env"), &mut loaded)?;
    merge_test_env_file(&root.join(".env.test"), &mut loaded)?;
    let mut environment = loaded
        .into_iter()
        .filter(|(name, _)| env::var_os(name).is_none())
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect::<BTreeMap<_, _>>();
    if let Some(database) = database {
        environment.insert(OsString::from("DATABASE_URL"), database.url());
    }
    Ok(environment)
}

fn merge_test_env_file(path: &Path, values: &mut BTreeMap<String, String>) -> Result<()> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    for (index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (name, value) = line.split_once('=').with_context(|| {
            format!("{}:{} is not a NAME=value entry", path.display(), index + 1)
        })?;
        let name = name.trim();
        if name.is_empty()
            || !name.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || index > 0 && byte.is_ascii_digit()
            })
        {
            bail!(
                "{}:{} has an invalid environment name",
                path.display(),
                index + 1
            );
        }
        values.insert(
            name.to_string(),
            value.trim().trim_matches(['\'', '"']).to_string(),
        );
    }
    Ok(())
}

#[cfg(unix)]
extern "C" fn test_signal_handler(_signal: libc::c_int) {
    TEST_INTERRUPTED.store(true, Ordering::SeqCst);
}

fn install_test_signal_handler() {
    #[cfg(unix)]
    unsafe {
        libc::signal(
            libc::SIGINT,
            test_signal_handler as extern "C" fn(libc::c_int) as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            test_signal_handler as extern "C" fn(libc::c_int) as libc::sighandler_t,
        );
    }
}

fn run_linked_execute(root: &Path, command: &[String], action: LinkedCargoAction) -> Result<()> {
    run_linked_execute_with_environment(root, command, action, &BTreeMap::new())
}

fn run_linked_execute_with_environment(
    root: &Path,
    command: &[String],
    action: LinkedCargoAction,
    environment: &BTreeMap<OsString, OsString>,
) -> Result<()> {
    let prepared = prepare_linked_cargo_with_environment(root, command, action, environment)?;
    println!(
        "running linked RSJS Cargo {}: {}",
        action.command_name(),
        shellish(command)
    );
    let output = run_linked_cargo_cached(&prepared.request, &prepared.descriptor_cache_directory)
        .with_context(|| format!("linked RSJS {} failed", action.command_name()))?;
    print_linked_cache_reuse(action, &output);
    print_linked_cargo_output(action, &prepared.request, &output);
    Ok(())
}

fn print_linked_cache_reuse(action: LinkedCargoAction, output: &LinkedCargoOutput) {
    let descriptor_artifacts = output.descriptor.cargo_artifacts.len();
    let reused_descriptor_artifacts = output
        .descriptor
        .cargo_artifacts
        .iter()
        .filter(|artifact| artifact.fresh)
        .count();
    let final_artifacts = output.final_cargo.cargo_artifacts.len();
    let reused_final_artifacts = output
        .final_cargo
        .cargo_artifacts
        .iter()
        .filter(|artifact| artifact.fresh)
        .count();
    println!(
        "linked RSJS {} cache: descriptor {reused_descriptor_artifacts}/{descriptor_artifacts} Cargo artifacts reused; final {reused_final_artifacts}/{final_artifacts} reused",
        action.command_name(),
    );
}

/// Hot-template scanning parses every source file with syn, whose syntax
/// errors are far less useful than rustc's. When the scan fails, run Cargo
/// with the active manifest anyway so a half-written file reports rustc's
/// diagnostics; the scan error surfaces only if Cargo accepted the source.
fn hot_scan_or_rustc_diagnostics<T>(
    root: &Path,
    command: &[String],
    active: Option<&crate::hot_static::HotState>,
    scan: Result<T>,
) -> Result<T> {
    scan.or_else(|scan_error| {
        run_linked_dev_cycle(root, command, active)?;
        Err(scan_error)
    })
}

fn run_linked_dev_cycle(
    root: &Path,
    command: &[String],
    hot: Option<&crate::hot_static::HotState>,
) -> Result<LinkedDevLaunch> {
    let _progress = DevBuildProgress::start();
    println!("running linked RSJS Cargo dev build: {}", shellish(command));
    println!("Preparing the Cargo project and build cache...");
    let environment = hot.map(|state| state.environment()).unwrap_or_default();
    let prepared =
        prepare_linked_cargo_with_environment(root, command, LinkedCargoAction::Dev, &environment)?;
    let output = run_linked_cargo_cached_with_progress(
        &prepared.request,
        &prepared.descriptor_cache_directory,
        |stage| match stage {
            LinkedCargoStage::Descriptor => {
                println!("Building site (1/2): resolving dependencies and compiling Rust...")
            }
            LinkedCargoStage::Link => println!("Linking browser components..."),
            LinkedCargoStage::Final => {
                println!("Building site (2/2): compiling the final server...")
            }
        },
    )
    .context("linked RSJS dev build failed")?;
    let executable =
        select_final_root_executable(&prepared.request, &output, LinkedCargoAction::Dev)?;
    let descriptor_artifacts = output.descriptor.cargo_artifacts.len();
    let reused_descriptor_artifacts = output
        .descriptor
        .cargo_artifacts
        .iter()
        .filter(|artifact| artifact.fresh)
        .count();
    let reused_final_artifacts = output
        .final_cargo
        .cargo_artifacts
        .iter()
        .filter(|artifact| artifact.fresh)
        .count();
    println!(
        "linked RSJS dev plan {} (descriptor cache: {reused_descriptor_artifacts}/{descriptor_artifacts} Cargo artifacts reused; final cache: {reused_final_artifacts}/{} reused)",
        output.semantic.plan_id,
        output.final_cargo.cargo_artifacts.len(),
    );

    Ok(LinkedDevLaunch {
        executable,
        arguments: prepared.runtime_args,
        plan_id: output.semantic.plan_id.to_string(),
        descriptor_artifacts,
        reused_descriptor_artifacts,
        reused_final_artifacts,
    })
}

// Cargo can be quiet while downloading, linking, or validating cached build
// artifacts. Keep the dev terminal visibly active until the entire build ends.
struct DevBuildProgress {
    done: std::sync::mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}

impl DevBuildProgress {
    fn start() -> Self {
        use std::sync::mpsc::{self, RecvTimeoutError};

        let (done, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            let started = Instant::now();
            while matches!(
                receiver.recv_timeout(Duration::from_secs(10)),
                Err(RecvTimeoutError::Timeout)
            ) {
                eprintln!(
                    "Still building your site ({}s elapsed)...",
                    started.elapsed().as_secs()
                );
            }
        });
        Self {
            done,
            thread: Some(thread),
        }
    }
}

impl Drop for DevBuildProgress {
    fn drop(&mut self) {
        let _ = self.done.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn select_final_root_executable(
    request: &LinkedCargoRequest,
    output: &LinkedCargoOutput,
    action: LinkedCargoAction,
) -> Result<PathBuf> {
    if output.final_cargo.plan_id != output.semantic.plan_id {
        bail!(
            "linked {} final Cargo consumed plan {}, but the semantic linker produced {}",
            action.command_name(),
            output.final_cargo.plan_id,
            output.semantic.plan_id
        );
    }
    if !output.semantic_plan_path.is_file() {
        bail!(
            "linked {} semantic plan disappeared before executable selection: `{}`",
            action.command_name(),
            output.semantic_plan_path.display()
        );
    }

    let artifact = &output.final_root_artifact;
    if artifact.package_id != request.root.package_id
        || artifact.target.name != request.root.target_name
        || artifact.target.kind != request.root.target_kinds
    {
        bail!(
            "attested final root no longer matches linked {} package `{}` target `{}`",
            action.command_name(),
            request.root.package_id,
            request.root.target_name
        );
    }
    let executable = artifact.executable.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "final linked Cargo build emitted no executable for package `{}` binary `{}`",
            request.root.package_id,
            request.root.target_name
        )
    })?;
    if !executable.is_absolute() || !executable.is_file() {
        bail!(
            "final linked Cargo executable is not an existing absolute file: `{}`",
            executable.display()
        );
    }
    Ok(executable)
}

fn resolve_artifact_output(root: &Path, requested: &Path) -> PathBuf {
    if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    }
}

fn warn_if_artifact_output_enters_source_closure(
    destination: &Path,
    package_source_root: &Path,
    source_exclusion: &Path,
) {
    if artifact_output_enters_source_closure(destination, package_source_root, source_exclusion) {
        eprintln!(
            "warning[proa/artifact-out-source-closure]: artifact output `{}` is inside selected package `{}` and will participate in its next source-closure check; use the Cargo target directory or a path outside the package for warm rebuilds",
            destination.display(),
            package_source_root.display()
        );
    }
}

fn artifact_output_enters_source_closure(
    destination: &Path,
    package_source_root: &Path,
    source_exclusion: &Path,
) -> bool {
    let Some(destination) = normalize_absolute_lexical_path(destination) else {
        return destination.starts_with(package_source_root);
    };
    let Some(package_source_root) = normalize_absolute_lexical_path(package_source_root) else {
        return false;
    };
    let Some(source_exclusion) = normalize_absolute_lexical_path(source_exclusion) else {
        return false;
    };
    if !destination.starts_with(&package_source_root) || destination.starts_with(&source_exclusion)
    {
        return false;
    }
    let Ok(relative) = destination.strip_prefix(&package_source_root) else {
        return false;
    };
    let mut components = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value),
            _ => None,
        });
    let Some(first) = components.next() else {
        return true;
    };
    if matches!(
        first.to_str(),
        Some(".git" | ".hg" | ".svn" | ".jj" | "target" | "node_modules")
    ) {
        return false;
    }
    !components.any(|component| component == OsStr::new("node_modules"))
}

fn normalize_absolute_lexical_path(path: &Path) -> Option<PathBuf> {
    use std::path::Component;

    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
        }
    }
    Some(normalized)
}

fn copy_executable_atomically(source: &Path, destination: &Path) -> io::Result<()> {
    if source == destination {
        return Ok(());
    }
    if destination.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "artifact output must name a file, but `{}` is a directory",
                destination.display()
            ),
        ));
    }

    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "artifact output `{}` has no parent directory",
                destination.display()
            ),
        )
    })?;
    fs::create_dir_all(parent)?;

    let file_name = destination.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "artifact output `{}` does not name a file",
                destination.display()
            ),
        )
    })?;
    let (temporary, mut temporary_file) = loop {
        let nonce = RSJS_BUILD_NONCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{}.proa-export-{}-{nonce}",
            file_name.to_string_lossy(),
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };

    let copy_result = (|| {
        let mut source_file = fs::File::open(source)?;
        let source_permissions = source_file.metadata()?.permissions();
        io::copy(&mut source_file, &mut temporary_file)?;
        temporary_file.set_permissions(source_permissions)?;
        temporary_file.sync_all()?;
        drop(temporary_file);
        replace_file(&temporary, destination)
    })();

    if copy_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    copy_result
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn apply_dev_overrides(config: &mut ProjectConfig, options: &DevOptions) {
    if let Some(host) = &options.host {
        config.dev.host = host.clone();
    }
    if let Some(port) = options.port {
        config.dev.port = port;
    }
    if !options.command.is_empty() {
        config.dev.command = options.command.clone();
    }
    if options.offline && !config.dev.command.iter().any(|arg| arg == "--offline") {
        config.dev.command.push("--offline".to_string());
    }
}

fn build_command(config: &ProjectConfig, options: &BuildOptions) -> Vec<String> {
    let mut command = if !options.command.is_empty() {
        options.command.clone()
    } else if let Some(profile) = &options.profile {
        vec![
            "cargo".into(),
            "build".into(),
            "--profile".into(),
            profile.clone(),
        ]
    } else {
        config.build.command.clone()
    };

    if let Some(package) = &options.package {
        command.push("-p".into());
        command.push(package.clone());
    }
    if let Some(bin) = &options.bin {
        command.push("--bin".into());
        command.push(bin.clone());
    }
    if let Some(example) = &options.example {
        command.push("--example".into());
        command.push(example.clone());
    }
    if let Some(target) = &options.target {
        command.push("--target".into());
        command.push(target.clone());
    }
    if !options.features.is_empty() {
        command.push("--features".into());
        command.push(options.features.join(","));
    }
    if options.all_features {
        command.push("--all-features".into());
    }
    if options.no_default_features {
        command.push("--no-default-features".into());
    }
    if options.locked {
        command.push("--locked".into());
    }
    if options.offline {
        command.push("--offline".into());
    }

    command
}

fn check_command(options: &CheckOptions) -> Vec<String> {
    let mut command = vec!["cargo".into(), "check".into()];
    append_cargo_selection(
        &mut command,
        options.profile.as_deref(),
        options.package.as_deref(),
        options.bin.as_deref(),
        options.target.as_deref(),
        &options.features,
        options.all_features,
        options.no_default_features,
        options.locked,
        options.offline,
    );
    command
}

fn run_command(options: &RunOptions) -> Vec<String> {
    let mut command = vec!["cargo".into(), "run".into()];
    append_cargo_selection(
        &mut command,
        options.profile.as_deref(),
        options.package.as_deref(),
        options.bin.as_deref(),
        options.target.as_deref(),
        &options.features,
        options.all_features,
        options.no_default_features,
        options.locked,
        options.offline,
    );
    append_runtime_arguments(&mut command, &options.arguments);
    command
}

fn test_command(options: &TestOptions) -> Vec<String> {
    let mut command = vec!["cargo".into(), "test".into()];
    append_cargo_selection(
        &mut command,
        options.profile.as_deref(),
        options.package.as_deref(),
        options.bin.as_deref(),
        options.target.as_deref(),
        &options.features,
        options.all_features,
        options.no_default_features,
        options.locked,
        options.offline,
    );
    if let Some(test) = &options.test {
        command.push("--test".into());
        command.push(test.clone());
    }
    append_runtime_arguments(&mut command, &options.arguments);
    command
}

fn append_runtime_arguments(command: &mut Vec<String>, arguments: &[String]) {
    if !arguments.is_empty() {
        command.push("--".into());
        command.extend(arguments.iter().cloned());
    }
}

fn append_cargo_selection(
    command: &mut Vec<String>,
    profile: Option<&str>,
    package: Option<&str>,
    bin: Option<&str>,
    target: Option<&str>,
    features: &[String],
    all_features: bool,
    no_default_features: bool,
    locked: bool,
    offline: bool,
) {
    if let Some(profile) = profile {
        command.push("--profile".into());
        command.push(profile.into());
    }
    if let Some(package) = package {
        command.push("-p".into());
        command.push(package.into());
    }
    if let Some(bin) = bin {
        command.push("--bin".into());
        command.push(bin.into());
    }
    if let Some(target) = target {
        command.push("--target".into());
        command.push(target.into());
    }
    if !features.is_empty() {
        command.push("--features".into());
        command.push(features.join(","));
    }
    if all_features {
        command.push("--all-features".into());
    }
    if no_default_features {
        command.push("--no-default-features".into());
    }
    if locked {
        command.push("--locked".into());
    }
    if offline {
        command.push("--offline".into());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCargoInvocation {
    cargo: OsString,
    cargo_args: Vec<OsString>,
    runtime_args: Vec<OsString>,
    package: Option<String>,
    bin: Option<String>,
    example: Option<String>,
    test: Option<String>,
    profile: String,
    target: Option<String>,
    features: BTreeSet<String>,
    locked: bool,
    offline: bool,
    metadata_flags: Vec<OsString>,
    selection_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkedCargoAction {
    Build,
    Check,
    Dev,
    Run,
    Test,
}

impl LinkedCargoAction {
    const fn command_name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Check => "check",
            Self::Dev => "dev",
            Self::Run => "run",
            Self::Test => "test",
        }
    }

    const fn subcommand(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Check => "check",
            Self::Dev => "run",
            Self::Run => "run",
            Self::Test => "test",
        }
    }

    const fn final_subcommand(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Check => "check",
            Self::Dev => "build",
            Self::Run => "run",
            Self::Test => "test",
        }
    }

    const fn descriptor_subcommand(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Check => "check",
            Self::Build | Self::Dev | Self::Run => "build",
        }
    }

    const fn permits_runtime_arguments(self) -> bool {
        matches!(self, Self::Dev | Self::Run | Self::Test)
    }

    const fn final_cargo_executes(self) -> bool {
        matches!(self, Self::Run | Self::Test)
    }

    const fn root_role(self) -> ArtifactRole {
        match self {
            Self::Test => ArtifactRole::RootTest,
            Self::Build | Self::Check | Self::Dev | Self::Run => ArtifactRole::RootExecutable,
        }
    }

    const fn descriptor_identity_marker(self) -> &'static str {
        match self {
            Self::Test => "proa:descriptor-mode=test-no-run",
            Self::Check => "proa:descriptor-mode=check",
            Self::Build | Self::Dev | Self::Run => "proa:descriptor-mode=build",
        }
    }
}

#[derive(Debug)]
struct PreparedLinkedCargo {
    request: LinkedCargoRequest,
    runtime_args: Vec<OsString>,
    descriptor_cache_directory: PathBuf,
    package_source_root: PathBuf,
    // Windows cannot safely relocate arbitrary PE executables without a
    // complete delay-import contract. Hold a no-write/no-delete sharing lock
    // instead so the fingerprinted wrapper path cannot be replaced before
    // Cargo opens it. Other hosts execute a content-addressed snapshot.
    _wrapper_lock: Option<fs::File>,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoMetadataPackage>,
    workspace_members: Vec<String>,
    #[serde(default)]
    workspace_default_members: Vec<String>,
    target_directory: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CargoMetadataPackage {
    id: String,
    name: String,
    version: String,
    manifest_path: PathBuf,
    targets: Vec<CargoMetadataTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoMetadataTarget {
    name: String,
    kind: Vec<String>,
}

#[derive(Debug)]
struct ExactCargoRoot {
    package_id: String,
    package_name: String,
    package_source_root: PathBuf,
    target_name: String,
    target_kinds: BTreeSet<String>,
    requested_target_kind: String,
}

fn prepare_linked_cargo(
    root: &Path,
    command: &[String],
    action: LinkedCargoAction,
) -> Result<PreparedLinkedCargo> {
    prepare_linked_cargo_with_environment(root, command, action, &BTreeMap::new())
}

fn prepare_linked_cargo_with_environment(
    root: &Path,
    command: &[String],
    action: LinkedCargoAction,
    environment: &BTreeMap<OsString, OsString>,
) -> Result<PreparedLinkedCargo> {
    let wrapper = std::env::current_exe().context("resolve the cargo-proa wrapper executable")?;
    prepare_linked_cargo_with_wrapper_and_environment(root, command, action, wrapper, environment)
}

#[cfg(test)]
fn prepare_linked_cargo_with_wrapper(
    root: &Path,
    command: &[String],
    action: LinkedCargoAction,
    wrapper: PathBuf,
) -> Result<PreparedLinkedCargo> {
    prepare_linked_cargo_with_wrapper_and_environment(
        root,
        command,
        action,
        wrapper,
        &BTreeMap::new(),
    )
}

fn prepare_linked_cargo_with_wrapper_and_environment(
    root: &Path,
    command: &[String],
    action: LinkedCargoAction,
    wrapper: PathBuf,
    environment: &BTreeMap<OsString, OsString>,
) -> Result<PreparedLinkedCargo> {
    if !wrapper.is_absolute() {
        bail!(
            "the cargo-proa wrapper executable is not absolute: `{}`",
            wrapper.display()
        );
    }
    let parsed = parse_cargo_command(command, action)?;
    let mut timer = crate::rsjs::timing::StageTimer::new();
    // Query every current tool and Cargo's authoritative metadata on each
    // cycle. They are independent subprocesses; overlapping them avoids
    // adding their startup costs together without caching mutable tool state.
    let (metadata, cargo_version, rustc_version) = thread::scope(|scope| {
        let cargo = scope.spawn(|| tool_version(&parsed.cargo, &["-Vv"], "Cargo", root));
        let rustc = scope.spawn(|| selected_rustc_version(root));
        let metadata = load_cargo_metadata(root, &parsed);
        let cargo = cargo
            .join()
            .map_err(|_| anyhow::anyhow!("Cargo version query thread panicked"))
            .and_then(|result| result);
        let rustc = rustc
            .join()
            .map_err(|_| anyhow::anyhow!("rustc version query thread panicked"))
            .and_then(|result| result);
        (metadata, cargo, rustc)
    });
    let metadata = metadata?;
    timer.mark("prepare Cargo metadata and tool versions");
    let selected_root = select_exact_cargo_root(&metadata, &parsed, action)?;
    if !metadata.target_directory.is_absolute() {
        bail!(
            "Cargo metadata returned a relative target directory `{}`; the linked RSJS pipeline requires an exact absolute artifact namespace",
            metadata.target_directory.display()
        );
    }
    let wrapper_snapshot_root = metadata
        .target_directory
        .join("proa-rsjs")
        .join("wrapper-snapshots-v1");
    let (wrapper, wrapper_lock) =
        prepare_wrapper_executable(&metadata.target_directory, &wrapper_snapshot_root, &wrapper)?;
    let wrapper_fingerprint = wrapper_executable_fingerprint(&wrapper)?;
    timer.mark("prepare wrapper snapshot and fingerprint");
    let package_source_root = selected_root.package_source_root.clone();
    let cli_producer = cli_tool_producer_version()?;
    let mut selection_args = parsed.selection_args.clone();
    // A binary compiled as a test harness is a different Cargo/rustc unit from
    // the same binary compiled normally. Keep that mode in the pre-Cargo
    // identity so persistent descriptor targets and caches cannot alias.
    selection_args.push(action.descriptor_identity_marker().to_string());
    // Producer semver is insufficient during local development and rolling
    // upgrades: the same absolute executable path can be atomically replaced
    // with different wrapper behavior. Bind every persistent descriptor and
    // final Cargo namespace to the exact wrapper bytes that will be invoked.
    selection_args.push(format!(
        "proa:wrapper-executable-fingerprint={wrapper_fingerprint}"
    ));
    let invocation_spec = InvocationSpec {
        roots: BTreeSet::from([RequestedRootTarget {
            package: selected_root.package_id.clone(),
            target_kind: selected_root.requested_target_kind.clone(),
            target_name: Some(selected_root.target_name.clone()),
        }]),
        target_triple: parsed.target.clone(),
        features: parsed.features.clone(),
        profile: parsed.profile.clone(),
        cargo_config: BTreeMap::new(),
        locked: parsed.locked,
        offline: parsed.offline,
        selection_args,
        // Test fixture values are supplied identically to descriptor and final
        // Cargo below, but are not semantic target selectors. Cargo build
        // scripts and the wrapper's observed-environment sidecars still
        // invalidate units that actually consume one of these values.
        environment: declared_build_environment(),
        toolchain: BuildToolchain {
            cargo: cargo_version?,
            rustc: rustc_version?,
            proa: cli_producer,
            compiler_abi: RSJS_COMPILER_ABI,
            transport_version: TRANSPORT_FORMAT_VERSION,
        },
    };
    let invocation = invocation_spec
        .invocation_id()
        .context("derive the pre-descriptor RSJS invocation identity")?;

    let build_root = metadata
        .target_directory
        .join("proa-rsjs")
        .join(invocation.to_string());
    let source_exclusion_directory = metadata.target_directory.clone();
    let nonce = fresh_build_nonce();
    let descriptor_staging = build_root.join("descriptor-staging").join(&nonce);
    let descriptor_target = build_root.join(format!(
        "descriptor-target-policy-v{DESCRIPTOR_TARGET_POLICY_VERSION}-sidecar-v{DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION}-persistent-v{PERSISTENT_DESCRIPTOR_TARGET_LAYOUT_VERSION}"
    ));
    let descriptor_cache_directory = build_root.join(format!(
        "descriptor-cache-v{DESCRIPTOR_CACHE_SCHEMA_VERSION}"
    ));
    let plan_directory = metadata.target_directory.join("proa-rsjs").join("plans");
    let final_target_root = metadata
        .target_directory
        .join("proa-rsjs")
        .join("final-target");
    let mut descriptor_cargo_args = parsed.cargo_args.clone();
    // The wrapper adds one explicit object output even when Cargo selected
    // `check`, so linked checks can keep descriptor and final rustc unit
    // coordinates identical. Tests require their exact `cfg(test)`/harness
    // compilation, so both phases use `cargo test`; only the descriptor phase
    // adds `--no-run`.
    descriptor_cargo_args[0] = OsString::from(action.descriptor_subcommand());
    if action == LinkedCargoAction::Test {
        descriptor_cargo_args.push("--no-run".into());
    }
    let mut final_cargo_args = parsed.cargo_args.clone();
    final_cargo_args[0] = OsString::from(action.final_subcommand());
    if action.final_cargo_executes() && !parsed.runtime_args.is_empty() {
        final_cargo_args.push("--".into());
        final_cargo_args.extend(parsed.runtime_args.iter().cloned());
    }

    Ok(PreparedLinkedCargo {
        request: LinkedCargoRequest {
            descriptor: DescriptorCargoRequest {
                cargo: parsed.cargo,
                wrapper,
                working_directory: root.to_path_buf(),
                staging_directory: descriptor_staging,
                target_directory: descriptor_target,
                source_exclusion_directory,
                cargo_args: descriptor_cargo_args,
                environment: environment.clone(),
            },
            root: DescriptorRootSelector {
                package_id: selected_root.package_id,
                target_name: selected_root.target_name,
                target_kinds: selected_root.target_kinds,
                role: action.root_role(),
                artifact_path: None,
            },
            invocation,
            producer: RSJS_COMPILER_PRODUCER,
            runtime_protocol: RSJS_RUNTIME_PROTOCOL,
            plan_directory,
            final_target_root,
            final_cargo_args,
            final_environment: environment.clone(),
        },
        runtime_args: parsed.runtime_args,
        descriptor_cache_directory,
        package_source_root,
        _wrapper_lock: wrapper_lock,
    })
}

#[cfg(not(windows))]
fn prepare_wrapper_executable(
    source_exclusion: &Path,
    snapshot_root: &Path,
    wrapper: &Path,
) -> Result<(PathBuf, Option<fs::File>)> {
    let snapshot = snapshot_wrapper_executable(source_exclusion, snapshot_root, wrapper)
        .context("publish the cargo-proa wrapper into its content-addressed snapshot")?;
    Ok((snapshot, None))
}

#[cfg(windows)]
fn prepare_wrapper_executable(
    _source_exclusion: &Path,
    _snapshot_root: &Path,
    wrapper: &Path,
) -> Result<(PathBuf, Option<fs::File>)> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

    let wrapper = fs::canonicalize(wrapper)
        .with_context(|| format!("resolve cargo-proa wrapper `{}`", wrapper.display()))?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&wrapper)
        .with_context(|| {
            format!(
                "lock cargo-proa wrapper `{}` against replacement during linked compilation",
                wrapper.display()
            )
        })?;
    Ok((wrapper, Some(lock)))
}

fn wrapper_executable_fingerprint(path: &Path) -> Result<String> {
    use std::fmt::Write as _;
    use std::io::Read as _;

    let mut executable = fs::File::open(path)
        .with_context(|| format!("open cargo-proa wrapper executable `{}`", path.display()))?;
    let metadata = executable
        .metadata()
        .with_context(|| format!("inspect cargo-proa wrapper executable `{}`", path.display()))?;
    if !metadata.is_file() {
        bail!(
            "cargo-proa wrapper executable is not a regular file: `{}`",
            path.display()
        );
    }

    let mut hasher = Sha256::new();
    hasher.update(RSJS_WRAPPER_EXECUTABLE_FINGERPRINT_DOMAIN);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = executable
            .read(&mut buffer)
            .with_context(|| format!("hash cargo-proa wrapper executable `{}`", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let digest: [u8; 32] = hasher.finalize().into();
    let mut encoded = String::with_capacity("sha256:".len() + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        let _ = write!(encoded, "{byte:02x}");
    }
    Ok(encoded)
}

#[cfg(test)]
fn parse_cargo_build_command(command: &[String]) -> Result<ParsedCargoInvocation> {
    parse_cargo_command(command, LinkedCargoAction::Build)
}

fn parse_cargo_command(
    command: &[String],
    action: LinkedCargoAction,
) -> Result<ParsedCargoInvocation> {
    let Some((program, raw_args)) = command.split_first() else {
        bail!(
            "linked Cargo {} command cannot be empty",
            action.subcommand()
        );
    };
    if !is_cargo_program(program) {
        bail!(
            "`cargo proa {command}` requires a Cargo {cargo_action} command for the linked RSJS pipeline; found `{}`. Custom non-Cargo commands are not supported yet",
            shellish(command),
            command = action.command_name(),
            cargo_action = action.subcommand(),
        );
    }
    if raw_args.first().map(String::as_str) != Some(action.subcommand()) {
        bail!(
            "`cargo proa {command}` currently supports only `cargo {cargo_action}`; found `{}`. Other Cargo subcommands do not yet have linked-root semantics for this workflow",
            shellish(command),
            command = action.command_name(),
            cargo_action = action.subcommand(),
        );
    }

    let mut package = None;
    let mut bin = None;
    let mut example = None;
    let mut test = None;
    let mut explicit_profile = None;
    let mut release = false;
    let mut target = None;
    let mut features = BTreeSet::new();
    let mut locked = false;
    let mut offline = false;
    let mut metadata_flags = Vec::new();
    let mut selection_args = Vec::new();
    let mut runtime_args = Vec::new();
    let mut cargo_arg_count = raw_args.len();
    let mut index = 1;
    while index < raw_args.len() {
        let argument = &raw_args[index];
        match argument.as_str() {
            "--message-format" | "--target-dir" => {
                let value = raw_args.get(index + 1).cloned();
                bail!(
                    "Cargo argument `{argument}{}` is reserved by the linked RSJS pipeline; remove it so cargo-proa can isolate descriptor and final artifacts",
                    value
                        .as_deref()
                        .map(|value| format!(" {value}"))
                        .unwrap_or_default()
                );
            }
            value
                if value.starts_with("--message-format=") || value.starts_with("--target-dir=") =>
            {
                bail!(
                    "Cargo argument `{value}` is reserved by the linked RSJS pipeline; remove it so cargo-proa can own Cargo messages and artifact namespaces"
                );
            }
            "--workspace" | "--all" | "--exclude" | "--bins" | "--examples" | "--tests"
            | "--benches" | "--all-targets" | "--lib" | "--bench" => {
                bail!(
                    "Cargo target-selection argument `{argument}` can produce multiple or unsupported roots. The linked RSJS pipeline requires one exact package plus binary, example, or integration-test target"
                );
            }
            "--manifest-path" | "--config" | "--artifact-dir" => {
                bail!(
                    "Cargo argument `{argument}` is not supported by the first linked RSJS pipeline contract; select the project with `--root` and let cargo-proa own build configuration/output paths"
                );
            }
            value
                if value.starts_with("--manifest-path=")
                    || value.starts_with("--config=")
                    || value.starts_with("--artifact-dir=") =>
            {
                bail!(
                    "Cargo argument `{value}` is not supported by the first linked RSJS pipeline contract; select the project with `--root` and let cargo-proa own build configuration/output paths"
                );
            }
            "--" => {
                if !action.permits_runtime_arguments() {
                    bail!(
                        "arguments after `--` are supported only by `cargo proa dev`, `cargo proa run`, and `cargo proa test`; descriptor and final compiler inputs for this workflow must be derived by cargo-proa"
                    );
                }
                cargo_arg_count = index;
                runtime_args.extend(raw_args[index + 1..].iter().map(OsString::from));
                break;
            }
            "-p" | "--package" => {
                let value = required_cargo_value(raw_args, &mut index, argument)?;
                set_once(&mut package, value, "Cargo package")?;
            }
            value if value.starts_with("--package=") => {
                set_once(
                    &mut package,
                    nonempty_option_value(value, "--package=")?,
                    "Cargo package",
                )?;
            }
            value if value.starts_with("-p") && value.len() > 2 => {
                set_once(&mut package, value[2..].to_string(), "Cargo package")?;
            }
            "--bin" => {
                let value = required_cargo_value(raw_args, &mut index, argument)?;
                set_once(&mut bin, value, "Cargo binary")?;
            }
            value if value.starts_with("--bin=") => {
                set_once(
                    &mut bin,
                    nonempty_option_value(value, "--bin=")?,
                    "Cargo binary",
                )?;
            }
            "--example" if action != LinkedCargoAction::Test => {
                let value = required_cargo_value(raw_args, &mut index, argument)?;
                set_once(&mut example, value, "Cargo example")?;
            }
            value if action != LinkedCargoAction::Test && value.starts_with("--example=") => {
                set_once(
                    &mut example,
                    nonempty_option_value(value, "--example=")?,
                    "Cargo example",
                )?;
            }
            "--example" => {
                bail!(
                    "Cargo target-selection argument `--example` is not supported by `cargo proa test`; use `--bin <name>` or `--test <name>`"
                );
            }
            value if value.starts_with("--example=") => {
                bail!(
                    "Cargo target-selection argument `{value}` is not supported by `cargo proa test`; use `--bin <name>` or `--test <name>`"
                );
            }
            "--test" if action == LinkedCargoAction::Test => {
                let value = required_cargo_value(raw_args, &mut index, argument)?;
                set_once(&mut test, value, "Cargo integration test")?;
            }
            value if action == LinkedCargoAction::Test && value.starts_with("--test=") => {
                set_once(
                    &mut test,
                    nonempty_option_value(value, "--test=")?,
                    "Cargo integration test",
                )?;
            }
            "--test" => {
                bail!(
                    "Cargo target-selection argument `--test` is valid only for `cargo proa test`"
                );
            }
            value if value.starts_with("--test=") => {
                bail!(
                    "Cargo target-selection argument `{value}` is valid only for `cargo proa test`"
                );
            }
            "--profile" => {
                let value = required_cargo_value(raw_args, &mut index, argument)?;
                set_once(&mut explicit_profile, value, "Cargo profile")?;
            }
            value if value.starts_with("--profile=") => {
                set_once(
                    &mut explicit_profile,
                    nonempty_option_value(value, "--profile=")?,
                    "Cargo profile",
                )?;
            }
            "--release" => {
                if release {
                    bail!("Cargo profile selector `--release` was supplied more than once");
                }
                release = true;
            }
            "--target" => {
                let value = required_cargo_value(raw_args, &mut index, argument)?;
                set_once(&mut target, value, "Cargo target")?;
            }
            value if value.starts_with("--target=") => {
                set_once(
                    &mut target,
                    nonempty_option_value(value, "--target=")?,
                    "Cargo target",
                )?;
            }
            "--features" | "-F" => {
                let value = required_cargo_value(raw_args, &mut index, argument)?;
                extend_features(&mut features, &value)?;
            }
            value if value.starts_with("--features=") => {
                extend_features(&mut features, &nonempty_option_value(value, "--features=")?)?;
            }
            value if value.starts_with("-F") && value.len() > 2 => {
                extend_features(&mut features, &value[2..])?;
            }
            "--all-features" | "--no-default-features" => {
                metadata_flags.push(argument.clone().into());
                selection_args.push(argument.clone());
            }
            "--locked" => {
                locked = true;
                metadata_flags.push(argument.clone().into());
            }
            "--offline" => {
                offline = true;
                metadata_flags.push(argument.clone().into());
            }
            "--frozen" => {
                locked = true;
                offline = true;
                metadata_flags.push(argument.clone().into());
            }
            "-q"
            | "--quiet"
            | "-v"
            | "-vv"
            | "--verbose"
            | "--keep-going"
            | "--future-incompat-report"
            | "--timings"
            | "--ignore-rust-version" => {
                selection_args.push(argument.clone());
            }
            "--jobs" | "-j" | "--color" => {
                let value = required_cargo_value(raw_args, &mut index, argument)?;
                selection_args.push(argument.clone());
                selection_args.push(value);
            }
            value
                if value.starts_with("--jobs=")
                    || value.starts_with("--color=")
                    || value.starts_with("--timings=")
                    || (value.starts_with("-j") && value.len() > 2) =>
            {
                selection_args.push(value.to_string());
            }
            other => {
                bail!(
                    "Cargo {action} argument `{other}` is not yet classified by the linked RSJS pipeline. Remove it or use one of the supported package/bin/example/profile/feature/target/offline/locked/output flags",
                    action = action.subcommand(),
                );
            }
        }
        index += 1;
    }

    if release && explicit_profile.is_some() {
        bail!("Cargo profile is ambiguous: use either `--release` or `--profile`, not both");
    }
    if bin.is_some() && example.is_some() {
        bail!(
            "linked Cargo target selection is ambiguous: use either `--bin <name>` or `--example <name>`, not both"
        );
    }
    if action == LinkedCargoAction::Test && bin.is_some() == test.is_some() {
        bail!(
            "linked Cargo test requires exactly one target selector: use either `--bin <name>` or `--test <name>`"
        );
    }
    let profile = explicit_profile.unwrap_or_else(|| {
        if release {
            "release".to_string()
        } else if action == LinkedCargoAction::Test {
            "test".to_string()
        } else {
            "dev".to_string()
        }
    });

    Ok(ParsedCargoInvocation {
        cargo: OsString::from(program),
        cargo_args: raw_args[..cargo_arg_count]
            .iter()
            .map(OsString::from)
            .collect(),
        runtime_args,
        package,
        bin,
        example,
        test,
        profile,
        target,
        features,
        locked,
        offline,
        metadata_flags,
        selection_args,
    })
}

fn required_cargo_value(arguments: &[String], index: &mut usize, option: &str) -> Result<String> {
    *index += 1;
    let Some(value) = arguments.get(*index) else {
        bail!("Cargo option `{option}` requires a value");
    };
    if value.is_empty() || value.starts_with('-') {
        bail!("Cargo option `{option}` requires a nonempty value");
    }
    Ok(value.clone())
}

fn nonempty_option_value(argument: &str, prefix: &str) -> Result<String> {
    let value = &argument[prefix.len()..];
    if value.is_empty() {
        bail!(
            "Cargo option `{}` requires a nonempty value",
            &prefix[..prefix.len() - 1]
        );
    }
    Ok(value.to_string())
}

fn set_once(slot: &mut Option<String>, value: String, label: &str) -> Result<()> {
    if let Some(previous) = slot {
        bail!(
            "{label} selection is ambiguous: both `{previous}` and `{value}` were supplied; select exactly one"
        );
    }
    *slot = Some(value);
    Ok(())
}

fn extend_features(features: &mut BTreeSet<String>, value: &str) -> Result<()> {
    let parsed = value
        .split([',', ' '])
        .filter(|feature| !feature.is_empty())
        .collect::<Vec<_>>();
    if parsed.is_empty() {
        bail!("Cargo `--features` requires at least one feature name");
    }
    features.extend(parsed.into_iter().map(str::to_string));
    Ok(())
}

fn is_cargo_program(program: &str) -> bool {
    Path::new(program)
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| matches!(name, "cargo" | "cargo.exe"))
}

fn load_cargo_metadata(root: &Path, parsed: &ParsedCargoInvocation) -> Result<CargoMetadata> {
    let manifest = root.join("Cargo.toml");
    let mut command = Command::new(&parsed.cargo);
    command
        .current_dir(root)
        .args([
            OsStr::new("metadata"),
            OsStr::new("--format-version=1"),
            OsStr::new("--no-deps"),
            OsStr::new("--manifest-path"),
        ])
        .arg(&manifest)
        .args(&parsed.metadata_flags);
    let output = command.output().with_context(|| {
        format!(
            "run Cargo metadata with `{}` for exact RSJS root selection",
            Path::new(&parsed.cargo).display()
        )
    })?;
    if !output.status.success() {
        bail!(
            "Cargo metadata failed while selecting the exact RSJS root ({status}):\n{stderr}",
            status = output.status,
            stderr = String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).context("parse Cargo metadata for RSJS root selection")
}

fn select_exact_cargo_root(
    metadata: &CargoMetadata,
    parsed: &ParsedCargoInvocation,
    action: LinkedCargoAction,
) -> Result<ExactCargoRoot> {
    let (requested_target_kind, requested_target_name) = match action {
        LinkedCargoAction::Test => match (&parsed.bin, &parsed.test) {
            (Some(name), None) => ("bin", name.as_str()),
            (None, Some(name)) => ("test", name.as_str()),
            _ => {
                bail!(
                    "linked Cargo test requires exactly one target selector: use either `--bin <name>` or `--test <name>`"
                )
            }
        },
        LinkedCargoAction::Build
        | LinkedCargoAction::Check
        | LinkedCargoAction::Dev
        | LinkedCargoAction::Run => match (&parsed.bin, &parsed.example) {
            (Some(name), None) => ("bin", name.as_str()),
            (None, Some(name)) => ("example", name.as_str()),
            (None, None) => ("bin", ""),
            (Some(_), Some(_)) => {
                bail!(
                    "linked Cargo target selection is ambiguous: use either `--bin <name>` or `--example <name>`, not both"
                )
            }
        },
    };
    let workspace_members = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let default_members = metadata
        .workspace_default_members
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if parsed.package.is_none() && default_members.len() != 1 {
        bail!(
            "Cargo defaults select {} workspace packages; the linked RSJS pipeline accepts one exact package and target. Pass `-p <package>` plus `--bin <name>`, `--example <name>`, or `--test <name>`",
            default_members.len(),
        );
    }
    let mut packages = metadata
        .packages
        .iter()
        .filter(|package| workspace_members.contains(package.id.as_str()))
        .filter(|package| match &parsed.package {
            Some(selector) => cargo_package_matches(package, selector),
            None => default_members.contains(package.id.as_str()),
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| left.id.cmp(&right.id));

    if packages.is_empty() {
        match &parsed.package {
            Some(package) => bail!(
                "Cargo metadata contains no exact workspace package matching `-p {package}`; package specs must resolve to one workspace package"
            ),
            None => bail!(
                "Cargo metadata has no default workspace member to use as an RSJS root; pass `-p <package>` plus an exact target"
            ),
        }
    }
    if parsed.package.is_some() && packages.len() != 1 {
        bail!(
            "Cargo package selector `{}` matches {} workspace packages; pass one unambiguous package ID and exact target",
            parsed.package.as_deref().unwrap_or_default(),
            packages.len()
        );
    }

    let mut candidates = Vec::new();
    for package in packages {
        if !package.manifest_path.is_absolute() {
            bail!(
                "Cargo package `{}` has a relative manifest path `{}`; exact source-closure diagnostics require an absolute package root",
                package.name,
                package.manifest_path.display()
            );
        }
        let package_source_root = package.manifest_path.parent().ok_or_else(|| {
            anyhow::anyhow!(
                "Cargo package `{}` manifest path `{}` has no parent directory",
                package.name,
                package.manifest_path.display()
            )
        })?;
        for target in &package.targets {
            let kinds = target.kind.iter().cloned().collect::<BTreeSet<_>>();
            if !kinds.contains(requested_target_kind) {
                continue;
            }
            if !requested_target_name.is_empty() && requested_target_name != target.name {
                continue;
            }
            candidates.push(ExactCargoRoot {
                package_id: package.id.clone(),
                package_name: package.name.clone(),
                package_source_root: package_source_root.to_path_buf(),
                target_name: target.name.clone(),
                target_kinds: kinds,
                requested_target_kind: requested_target_kind.to_string(),
            });
        }
    }
    candidates.sort_by(|left, right| {
        (&left.package_id, &left.target_name).cmp(&(&right.package_id, &right.target_name))
    });

    match candidates.len() {
        0 => {
            if !requested_target_name.is_empty() {
                bail!(
                    "Cargo metadata contains no `{requested_target_kind}` target named `{requested_target_name}` in the selected package set"
                );
            }
            bail!(
                "the selected Cargo package set has no `{requested_target_kind}` target; the linked RSJS pipeline requires one executable root"
            );
        }
        1 => Ok(candidates.remove(0)),
        count => {
            let rendered = candidates
                .iter()
                .map(|candidate| format!("{}::{}", candidate.package_name, candidate.target_name))
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "Cargo selection resolves to {count} `{requested_target_kind}` roots ({rendered}); cargo-proa will not guess. Pass an exact package and target"
            );
        }
    }
}

fn cargo_package_matches(package: &CargoMetadataPackage, selector: &str) -> bool {
    selector == package.id
        || selector == package.name
        || selector == format!("{}@{}", package.name, package.version)
        || selector == format!("{}:{}", package.name, package.version)
}

fn parse_producer_version(version: &str) -> Result<ProducerVersion> {
    let core = version.split_once('-').map_or(version, |(core, _)| core);
    let mut parts = core.split('.');
    let major = parse_version_part(parts.next(), "major", version)?;
    let minor = parse_version_part(parts.next(), "minor", version)?;
    let patch = parse_version_part(parts.next(), "patch", version)?;
    if parts.next().is_some() {
        bail!("unsupported cargo-proa producer version `{version}`");
    }
    Ok(ProducerVersion::new(major, minor, patch))
}

fn cli_tool_producer_version() -> Result<ProducerVersion> {
    parse_producer_version(env!("CARGO_PKG_VERSION"))
}

fn parse_version_part(part: Option<&str>, name: &str, version: &str) -> Result<u16> {
    part.ok_or_else(|| anyhow::anyhow!("cargo-proa version `{version}` has no {name} component"))?
        .parse::<u16>()
        .with_context(|| format!("parse cargo-proa {name} version from `{version}`"))
}

fn tool_version(
    program: &OsStr,
    args: &[&str],
    label: &str,
    working_directory: &Path,
) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(working_directory)
        .output()
        .with_context(|| {
            format!(
                "query {label} version from `{}`",
                Path::new(program).display()
            )
        })?;
    if !output.status.success() {
        bail!(
            "{label} version query failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let version = String::from_utf8(output.stdout)
        .with_context(|| format!("{label} version output is not UTF-8"))?
        .trim()
        .to_string();
    if version.is_empty() {
        bail!("{label} version query returned empty output");
    }
    Ok(version)
}

fn selected_rustc_version(working_directory: &Path) -> Result<String> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
    tool_version(&rustc, &["-vV"], "rustc", working_directory)
}

fn declared_build_environment() -> BTreeMap<String, DeclaredEnvironmentInput> {
    [
        "RUSTC",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_TARGET",
        "RUSTUP_TOOLCHAIN",
    ]
    .into_iter()
    .map(|name| {
        let value = std::env::var_os(name).map(|value| os_value_bytes(&value));
        (
            name.to_string(),
            DeclaredEnvironmentInput {
                name: name.to_string(),
                value_hash: SemanticHash::derive(&value),
            },
        )
    })
    .collect()
}

#[cfg(unix)]
fn os_value_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().to_vec()
}

#[cfg(windows)]
fn os_value_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().flat_map(u16::to_le_bytes).collect()
}

#[cfg(not(any(unix, windows)))]
fn os_value_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().into_owned().into_bytes()
}

fn absolute_project_root(root: PathBuf) -> Result<PathBuf> {
    let root = if root.is_absolute() {
        root
    } else {
        std::env::current_dir()?.join(root)
    };
    fs::canonicalize(&root)
        .with_context(|| format!("resolve absolute Proa project root `{}`", root.display()))
}

fn fresh_build_nonce() -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = RSJS_BUILD_NONCE.fetch_add(1, Ordering::Relaxed);
    format!("{}-{elapsed}-{sequence}", std::process::id())
}

fn print_linked_cargo_output(
    action: LinkedCargoAction,
    request: &LinkedCargoRequest,
    output: &LinkedCargoOutput,
) {
    println!(
        "linked RSJS plan {} for package `{}` binary `{}`",
        output.semantic.plan_id, request.root.package_id, request.root.target_name
    );
    match action {
        LinkedCargoAction::Build => println!(
            "final Cargo artifacts: {}",
            output.final_cargo.target_directory.display()
        ),
        LinkedCargoAction::Check => println!(
            "final Cargo check artifacts: {}",
            output.final_cargo.target_directory.display()
        ),
        LinkedCargoAction::Dev => println!(
            "final Cargo dev artifacts: {}",
            output.final_cargo.target_directory.display()
        ),
        LinkedCargoAction::Run => println!(
            "final Cargo run artifacts: {}",
            output.final_cargo.target_directory.display()
        ),
        LinkedCargoAction::Test => println!(
            "final Cargo test artifacts: {}",
            output.final_cargo.target_directory.display()
        ),
    }
}

fn run_project_check(root: &Path) -> Result<()> {
    let report = scaffold::site_check(Some(root.to_path_buf()))?;
    scaffold::print_site_check(&report, false)?;
    if !report.errors.is_empty() {
        bail!("Proa site check failed");
    }

    let src = root.join("src");
    if src.exists() {
        let paths = vec![src];
        let format_summary = crate::fmt::format_paths_with_options(&paths, true, true)?;
        for diagnostic in &format_summary.diagnostics {
            eprint!("{diagnostic}");
        }
        if !format_summary.changed.is_empty() {
            for path in &format_summary.changed {
                eprintln!("{}: Proa macro formatting differs", path.display());
            }
            bail!("Proa formatting check failed");
        }
        if format_summary
            .diagnostics
            .iter()
            .any(|diag| diag.severity == Severity::Error)
        {
            bail!("Proa formatting failed");
        }

        let diagnostics = crate::lint::lint_paths(&paths, crate::lint::LintOptions::default())?;
        for diagnostic in &diagnostics {
            if diagnostic.severity.rank() >= Severity::Warning.rank() {
                eprint!("{diagnostic}");
            }
        }
        if diagnostics
            .iter()
            .any(|diag| matches!(diag.severity, Severity::Error | Severity::Warning))
        {
            bail!("Proa check failed");
        }
    }

    Ok(())
}

pub fn build_assets_for_project(root: &Path) -> Result<()> {
    let config = load_project_config(root)?;
    build_assets(root, &config)
}

fn build_assets(root: &Path, config: &ProjectConfig) -> Result<()> {
    for asset in config.build.assets() {
        build_asset(root, asset)?;
    }
    Ok(())
}

fn build_asset(root: &Path, asset: &AssetConfig) -> Result<()> {
    let command = asset_command(root, asset)?;
    if command.is_empty() {
        return Ok(());
    }

    if asset.kind == "tailwind"
        && asset.command.is_empty()
        && command
            .first()
            .is_some_and(|program| program == "tailwindcss")
        && !program_available("tailwindcss")
    {
        if root.join(&asset.output).is_file() {
            eprintln!(
                "warning: managed Tailwind binary is not cached; preserving prebuilt {}",
                asset.output.display()
            );
            return Ok(());
        }
        bail!(
            "Tailwind output `{}` is missing and the managed compiler is unavailable",
            asset.output.display()
        );
    }

    if let Some(output_parent) = asset.output.parent() {
        let output_parent = root.join(output_parent);
        fs::create_dir_all(&output_parent)
            .with_context(|| format!("create {}", output_parent.display()))?;
    }

    println!("building asset: {}", asset.output.display());
    run_command_status(root, &command, "asset command")
}

fn asset_command(root: &Path, asset: &AssetConfig) -> Result<Vec<String>> {
    if !asset.command.is_empty() {
        return Ok(asset.command.clone());
    }

    match asset.kind.as_str() {
        "tailwind" => {
            if !root.join(&asset.input).exists() {
                eprintln!(
                    "warning: tailwind input `{}` not found; skipping asset build for `{}`",
                    asset.input.display(),
                    asset.output.display()
                );
                return Ok(Vec::new());
            }

            let mut command = vec![
                "tailwindcss".into(),
                "-i".into(),
                asset.input.display().to_string(),
                "-o".into(),
                asset.output.display().to_string(),
            ];
            if let Some(config) = &asset.config {
                if root.join(config).exists() {
                    command.push("-c".into());
                    command.push(config.display().to_string());
                }
            }
            Ok(command)
        }
        "command" if asset.command.is_empty() => Ok(Vec::new()),
        other => bail!("unknown asset kind `{other}` in proa.config.json"),
    }
}

fn program_available(program: &str) -> bool {
    let Some(search_path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&search_path).any(|directory| {
        let candidate = directory.join(program);
        candidate.is_file() || cfg!(windows) && directory.join(format!("{program}.exe")).is_file()
    })
}

fn run_linked_dev_foreground(
    root: &Path,
    launch: &LinkedDevLaunch,
    host: &str,
    port: u16,
    open: bool,
) -> Result<()> {
    let mut child = spawn_linked_dev(root, launch, host, port, None)?;
    set_active_child_group(&child);
    if open {
        announce_dev_start(true, host, port, &mut child);
    }
    let status = child.wait().context("wait for linked dev server");
    clear_active_child_group();
    exit_status_to_result(status?, "linked dev server")
}

fn spawn_linked_dev(
    root: &Path,
    launch: &LinkedDevLaunch,
    host: &str,
    port: u16,
    session: Option<&(u16, String)>,
) -> Result<Child> {
    print_linked_dev_launch(launch);
    let mut cmd = linked_dev_command(root, launch, host, port, session);
    // Put the dev command in its own process group so restarts can stop the
    // whole server tree and release its port before activating the next plan.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().with_context(|| {
        format!(
            "spawn linked dev executable `{}`",
            launch.executable.display()
        )
    })
}

fn linked_dev_command(
    root: &Path,
    launch: &LinkedDevLaunch,
    host: &str,
    port: u16,
    session: Option<&(u16, String)>,
) -> Command {
    let mut cmd = Command::new(&launch.executable);
    cmd.args(&launch.arguments);
    cmd.current_dir(root);
    cmd.env("HOST", host);
    cmd.env("PROA_HOST", host);
    cmd.env("PORT", port.to_string());
    // These two are the whole contract with `proa_framework_axum::serve`, which
    // injects the live-reload client only when both are present. A stale
    // value inherited from the environment must never survive into a run with
    // live reload switched off.
    match session {
        Some((reload_port, token)) => {
            cmd.env(reload::RELOAD_PORT_ENV, reload_port.to_string());
            cmd.env(reload::RELOAD_TOKEN_ENV, token);
        }
        None => {
            cmd.env_remove(reload::RELOAD_PORT_ENV);
            cmd.env_remove(reload::RELOAD_TOKEN_ENV);
        }
    }
    cmd
}

fn print_linked_dev_launch(launch: &LinkedDevLaunch) {
    println!(
        "starting final linked server `{}` with plan {} (descriptor cache {}/{}, final cache {} reused)",
        launch.executable.display(),
        launch.plan_id,
        launch.reused_descriptor_artifacts,
        launch.descriptor_artifacts,
        launch.reused_final_artifacts,
    );
}

fn run_command_status(root: &Path, command: &[String], label: &str) -> Result<()> {
    println!("running {}", shellish(command));
    let status = match command_base(root, command)?.status() {
        Ok(status) => status,
        // A missing program surfaces as a bare "No such file or directory",
        // which reads as a missing input file rather than a tool that was
        // never installed. Name the program and say how to get it.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let program = command.first().map(String::as_str).unwrap_or(label);
            bail!("{}", missing_program_message(program));
        }
        Err(error) => return Err(error).with_context(|| format!("run `{}`", shellish(command))),
    };
    exit_status_to_result(status, label)
}

fn missing_program_message(program: &str) -> String {
    let hint = match program {
        "tailwindcss" => {
            "\n  Install Tailwind CSS CLI v4 with the Proa installer:\n  \
             curl -fsSL https://proa.so/install.sh | \
             sh -s -- --tailwind-only\n  \
             Or, on macOS or Linux with Homebrew: brew install tailwindcss"
        }
        _ => "\n  Install it and make sure it is on PATH, or set an explicit `command` for this asset in proa.config.json.",
    };
    format!("`{program}` was not found on PATH.{hint}")
}

fn command_base(root: &Path, command: &[String]) -> Result<Command> {
    let Some((program, args)) = command.split_first() else {
        bail!("command cannot be empty")
    };
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.current_dir(root);
    Ok(cmd)
}

fn exit_status_to_result(status: ExitStatus, label: &str) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        bail!("{label} exited with {status}")
    }
}

fn stop_child(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }

    #[cfg(unix)]
    {
        // Signal the whole process group: TERM first so the server can shut
        // down gracefully and release its port, then KILL if it lingers.
        let group = -(child.id() as i32);
        unsafe { libc::kill(group, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            // Most servers exit promptly on TERM. Observe that exit without
            // imposing a whole watch-poll interval before activating the build.
            thread::sleep(Duration::from_millis(5));
        }
        unsafe { libc::kill(group, libc::SIGKILL) };
        let _ = child.wait();
        Ok(())
    }

    #[cfg(not(unix))]
    {
        child.kill().context("stop dev command")?;
        let _ = child.wait();
        Ok(())
    }
}

type WatchSnapshot = BTreeMap<PathBuf, FileStamp>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum DevWatchDecision {
    Idle,
    Debouncing,
    Ready(WatchSnapshot),
}

#[derive(Debug)]
struct DevWatchDebouncer {
    committed: WatchSnapshot,
    pending: Option<(WatchSnapshot, Duration)>,
    delay: Duration,
}

impl DevWatchDebouncer {
    fn new(committed: WatchSnapshot, delay: Duration) -> Self {
        Self {
            committed,
            pending: None,
            delay,
        }
    }

    fn next_poll_delay(&self, now: Duration) -> Duration {
        let Some((_, first_seen)) = self.pending.as_ref() else {
            return DEV_WATCH_POLL;
        };
        // Wake at the end of the quiet window instead of rounding it up to
        // another whole poll interval. A fresh snapshot still authorizes the
        // rebuild; changed bytes reset the full window in observe().
        self.delay
            .saturating_sub(now.saturating_sub(*first_seen))
            .clamp(Duration::from_millis(1), DEV_WATCH_POLL)
    }

    fn observe(&mut self, now: Duration, snapshot: WatchSnapshot) -> DevWatchDecision {
        if snapshot == self.committed {
            self.pending = None;
            return DevWatchDecision::Idle;
        }

        match &mut self.pending {
            Some((pending, first_seen)) if *pending == snapshot => {
                if now.saturating_sub(*first_seen) >= self.delay {
                    DevWatchDecision::Ready(snapshot)
                } else {
                    DevWatchDecision::Debouncing
                }
            }
            pending => {
                *pending = Some((snapshot, now));
                DevWatchDecision::Debouncing
            }
        }
    }

    fn commit(&mut self, snapshot: WatchSnapshot) {
        self.committed = snapshot;
        self.pending = None;
    }
}

fn watch_snapshot(root: &Path, paths: &[PathBuf]) -> Result<WatchSnapshot> {
    let mut snapshot = BTreeMap::new();
    for rel in paths {
        let path = root.join(rel);
        if path.exists() {
            collect_file_stamps(root, &path, &mut snapshot)?;
        }
    }
    Ok(snapshot)
}

fn collect_file_stamps(
    root: &Path,
    path: &Path,
    snapshot: &mut BTreeMap<PathBuf, FileStamp>,
) -> Result<()> {
    let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if metadata.is_dir() {
        if should_ignore_dir(path) {
            return Ok(());
        }
        for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
            collect_file_stamps(root, &entry?.path(), snapshot)?;
        }
    } else if metadata.is_file() {
        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        snapshot.insert(rel, FileStamp::from_metadata(&metadata));
    }
    Ok(())
}

fn should_ignore_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("target" | "node_modules" | ".git")
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified_nanos: u128,
}

impl FileStamp {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);

        Self {
            len: metadata.len(),
            modified_nanos,
        }
    }
}

fn shellish(command: &[String]) -> String {
    command.join(" ")
}

#[cfg(test)]
mod tests;
