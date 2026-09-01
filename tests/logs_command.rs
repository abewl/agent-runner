//! Integration tests for `runner logs <run-id>` (F-12). Rows seeded
//! directly via `sqlite3`, same rationale as `tests/status_command.rs`.

mod common;

use std::process::Command;

use common::*;

fn seed_done_run(home: &std::path::Path, id: &str, result_text: &str) {
    let _ = runner_cmd(home).args(["status"]).output();
    let db_path = home.join("runner.db");
    let status = Command::new("sqlite3")
        .arg(&db_path)
        .arg(format!(
            "INSERT INTO runs (id, task_identity, task, status, started_at, retry_count, owner_pid, session_id, cost_usd, ended_at, next_action, next_action_reason, recheck_after, result_text) VALUES ('{id}', 't', 'task', 'done', '2026-09-01T00:00:00Z', 0, 1, 'sess-1', 0.01, '2026-09-01T00:01:00Z', 'idle', 'nothing left to do', '2026-09-01T00:31:00Z', '{result_text}');"
        ))
        .status()
        .expect("failed to run sqlite3 to seed a done run row");
    assert!(status.success(), "sqlite3 seed insert failed");
}

fn seed_failed_run(home: &std::path::Path, id: &str, exit_reason: &str) {
    let _ = runner_cmd(home).args(["status"]).output();
    let db_path = home.join("runner.db");
    let status = Command::new("sqlite3")
        .arg(&db_path)
        .arg(format!(
            "INSERT INTO runs (id, task_identity, task, status, started_at, retry_count, owner_pid, ended_at, exit_reason) VALUES ('{id}', 't', 'task', 'failed', '2026-09-01T00:00:00Z', 1, 1, '2026-09-01T00:01:00Z', '{exit_reason}');"
        ))
        .status()
        .expect("failed to run sqlite3 to seed a failed run row");
    assert!(status.success(), "sqlite3 seed insert failed");
}

// --- AC-02: unknown run id, clear error, non-zero exit, never empty stdout ---
#[test]
fn logs_unknown_run_id_reports_a_clear_error() {
    let home = unique_runner_home("logs-unknown");

    let output = runner_cmd(&home)
        .args(["logs", "does-not-exist"])
        .output()
        .expect("failed to run `runner logs`");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no such run"));
}

// --- AC-01: full result text on success, plus next_action/reason/recheck_after ---
#[test]
fn logs_done_run_shows_result_text_and_signal_fields() {
    let home = unique_runner_home("logs-done");
    seed_done_run(&home, "r1", "This is the full answer.");

    let output = runner_cmd(&home)
        .args(["logs", "r1"])
        .output()
        .expect("failed to run `runner logs`");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("This is the full answer."));
    assert!(stdout.contains("next_action: idle"));
    assert!(stdout.contains("next_action_reason: nothing left to do"));
    assert!(stdout.contains("recheck_after: 2026-09-01T00:31:00Z"));
}

// --- AC-01: failure detail on a failed run ---
#[test]
fn logs_failed_run_shows_exit_reason() {
    let home = unique_runner_home("logs-failed");
    seed_failed_run(
        &home,
        "r1",
        "first attempt: spawn error; retry also failed: spawn error",
    );

    let output = runner_cmd(&home)
        .args(["logs", "r1"])
        .output()
        .expect("failed to run `runner logs`");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("first attempt: spawn error"));
    // A failed run never has a continuation signal — SPEC.md F-08 leaves
    // these null, and F-12 must show "none" for them, not blank/panic.
    assert!(stdout.contains("next_action: none"));
    assert!(stdout.contains("recheck_after: none"));
}
