# Feature Backlog — Runner

**Last updated:** 2026-08-30

---

## Batch 1 — Stage 1: Local Persistent Daemon, CLI, Cron, TUI (E-01 – E-05)

Scope: `agent_docs/PROJECT.md` §2–§4, Stage 1 only. A single static Rust binary (`runner`) providing a macOS-only, fully local persistent daemon that runs the `claude` CLI non-interactively, under ambient subscription auth, against exactly one configured target repository's own `agent_docs/` convention — plus a CLI for manual control, built-in cron-style scheduling, and a simple read-only TUI. PM/Engineer chaining is an emergent property of a self-reported continuation signal the agent produces every turn, never orchestration logic in Runner. No chili-jar network/MCP dependency, no push/PAT, no remote/API-key mode — all explicitly deferred (`PROJECT.md` §2 "Out of scope"). Ordered by dependency; build top to bottom.

**Revision note:** supersedes the original Batch 1 draft. Nothing below has been implemented — this is a clean pre-implementation renumbering, not an append, per `EPICS.md`'s revision note.

---

## F-01: CLI Skeleton & Daemon Lifecycle [2026-08-30] [2026-09-01]
- Status: [x] done
- AC: AC-01, AC-02, AC-03, AC-04, AC-05, AC-06, AC-07, AC-08, AC-09
- Ticket:
- Description: Initialise the `runner` binary crate. `clap` (derive)-based CLI with a `daemon` subcommand group: `runner daemon start`, `runner daemon stop`, `runner daemon status`. `start` forks/detaches into a long-lived background process, writes a PID file to the Stage 1 data directory, and installs SIGTERM/SIGINT handlers for clean shutdown. Once detached and its PID file is written, the daemon spawns `caffeinate -s -w <own-pid>` as an independent child so the machine cannot sleep for as long as the daemon is alive — no separate lifecycle management needed, since `-w <pid>` makes `caffeinate` self-terminate the moment the daemon's PID exits, including on a crash, not just a clean `daemon stop`. `stop` reads the PID file and sends SIGTERM, waiting briefly for exit before reporting failure. `status` reports running/stopped by checking the PID file against the live process table. Logging via `tracing` + `tracing-subscriber` to a log file under the data directory. This feature does not yet run any `claude` invocation or scheduler tick — it's the process shell everything else attaches to. Foundational — every other feature depends on this.
- Completion note: All 9 ACs implemented and covered by tests — 9 unit tests (`src/paths.rs`, `src/pid.rs`) for pure path/pid-parsing logic, 10 integration tests (`tests/daemon_lifecycle.rs`) driving the compiled binary end-to-end for the lifecycle behavior (detach timing, already-running rejection, stop/status/stale-pid-file handling, log file content, caffeinate spawn + self-termination, SIGINT as well as SIGTERM). `cargo build`, `cargo clippy --all-targets`, and `cargo fmt --check` all clean; full suite re-run 5×/3× (isolated and combined) with zero flakes and zero leaked processes after the fix below. **One real bug found and fixed via the AC-06 SIGINT test, not by inspection**: `daemonize`'s built-in `.pid_file()` option writes the pid file *before* our code gets a chance to install `tokio::signal::unix` handlers, leaving a real window where a signal sent immediately after the pid file appears hits the OS's default disposition (immediate termination, no cleanup) instead of our handler — contradicting AC-06's own "rather than relying on default OS termination behavior" intent. Fixed by not using `daemonize`'s `.pid_file()` at all: signal handlers are now registered first inside `daemon::run()`, and only then does the daemon write its own pid file, closing the race structurally rather than papering over it with a test-side delay. No AC conflicts. No follow-up items — the next CLAUDE-PM-relevant note is in `DICT.md`'s repo-layout section, which now reflects the actual module split (`daemon::write_pid_file` owns the pid file, not `cli::daemon`/`daemonize`).

