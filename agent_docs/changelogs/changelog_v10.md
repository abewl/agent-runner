# Changelog v10

**Feature:** F-10 — `runner run` (Manual Trigger)
**Date:** 2026-09-01
**Branch:** `feature/F-10-runner-run` (off `feature/F-09-session-continuation-lookup`)

## Summary

`runner run "<task>"` — the command this entire Stage 1 batch has been building toward. Wires everything built so far into one straight-line pipeline: repo config → auth preflight → store open → startup reconciliation → continuation lookup → prompt build → run-with-retry-and-persist. Prints the result and the parsed continuation signal; exits non-zero with a stderr message on any failure.

## Real, end-to-end verification — not just automated tests

The success path was deliberately not automated (see below), so it was verified for real: two manual `runner run` calls against this repo itself as the target (it has `agent_docs/AGENT.md`, satisfying F-02's validation), with the resulting `runs` rows inspected directly via `sqlite3`. Both calls produced `status = done` with the **identical `session_id`**, confirming F-09's `--resume` lookup genuinely works, not just in unit tests — and the second call's cost was substantially lower than the first, consistent with resuming a session with cached context rather than starting fresh.

## A real bug found by that manual test, not by inspection

The printed output showed the `NEXT_ACTION`/`RECHECK_AFTER` trailer **twice**: once embedded in the agent's raw response text (F-05 parses these lines out but correctly never strips them from the stored text — that's not its job) and once from the separately-printed parsed signal. Fixed at the presentation layer only — `cli::run::strip_trailer_lines` removes trailer lines from what gets *displayed*, leaving the stored `runs` row and F-05's parsing untouched.

## Why the success path isn't in the automated suite

Same reasoning as F-04 (`DICT.md` carries the general rationale): a real `claude` call is slow, network-dependent, and consumes real subscription usage. This project's tests get re-run 5–10× per feature to catch races — doing that against the real CLI would be both wasteful and flaky on network conditions for no correctness benefit the failure-path tests and manual verification don't already provide together.

## AC items validated

`SPEC.md`'s "`runner run` — Manual Trigger (F-10)":

- **AC-01** — full pipeline completes standalone, prints result + signal. Verified manually (see above); the pipeline's individual stages are each already unit-tested in their own features.
- **AC-02** — failure exits non-zero with a non-empty stderr message. `run_fails_when_no_repo_configured`, `run_fails_when_claude_not_on_path`, `run_never_exits_nonzero_with_empty_stderr`.
- **AC-03** — task identity is the literal, verbatim task string. Implemented directly (`let task_identity = task;`); exercised by the real manual test reusing the identical string to prove the resume chain.
- **AC-04** — works without the daemon running. True by construction — no test file in this project ever starts a daemon before running these tests, and the manual verification also never started one.

4 new unit tests (`strip_trailer_lines`, `src/cli/run.rs`) + 3 new integration tests (`tests/run_command.rs`). `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (114 tests: 94 unit + 20 integration) re-run 5× with zero flakes.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

E-03 (CLI Manual Control) continues with F-11 (`runner status`/`ps`) and F-12 (`runner logs`), both of which can now show real data from the runs this feature produces.
