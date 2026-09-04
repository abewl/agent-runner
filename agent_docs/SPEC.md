# SPEC.md — Acceptance Criteria Glossary

AC items are append-only. Revise in place when behavior changes; never delete historical entries.
**Last updated:** 2026-08-30

---

## CLI Skeleton & Daemon Lifecycle (F-01)
- AC-01: `cargo build --release` produces a single binary named `runner` with no required runtime dependency beyond the OS.
- AC-02: `runner daemon start` launches a detached, long-lived background process and returns control to the shell immediately (does not block the terminal). A PID file is written to `$RUNNER_HOME/runner.pid` (default `~/Library/Application Support/runner/runner.pid`).
- AC-03: `runner daemon start` run a second time while an instance is already running (live PID file, process confirmed alive) exits non-zero with a clear "already running (pid N)" message and does not spawn a second daemon.
- AC-04: `runner daemon stop` reads the PID file, sends `SIGTERM`, and waits up to 5 seconds for the process to exit before reporting failure. On confirmed exit, the PID file is removed.
- AC-05: `runner daemon status` reports `running (pid N)` or `stopped` correctly, including correctly reporting `stopped` when a stale PID file exists but the process is not alive (and removes the stale file).
- AC-06: The daemon process installs handlers for `SIGTERM` and `SIGINT` that perform a clean shutdown (no orphaned child processes, PID file removed) rather than relying on default OS termination behavior.
- AC-07: All daemon/CLI log output is written via `tracing` to a log file under `$RUNNER_HOME/logs/`, not solely to stdout, so `daemon start`'s detached process's logs remain inspectable.
- AC-08: After the daemon has detached and written its PID file, it spawns `caffeinate -s -w <own-pid>` (own PID, not the CLI invocation's PID) as an independent child process, not tracked or waited on further by the daemon. If the `caffeinate` binary is not found on `PATH` (e.g. running somewhere other than macOS), this is logged as a warning — the daemon still starts and runs normally, sleep-prevention simply isn't active.
- AC-09: No explicit kill of the `caffeinate` child is issued by `daemon stop` or by crash-cleanup — `-w <pid>` already ties its lifetime to the daemon's PID, so it self-terminates on any daemon exit, clean or not. A test verifying `daemon stop` leaves no orphaned `caffeinate` process for that instance is sufficient; no new termination code path should be added for this.

## Target Repo Configuration (F-02)
- AC-01: `runner repo set <path>` fails clearly (non-zero exit, no config written) if `<path>` does not exist, is not a directory, or does not contain `agent_docs/AGENT.md`.
- AC-02: On success, `runner repo set <path>` writes the **canonicalized, absolute** path to `$RUNNER_HOME/config.toml` under a `repo_path` key, overwriting any previously configured value.
- AC-03: `runner daemon start --repo <path>` performs the same validation and write as `runner repo set <path>` before proceeding to daemon startup; a validation failure prevents the daemon from starting.
- AC-04: `runner repo show` prints the currently configured absolute path, or a clear "no repo configured — run `runner repo set <path>`" message if none is set (distinguishing this from any other error state).

## Local Ambient-Auth Preflight (F-03)
- AC-01: Before any `claude` subprocess is spawned (manual or scheduled), a preflight check runs that confirms the `claude` binary resolves on `PATH`. If not found, the run fails immediately with a message naming the missing binary — no subprocess spawn is attempted.
- AC-02: The preflight check further confirms a usable `claude` login/session exists, distinguishing "not logged in" from "binary present but unauthenticated" in the error message where the underlying `claude` CLI makes that distinguishable.
- AC-03: A preflight failure is recorded on the resulting `runs` row (once F-08 exists) with a distinct `exit_reason` category from a normal task-execution failure, so `runner logs` can show "auth preflight failed" rather than a generic error.

## Claude Subprocess Runner (F-04)
- AC-01: Given a non-empty prompt string, the runner spawns `claude --print --output-format json "<prompt>"` (plus `--resume <sessionId>` when provided) as a subprocess with piped stdout/stderr and no PTY allocation, with its working directory set to the path from F-02's config (fails clearly, per F-02 AC-04's "no repo configured" case, if none is set).
- AC-02: On successful completion, the parsed `result` event's `result` text, `session_id`, and `cost_usd` are returned in a typed success value.
- AC-03: If `--output-format json` output is not valid JSON, or contains no `result` event, the runner falls back to extracting text from any `assistant` message content blocks present, matching the fallback behavior already proven in chili-jar's adapter; if neither is present, it returns a typed error containing the raw captured output (truncated) rather than panicking.
- AC-04: A non-zero subprocess exit code always produces a typed error (never a silent success), even if partial stdout was captured.
- AC-05: `claude` is invoked with its working directory as the F-02-configured target repo (not a Runner-owned scratch directory) — this is a required parameter to the spawn call, not a default with an override.
- AC-06: No git operations (clone, branch, push) or MCP config file writing occur in this feature — invocation is a plain prompt string in, result out; file changes happen because `claude`'s own tools write directly into the repo working tree it was launched in.

