//! Local ambient-auth preflight check.
//!
//! Confirms `claude` resolves on PATH and appears to have a usable login
//! session, before any subprocess is spawned to do actual work — this
//! module only checks, it never invokes `claude` for real work itself.
//! No credential injection or credential file handling of any kind —
//! Runner trusts whatever `claude` login state already exists on the
//! machine.

use std::fmt;
use std::path::{Path, PathBuf};

const CLAUDE_BIN: &str = "claude";
/// The macOS Keychain service name `claude login` creates, confirmed
/// against a live, currently-logged-in Claude Code install on this
/// machine (`security find-generic-password -s "Claude Code-credentials"`).
/// Existence-only check below — the credential value itself is never read.
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

#[derive(Debug, PartialEq, Eq)]
pub enum PreflightError {
    /// `claude` does not resolve on PATH at all.
    ClaudeNotFound,
    /// `claude` is present but no usable login/session was detected.
    NotLoggedIn,
}

impl fmt::Display for PreflightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreflightError::ClaudeNotFound => write!(
                f,
                "`claude` was not found on PATH — install Claude Code and ensure it's on PATH"
            ),
            PreflightError::NotLoggedIn => write!(
                f,
                "`claude` is installed but does not appear to be logged in — run `claude login` first"
            ),
        }
    }
}

/// Runs before any `claude` subprocess is spawned for real work. PATH
/// resolution is checked first via a plain directory scan — no subprocess
/// spawn attempted for this check at all — then macOS Keychain presence
/// of the login credential entry.
///
/// `PreflightError` is a distinct type from whatever error the subprocess
/// runner itself produces, so a caller can always tell "never even tried
/// to run claude" from "tried and it failed."
pub fn check() -> Result<(), PreflightError> {
    check_with(CLAUDE_BIN, KEYCHAIN_SERVICE)
}

fn check_with(bin_name: &str, keychain_service: &str) -> Result<(), PreflightError> {
    if find_on_path(bin_name).is_none() {
        return Err(PreflightError::ClaudeNotFound);
    }
    if !keychain_has_entry(keychain_service) {
        return Err(PreflightError::NotLoggedIn);
    }
    Ok(())
}

fn find_on_path(bin: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(bin))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Existence-only Keychain lookup — never reads or logs the credential
/// value itself, only whether an entry is present.
fn keychain_has_entry(service: &str) -> bool {
    std::process::Command::new("security")
        .args(["find-generic-password", "-s", service])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_on_path_locates_a_known_binary() {
        // `sh` is guaranteed present on any Unix machine tests run on.
        assert!(find_on_path("sh").is_some());
    }

    #[test]
    fn find_on_path_returns_none_for_a_nonexistent_binary() {
        assert!(find_on_path("definitely-not-a-real-binary-xyz-123").is_none());
    }

    #[test]
    fn is_executable_file_true_for_a_real_executable() {
        assert!(is_executable_file(Path::new("/bin/sh")));
    }

    #[test]
    fn is_executable_file_false_for_a_non_executable_file() {
        let dir = std::env::temp_dir().join(format!(
            "runner-preflight-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("not-executable.txt");
        std::fs::write(&file, "hello").unwrap();
        assert!(!is_executable_file(&file));
    }

    #[test]
    fn is_executable_file_false_for_missing_path() {
        assert!(!is_executable_file(Path::new(
            "/definitely/not/a/real/path/anywhere"
        )));
    }

    #[test]
    fn keychain_has_entry_false_for_a_service_that_does_not_exist() {
        // Deterministic, machine-independent negative case — doesn't
        // assume anything about whether Claude Code is installed/logged
        // in on whatever machine runs this test.
        assert!(!keychain_has_entry(
            "definitely-not-a-real-keychain-service-runner-test-xyz"
        ));
    }

    // `check()`/`check_with` parameterized on bin name and keychain
    // service specifically so both failure branches are testable without
    // mutating the real, process-global PATH env var — a shared mutable
    // global that other tests running in parallel would race on.

    #[test]
    fn check_with_fails_claude_not_found_when_binary_is_missing() {
        let err = check_with("definitely-not-a-real-binary-xyz-123", "irrelevant").unwrap_err();
        assert_eq!(err, PreflightError::ClaudeNotFound);
    }

    #[test]
    fn check_with_fails_not_logged_in_when_binary_present_but_no_keychain_entry() {
        // `sh` stands in for a "present" binary; the keychain service name
        // is deliberately fake so this doesn't depend on real login state.
        let err = check_with(
            "sh",
            "definitely-not-a-real-keychain-service-runner-test-xyz",
        )
        .unwrap_err();
        assert_eq!(err, PreflightError::NotLoggedIn);
    }
}
