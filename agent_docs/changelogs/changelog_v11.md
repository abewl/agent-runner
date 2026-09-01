# Changelog v11

**Feature:** F-11 — `runner status` / `runner ps`
**Date:** 2026-09-01
**Branch:** `feature/F-11-runner-status` (off `feature/F-10-runner-run`)

## Summary

`runner status` (alias `ps`, via clap's `#[command(alias = "ps")]`), optionally `--running`. Lists recent runs — id, status, task (truncated to 40 chars with an ellipsis), started_at, duration, next_action — most recent first, reading the store directly. `impl Display for RunStatus` added to `store/runs.rs` as the one canonical status-formatter for this and future features (F-12, F-15) to share.

## Two real things caught before/via testing, not assumed correct

1. **AC gap, caught by re-reading the spec before closing out.** The first draft only printed a computed `duration`, omitting `started_at` as its own displayed field — AC-01 lists both separately. Fixed and covered by a dedicated assertion.
2. **A third instance of the `wait_until`-result-discarded bug**, this time in the shared test helper `force_stop` (`tests/common/mod.rs`) — found via an 8-run back-to-back stress test (more aggressive than the usual 5×), where every run reported "all tests passed" yet a real daemon + `caffeinate` process was left behind, undetected, because `force_stop` sent `SIGTERM` and called `wait_until` without ever checking its return value. Fixed the same way as the first two instances: assert on the result, with a generous 15s budget (test-cleanup slack, not a product AC). Verified fixed across a further 8 consecutive stress runs with zero leaks. `DICT.md` now names the general pattern explicitly — any `wait_until` call whose result isn't asserted is a latent version of this bug, not just these three specific spots.

## AC items validated

`SPEC.md`'s "`runner status` / `runner ps` (F-11)":

- **AC-01** — id, task (truncated), status, started_at, duration, next_action, most-recent-first. `status_lists_a_seeded_run_with_expected_fields` (now also asserting `started_at` appears verbatim), `status_truncates_long_task_text`, plus 8 unit tests for the pure formatting helpers.
- **AC-02** — `--running` filters to only running rows. `status_running_flag_filters_to_running_only`.
- **AC-03** — works without the daemon running. True by construction, same as every other CLI command so far — no test in this project starts a daemon before running non-daemon commands.

13 new tests (8 unit + 5 integration). `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (127 tests: 102 unit + 25 integration) re-run 8× with zero flakes and zero leaked processes after the `force_stop` fix.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

None required to proceed to F-12 (`runner logs`).
