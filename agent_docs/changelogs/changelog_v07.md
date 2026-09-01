# Changelog v07

**Feature:** F-07 — Local State Store
**Date:** 2026-09-01
**Branch:** `feature/F-07-local-state-store` (off `feature/F-06-bounded-retry`)

## Summary

`store::open()` (schema init/migration at `$RUNNER_HOME/runner.db`) plus typed CRUD in `store::runs` and `store::schedules`. `paths::db_file()` added alongside the existing `pid_file()`/`log_file()`. Schema: `runs` (id, task_identity, task, status, session_id, cost_usd, started_at, ended_at, exit_reason, retry_count, next_action, next_action_reason, recheck_after) and `schedules` (id, cron_expr, task, enabled, created_at, last_run_at) — exactly `SPEC.md`'s column list.

Not wired into the run pipeline yet — that's F-08.

## AC items validated

`SPEC.md`'s "Local State Store (F-07)":

- **AC-01** — idempotent schema application. `migrate_is_idempotent`. Achieved via `CREATE TABLE IF NOT EXISTS`; `PRAGMA user_version` is set (`migrate_sets_schema_version`) as groundwork for a future migration to check against, not because anything branches on it yet — there's only one schema version so far.
- **AC-02**/**AC-03** — exact column sets for both tables. `migrate_creates_both_tables`, plus every CRUD test round-tripping the full field set.
- **AC-04** — typed CRUD, only code path touching the DB. `create`/`read`/`list`/`update_status` for `runs`; `create`/`read`/`list`/`update_last_run_at`/`delete` for `schedules` — all parameterized queries, no string-built SQL from caller values anywhere.
- **AC-05** — `RUNNER_HOME` override reaches the DB path too. Covered by `paths.rs`'s existing `derived_paths_are_nested_under_runner_home` test, extended to assert `db_file()`.

21 new unit tests. Every one uses `Connection::open_in_memory()` — no filesystem, no `RUNNER_HOME`, and therefore no env-var race class at all for this feature (a deliberate choice, given `DICT.md`'s "Testing conventions" already documents two prior races from filesystem/env-var polling). `runs.rs`/`schedules.rs`'s test modules call the real, private `migrate` function directly (visible to them as child modules of `store`) rather than maintaining a second copy of the schema SQL that could silently drift out of sync.

One thing worth a second look, not a bug: `rusqlite`'s `bundled` feature pulled `wasm-bindgen`/`sqlite-wasm-rs` into `Cargo.lock` as transitive dependencies, which looked like a red flag for a native macOS build. Confirmed via `cargo build`'s actual compile output that the real path — `libsqlite3-sys` compiled via `cc` — is what gets built for this target; the wasm-related crates are declared for other targets in the lockfile but never compiled here.

`cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (92 tests) re-run 5× with zero flakes.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

None required to proceed to F-08 (Run Lifecycle Persistence), which wires this store into the actual run pipeline.
