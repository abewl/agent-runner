//! The daemon process body — what actually runs once `runner daemon start`
//! has detached (F-01). Installs signal handlers, spawns the `caffeinate`
//! sleep-prevention child, waits for shutdown, cleans up.
//!
//! Must only be entered *after* `daemonize::Daemonize::start()` has
//! succeeded — building a tokio runtime before the fork is unsafe (fork()
//! only duplicates the calling thread; a multi-threaded runtime's other
//! threads would be left in an undefined state in the child).

use std::fs::OpenOptions;
use std::sync::Mutex;

use tokio::signal::unix::{SignalKind, signal};

use crate::{paths, pid};

/// Runs the daemon's main loop to completion (blocks until a shutdown
/// signal is received and cleanup finishes).
pub fn run() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    runtime.block_on(async_main());
}

async fn async_main() {
    init_logging();
    tracing::info!(pid = std::process::id(), "daemon started");

    // Signal handlers must be installed before the pid file becomes
    // visible to anyone (including our own `daemon stop`), not after.
    // `tokio::signal::unix::signal()` installs the OS-level handler
    // synchronously when called, not lazily on first `.recv().await` — so
    // registering here, before `write_pid_file()`, closes the window where
    // an external kill sent the instant the pid file appears could
    // otherwise hit the default (uncaught) signal disposition and
    // terminate the process without running cleanup at all.
    let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");

    if let Err(e) = write_pid_file() {
        tracing::error!("failed to write pid file: {e}");
    }

    spawn_caffeinate();

    tokio::select! {
        _ = sigterm.recv() => { tracing::info!("received SIGTERM"); }
        _ = sigint.recv() => { tracing::info!("received SIGINT"); }
    }

    tracing::info!("shutdown signal received, cleaning up");
    cleanup();
}

/// Writes the daemon's own pid to the pid file. Done here — after signal
/// handlers are installed — rather than left to `daemonize`'s built-in
/// `.pid_file()` option, which would write it earlier, before this process
/// can safely receive a signal without racing default disposition.
fn write_pid_file() -> std::io::Result<()> {
    std::fs::write(paths::pid_file(), format!("{}\n", std::process::id()))
}

/// Sends daemon log output through `tracing` to a real file under
/// `$RUNNER_HOME/logs/`, independent of stdio redirection, so logs remain
/// inspectable regardless of how stdout/stderr ended up wired (SPEC.md F-01
/// AC-07).
fn init_logging() {
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths::log_file())
    {
        Ok(file) => {
            let _ = tracing_subscriber::fmt()
                .with_writer(Mutex::new(file))
                .with_ansi(false)
                .try_init();
        }
        Err(e) => {
            // Fall back to whatever stdout/stderr already are (daemonize
            // redirected them to the same log file) rather than losing all
            // log output because the dedicated file handle failed to open.
            eprintln!("warning: could not open dedicated log file: {e}");
        }
    }
}

/// Spawns `caffeinate -s -w <own-pid>` so the machine cannot sleep for as
/// long as the daemon is alive. Fire-and-forget: not tracked, not waited
/// on, never explicitly killed — `-w <pid>` ties its lifetime to ours
/// automatically, on a clean stop or a crash alike (SPEC.md F-01 AC-08/09).
fn spawn_caffeinate() {
    let pid = std::process::id().to_string();
    match std::process::Command::new("caffeinate")
        .arg("-s")
        .arg("-w")
        .arg(&pid)
        .spawn()
    {
        Ok(_) => tracing::info!(watched_pid = %pid, "caffeinate spawned"),
        Err(e) => tracing::warn!(
            "caffeinate not available ({e}); sleep prevention is not active for this run"
        ),
    }
}

fn cleanup() {
    pid::remove_pid_file(&paths::pid_file());
}