## Continuation Signal — Prompt Convention & Parsing (F-05)
- AC-01: Every prompt constructed by the run pipeline (regardless of caller — `runner run` or the cron tick engine) has the identical fixed trailer instruction appended, verbatim, requesting a `NEXT_ACTION: <label> — <reason>` line and an optional `RECHECK_AFTER: <duration>` line. There is no second prompt template anywhere in the codebase.
- AC-02: `NEXT_ACTION: ...` is parsed from F-04's result text by matching the fixed line prefix (not a general-purpose markdown/prose parse) into `next_action: String` and `reason: String`.
- AC-03: `RECHECK_AFTER: <duration>`, when present, is parsed into a `Duration` value; recognised units are at minimum minutes (`m`), hours (`h`), and days (`d`) (e.g. `30m`, `2h`). An unparseable duration string is treated the same as F-06's "malformed output" case, not silently dropped.
- AC-04: When no `RECHECK_AFTER:` line is present, the parsed result carries `recheck_after: None` — this is a valid, expected outcome (not an error), used whenever the agent has nothing to suggest deferring.
- AC-05: No code path anywhere in the codebase branches on the *value* of `next_action` or `reason` — grep-verifiable: these two fields are only ever stored, formatted for display, or formatted into the next prompt's context line, never compared against a string constant in an `if`/`match`.

