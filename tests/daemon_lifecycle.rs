//! Integration tests for `runner daemon start|stop|status` (F-01), driving
//! the actual compiled binary — the lifecycle, detach, and signal behavior
//! these ACs describe can't be meaningfully verified as pure unit tests.
//!
//! Every test gets its own isolated `RUNNER_HOME` temp directory so tests
//! can run concurrently and never touch a real `~/Library/Application
//! Support/runner/`.

mod common;

use std::process::Command;
use std::time::{Duration, Instant};

use common::*;

// --- AC-05: status reports "stopped" before anything has ever run ---
#[test]
fn status_reports_stopped_when_never_started() {
    let home = unique_runner_home("status-fresh");

    let output = runner_cmd(&home)
        .args(["daemon", "status"])
        .output()
        .expect("failed to run `runner daemon status`");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "stopped");
}

// --- AC-02, AC-06: start detaches promptly and writes a live pid file ---
#[test]
fn start_detaches_promptly_and_writes_pid_file() {
    let home = unique_runner_home("start-detach");

    let began = Instant::now();
    let status = runner_cmd(&home)
        .args(["daemon", "start"])
        .status()
        .expect("failed to run `runner daemon start`");
    let elapsed = began.elapsed();

    assert!(status.success(), "daemon start should exit 0");
    assert!(
        elapsed < Duration::from_secs(2),
        "start should return control promptly, took {elapsed:?}"
    );

    let pid = wait_for_pid_file(&home);
    assert!(process_alive(pid), "pid file's process should be alive");

    force_stop(&home);
}

// --- AC-03: a second start while already running is rejected ---
#[test]
fn second_start_while_running_is_rejected() {
    let home = unique_runner_home("start-twice");

    let first = runner_cmd(&home)
        .args(["daemon", "start"])
        .status()
        .expect("failed to run first `runner daemon start`");
    assert!(first.success());
    let pid = wait_for_pid_file(&home);

    let second = runner_cmd(&home)
        .args(["daemon", "start"])
        .output()
        .expect("failed to run second `runner daemon start`");

    assert!(!second.status.success(), "second start should fail");
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains(&format!("already running (pid {pid})")),
        "stderr should name the existing pid, got: {stderr}"
    );

    force_stop(&home);
}

// --- AC-04, AC-06: stop sends SIGTERM, waits for exit, removes pid file ---
#[test]
fn stop_terminates_daemon_and_removes_pid_file() {
    let home = unique_runner_home("stop-basic");

    let start = runner_cmd(&home)
        .args(["daemon", "start"])
        .status()
        .expect("failed to run `runner daemon start`");
    assert!(start.success());
    let pid = wait_for_pid_file(&home);

    let stop = runner_cmd(&home)
        .args(["daemon", "stop"])
        .output()
        .expect("failed to run `runner daemon stop`");

    assert!(stop.status.success(), "stop should exit 0");
    assert!(String::from_utf8_lossy(&stop.stdout).contains(&format!("stopped (pid {pid})")));
    assert!(!process_alive(pid), "process should be gone after stop");
    assert!(
        !pid_file_path(&home).exists(),
        "pid file should be removed after a clean stop"
    );
}

// --- AC-06: SIGINT (not just SIGTERM via `stop`) also triggers clean
// shutdown — sent directly, since the CLI only ever issues SIGTERM itself.
#[test]
fn sigint_also_triggers_clean_shutdown() {
    let home = unique_runner_home("sigint");

    let start = runner_cmd(&home)
        .args(["daemon", "start"])
        .status()
        .expect("failed to run `runner daemon start`");
    assert!(start.success());
    let pid = wait_for_pid_file(&home);

    unsafe {
        libc::kill(pid, libc::SIGINT);
    }

    let exited = wait_until(Duration::from_secs(5), || !process_alive(pid));
    assert!(exited, "process should exit after SIGINT");

    let cleaned_up = wait_until(Duration::from_secs(2), || !pid_file_path(&home).exists());
    assert!(
        cleaned_up,
        "pid file should be removed after a SIGINT-triggered shutdown"
    );
}

