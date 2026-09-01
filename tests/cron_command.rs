//! Integration tests for `runner cron add|list|remove` (F-13).

mod common;

use common::*;

// --- AC-01: invalid expression rejected at add-time, nothing written ---
#[test]
fn cron_add_rejects_an_invalid_expression() {
    let home = unique_runner_home("cron-add-invalid");

    let output = runner_cmd(&home)
        .args(["cron", "add", "not a cron expression", "do something"])
        .output()
        .expect("failed to run `runner cron add`");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid cron expression"));

    let list_output = runner_cmd(&home)
        .args(["cron", "list"])
        .output()
        .expect("failed to run `runner cron list`");
    assert!(String::from_utf8_lossy(&list_output.stdout).contains("no schedules"));
}

// --- AC-01/AC-04: a valid expression is accepted and defaults enabled ---
#[test]
fn cron_add_accepts_a_valid_expression_and_defaults_enabled() {
    let home = unique_runner_home("cron-add-valid");

    let add_output = runner_cmd(&home)
        .args(["cron", "add", "*/15 * * * *", "check tickets"])
        .output()
        .expect("failed to run `runner cron add`");
    assert!(add_output.status.success());
    assert!(String::from_utf8_lossy(&add_output.stdout).contains("schedule added"));

    let list_output = runner_cmd(&home)
        .args(["cron", "list"])
        .output()
        .expect("failed to run `runner cron list`");
    let stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(stdout.contains("*/15 * * * *"));
    assert!(stdout.contains("check tickets"));
    assert!(stdout.contains("enabled"));
    assert!(stdout.contains("never"), "a fresh schedule has never run");
}

// --- AC-02: list shows nothing when empty ---
#[test]
fn cron_list_reports_no_schedules_when_empty() {
    let home = unique_runner_home("cron-list-empty");

    let output = runner_cmd(&home)
        .args(["cron", "list"])
        .output()
        .expect("failed to run `runner cron list`");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("no schedules"));
}

// --- AC-03: remove deletes by id ---
#[test]
fn cron_remove_deletes_an_existing_schedule() {
    let home = unique_runner_home("cron-remove");

    let add_output = runner_cmd(&home)
        .args(["cron", "add", "0 12 * * *", "daily task"])
        .output()
        .expect("failed to run `runner cron add`");
    let stdout = String::from_utf8_lossy(&add_output.stdout);
    let schedule_id = stdout
        .trim()
        .strip_prefix("schedule added: ")
        .expect("expected `schedule added: <id>` output")
        .to_string();

    let remove_output = runner_cmd(&home)
        .args(["cron", "remove", &schedule_id])
        .output()
        .expect("failed to run `runner cron remove`");
    assert!(remove_output.status.success());

    let list_output = runner_cmd(&home)
        .args(["cron", "list"])
        .output()
        .expect("failed to run `runner cron list`");
    assert!(String::from_utf8_lossy(&list_output.stdout).contains("no schedules"));
}

// --- AC-03: removing a nonexistent id is a clear error ---
#[test]
fn cron_remove_nonexistent_id_reports_a_clear_error() {
    let home = unique_runner_home("cron-remove-missing");

    let output = runner_cmd(&home)
        .args(["cron", "remove", "does-not-exist"])
        .output()
        .expect("failed to run `runner cron remove`");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no such schedule"));
}
