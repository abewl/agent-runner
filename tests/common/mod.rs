//! Shared helpers for integration tests that drive the compiled `runner`
//! binary end-to-end. Lives at `tests/common/mod.rs` (not `tests/common.rs`)
//! specifically so Cargo treats it as a shared module, not its own test
//! binary.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A fresh, isolated `RUNNER_HOME` directory for one test — never the real
/// `~/Library/Application Support/runner/`, and unique enough that
/// concurrently-running tests never collide.
#[allow(dead_code)]
pub fn unique_runner_home(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("runner-it-{label}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("failed to create isolated RUNNER_HOME for test");
    dir
}

#[allow(dead_code)]
pub fn runner_cmd(runner_home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_runner"));
    cmd.env("RUNNER_HOME", runner_home);
    cmd
}

#[allow(dead_code)]
pub fn pid_file_path(runner_home: &Path) -> PathBuf {
    runner_home.join("runner.pid")
}

#[allow(dead_code)]
pub fn read_pid(runner_home: &Path) -> Option<i32> {
    std::fs::read_to_string(pid_file_path(runner_home))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Waits (bounded) for a condition to become true — used instead of a fixed
/// sleep so tests aren't flaky on a slow CI box but also don't wait longer
/// than necessary.
#[allow(dead_code)]
pub fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    cond()
}

/// Polls for a pid file that both exists *and* parses — `daemon::write_pid_file`
/// creates/truncates before its content lands, so checking existence and
/// then reading separately can catch it empty mid-write. Polling the read
/// itself (not just existence) closes that gap. Fails clearly rather than
/// falling through to a confusing downstream panic if it never shows up.
#[allow(dead_code)]
pub fn wait_for_pid_file(runner_home: &Path) -> i32 {
    let mut last: Option<i32> = None;
    let found = wait_until(Duration::from_secs(5), || {
        last = read_pid(runner_home);
        last.is_some()
    });
    assert!(
        found,
        "pid file at {} should exist and parse within 5s of `daemon start`",
        pid_file_path(runner_home).display()
    );
    last.unwrap()
}

#[allow(dead_code)]
pub fn process_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Best-effort cleanup so a failing assertion never leaks a real running
/// daemon (and its caffeinate child) on the machine running the tests.
#[allow(dead_code)]
pub fn force_stop(runner_home: &Path) {
    if let Some(pid) = read_pid(runner_home) {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        wait_until(Duration::from_secs(5), || !process_alive(pid));
    }
}

#[allow(dead_code)]
pub fn caffeinate_available() -> bool {
    Command::new("which")
        .arg("caffeinate")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A valid F-02 target repo: a temp directory containing `agent_docs/AGENT.md`.
#[allow(dead_code)]
pub fn valid_target_repo(label: &str) -> PathBuf {
    let dir = unique_runner_home(&format!("repo-{label}"));
    std::fs::create_dir_all(dir.join("agent_docs")).unwrap();
    std::fs::write(dir.join("agent_docs").join("AGENT.md"), "# Agent Runbook\n").unwrap();
    dir
}
