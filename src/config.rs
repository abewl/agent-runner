//! Target repo configuration — `$RUNNER_HOME/config.toml`.
//!
//! Stage 1 supports exactly one configured repo at a time, machine-wide.
//! Setting a new one replaces the old value; there is no list.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub repo_path: Option<PathBuf>,
}

#[derive(Debug)]
pub enum RepoValidationError {
    NotFound(PathBuf),
    NotADirectory(PathBuf),
    MissingAgentDocs(PathBuf),
    Canonicalize(PathBuf, std::io::Error),
}

impl fmt::Display for RepoValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepoValidationError::NotFound(p) => write!(f, "path does not exist: {}", p.display()),
            RepoValidationError::NotADirectory(p) => {
                write!(f, "not a directory: {}", p.display())
            }
            RepoValidationError::MissingAgentDocs(p) => write!(
                f,
                "not a valid target repo (missing agent_docs/AGENT.md): {}",
                p.display()
            ),
            RepoValidationError::Canonicalize(p, e) => {
                write!(
                    f,
                    "failed to resolve absolute path for {}: {e}",
                    p.display()
                )
            }
        }
    }
}

pub fn config_file() -> PathBuf {
    crate::paths::runner_home().join("config.toml")
}

/// Loads the config file, defaulting to an empty `Config` if it doesn't
/// exist yet or fails to parse (a missing/corrupt config is "nothing set",
/// not a hard error — `repo_path` being `None` is already a valid state
/// every caller has to handle).
pub fn load() -> Config {
    fs::read_to_string(config_file())
        .ok()
        .and_then(|contents| toml::from_str(&contents).ok())
        .unwrap_or_default()
}

pub fn save(config: &Config) -> std::io::Result<()> {
    let contents = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::other(format!("failed to serialise config: {e}")))?;
    fs::write(config_file(), contents)
}

/// Validates `path` as a target repo: must exist, be a directory, and
/// contain `agent_docs/AGENT.md`. Returns the canonicalized absolute path
/// on success. Performs no writes — callers decide whether/when to persist
/// the result, so a validation failure never leaves a partial config
/// written.
pub fn validate_repo_path(path: &Path) -> Result<PathBuf, RepoValidationError> {
    if !path.exists() {
        return Err(RepoValidationError::NotFound(path.to_path_buf()));
    }
    if !path.is_dir() {
        return Err(RepoValidationError::NotADirectory(path.to_path_buf()));
    }
    if !path.join("agent_docs").join("AGENT.md").is_file() {
        return Err(RepoValidationError::MissingAgentDocs(path.to_path_buf()));
    }
    path.canonicalize()
        .map_err(|e| RepoValidationError::Canonicalize(path.to_path_buf(), e))
}

/// Validates and persists `path` as the configured target repo, replacing
/// any previously configured value. Returns the canonicalized path that
/// was written.
pub fn set_repo_path(path: &Path) -> Result<PathBuf, String> {
    let canonical = validate_repo_path(path).map_err(|e| e.to_string())?;

    crate::paths::ensure_runner_home().map_err(|e| format!("failed to create RUNNER_HOME: {e}"))?;

    let mut config = load();
    config.repo_path = Some(canonical.clone());
    save(&config).map_err(|e| format!("failed to write config file: {e}"))?;

    Ok(canonical)
}

pub fn repo_path() -> Option<PathBuf> {
    load().repo_path
}

#[cfg(test)]
mod tests {
    use super::*;
    // Shared with `paths::tests` — see that module's doc comment on
    // `ENV_LOCK` for why this must not be a second, independent lock.
    use crate::paths::ENV_LOCK;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "runner-config-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn valid_repo(label: &str) -> PathBuf {
        let dir = temp_dir(label);
        fs::create_dir_all(dir.join("agent_docs")).unwrap();
        fs::write(dir.join("agent_docs").join("AGENT.md"), "# Agent Runbook\n").unwrap();
        dir
    }

    #[test]
    fn validate_repo_path_fails_for_nonexistent_path() {
        let path = std::env::temp_dir().join("runner-config-test-does-not-exist");
        let err = validate_repo_path(&path).unwrap_err();
        assert!(matches!(err, RepoValidationError::NotFound(_)));
    }

    #[test]
    fn validate_repo_path_fails_for_a_file_not_a_directory() {
        let dir = temp_dir("file-not-dir");
        let file_path = dir.join("not-a-dir.txt");
        fs::write(&file_path, "hello").unwrap();
        let err = validate_repo_path(&file_path).unwrap_err();
        assert!(matches!(err, RepoValidationError::NotADirectory(_)));
    }

    #[test]
    fn validate_repo_path_fails_when_agent_docs_missing() {
        let dir = temp_dir("missing-agent-docs");
        let err = validate_repo_path(&dir).unwrap_err();
        assert!(matches!(err, RepoValidationError::MissingAgentDocs(_)));
    }

    #[test]
    fn validate_repo_path_succeeds_and_canonicalizes() {
        let dir = valid_repo("valid");
        let canonical = validate_repo_path(&dir).expect("should validate");
        assert!(canonical.is_absolute());
        assert_eq!(canonical, dir.canonicalize().unwrap());
    }

    #[test]
    fn load_defaults_when_config_file_absent() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("load-absent");
        unsafe {
            std::env::set_var("RUNNER_HOME", &home);
        }
        let config = load();
        assert!(config.repo_path.is_none());
        unsafe {
            std::env::remove_var("RUNNER_HOME");
        }
    }

    #[test]
    fn save_and_load_roundtrip() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("roundtrip");
        unsafe {
            std::env::set_var("RUNNER_HOME", &home);
        }

        let repo = valid_repo("roundtrip-repo");
        let canonical = set_repo_path(&repo).expect("set_repo_path should succeed");
        assert_eq!(repo_path(), Some(canonical));

        unsafe {
            std::env::remove_var("RUNNER_HOME");
        }
    }

    #[test]
    fn set_repo_path_writes_nothing_on_validation_failure() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("no-write-on-failure");
        unsafe {
            std::env::set_var("RUNNER_HOME", &home);
        }

        let bad_path = std::env::temp_dir().join("runner-config-test-still-does-not-exist");
        let result = set_repo_path(&bad_path);
        assert!(result.is_err());
        assert!(
            !config_file().exists(),
            "no config file should be written on a validation failure"
        );

        unsafe {
            std::env::remove_var("RUNNER_HOME");
        }
    }

    #[test]
    fn set_repo_path_overwrites_previous_value() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("overwrite");
        unsafe {
            std::env::set_var("RUNNER_HOME", &home);
        }

        let first = valid_repo("overwrite-first");
        let second = valid_repo("overwrite-second");

        set_repo_path(&first).unwrap();
        let second_canonical = set_repo_path(&second).unwrap();

        assert_eq!(repo_path(), Some(second_canonical));

        unsafe {
            std::env::remove_var("RUNNER_HOME");
        }
    }
}
