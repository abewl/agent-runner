//! `RUNNER_HOME` resolution and the paths derived from it (F-01, F-05, F-02).
//!
//! `RUNNER_HOME` env var, when set, overrides the default
//! `~/Library/Application Support/runner/` base path for everything —
//! DB file, PID file, config file, logs (`DICT.md` "RUNNER_HOME").

use std::io;
use std::path::PathBuf;

/// Shared across every test module that mutates the process-global
/// `RUNNER_HOME` env var (currently `paths::tests` and `config::tests`) —
/// a per-module lock does *not* serialize against a different module's own
/// lock, so this has to be the one and only lock any such test uses, or
/// two tests in different modules can race on the same env var under
/// parallel test execution.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Base data directory. Default `~/Library/Application Support/runner/`,
/// overridable via `RUNNER_HOME`.
pub fn runner_home() -> PathBuf {
    if let Ok(val) = std::env::var("RUNNER_HOME") {
        return PathBuf::from(val);
    }
    let home = std::env::var("HOME").expect("HOME environment variable must be set");
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("runner")
}

pub fn pid_file() -> PathBuf {
    runner_home().join("runner.pid")
}

pub fn log_dir() -> PathBuf {
    runner_home().join("logs")
}

pub fn log_file() -> PathBuf {
    log_dir().join("runner.log")
}

/// Creates `RUNNER_HOME` and its `logs/` subdirectory if they don't exist yet.
pub fn ensure_runner_home() -> io::Result<()> {
    std::fs::create_dir_all(runner_home())?;
    std::fs::create_dir_all(log_dir())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_home_respects_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("RUNNER_HOME", "/tmp/runner-test-override");
        }
        assert_eq!(runner_home(), PathBuf::from("/tmp/runner-test-override"));
        unsafe {
            std::env::remove_var("RUNNER_HOME");
        }
    }

    #[test]
    fn runner_home_defaults_under_application_support() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("RUNNER_HOME");
        }
        let home = runner_home();
        assert!(home.ends_with("Library/Application Support/runner"));
    }

    #[test]
    fn derived_paths_are_nested_under_runner_home() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("RUNNER_HOME", "/tmp/runner-test-derived");
        }
        assert_eq!(
            pid_file(),
            PathBuf::from("/tmp/runner-test-derived/runner.pid")
        );
        assert_eq!(
            log_file(),
            PathBuf::from("/tmp/runner-test-derived/logs/runner.log")
        );
        unsafe {
            std::env::remove_var("RUNNER_HOME");
        }
    }
}
