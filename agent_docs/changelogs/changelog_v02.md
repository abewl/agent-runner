# Changelog v02

**Feature:** F-02 — Target Repo Configuration
**Date:** 2026-09-01
**Branch:** `feature/F-02-target-repo-configuration` (off `feature/F-01-cli-skeleton-daemon-lifecycle`, since `develop` doesn't yet carry F-01 — no GitHub remote is set up for this repo yet, so there's no PR chain to branch off; noted in `changelog_v01.md`'s open item and still unresolved, by choice, per the project owner)

## Summary

`runner repo set <path>` / `runner repo show`, plus a `--repo <path>` flag on `runner daemon start`:

- `repo set` validates the path exists, is a directory, and contains `agent_docs/AGENT.md`, then writes the canonicalized absolute path to `$RUNNER_HOME/config.toml` (`toml` + `serde`). A validation failure writes nothing.
- `repo show` prints the configured path, or a clear "no repo configured" message.
- `daemon start --repo <path>` runs the identical validation/persist step before daemonizing — an invalid `--repo` prevents the daemon from starting at all, not just from having a repo configured.
- Exactly one repo, machine-wide — setting a new one replaces the old value.

Also refactored: F-01's integration test helpers (`unique_runner_home`, `runner_cmd`, `wait_for_pid_file`, `force_stop`, etc.) moved out of `tests/daemon_lifecycle.rs` into a shared `tests/common/mod.rs`, since F-02's own integration tests needed the same machinery and copy-pasting a second time would just invite drift.

## AC items validated

`SPEC.md`'s "Target Repo Configuration (F-02)", AC-01 through AC-04:

- **AC-01** — invalid paths (nonexistent, not a directory, missing `agent_docs/AGENT.md`) are rejected with nothing written. Unit: `validate_repo_path_fails_for_nonexistent_path`, `validate_repo_path_fails_for_a_file_not_a_directory`, `validate_repo_path_fails_when_agent_docs_missing`, `set_repo_path_writes_nothing_on_validation_failure`. Integration: `repo_set_rejects_nonexistent_path`, `repo_set_rejects_path_without_agent_docs`.
- **AC-02** — a valid path is canonicalized and persisted, overwriting any prior value. Unit: `validate_repo_path_succeeds_and_canonicalizes`, `save_and_load_roundtrip`, `set_repo_path_overwrites_previous_value`. Integration: `repo_set_succeeds_and_show_reflects_it`, `repo_set_overwrites_previous_value`.
- **AC-03** — `daemon start --repo` validates and persists before proceeding; a bad `--repo` means the daemon never starts. Integration: `daemon_start_with_invalid_repo_does_not_start`, `daemon_start_with_valid_repo_starts_and_sets_config`.
- **AC-04** — `repo show` distinguishes "not configured" from a configured path. Unit: `load_defaults_when_config_file_absent`. Integration: `repo_show_reports_none_when_unset` (the "configured" half is exercised by AC-02's `repo_set_succeeds_and_show_reflects_it`, which also checks `show`'s output).

## Bugs found and fixed during this feature

Both surfaced by running the suite repeatedly (10× in a row), not by code review — recorded in `DICT.md`'s new "Testing conventions" section so they aren't rediscovered from scratch on a future feature:

1. **Two independent `ENV_LOCK` mutexes, one per test module (`paths::tests`, `config::tests`), that didn't actually serialize against each other.** Both mutate the process-global `RUNNER_HOME` env var; under `cargo test`'s parallel execution, a test in one module could race a test in the other. Fixed by consolidating to a single `paths::ENV_LOCK` that `config::tests` now imports rather than shadowing.
2. **`wait_for_pid_file` checked file existence, then read separately** — `daemonize::write_pid_file`'s create/truncate-then-write leaves a real (if narrow) window where the file exists but is empty. Fixed to poll the parse itself (existence + valid content together), not existence alone.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

None required to proceed to F-03. The open item from `changelog_v01.md` (no GitHub remote, so no PR chain and features are branching off each other directly rather than off `develop`) is unchanged — still deferred by the project owner's own call, not an oversight.