## F-02: Target Repo Configuration [2026-08-30] [2026-09-01]
- Status: [x] done
- AC: AC-01, AC-02, AC-03, AC-04
- Ticket:
- Description: `runner repo set <path>` validates the path exists, is a directory, and contains `agent_docs/AGENT.md` (Runner operates on the `agent_docs/` convention — a directory without it isn't a valid target), then writes it to a small config file (`$RUNNER_HOME/config.toml`). `runner daemon start --repo <path>` is a convenience that calls the same validation/write before starting. `runner repo show` prints the currently configured path (or a clear "not set" message). Stage 1 supports exactly one configured repo at a time — setting a new one replaces the old value, it does not add to a list. Depends on: F-01.
- Completion note: All 4 ACs implemented and covered by tests — 8 unit tests (`src/config.rs`, validation edge cases + save/load roundtrip) and 7 integration tests (`tests/repo_command.rs`, CLI behavior including the `daemon start --repo` path). Also refactored F-01's integration tests to share a new `tests/common/mod.rs` module instead of duplicating helpers. `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (34 tests across unit + 2 integration files) re-run 10× with zero flakes and zero leaked processes after the fixes below. **Two more real races found and fixed via repeated runs, not by inspection** (see `DICT.md` "Testing conventions" for the general lesson each one leaves behind): (1) `config::tests` and `paths::tests` each declared their own `ENV_LOCK` mutex to serialize mutation of the process-global `RUNNER_HOME` env var — two independent locks don't serialize against each other, so the two modules' tests could still race on the same env var under parallel execution; consolidated to one shared `paths::ENV_LOCK`. (2) `wait_for_pid_file` checked file existence and then read separately, which could observe the file present-but-empty in the gap between `daemonize::write_pid_file`'s create/truncate and its actual write; fixed to poll the parse itself, not just existence. No AC conflicts. No follow-up items for the next CLAUDE-PM pass.

## F-03: Local Ambient-Auth Preflight [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03
- Ticket:
- Description: A preflight check, run before any `claude` invocation, that confirms the `claude` binary resolves on `PATH` and that a usable login/session exists (via a lightweight, non-mutating check — e.g. `claude --version` succeeding is necessary but not sufficient; prefer whatever check most reliably distinguishes "not logged in" from "logged in" without starting a billable session). On failure, produces a distinct, actionable error message (not a generic "command failed") surfaced identically whether triggered from `runner run` or the daemon's scheduler tick. No credential injection or credential file handling of any kind — Stage 1 trusts whatever `claude` login state already exists on the machine. Depends on: F-01.
- Completion note:

## F-04: Claude Subprocess Runner [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03, AC-04, AC-05, AC-06
- Ticket:
- Description: The core "run one claude turn" primitive, reusable by both manual and scheduled runs. Given a task prompt string and an optional `--resume` session ID, spawns `claude --print --output-format json [--resume <sessionId>] "<prompt>"` as a plain subprocess (piped stdout/stderr, no PTY) with its working directory set to the configured target repo (F-02) — `claude` reads and writes that repo's own `agent_docs/*` files using its own file tools; Runner passes no MCP config and does not itself read those files. Parses the `--output-format json` event array to extract the result text, `session_id`, and `cost_usd` from the `result` event, following the same parsing shape already proven in chili-jar's `adapters/claude-code/index.mjs` (`chili-jar/packages/harness/adapters/claude-code/index.mjs` lines ~296–337). Non-zero exit or unparseable output produces a typed error, never a panic. Depends on: F-01, F-02, F-03.
- Completion note:

## F-05: Continuation Signal — Prompt Convention & Parsing [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03, AC-04, AC-05
- Ticket:
- Description: Every prompt Runner constructs (manual or scheduled — one shared template, no PM-prompt/Engineer-prompt branching) appends a fixed trailer instruction asking the agent to end its response with `NEXT_ACTION: <short label> — <one-line reason>` and, optionally, `RECHECK_AFTER: <duration>` (e.g. `30m`, `2h`). This feature owns both sides: constructing that trailer instruction, and parsing the two fixed-prefix lines back out of F-04's result text into a typed struct (`next_action: String`, `reason: String`, `recheck_after: Option<Duration>`). `next_action` and `reason` are treated as opaque strings — no validation against a fixed vocabulary, no branching on their value anywhere in this codebase. A missing `NEXT_ACTION:` line is treated as malformed output (feeds into F-06's retry path), not silently ignored. Depends on: F-04.
- Completion note:

## F-06: Child Crash Handling & Bounded Retry [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03
- Ticket:
- Description: Wraps F-04+F-05 with a single, automatic, immediate retry when the `claude` invocation exits non-zero, produces unparseable JSON, or produces output with no `NEXT_ACTION:` trailer — all three count as "didn't get a usable response," one retry bucket. A second consecutive failure returns a terminal error to the caller with both failure reasons captured. No retry loop beyond one — this is not a backoff/supervisor policy, just enough resilience to absorb a single flaky invocation. Depends on: F-04, F-05.
- Completion note:

