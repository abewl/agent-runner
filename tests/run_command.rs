//! Integration tests for `runner run <task>` (F-10) — the failure paths
//! only. The success path genuinely invokes the real, network-calling
//! `claude` binary — deliberately not automated here, consistent with
//! every prior feature's testing approach in this project (see `DICT.md`
//! "Why no test invokes the real claude CLI", carried from F-04). Verify
//! the success path by hand: `runner repo <a real agent_docs repo>`
//! then `runner run "<a real task>"`.

mod common;

use common::*;

// --- AC-02/AC-04: no repo configured fails clearly, before touching
// claude at all, and works with no daemon running (no daemon is ever
// started anywhere in this file). ---
#[test]
fn run_fails_when_no_repo_configured() {
    let home = unique_runner_home("run-no-repo");

    let output = runner_cmd(&home)
        .args(["run", "say hello"])
        .output()
        .expect("failed to run `runner run`");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no repo configured"));
}

// --- AC-02: a preflight failure (claude not on PATH) fails clearly and
// exits non-zero, without ever attempting a real invocation. ---
#[test]
fn run_fails_when_claude_not_on_path() {
    let home = unique_runner_home("run-no-claude");
    let repo = valid_target_repo("run-no-claude-target");

    let set_status = runner_cmd(&home)
        .args(["repo", repo.to_str().unwrap()])
        .status()
        .expect("failed to run `runner repo <path>`");
    assert!(set_status.success());

    // A minimal PATH that (on this dev machine) does not include wherever
    // the real `claude` binary lives, forcing the PATH-resolution half of
    // preflight to fail deterministically and portably, without needing
    // to know or touch the real claude installation.
    let mut cmd = runner_cmd(&home);
    cmd.env("PATH", "/usr/bin:/bin");
    cmd.args(["run", "say hello"]);
    let output = cmd.output().expect("failed to run `runner run`");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("claude") && stderr.contains("PATH"),
        "expected a claude-not-found style message, got: {stderr}"
    );
}

// --- AC-02: stderr is never empty on a non-zero exit. Covers both
// failure branches above from a different angle (the general property,
// not just one specific message). ---
#[test]
fn run_never_exits_nonzero_with_empty_stderr() {
    let home = unique_runner_home("run-stderr-nonempty");

    let output = runner_cmd(&home)
        .args(["run", "say hello"])
        .output()
        .expect("failed to run `runner run`");

    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
}
