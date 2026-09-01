//! Integration tests for `runner status`/`ps` (F-11).
//!
//! Rows are seeded directly via `sqlite3` rather than a real `claude`
//! call — this is testing *display* logic given some data exists, not
//! the write path (already covered by F-07/F-08's own unit tests), so
//! there's no reason to pay for a real invocation just to get a row into
//! the table. `runner status` is run once first specifically to make the
//! daemon/CLI create the DB + schema, before seeding rows directly.

mod common;

use std::process::Command;

use common::*;

fn seed_run(home: &std::path::Path, sql_values: &str) {
    // Ensure schema exists.
    let _ = runner_cmd(home).args(["status"]).output();

    let db_path = home.join("runner.db");
    let status = Command::new("sqlite3")
        .arg(&db_path)
        .arg(format!(
            "INSERT INTO runs (id, task_identity, task, status, started_at, retry_count, owner_pid) VALUES {sql_values};"
        ))
        .status()
        .expect("failed to run sqlite3 to seed a run row");
    assert!(status.success(), "sqlite3 seed insert failed");
}

#[test]
fn status_reports_no_runs_yet_when_empty() {
    let home = unique_runner_home("status-empty");

    let output = runner_cmd(&home)
        .args(["status"])
        .output()
        .expect("failed to run `runner status`");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("no runs yet"));
}

#[test]
fn status_lists_a_seeded_run_with_expected_fields() {
    let home = unique_runner_home("status-listed");
    seed_run(
        &home,
        "('r1', 't1', 'a short task', 'done', '2026-09-01T00:00:00Z', 0, 1)",
    );

    let output = runner_cmd(&home)
        .args(["status"])
        .output()
        .expect("failed to run `runner status`");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("r1"));
    assert!(stdout.contains("done"));
    assert!(stdout.contains("a short task"));
    assert!(
        stdout.contains("2026-09-01T00:00:00Z"),
        "started_at must be shown as its own field, not just folded into duration (SPEC.md AC-01)"
    );
}

#[test]
fn ps_alias_works_the_same_as_status() {
    let home = unique_runner_home("ps-alias");
    seed_run(
        &home,
        "('r1', 't1', 'task', 'done', '2026-09-01T00:00:00Z', 0, 1)",
    );

    let output = runner_cmd(&home)
        .args(["ps"])
        .output()
        .expect("failed to run `runner ps`");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("r1"));
}

#[test]
fn status_running_flag_filters_to_running_only() {
    let home = unique_runner_home("status-running-filter");
    seed_run(
        &home,
        "('done-1', 't1', 'finished task', 'done', '2026-09-01T00:00:00Z', 0, 1)",
    );
    seed_run(
        &home,
        "('running-1', 't2', 'in flight task', 'running', '2026-09-01T00:01:00Z', 0, 1)",
    );

    let all_output = runner_cmd(&home)
        .args(["status"])
        .output()
        .expect("failed to run `runner status`");
    let all_stdout = String::from_utf8_lossy(&all_output.stdout);
    assert!(all_stdout.contains("done-1"));
    assert!(all_stdout.contains("running-1"));

    let running_output = runner_cmd(&home)
        .args(["status", "--running"])
        .output()
        .expect("failed to run `runner status --running`");
    let running_stdout = String::from_utf8_lossy(&running_output.stdout);
    assert!(!running_stdout.contains("done-1"));
    assert!(running_stdout.contains("running-1"));
}

#[test]
fn status_truncates_long_task_text() {
    let home = unique_runner_home("status-truncate");
    let long_task = "x".repeat(80);
    seed_run(
        &home,
        &format!("('r1', 't1', '{long_task}', 'done', '2026-09-01T00:00:00Z', 0, 1)"),
    );

    let output = runner_cmd(&home)
        .args(["status"])
        .output()
        .expect("failed to run `runner status`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains(&long_task),
        "the full 80-char task should not appear verbatim"
    );
    assert!(
        stdout.contains('…'),
        "truncated output should show an ellipsis"
    );
}
