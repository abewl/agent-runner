# Changelog v13

**Feature:** F-13 — Schedule Store & CLI
**Date:** 2026-09-01
**Branch:** `feature/F-13-schedule-store-cli` (off `feature/F-12-runner-logs`)

## Summary

`runner cron add "<cron-expr>" "<task>"` / `list` / `remove <id>`. New `src/cron_engine.rs` owns cron-expression validation, shared with F-14's tick engine per the AC's own requirement.

## A real discrepancy caught by probing before writing any production code

`DICT.md`'s dependency table claimed the `cron` crate does "standard 5-field cron expression parsing." Before writing `cron_engine.rs`, an isolated scratch project probed the actual crate (`cron` 0.17.0) empirically: it requires a **leading seconds field** — 6 fields minimum, 7 with an optional year, matching its own README example (`sec min hour day month dow year`). A bare 5-field expression like `*/15 * * * *` fails to parse against it directly.

Two options considered: switch to a different crate that natively supports 5-field syntax, or keep `cron` and bridge the gap. Chosen: keep `cron`, and have `cron_engine::parse`/`validate` accept the standard 5-field form Runner's own docs already promised, internally prepending a fixed `"0 "` seconds field before handing the expression to the crate — never exposed to callers. This was also the more correct choice on its own merits, not just the path of least resistance: Runner's daemon tick interval is fixed at 60s (`DICT.md`), so a 6-field syntax with real seconds precision would have been misleading regardless of which crate was used.

Verified against several real expressions (`*/15 * * * *`, daily-at-noon, weekday ranges) producing correct upcoming fire times, and against garbage/wrong-field-count input failing with a descriptive error — before any of this went into the real codebase, not discovered via a later test failure.

## A small duplication cleaned up in passing

`truncate` (task-text truncation for tabular CLI output) was copy-pasted between F-11's `cli/status.rs` and this feature's `cli/cron.rs`. Extracted into a shared `cli/display.rs` — third instance of a "wrote it twice, worth sharing" moment this project has hit (see `DICT.md`'s testing-conventions entries for the test-helper equivalents from F-01/F-02).

## AC items validated

`SPEC.md`'s "Schedule Store & CLI (F-13)":

- **AC-01** — invalid expressions rejected at add-time, nothing written. `cron_add_rejects_an_invalid_expression` (also confirms `runner cron list` shows "no schedules" afterward — the write genuinely didn't happen).
- **AC-02** — `list` shows id, cron expression, task, enabled state, last_run_at (or "never"). `cron_add_accepts_a_valid_expression_and_defaults_enabled`, `cron_list_reports_no_schedules_when_empty`.
- **AC-03** — `remove` deletes by id; a nonexistent id is a clear error. `cron_remove_deletes_an_existing_schedule`, `cron_remove_nonexistent_id_reports_a_clear_error`.
- **AC-04** — new schedules default `enabled = true`. Covered by the same `cron_add_accepts_a_valid_expression_and_defaults_enabled` test.

7 new unit tests (`cron_engine.rs`: 5, `cli/display.rs`: 2 relocated from `status.rs`) and 5 new integration tests (`tests/cron_command.rs`). `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (141 tests: 108 unit + 33 integration) re-run 8× with zero flakes and zero leaked processes.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

One feature left in this batch: F-14 (In-Daemon Cron Tick Engine), which consumes `cron_engine::parse` and the already-implemented-but-unwired `schedules::read`/`update_last_run_at`.
