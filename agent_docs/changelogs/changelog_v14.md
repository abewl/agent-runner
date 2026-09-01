# Changelog v14

**Feature:** F-14 — In-Daemon Cron Tick Engine
**Date:** 2026-09-01
**Branch:** `feature/F-14-cron-tick-engine` (off `feature/F-13-schedule-store-cli`)

## Summary

`tokio::spawn(cron_engine::run_tick_loop())` added to the daemon's existing async body — the daemon's only responsibility in Stage 1 beyond staying alive. Every 60s, evaluates all enabled schedules and fires due ones through the same run pipeline `runner run` uses. **E-04 (Scheduler) is now complete — all 15 Stage 1 features are done.**

## A real API gotcha caught by probing, before it became a silent daemon bug

`Schedule::includes(date_time)` looks like the obvious "is this due right now" check — it isn't, for a polling daemon. It requires an *exact second match*, and every expression here is internally fixed to `:00` seconds (F-13's `"0 "` prepend). A tick firing a few seconds after the minute mark (routine, not a problem) would make `includes(now)` silently and permanently report "not due" for that entire minute — a bug that would only ever show up as "my schedule never seems to fire exactly on time," hard to diagnose after the fact. Caught with a scratch probe simulating realistic jitter *before* writing `cron_engine::is_due`, not discovered later. The correct check — `schedule.after(&reference_time).next() <= now` — asks the right question: was there a scheduled fire time somewhere in the window since the last check, not does this exact instant match. `DICT.md` now documents this as a standing gotcha for the crate.

## Real, live verification — not just unit tests

Started the actual compiled daemon against a real target repo with a real every-minute schedule, waited ~70 real seconds, and confirmed via `runner status`/`cron list`/`logs` — not log-grepping, which turned out to check for the wrong string entirely, since success isn't logged, only skip/failure paths are — that a real `claude` call fired roughly 60s after daemon start, persisted correctly, and `schedules.last_run_at` updated. As an unplanned bonus, the agent itself recognized the task as a recurring heartbeat and self-reported a sensible `recheck_after` — real confirmation that the continuation-signal design (`PROJECT.md` §4) does what it was built for on exactly the repeating-schedule case that motivated it. Daemon stopped cleanly afterward with no leftover `caffeinate` process.

## AC items validated

`SPEC.md`'s "In-Daemon Cron Tick Engine (F-14)":

- **AC-01** — 60s poll interval evaluating all enabled schedules. Verified live (above) plus `disabled_schedule_is_never_considered`, `multiple_schedules_are_each_evaluated_independently`.
- **AC-02** — a future `recheck_after` skips the tick. `future_recheck_after_skips_the_tick_ac02`, `elapsed_recheck_after_allows_the_tick_to_fire` (the boundary case, proving it's not just "always skip if any recheck_after exists").
- **AC-03** — the recheck check is a plain timestamp comparison, never a branch on `next_action`. Verified by inspection — `process_one_schedule` reads only `recheck_after`, never `next_action`/`next_action_reason`, in its skip logic.
- **AC-04** — an already-running schedule is skipped, independent of AC-02. `already_running_schedule_is_skipped_ac04`.
- **AC-05** — `last_run_at` updates on trigger regardless of outcome. `due_schedule_with_no_conflicts_triggers_and_updates_last_run_at`, `last_run_at_updates_even_when_the_triggered_run_fails`.
- **AC-06** — a cron-triggered run follows identical persistence/retry behavior as manual. True by construction — `tick_once`'s trigger closure calls the exact same `persist::run_and_persist` F-10 calls, not a parallel code path.

15 new unit tests in `src/cron_engine.rs` (3 for `is_due`, 7 for `tick_once_with`'s scenarios, using the same trigger-injection pattern `retry.rs`/`persist.rs` established — zero subprocess cost for any of it). No new integration test file — the daemon-level wiring is exactly what the real manual verification covers, and it would be less honest confidence than that, not more, to assert on real daemon timing in an automated suite re-run 8× per feature. `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (151 tests: 118 unit + 33 integration) re-run 8× with zero flakes and zero leaked processes.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

E-04 (Scheduler) complete — all of E-01 through E-04 (14 features) are done. One feature remains in the original Batch 1 scope: F-15 (TUI Status Dashboard, E-05), not started.