// --- AC-04: stopping when nothing is running reports a clear error ---
#[test]
fn stop_when_not_running_reports_error() {
    let home = unique_runner_home("stop-absent");

    let output = runner_cmd(&home)
        .args(["daemon", "stop"])
        .output()
        .expect("failed to run `runner daemon stop`");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not running (no pid file)"));
}

// --- AC-05: status reports "running (pid N)" while the daemon is up ---
#[test]
fn status_reports_running_while_daemon_is_up() {
    let home = unique_runner_home("status-running");

    let start = runner_cmd(&home)
        .args(["daemon", "start"])
        .status()
        .expect("failed to run `runner daemon start`");
    assert!(start.success());
    let pid = wait_for_pid_file(&home);

    let status = runner_cmd(&home)
        .args(["daemon", "status"])
        .output()
        .expect("failed to run `runner daemon status`");

    assert!(status.status.success());
    assert_eq!(
        String::from_utf8_lossy(&status.stdout).trim(),
        format!("running (pid {pid})")
    );

    force_stop(&home);
}

// --- AC-05: status cleans up a stale pid file for a dead process ---
#[test]
fn status_cleans_up_stale_pid_file() {
    let home = unique_runner_home("status-stale");

    // A pid guaranteed not to refer to a live process: spawn a trivial
    // child and wait for it to be reaped.
    let mut child = Command::new("true")
        .spawn()
        .expect("failed to spawn `true`");
    let dead_pid = child.id() as i32;
    child.wait().expect("failed to wait for child");

    std::fs::write(pid_file_path(&home), format!("{dead_pid}\n")).unwrap();

    let output = runner_cmd(&home)
        .args(["daemon", "status"])
        .output()
        .expect("failed to run `runner daemon status`");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "stopped");
    assert!(
        !pid_file_path(&home).exists(),
        "stale pid file should be removed by status"
    );
}

// --- AC-07: daemon log output lands in a real file under RUNNER_HOME/logs/ ---
#[test]
fn log_file_is_created_and_non_empty_after_start() {
    let home = unique_runner_home("logs");

    let start = runner_cmd(&home)
        .args(["daemon", "start"])
        .status()
        .expect("failed to run `runner daemon start`");
    assert!(start.success());

    let log_path = home.join("logs").join("runner.log");
    let found = wait_until(Duration::from_secs(2), || {
        log_path.metadata().map(|m| m.len() > 0).unwrap_or(false)
    });
    assert!(
        found,
        "log file should exist and be non-empty shortly after start"
    );

    let contents = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        contents.contains("daemon started"),
        "log should contain the daemon-started line, got: {contents}"
    );

    force_stop(&home);
}

// --- AC-08/AC-09: caffeinate is spawned tied to the daemon's pid, and is
// gone again once the daemon stops, with no explicit kill needed. ---
#[test]
fn caffeinate_is_spawned_and_self_terminates_on_stop() {
    if !caffeinate_available() {
        eprintln!("skipping: `caffeinate` not on PATH (expected off macOS)");
        return;
    }

    let home = unique_runner_home("caffeinate");

    let start = runner_cmd(&home)
        .args(["daemon", "start"])
        .status()
        .expect("failed to run `runner daemon start`");
    assert!(start.success());
    let pid = wait_for_pid_file(&home);

    let pattern = format!("caffeinate -s -w {pid}");
    let caffeinate_running = wait_until(Duration::from_secs(2), || {
        Command::new("pgrep")
            .args(["-f", &pattern])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    });
    assert!(
        caffeinate_running,
        "expected a `caffeinate -s -w {pid}` process while the daemon is running"
    );

    let stop = runner_cmd(&home)
        .args(["daemon", "stop"])
        .status()
        .expect("failed to run `runner daemon stop`");
    assert!(stop.success());

    let caffeinate_gone = wait_until(Duration::from_secs(3), || {
        !Command::new("pgrep")
            .args(["-f", &pattern])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    });
    assert!(
        caffeinate_gone,
        "caffeinate should self-terminate shortly after the daemon it was watching exits"
    );
}
