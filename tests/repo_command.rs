//! Integration tests for `runner repo [path]` and `runner daemon start
//! --repo` (F-02), driving the compiled binary.

mod common;

use common::*;

// --- AC-04: `runner repo` (no path) before anything is configured ---
#[test]
fn repo_show_reports_none_when_unset() {
    let home = unique_runner_home("repo-show-unset");

    let output = runner_cmd(&home)
        .args(["repo"])
        .output()
        .expect("failed to run `runner repo`");

    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("no repo configured — run `runner repo <path>`")
    );
}

// --- AC-01: a nonexistent path is rejected, nothing written ---
#[test]
fn repo_set_rejects_nonexistent_path() {
    let home = unique_runner_home("repo-set-missing");
    let bad_path = home.join("does-not-exist");

    let output = runner_cmd(&home)
        .args(["repo", bad_path.to_str().unwrap()])
        .output()
        .expect("failed to run `runner repo <path>`");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not exist"));
    assert!(
        !home.join("config.toml").exists(),
        "no config file should be written on a validation failure"
    );
}

// --- AC-01: a directory without agent_docs/AGENT.md is rejected ---
#[test]
fn repo_set_rejects_path_without_agent_docs() {
    let home = unique_runner_home("repo-set-no-agent-docs");
    let plain_dir = home.join("plain");
    std::fs::create_dir_all(&plain_dir).unwrap();

    let output = runner_cmd(&home)
        .args(["repo", plain_dir.to_str().unwrap()])
        .output()
        .expect("failed to run `runner repo <path>`");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("agent_docs/AGENT.md"),
        "error should name what's missing, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!home.join("config.toml").exists());
}

// --- AC-02: a valid repo is set and reflected by `runner repo` (no path) ---
#[test]
fn repo_set_succeeds_and_show_reflects_it() {
    let home = unique_runner_home("repo-set-valid");
    let repo = valid_target_repo("set-valid-target");

    let set_output = runner_cmd(&home)
        .args(["repo", repo.to_str().unwrap()])
        .output()
        .expect("failed to run `runner repo <path>`");
    assert!(set_output.status.success());

    let canonical = repo.canonicalize().unwrap();
    assert!(
        String::from_utf8_lossy(&set_output.stdout).contains(canonical.to_str().unwrap()),
        "set output should echo the canonical path"
    );

    let show_output = runner_cmd(&home)
        .args(["repo"])
        .output()
        .expect("failed to run `runner repo`");
    assert!(show_output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&show_output.stdout).trim(),
        canonical.to_str().unwrap()
    );
}

// --- AC-02: setting a new repo replaces the old value, not a list ---
#[test]
fn repo_set_overwrites_previous_value() {
    let home = unique_runner_home("repo-set-overwrite");
    let first = valid_target_repo("overwrite-first");
    let second = valid_target_repo("overwrite-second");

    runner_cmd(&home)
        .args(["repo", first.to_str().unwrap()])
        .status()
        .expect("failed to run first `runner repo <path>`");
    runner_cmd(&home)
        .args(["repo", second.to_str().unwrap()])
        .status()
        .expect("failed to run second `runner repo <path>`");

    let show_output = runner_cmd(&home)
        .args(["repo"])
        .output()
        .expect("failed to run `runner repo`");

    let canonical_second = second.canonicalize().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&show_output.stdout).trim(),
        canonical_second.to_str().unwrap()
    );
}

// --- AC-03: `daemon start --repo <bad>` validates before starting, at all ---
#[test]
fn daemon_start_with_invalid_repo_does_not_start() {
    let home = unique_runner_home("daemon-start-bad-repo");
    let bad_path = home.join("nope");

    let output = runner_cmd(&home)
        .args(["daemon", "start", "--repo", bad_path.to_str().unwrap()])
        .output()
        .expect("failed to run `runner daemon start --repo`");

    assert!(
        !output.status.success(),
        "daemon start should fail when --repo validation fails"
    );
    assert!(
        !pid_file_path(&home).exists(),
        "the daemon must not have started at all"
    );
}

// --- AC-03: `daemon start --repo <valid>` validates, persists, then starts ---
#[test]
fn daemon_start_with_valid_repo_starts_and_sets_config() {
    let home = unique_runner_home("daemon-start-good-repo");
    let repo = valid_target_repo("daemon-start-target");

    let status = runner_cmd(&home)
        .args(["daemon", "start", "--repo", repo.to_str().unwrap()])
        .status()
        .expect("failed to run `runner daemon start --repo`");
    assert!(status.success());

    let _pid = wait_for_pid_file(&home);

    let show_output = runner_cmd(&home)
        .args(["repo"])
        .output()
        .expect("failed to run `runner repo`");
    let canonical = repo.canonicalize().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&show_output.stdout).trim(),
        canonical.to_str().unwrap(),
        "starting with --repo should persist it, same as `runner repo <path>` would"
    );

    force_stop(&home);
}
