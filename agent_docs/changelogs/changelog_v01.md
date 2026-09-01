# Changelog v01

**Feature:** F-01 — CLI Skeleton & Daemon Lifecycle
**Date:** 2026-09-01
**Branch:** `feature/F-01-cli-skeleton-daemon-lifecycle` (off `develop`)

## Summary

First implemented feature of the Runner project. Stood up the `runner` Rust binary crate (`clap` CLI, `edition = "2024"`) with a `daemon` subcommand group:

- `runner daemon start` — validates no other instance is already running (own liveness pre-check against the pid file, not relying solely on OS-level locking), then daemonizes via the `daemonize` crate (double-fork/setsid, stdio redirected to `$RUNNER_HOME/logs/runner.log`, working directory `$RUNNER_HOME`). Once detached, the daemon body (`daemon::run()`) builds a `tokio` multi-threaded runtime, registers `SIGTERM`/`SIGINT` handlers, **then** writes its own pid file, spawns `caffeinate -s -w <own-pid>` (fire-and-forget, self-terminating with the daemon), and awaits a shutdown signal.
- `runner daemon stop` — sends `SIGTERM`, polls for exit up to 5s, removes the pid file on confirmed exit; reports a clear error if nothing is running or if the wait times out.
- `runner daemon status` — reports `running (pid N)` / `stopped`, cleaning up a stale pid file (dead process, file left behind) as a side effect.

Logging goes through `tracing` to a dedicated file under `$RUNNER_HOME/logs/`, independent of the stdio redirection `daemonize` also sets up as a fallback.

## AC items validated

All of `SPEC.md`'s "CLI Skeleton & Daemon Lifecycle (F-01)", AC-01 through AC-09:

- **AC-01** — `cargo build --release` produces a single `runner` binary. Validated by a clean `cargo build`.
- **AC-02** — `daemon start` detaches and returns control promptly, pid file appears. `start_detaches_promptly_and_writes_pid_file`.
- **AC-03** — a second `start` while already running is rejected with `already running (pid N)`. `second_start_while_running_is_rejected`.
- **AC-04** — `stop` sends `SIGTERM`, waits up to 5s, removes the pid file on exit; clear error when nothing is running. `stop_terminates_daemon_and_removes_pid_file`, `stop_when_not_running_reports_error`.
- **AC-05** — `status` reports `running (pid N)` / `stopped`, and cleans up a stale pid file. `status_reports_stopped_when_never_started`, `status_reports_running_while_daemon_is_up`, `status_cleans_up_stale_pid_file`.
- **AC-06** — clean shutdown on both `SIGTERM` and `SIGINT`, no orphaned children. `stop_terminates_daemon_and_removes_pid_file` (SIGTERM via `stop`), `sigint_also_triggers_clean_shutdown` (SIGINT sent directly — this is the test that caught the real bug below).
- **AC-07** — log output goes through `tracing` to a real file under `$RUNNER_HOME/logs/`. `log_file_is_created_and_non_empty_after_start`.
- **AC-08** — `caffeinate -s -w <own-pid>` spawned after detach; missing binary degrades to a warning, not a crash. `caffeinate_is_spawned_and_self_terminates_on_stop` (spawn half); graceful-degradation half covered by code review — `Command::spawn()`'s `Err` arm logs and continues rather than propagating, no dedicated test forces `caffeinate` off `PATH` in this batch.
- **AC-09** — no explicit kill of the `caffeinate` child anywhere in the codebase; it self-terminates via `-w <pid>`. `caffeinate_is_spawned_and_self_terminates_on_stop` (terminates-on-stop half); "no kill code path exists" is a structural property, verified by inspection rather than a test that could assert an absence.

19 tests total: 9 unit (`src/paths.rs`, `src/pid.rs`), 10 integration (`tests/daemon_lifecycle.rs`). `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` all clean. Full suite re-run repeatedly (isolated and combined, single- and multi-threaded) with zero flakes and zero leaked processes after the fix below.

## Bug found and fixed during this feature

`daemonize`'s built-in `.pid_file()` option writes the pid file *before* this codebase's own code runs inside the detached child — meaning it was written before `tokio::signal::unix` handlers got installed. That left a real window where a signal sent the instant the pid file appeared would hit the OS's default disposition (immediate termination, no cleanup) instead of our handler, contradicting AC-06's own "rather than relying on default OS termination behavior" intent.

This wasn't caught by the `SIGTERM`-via-`stop` test, because `cli::daemon::stop()` has its own redundant client-side pid-file removal after confirming the process exited — that removal happened regardless of whether the daemon's *own* internal cleanup ran, masking the race. It surfaced only once a test sent `SIGINT` directly (bypassing `stop`), which is why AC-06 — covering both signals — got its own dedicated test rather than being folded into the `SIGTERM` one.

Fixed structurally, not by adding a delay: `daemonize.pid_file(...)` was removed entirely; `daemon::run()` now registers signal handlers first and only then writes the pid file itself (`daemon::write_pid_file`). By the time the pid file is visible to anything (including our own `stop`), the handlers are already live.

## Conflicts

None — implementation matched `SPEC.md`'s ACs as written; no feature-definition tiebreak needed.

## Follow-ups for next CLAUDE-PM pass

None required to proceed to F-02. `DICT.md`'s repo layout and dependency table were updated in place to reflect what F-01 actually built (added `daemonize`/`libc` to the locked dependency table, added `paths.rs`/`pid.rs`, documented the signal-handlers-before-pid-file ordering as load-bearing) — worth reading before F-02, since F-02 (target repo config) will add `config.rs` alongside the `paths.rs` this feature introduced.