## F-07: Local State Store [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03, AC-04, AC-05
- Ticket:
- Description: An embedded SQLite store (`rusqlite`, bundled feature — no external SQLite dependency) initialised at `~/Library/Application Support/runner/runner.db` (overridable via a `RUNNER_HOME` env var for testing). Runs schema migration idempotently on every daemon/CLI startup. Two tables: `runs` (id, task_identity, task, status, session_id, cost_usd, started_at, ended_at, exit_reason, retry_count, next_action, next_action_reason, recheck_after) and `schedules` (id, cron_expr, task, enabled, created_at, last_run_at). This feature only stands up the store and schema plus typed CRUD functions — no wiring into the run pipeline yet (that's F-08). Depends on: F-01.
- Completion note:

## F-08: Run Lifecycle Persistence [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03, AC-04
- Ticket:
- Description: Wires F-06's run pipeline to F-07's store: a `runs` row is inserted with `status = "running"` before the `claude` subprocess starts, and updated exactly once on completion (`status = "done" | "failed"`, `session_id`, `cost_usd`, `ended_at`, `exit_reason` if failed, `retry_count`, and — when a run reaches a terminal state with a parsed signal — `next_action`, `next_action_reason`, `recheck_after`). On daemon/CLI startup, any `runs` row still `status = "running"` from a prior process that is confirmed not alive is reconciled to `status = "interrupted"` — never silently resumed automatically, and its `next_action`/`recheck_after` fields are left null (an interrupted run produced no valid signal). Depends on: F-06, F-07.
- Completion note:

## F-09: Session & Continuation Lookup [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03
- Ticket:
- Description: Given a task identity (the literal task string for manual runs; the schedule ID for scheduled runs — see F-13/F-14), retrieves the most recent `runs` row for that identity with `status = "done"` and returns both (a) its `session_id`, fed into F-04 as `--resume`, and (b) its `next_action`/`next_action_reason`, formatted into a short context line prepended to the next prompt (e.g. "Your own last recommendation was: <next_action> — <reason>"). If no prior `"done"` row exists for that identity, the invocation proceeds without `--resume` and without prior-signal context. Depends on: F-08.
- Completion note:

## F-10: `runner run` — Manual Trigger [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03, AC-04
- Ticket:
- Description: `runner run "<task>"` — a standalone CLI command that does not require the daemon to be running. Executes the full pipeline (F-03 preflight → F-09 continuation lookup → F-06 run-with-retry → F-08 persistence) synchronously and prints the result text to stdout on completion (plus the parsed `NEXT_ACTION`/`RECHECK_AFTER` for visibility), or the error to stderr with non-zero exit on failure. This is the command every other manual-control feature and the scheduler (F-14) ultimately calls into. Depends on: F-03, F-06, F-08, F-09.
- Completion note:

## F-11: `runner status` / `runner ps` [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03
- Ticket:
- Description: `runner status` (alias `runner ps`) lists recent runs from the store — id, task (truncated), status, started_at, duration, and last `next_action` — most recent first, with a flag to filter to only currently-`running` rows. Reads the store directly; does not require the daemon to be running. Depends on: F-08.
- Completion note:

## F-12: `runner logs <run-id>` [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02
- Ticket:
- Description: `runner logs <run-id>` prints the full stored result/output, failure detail (if any), and the parsed `next_action`/`next_action_reason`/`recheck_after` for a single run by id. Errors clearly if the id doesn't exist, rather than printing nothing. Reads the store directly; does not require the daemon to be running. Depends on: F-08.
- Completion note:

## F-13: Schedule Store & CLI [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03, AC-04
- Ticket:
- Description: `runner cron add "<cron-expr>" "<task>"`, `runner cron list`, `runner cron remove <schedule-id>` — CRUD against the `schedules` table (F-07). `add` validates the cron expression at input time (reject invalid expressions immediately, not at first tick) using the same cron-expression parser the tick engine (F-14) uses. `list` shows id, cron expression, task, enabled/disabled, last_run_at. `remove` deletes by id. No `enable`/`disable` toggle required in this batch — remove and re-add is sufficient for Stage 1. Depends on: F-07.
- Completion note:

## F-14: In-Daemon Cron Tick Engine [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03, AC-04, AC-05, AC-06
- Ticket:
- Description: A ticker running inside the daemon process (F-01) — evaluates enabled `schedules` rows against the current time using the `cron` crate on a fixed poll interval (default 60s, not configurable in this batch). For each schedule that's cron-due: if its most recent run's `recheck_after` timestamp is in the future, the tick is skipped and logged (not silently dropped) — this is the *only* place `recheck_after` is read, and the check is a plain timestamp comparison, never a branch on `next_action`'s value. Otherwise, triggers the same run pipeline `runner run` uses (F-10's underlying call, not a shell-out to the CLI binary), tagging the resulting `runs` row with the schedule id as its task identity for F-09's lookup. A schedule with a run already `status = "running"` is also skipped for that tick (no overlapping runs of the same schedule), independently of the recheck check. Updates `schedules.last_run_at` on trigger, whether or not the run ultimately succeeds. This is the daemon's only responsibility in Stage 1 beyond staying alive. Depends on: F-09, F-10, F-13.
- Completion note:

## F-15: TUI Status Dashboard [2026-08-30]
- Status: [ ] todo
- AC: AC-01, AC-02, AC-03, AC-04, AC-05
- Ticket:
- Description: `runner tui` launches a `ratatui` + `crossterm` terminal UI: a scrollable list of runs (id, task, status, started_at, next_action) sourced from the same store F-11 reads, refreshing on a fixed poll interval (default 2s); selecting a run shows its full result/output, failure detail, and next_action/reason/recheck_after in a detail pane (same data F-12 prints). Read-only — no keybindings that mutate state (no kill/retry/delete from the TUI in this batch); this is a structural property, not just behavioral (see `DICT.md`). `q` or `Ctrl+C` exits cleanly, restoring the terminal. Does not require the daemon to be running (reads the store directly, same as the CLI). Depends on: F-08, F-11, F-12.
- Completion note:

---

## Entry format (reference)

```markdown
## F-XX: [Feature Name] [Date created] [Date complete]
- Status: [ ] todo | [~] in progress | [x] done | [!] blocked | [-] deprecated
- AC: AC-XX, AC-XX, AC-XX
- Ticket:
- Description: [what this feature does]
- Completion note: [filled on done/blocked/deprecated]
```
