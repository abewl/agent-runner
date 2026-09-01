# Changelog v12

**Feature:** F-12 — `runner logs <run-id>`
**Date:** 2026-09-01
**Branch:** `feature/F-12-runner-logs` (off `feature/F-11-runner-status`)

## Summary

`runner logs <run-id>` — full result text (success) or exit reason (failure), plus `next_action`/`next_action_reason`/`recheck_after` (each showing `"none"` when null). **E-03 (CLI Manual Control) is now complete.**

## A second real schema gap, found the same way as the first

F-07's original `runs` column list (`SPEC.md` AC-02, authored during Stage 1 scoping before any feature actually needed to read the data back) had nowhere to store the agent's response text — only metadata and signal fields were persisted. This made F-12's own AC-01 unimplementable as specified. Added `runs.result_text TEXT` directly to the `CREATE TABLE` statement (schema version 2→3, no migration path needed — same reasoning as F-08's `owner_pid`: Stage 1 has never shipped, no real `runner.db` exists with an older shape). `DICT.md` now names this as a pattern worth noticing: two schema gaps (F-08, F-12) both found by writing the feature that needed the missing column, not by reviewing the schema in isolation.

## A refactor that came out of fixing it properly

Storing the raw response text would have reproduced F-10's earlier trailer-duplication bug *inside the stored data* (the `NEXT_ACTION`/`RECHECK_AFTER` lines already have their own columns). Rather than re-solve that in a second place, `strip_trailer_lines` was moved out of `cli/run.rs` into a shared `signal::strip_trailer`, and `persist.rs` now stores `result_text` already stripped. `runner run`'s immediate stdout and `runner logs`'s later retrieval now share one function instead of maintaining two independent copies of the same logic.

## Real verification, not just seeded test data

`runner run "Reply with exactly the word: pong"` followed by `runner logs <id>` against the real `claude` CLI showed the clean `"pong"` result plus a correctly parsed continuation signal — no duplication, matching what the seeded integration tests predicted.

## AC items validated

`SPEC.md`'s "`runner logs <run-id>` (F-12)":

- **AC-01** — full result text on success, exit reason on failure, signal fields with `"none"` fallback. `logs_done_run_shows_result_text_and_signal_fields`, `logs_failed_run_shows_exit_reason` (also confirms a failed run's null signal fields correctly show `"none"`, not blank or a panic).
- **AC-02** — unknown run id is a clear error, non-zero exit, never empty stdout. `logs_unknown_run_id_reports_a_clear_error`.

3 new integration tests, 1 new unit test (`persist::result_text_is_stored_stripped_of_the_trailer`) plus 4 tests relocated from `cli/run.rs` to `signal.rs` (moved, not duplicated — `strip_trailer`'s new home). `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (131 tests: 103 unit + 28 integration) re-run 8× with zero flakes and zero leaked processes.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

E-03 (CLI Manual Control) is complete. Next: E-04 (Scheduler), starting at F-13 (Schedule Store & CLI).
