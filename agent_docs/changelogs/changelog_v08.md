# Changelog v08

**Feature:** F-08 — Run Lifecycle Persistence
**Date:** 2026-09-01
**Branch:** `feature/F-08-run-lifecycle-persistence` (off `feature/F-07-local-state-store`)

## Summary

`persist::run_and_persist(conn, task_identity, task, prompt, resume, cwd)` wires F-06's retry pipeline to F-07's store: inserts a `running` row before the attempt, updates it exactly once — `done` with the full result and continuation signal, or `failed` with both failure reasons — never a second row. `persist::reconcile_interrupted_runs(conn)` handles startup cleanup.

## A real schema gap found while implementing AC-04, not before

F-07's `runs` schema (already closed out, `SPEC.md`'s own AC-02 column list) had no way to know *which process* owns a `running` row. AC-04 asks to reconcile "any running row whose owning process is confirmed not alive" — but without a per-row owner, there's no way to distinguish a genuinely orphaned row from one a concurrently active daemon tick is legitimately still working on, which a separate `runner status` invocation running its own startup reconciliation would otherwise see and wrongly mark interrupted. Added `runs.owner_pid` directly to F-07's `CREATE TABLE` statement — schema version bumped 1→2, no `ALTER TABLE` migration path, since Stage 1 has never shipped and no real `runner.db` exists anywhere with the old shape. `DICT.md` now documents this correction.

## AC items validated

`SPEC.md`'s "Run Lifecycle Persistence (F-08)":

- **AC-01** — a `running` row is inserted before the attempt, not after. Implicit in every test (each asserts exactly one row exists by the time the test checks).
- **AC-02** — the same row is updated exactly once on completion, never a second insert. `success_creates_a_running_row_then_marks_it_done`, `failure_marks_the_row_failed_not_a_second_insert` (both explicitly assert `rows.len() == 1`).
- **AC-03** — on success, `next_action`/`next_action_reason`/`recheck_after` are written in the same update as the rest of the completion fields, with `recheck_after` converted from the parsed relative duration to an absolute timestamp. `success_creates_a_running_row_then_marks_it_done`, `success_without_recheck_after_leaves_it_null` (the "no recheck hint" case stays null, not a placeholder).
- **AC-04** — startup reconciliation checks real per-row liveness, not "any running row is stale." `reconcile_marks_only_rows_owned_by_dead_processes` (one row owned by a confirmed-dead pid gets reconciled, one owned by the live test process does not), `reconcile_is_a_noop_when_nothing_is_running`.

Testing approach: `run_and_persist_using` takes the attempt as an injected closure (mirrors `retry.rs`'s own `run_with_retry_using` pattern), so both the success and failure paths are tested with canned `RunOutcome`/`RetryExhausted` values — no subprocess, no real `claude` call, consistent with every prior feature's testing approach in this project.

5 new unit tests in `src/persist.rs` (81 total unit tests, 97 overall with the two integration test files). `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite re-run 5× with zero flakes.

## Conflicts

None beyond the schema correction noted above, which is a gap-fill, not a behavioral conflict.

## Follow-ups for next CLAUDE-PM pass

None required to proceed to F-09 (Session & Continuation Lookup).
