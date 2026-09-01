//! `runner daemon start|stop|status` command implementations (F-01, F-02).

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use daemonize::Daemonize;

use crate::{config, paths, pid};

const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// `runner daemon start` — detaches into a long-lived background process.
///
/// On success in the *original* foreground process, this function never
/// actually returns: `daemonize::Daemonize::start()` calls `exit()`
/// internally for the parent, which is exactly what makes control return to
/// the shell immediately (SPEC.md F-01 AC-02). Only the detached child
/// continues past `.start()` — from its point of view this function runs
/// the daemon body and returns once the daemon has shut down.
///
/// `repo`, when given (`--repo <path>`), is validated and persisted via
/// `config::set_repo_path` *before* anything else — a validation failure
/// must prevent the daemon from starting at all (SPEC.md F-02 AC-03).
pub fn start(repo: Option<PathBuf>) -> Result<(), String> {
    paths::ensure_runner_home().map_err(|e| format!("failed to create RUNNER_HOME: {e}"))?;

    if let Some(repo_path) = repo {
        config::set_repo_path(&repo_path)?;
    }

    let pid_path = paths::pid_file();

    if let Some(existing_pid) = pid::read_pid_file(&pid_path)
        && pid::process_alive(existing_pid)
    {
        return Err(format!("already running (pid {existing_pid})"));
    }
    // A stale pid file (process no longer alive) needs no separate cleanup —
    // the detached daemon overwrites it itself once it writes its own
    // (see `daemon::write_pid_file` — deliberately not left to
    // `daemonize`'s built-in `.pid_file()` option, which would write it
    // before signal handlers are installed; see `daemon.rs` for why).

    let log_path = paths::log_file();
    let stdout_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("failed to open log file {}: {e}", log_path.display()))?;
    let stderr_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("failed to open log file {}: {e}", log_path.display()))?;

    let daemonize = Daemonize::new()
        .working_directory(paths::runner_home())
        .stdout(stdout_file)
        .stderr(stderr_file);

    match daemonize.start() {
        Ok(_) => {
            // We are now the detached daemon (see doc comment above).
            crate::daemon::run();
            Ok(())
        }
        Err(e) => Err(format!("failed to start daemon: {e}")),
    }
}

/// `runner daemon stop` — sends SIGTERM and waits up to 5s for exit.
pub fn stop() -> Result<(), String> {
    let pid_path = paths::pid_file();

    let Some(existing_pid) = pid::read_pid_file(&pid_path) else {
        return Err("not running (no pid file)".to_string());
    };

    if !pid::process_alive(existing_pid) {
        pid::remove_pid_file(&pid_path);
        return Err(format!(
            "not running (stale pid file for pid {existing_pid} removed)"
        ));
    }

    unsafe {
        libc::kill(existing_pid, libc::SIGTERM);
    }

    let deadline = Instant::now() + STOP_TIMEOUT;
    while Instant::now() < deadline {
        if !pid::process_alive(existing_pid) {
            pid::remove_pid_file(&pid_path);
            println!("stopped (pid {existing_pid})");
            return Ok(());
        }
        std::thread::sleep(STOP_POLL_INTERVAL);
    }

    Err(format!(
        "timed out waiting for pid {existing_pid} to exit after SIGTERM"
    ))
}

/// `runner daemon status` — reports running/stopped, cleaning up a stale
/// pid file if the recorded process is no longer alive.
pub fn status() -> Result<(), String> {
    let pid_path = paths::pid_file();

    let Some(existing_pid) = pid::read_pid_file(&pid_path) else {
        println!("stopped");
        return Ok(());
    };

    if pid::process_alive(existing_pid) {
        println!("running (pid {existing_pid})");
    } else {
        pid::remove_pid_file(&pid_path);
        println!("stopped");
    }

    Ok(())
}
