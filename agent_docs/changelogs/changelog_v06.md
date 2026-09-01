# Changelog v06

**Feature:** F-06 — Child Crash Handling & Bounded Retry
**Date:** 2026-09-01
**Branch:** `feature/F-06-bounded-retry` (off `feature/F-05-continuation-signal`)

## Summary

`retry::run_with_retry(prompt, resume, cwd)` — wraps F-04's `process::run` and F-05's `signal::parse_continuation_signal` with exactly one automatic retry when either step fails. E-01 (Runtime Core) is now complete: daemon skeleton, repo config, auth preflight, claude spawn+parse, continuation-signal convention, and bounded retry are all built — the raw "run one claude turn correctly and get a structured signal back" capability `EPICS.md` scoped for this epic.

Not yet wired into any CLI command — its consumer is F-10.

## AC items validated

`SPEC.md`'s "Child Crash Handling & Bounded Retry (F-06)":

- **AC-01** — exactly one retry on a process failure, a signal-parse failure, or (implicitly, since it's the same F-04 path) unparseable JSON. `process_failure_then_success_retries_once_and_succeeds`, `signal_parse_failure_then_success_retries_once_and_succeeds`.
- **AC-02** — a second consecutive failure carries both failure reasons, not just the second. `both_attempts_failing_surfaces_both_reasons_and_stops_at_two_calls`, `retry_exhausted_display_mentions_both_attempts`.
- **AC-03** — a successful retry isn't surfaced as an error, and the retry fact is available for F-08 to persist. `first_attempt_success_is_not_marked_retried` (confirms `retried: false` and exactly one call on the clean-success path); `RunOutcome.retried: bool` carries the fact itself.

The retry policy is tested via dependency injection (a closure standing in for "one attempt," driven by a `Cell<u32>` call counter) rather than real or fake subprocesses — this is a pure control-flow property (call once, maybe call again, stop) that doesn't need process-spawning machinery to verify, and it also directly proves the "never a third call" bound that matters most for AC-01/AC-02.

6 new unit tests in `src/retry.rs`. `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (76 tests) re-run 5× with zero flakes.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

E-01 (Runtime Core) is complete. Next: E-02 (Persistence), starting at F-07 (Local State Store — SQLite).