## Child Crash Handling & Bounded Retry (F-06)
- AC-01: When F-04 returns a typed error (non-zero exit or unparseable output), or F-05 fails to find a `NEXT_ACTION:` line at all, exactly one automatic retry is performed immediately (no delay/backoff required in this batch) before surfacing failure to the caller.
- AC-02: If the retry also fails (by any of the three trigger conditions in AC-01), the caller receives a terminal error value that includes both failure reasons (first and second attempt), not just the second.
- AC-03: A successful retry (first attempt failed, second succeeded with a valid trailer) is *not* surfaced as an error to the caller — the caller receives the successful result and parsed signal, with the retry fact available for logging/persistence (F-08's `retry_count`).

## Local State Store (F-07)
- AC-01: On first run (CLI or daemon), if `$RUNNER_HOME/runner.db` does not exist, it is created and the `runs` and `schedules` schemas are applied. On subsequent runs, schema application is idempotent (no error, no duplicate application) via a migration-version check.
- AC-02: `runs` table columns: `id` (TEXT PK), `task_identity` (TEXT NOT NULL — see `DICT.md` "Run identity vs. task identity"), `task` (TEXT NOT NULL), `status` (TEXT CHECK IN running|done|failed|interrupted), `session_id` (TEXT nullable), `cost_usd` (REAL nullable), `started_at` (TEXT ISO-8601 NOT NULL), `ended_at` (TEXT ISO-8601 nullable), `exit_reason` (TEXT nullable), `retry_count` (INTEGER NOT NULL DEFAULT 0), `next_action` (TEXT nullable), `next_action_reason` (TEXT nullable), `recheck_after` (TEXT ISO-8601 nullable).
- AC-03: `schedules` table columns: `id` (TEXT PK), `cron_expr` (TEXT NOT NULL), `task` (TEXT NOT NULL), `enabled` (INTEGER NOT NULL DEFAULT 1), `created_at` (TEXT ISO-8601 NOT NULL), `last_run_at` (TEXT ISO-8601 nullable).
- AC-04: Typed CRUD functions exist for both tables (create/read/list/update — delete only required for `schedules`, per F-13) and are the *only* code path that touches the database; no raw SQL string-building occurs outside this module.
- AC-05: `RUNNER_HOME` env var, when set, overrides the default `~/Library/Application Support/runner/` base path for the DB file, PID file, config file, logs, and any working paths alike.

## Run Lifecycle Persistence (F-08)
- AC-01: A `runs` row with `status = "running"`, a freshly generated `id`, and the caller-supplied `task_identity` is inserted before the `claude` subprocess is spawned, not after.
- AC-02: On completion, the same row is updated exactly once — never a second insert for the same logical run — setting `status`, `session_id`, `cost_usd`, `ended_at`, `exit_reason` (set only on failure), and `retry_count` (from F-06).
- AC-03: When the run reaches `status = "done"` with a successfully parsed continuation signal (F-05), `next_action`, `next_action_reason`, and `recheck_after` (converted from the parsed relative duration to an absolute ISO-8601 timestamp, `started_at`/completion time + duration) are written to the same row in the same update as AC-02 — not a separate write.
- AC-04: On daemon or CLI process startup, any `runs` row with `status = "running"` whose owning process is confirmed not alive (no matching live PID, or PID file absent/stale) is updated to `status = "interrupted"` within the same startup pass — before any new run is accepted — with `next_action`/`next_action_reason`/`recheck_after` left null on that row (an interrupted run produced no valid signal to persist).

## Session & Continuation Lookup (F-09)
- AC-01: Given a task identity string, the most recent `runs` row for that identity with `status = "done"` and a non-null `session_id` supplies the `--resume` value for the next invocation of that identity; if no such row exists, the invocation proceeds without `--resume`.
- AC-02: The same lookup, when it finds a qualifying row, also returns its `next_action`/`next_action_reason` (when non-null) formatted as a short one-line context string prepended to the next prompt (e.g. `"Your own last recommendation was: <next_action> — <reason>"`); when absent, no such line is added — the prompt is not padded with an empty placeholder.
- AC-03: A `runs` row with `status = "failed"` or `"interrupted"` is never used as the source for either the `--resume` value or the continuation context line — only `"done"` rows qualify.

## `runner run` — Manual Trigger (F-10)
- AC-01: `runner run "<task>"` with no daemon running completes the full pipeline (preflight → continuation lookup → run-with-retry → persistence) and exits 0 on success, printing the result text plus the parsed `NEXT_ACTION`/`RECHECK_AFTER` to stdout.
- AC-02: On failure (preflight or both attempts of the run), `runner run` exits non-zero and prints the error detail to stderr — never a silent non-zero exit with empty output.
- AC-03: The task identity used for F-09's lookup for a manual run is the literal, verbatim task string (two manual runs with the exact same task string share a resume/continuation chain; a different string starts fresh).
- AC-04: `runner run` works correctly whether or not `runner daemon start` has ever been invoked — it does not require or check for a running daemon.

## `runner status` / `runner ps` (F-11)
- AC-01: `runner status` lists runs most-recent-first, showing at minimum: id, task (truncated to a reasonable display width), status, started_at, duration (computed from `started_at`/`ended_at`, or "running" elapsed time for in-progress rows), and `next_action` (or blank if null).
- AC-02: `runner status --running` (or equivalent flag) filters the list to only rows with `status = "running"`.
- AC-03: The command reads the store directly and produces correct output whether or not the daemon process is currently running.

## `runner logs <run-id>` (F-12)
- AC-01: `runner logs <run-id>` prints the full result text (on success) or the captured error/failure detail (on failure) for the given run id, plus its `next_action`, `next_action_reason`, and `recheck_after` (or "none" for any null field).
- AC-02: An unknown run id produces a clear "no such run" error on stderr with non-zero exit, never empty stdout.

## Schedule Store & CLI (F-13)
- AC-01: `runner cron add "<cron-expr>" "<task>"` validates the cron expression using the same parser F-14's tick engine uses; an invalid expression is rejected at add-time with a clear error, and no `schedules` row is inserted.
- AC-02: `runner cron list` shows all schedules with id, cron expression, task (truncated), enabled state, and `last_run_at` (or "never").
- AC-03: `runner cron remove <schedule-id>` deletes the schedule by id; removing a non-existent id produces a clear error rather than silently succeeding.
- AC-04: A newly added schedule defaults to `enabled = true`.

## In-Daemon Cron Tick Engine (F-14)
- AC-01: While the daemon is running, an internal ticker evaluates all `enabled` schedules against the current time at a fixed 60-second poll interval.
- AC-02: When a schedule's cron expression indicates it is due, the daemon first checks whether that schedule's most recent `runs` row has a non-null `recheck_after` timestamp still in the future; if so, the tick is skipped for that schedule and a log line records the skip with the schedule id and the recheck timestamp.
- AC-03: The AC-02 check is a plain timestamp comparison (`recheck_after > now`) — it does not read, parse, or branch on `next_action` or `next_action_reason` in any way.
- AC-04: If a schedule already has a `runs` row with `status = "running"` at tick time, that schedule is also skipped for the current tick (independent of AC-02) — no second concurrent run of the same schedule is started — and a log line records this distinctly from an AC-02 skip.
- AC-05: When a schedule is not skipped, the daemon triggers the same underlying run pipeline `runner run` uses (in-process call, not a subprocess shell-out to the `runner` binary), using the schedule's `id` as the task identity for F-09's lookup, and updates `schedules.last_run_at` to the trigger time regardless of whether the resulting run ultimately succeeds or fails.
- AC-06: A cron-triggered run, once started, follows the identical persistence and retry behavior (F-06, F-08) as a manually triggered run — no separate code path.

## TUI Status Dashboard (F-15)
- AC-01: `runner log` renders a scrollable list of runs (id, task, status, started_at, next_action), most-recent-first, sourced from the same store `runner status` reads.
- AC-02: The list refreshes automatically on a fixed 2-second poll interval without requiring user input, and without flickering/full-redraw artifacts on each refresh.
- AC-03: Selecting a run (arrow keys + enter, or equivalent) shows a detail pane with the same result/error/next_action/reason/recheck_after content `runner logs` would print for that run id.
- AC-04: No keybinding in this view mutates any run or schedule state (no kill/retry/delete) — confirmed by inspection: no write-path store functions are reachable from the TUI's input handling in this batch.
- AC-05: `q` or `Ctrl+C` exits the TUI and fully restores the terminal to its prior state (no leftover alternate-screen mode or hidden cursor).

---

## Entry format (reference)

```markdown
## [Feature Name] (F-XX)
- AC-XX: [expected behavior]
- AC-XX: [expected behavior]
```
