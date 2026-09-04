# DICT.md — Function & Pattern Glossary

Project-specific patterns, naming conventions, and function signatures. Updated by CLAUDE-PM when a feature introduces or deprecates a pattern. Claude-as-engineer reads this before implementing; does not edit directly.

**Last updated:** 2026-09-01

---

## Repo layout

```
Cargo.toml        — binary crate, package name "runner"
src/
  main.rs          — CLI entrypoint (clap dispatch) [F-01]
  paths.rs         — RUNNER_HOME resolution + derived paths (pid file, log file, log dir) [F-01]
  pid.rs           — pid-file read + liveness-check helpers, shared by cli/daemon.rs [F-01]
  daemon.rs         — the daemon process body once detached: logging init, signal-handler
                      registration (before the pid file is written — see "Signal handlers
                      before pid file" below), pid file write, caffeinate spawn, shutdown
                      wait, cleanup. Cron ticker lands here too at F-14. [F-01, F-14]
  cli/             — subcommand implementations
    mod.rs
    daemon.rs        — `runner daemon start|stop|status` [F-01]
    (repo, run, status, logs, cron, tui — F-02, F-10–15)
  config.rs         — F-02 target-repo config file (read/write $RUNNER_HOME/config.toml)
  runner.rs         — the "run one claude turn" pipeline (preflight → continuation lookup → subprocess → signal parse → retry → persist)
  preflight.rs      — F-03 ambient-auth check
  process.rs        — F-04 claude subprocess spawn + result/session/cost parsing
  signal.rs         — F-05 continuation-signal trailer: prompt-template construction + NEXT_ACTION/RECHECK_AFTER parsing, plus strip_trailer (F-12) for display/storage contexts that show the parsed signal separately
  store/
    mod.rs           — F-07 SQLite init/migration + shared connection handling
    runs.rs          — typed CRUD for the `runs` table
    schedules.rs      — typed CRUD for the `schedules` table
  cron_engine.rs     — F-14 tick loop + cron expression evaluation + recheck_after gating
  tui/              — F-15 ratatui view + input handling (read-only)
tests/
  daemon_lifecycle.rs — F-01 integration tests, driving the compiled binary via
                        `env!("CARGO_BIN_EXE_runner")`, one isolated `RUNNER_HOME`
                        temp dir per test
```

No workspace, no sub-crates — single binary crate, matching the "thin" project goal. Split into modules for clarity only.

**Signal handlers before pid file (F-01, load-bearing — do not reorder).** `daemon::run()` registers `tokio::signal::unix::signal()` handlers *before* calling `write_pid_file()`, and does not use `daemonize`'s built-in `.pid_file()` option at all. `tokio::signal::unix::signal()` installs the OS-level handler synchronously at call time, not lazily on first `.recv().await` — so this ordering closes a real race: writing the pid file first (as `daemonize`'s own option would) creates a window where a signal sent the instant the pid file appears hits the OS's default disposition (immediate termination, no cleanup) instead of ours. This was caught by an integration test sending `SIGINT` directly rather than only through `runner daemon stop` (which has its own redundant client-side pid-file removal that was masking the bug for the `SIGTERM` path) — worth remembering when adding any future signal-adjacent behavior: test the signal path directly, not only through the CLI command that happens to send it.

---

## Dependencies (locked choices — don't re-litigate per feature)

| Concern | Crate | Why |
|---|---|---|
| CLI parsing | `clap` (derive) | Same choice Herdr made; well-trodden for exactly this. |
| Async runtime | `tokio` | Needed for concurrent subprocess I/O + the cron ticker's timer without blocking CLI responsiveness. |
| Subprocess | `std::process::Command` / `tokio::process::Command` | Plain piped subprocess — **no PTY** (`portable-pty` is explicitly not a dependency; there is no interactive terminal to emulate). |
| Local DB | `rusqlite`, `bundled` feature | Embedded SQLite, no external SQLite install required, single-file store. |
| Config file | `toml` + `serde` | `$RUNNER_HOME/config.toml` — currently just `repo_path`; keep it a flat, small file, not a DB table, for a value this size. |
| Cron parsing | `cron` crate | Runner's own user-facing syntax is standard 5-field, but the crate itself requires a leading seconds field (6–7 fields) — verified empirically, see `src/cron_engine.rs`'s doc comment. Runner prepends a fixed `"0 "` internally; never exposed to callers. |
| Time | `chrono` (or `time`, pick one and use it everywhere — do not mix) | Timestamps stored as ISO-8601 TEXT in SQLite (see Local State Store, F-07). |
| Logging | `tracing` + `tracing-subscriber` | Same choice Herdr made. |
| Daemonize (fork/detach) | `daemonize` | Handles the double-fork/setsid dance correctly (F-01). Its own `.pid_file()` option is deliberately **not used** — see "Signal handlers before pid file" above; we write the pid file ourselves, later, from inside `daemon.rs`. |
| Raw syscalls (`kill`, liveness checks) | `libc` | `kill(pid, 0)` for liveness checks (F-01 `pid.rs`), `SIGTERM`/`SIGINT` constants. Not in the original locked table — added when F-01 needed it; no conflict with anything else here. |
| TUI | `ratatui` + `crossterm` | Same choice Herdr made — proven, cross-platform terminal handling. |
| JSON | `serde` + `serde_json` | Parsing `claude --output-format json` output. |

---

## RUNNER_HOME

Base data directory. Default: `~/Library/Application Support/runner/`. Overridable via the `RUNNER_HOME` env var (used for tests and any non-default install). All of the following are resolved relative to it, never hardcoded elsewhere:

```
$RUNNER_HOME/runner.db     — SQLite store (F-07)
$RUNNER_HOME/runner.pid    — daemon PID file (F-01)
$RUNNER_HOME/config.toml   — target repo path + any future flat config (F-02)
$RUNNER_HOME/logs/         — tracing log output (F-01)
```

Note there is no `$RUNNER_HOME/work/` in this revision — `claude`'s working directory is always the F-02-configured target repo (F-04 AC-05), not a Runner-owned scratch directory. If a future stage needs Runner to operate without a configured repo, that's a deliberate new mode, not a silent fallback.

---

## Target repo (F-02)

Exactly one, at a time, machine-wide (not per-schedule, not per-invocation). Stored as a canonicalized absolute path in `config.toml`. Validated at set-time to contain `agent_docs/AGENT.md` — Runner assumes the target repo already follows the `agent_docs/` convention (`PROJECT.md`/`AGENT.md`/`FEATURE.md`/`SPEC.md`) this repo itself uses; it does not scaffold that convention into a fresh repo. `claude` is always invoked with this path as its working directory (F-04 AC-01/AC-05) — Runner never passes the repo path in the prompt text, it's a process-spawn parameter.

---

## Run identity vs. task identity

Two distinct concepts — do not conflate:

- **`runs.id`** — a fresh UUID (or equivalent) per individual invocation. Every call to the run pipeline, manual or scheduled, creates exactly one new `runs` row with a new id.
- **`runs.task_identity`** — the string used for F-09's session/continuation lookup. For a manual `runner run "<task>"`, task identity is the literal task string. For a scheduled run (F-14), task identity is the **schedule's id**, not its task string — so two different manual invocations with the same literal task string share a resume/continuation chain by design, and a schedule's chain is stable even though its task text could in principle change.

---

## `runs.owner_pid` (F-08, corrects F-07's original column list)

F-07's schema didn't include a per-row process owner. F-08's startup reconciliation (SPEC.md AC-04) needs one: reconciling "any `running` row whose owning process is confirmed not alive" only works if you can tell *which* process owns a row — otherwise a concurrently active daemon tick's legitimately-in-flight run looks identical to one abandoned by a dead process, and a separate `runner status` invocation running reconciliation would wrongly mark it interrupted. `owner_pid INTEGER NOT NULL DEFAULT 0` was added directly to F-07's `CREATE TABLE` statement (schema version bumped 1→2 — no `ALTER TABLE` path, since Stage 1 has never shipped, so no real `runner.db` exists anywhere with the old shape to migrate). `persist::reconcile_interrupted_runs` checks each row's `owner_pid` via `pid::process_alive` individually, never treats "found a `running` row" alone as sufficient.

## `runs.result_text` (F-12, a second correction to F-07's original column list)

F-07's schema also never included anywhere to store the agent's actual response text — only the signal fields (`next_action`/`next_action_reason`/`recheck_after`) and metadata (`session_id`/`cost_usd`) were persisted. This made F-12's own AC-01 ("`runner logs` prints the full result text on success") unimplementable until `result_text TEXT` was added (schema version bumped 2→3, same "edit the `CREATE TABLE` directly, no real DB exists yet to migrate" reasoning as `owner_pid`). Stored **stripped** of the `NEXT_ACTION`/`RECHECK_AFTER` trailer via `signal::strip_trailer` — those fields already have their own columns, so the stored text and the signal fields don't duplicate each other, and `runner run`'s immediate stdout (F-10) and `runner logs`'s later retrieval (F-12) show identical clean text from the one shared stripping function rather than two independent copies. **Two schema gaps found in exactly this way (F-08, F-12) is worth noticing as a pattern**: F-07's original column list was authored during Stage 1 scoping, before any feature that actually *needed* to read specific data back out had been implemented — each gap was found by writing the feature that needed the missing column, not by reviewing the schema in isolation. If a future feature hits the same kind of wall, adding the column directly (not a formal migration) remains correct only as long as Stage 1 has never shipped with real user data.

---

## Run status values

`runs.status` is one of exactly four values — do not introduce new ones without a DICT.md update:

- `running` — subprocess in flight, no terminal outcome yet.
- `done` — completed successfully; `session_id`, `cost_usd`, and (when the trailer parsed) `next_action`/`next_action_reason`/`recheck_after` populated.
- `failed` — both the primary attempt and the F-06 retry failed; `exit_reason` populated; continuation-signal fields left null.
- `interrupted` — was `running` when its owning process died without reaching a terminal state (F-08 AC-04); never set by the run pipeline itself, only by startup reconciliation; continuation-signal fields left null.

`interrupted` is not a failure verdict on the task — it's "we don't know what happened, the process that was running it is gone." F-09's lookup explicitly requires `status = "done"`, skipping both `failed` and `interrupted` rows for both the resume value and the continuation context line.

---

## The continuation signal (F-05) — what Runner relays, and what it never interprets

Every prompt the run pipeline sends ends with a fixed, invariant trailer instruction (one template, no PM-mode/Engineer-mode variants — see `PROJECT.md` §4 "Continuation is a relayed signal, not inferred state"):

```
NEXT_ACTION: <short label> — <one-line reason>
RECHECK_AFTER: <duration, e.g. "30m">        (only when applicable — omit the line otherwise)
```

Parsed into:

```rust
struct ContinuationSignal {
    next_action: String,           // opaque — never matched against a fixed vocabulary in code
    reason: String,                // opaque — display/context only
    recheck_after: Option<Duration>,
}
```

**Hard rule, not a style preference:** `next_action` and `reason` are stored on the `runs` row, shown by `status`/`logs`/the TUI, and formatted into the *next* invocation's prompt as a one-line context hint (F-09 AC-02) — and that's the entire set of things this codebase is allowed to do with their values. No `if next_action == "..."` or `match next_action.as_str()` anywhere. The **only** field the codebase's control flow branches on is `recheck_after`, and only as a timestamp comparison in the cron tick engine (F-14 AC-02/AC-03) — never its presence/absence meaning anything beyond "is there a future timestamp to wait past." This is what keeps PM/Engineer chaining an emergent property of the agent's own judgment rather than orchestration logic living in Runner — see `PROJECT.md` §1/§4 for the full rationale.

---

## Claude invocation shape

The one and only way `claude` is invoked in this codebase (F-04):

```
claude --print --output-format json [--resume <sessionId>] "<prompt>"
```

...where `<prompt>` is: (optional) the F-09 continuation context line, then the F-05 trailer instruction appended after the caller's actual task text. Working directory is always the F-02 target repo. Parsing follows the same shape already proven in `chili-jar/packages/harness/adapters/claude-code/index.mjs` (`runClaudeMessage`, lines ~247–346), with one correction: find the `result` object (`subtype: "success"` or `"error_max_turns"` are both usable outcomes, anything else is a typed error); if absent, fall back to concatenating `assistant` message text blocks; if neither, typed error with truncated raw output. Never a bare `claude` invocation without `--print` — there is no code path in Runner that expects or handles an interactive session.

**Verified real shape (F-04, corrects an assumption).** Running the actual `claude` CLI (2.1.252) once against this machine — `claude --print --output-format json "say the single word: pong"` — showed `--output-format json` currently returns a **single flat JSON object**, not a JSON array of events as chili-jar's adapter code assumed (probably written against an older CLI version). `src/process.rs`'s parser already handles this correctly (a non-array value is wrapped into a one-element list before searching for the `result` object), but the **cost field is named `total_cost_usd`, not `cost_usd`** — chili-jar's adapter, and this project's own earlier `SPEC.md`/`PROJECT.md` wording, assumed `cost_usd`. `src/process.rs` reads `total_cost_usd` first, falling back to `cost_usd` for defensiveness. Any future code reading a `claude --output-format json` result should use `total_cost_usd` as the primary field name.

---

## Error handling convention

No `panic!`, `unwrap()`, or `expect()` on any path reachable from a subprocess spawn, file I/O, or DB call — these are exactly the failure modes Runner exists to detect and report, per `PROJECT.md` §4's "compile-time exhaustiveness for a component whose entire job is failure detection." Use a project-wide `Result<T, RunnerError>` (a single error enum covering config/preflight/process/signal/store/cron variants) rather than ad hoc `Box<dyn Error>` per module — callers (CLI commands, the cron engine, the TUI) need to match on failure category, not just print a message.

---

## Cron tick interval

Fixed at 60 seconds (F-14 AC-01), not configurable in this batch. Don't add a config flag for it speculatively — if a use case needs finer granularity later, that's a deliberate follow-up feature, not a default to guess now. This is a different number from `recheck_after` — the tick interval is how often the daemon *checks* whether any schedule is due at all; `recheck_after` is a per-task hint for *skipping* a due tick. Don't conflate the two in implementation.

## Cron due-check: `after()`, never `includes()` (F-14)

The `cron` crate's `Schedule::includes(date_time)` looks like the obvious way to check "is this schedule due right now" — it isn't, for a polling daemon. `includes` requires an *exact second match*, and every expression Runner accepts is internally fixed to `:00` seconds (F-13's `"0 "` prepend). A tick that runs a few seconds after the minute mark — routine, not drift worth caring about — would make `includes(now)` wrongly report "not due." Verified empirically with a scratch probe simulating that jitter before writing any real code. The correct check, used by `cron_engine::is_due`, is `schedule.after(&reference_time).next() <= now` — "was there a scheduled fire time somewhere in the window since I last checked," which is what a poll-based tick actually needs, not "does this exact instant match."

---

## Sleep prevention (`caffeinate`, F-01)

The daemon spawns `caffeinate -s -w <own-pid>` right after detaching, as a fire-and-forget child — not stored, not polled, not explicitly killed anywhere in the codebase. `-w <pid>` is what does the work: it makes `caffeinate` watch that PID and exit the instant it dies, clean shutdown or crash alike, so there is deliberately no corresponding "stop caffeinate" code path to write or maintain.

This only prevents *system* sleep (the AC power / idle-timeout trigger). It does not by itself defeat a laptop-lid-close-triggered sleep — that requires clamshell mode (external display attached, machine on power) as a separate, operator-side condition; Runner has no code involvement in that half, it's purely a macOS power-management behavior triggered by the physical hardware state.

macOS-only. If `caffeinate` isn't on `PATH` (i.e. anywhere other than macOS), this is a logged warning, not a startup failure — matches `PROJECT.md` §5's "Stage 1 targets macOS only" without turning a missing binary into a hard crash on a platform this batch was never meant to run on anyway. A future Linux target would need `systemd-inhibit`, which wraps-and-spawns the command it protects rather than attaching to an existing PID like `caffeinate -w` does — a different process-tree shape, not a one-line substitution. Not built now; noted here so it isn't re-discovered from scratch if that stage ever happens.

---

## TUI is read-only

No store-mutating function (anything in `store/runs.rs` or `store/schedules.rs` beyond read/list) is ever called from `src/cli/tui/` (implemented in F-15 — lives under `cli/`, alongside every other command, not as a separate top-level `src/tui/`). This is a structural rule, not just a behavioral one — F-15 AC-04 expects it to be verifiable by inspection of what the TUI module can reach, not just by testing: `App::refresh` (`cli/tui/app.rs`) is the *only* function in the module that touches a `Connection` at all, and it only ever calls `runs::list`.

---

## Manually smoke-testing a raw-mode terminal app non-interactively (F-15)

`runner log` (`ratatui` + `crossterm`, alternate screen + raw mode) can't be exercised by piping stdin the way every other command's manual verification has been — a plain `printf 'q' | runner log` doesn't give it a real pty at all, and `expect`, which does allocate one, defaults that pty to a **0x0 window** when `expect` itself has no controlling terminal (true here — this project's manual verification runs inside a non-interactive tool shell, not a real terminal session). A 0x0 `Rect` from `frame.area()` makes `ratatui` lay out and draw nothing at all — the process runs, enters/leaves the alternate screen correctly, and exits 0, which *looks* like a clean pass while silently never having rendered a single widget. Caught by checking the raw captured session bytes for actual rendered text (`strings -n 3` on the log), not just checking the exit code — an empty result there is the tell. Fixed by explicitly sizing the pty before the first draw, using Expect's builtin `stty` command against the pty slave device:
```tcl
spawn $bin log
stty rows 40 columns 120 < $spawn_out(slave,name)
```
After that fix, the same session log showed genuine rendered content (row text, reverse-video selection highlight, the detail pane's `next_action`/`next_action_reason` labels) — confirmed with `strings -n 3 session.log | grep <expected row/field text>`. Any future manual verification of a raw-mode/full-screen terminal feature in this project should size the pty this way from the start, rather than rediscovering the 0x0-default gap.

---

## Ambient auth mechanism (F-03) — corrects an earlier assumption

`claude`'s login credentials, on a real, currently-logged-in Claude Code install, live in the **macOS Keychain** under the generic-password service name `"Claude Code-credentials"` — verified directly (`security find-generic-password -s "Claude Code-credentials"`, `ls ~/.claude/`), not assumed. This supersedes an earlier assumption in `PROJECT.md`/`SPEC.md`'s Stage-1 scoping (carried over from reading chili-jar's `claude-code` adapter, which references a `~/.claude/.credentials.json` file) — that file does not exist on this machine; the Keychain entry does. `src/preflight.rs`'s login check queries Keychain presence only (`security find-generic-password -s <service>`, checking exit status — **never** `-w`, which would print the actual secret). Any future code that needs to reason about "is `claude` logged in" should use this mechanism, not the credentials-file assumption.

---

## Testing conventions (F-01, F-02)

- **Integration tests share `tests/common/mod.rs`.** (Named `common/mod.rs`, not `common.rs`, specifically so Cargo treats it as a shared module rather than compiling it as its own empty test binary.) Any new integration test file that needs to spawn the compiled binary against an isolated `RUNNER_HOME`, wait for a pid file, or force-stop a leaked daemon should `mod common; use common::*;` rather than re-deriving these helpers — F-01 and F-02 originally each had their own copy, which is exactly the kind of drift this file exists to prevent.
- **Unit tests that mutate `RUNNER_HOME` (or any other process-global env var) must all share one lock, not one per module.** `paths::ENV_LOCK` (`#[cfg(test)] pub(crate)`, defined once in `paths.rs`) is that lock — `config::tests` imports it via `use crate::paths::ENV_LOCK` rather than declaring its own. A per-module `static ENV_LOCK: Mutex<()>` looks like it serializes access, but two independent mutexes don't protect each other: under `cargo test`'s default parallel execution, a `paths::tests` test and a `config::tests` test each holding their *own* lock can still race on the same real env var at the same time. This was a real, repeatably-triggered flake before the fix, not a hypothetical.
- **When polling for a file another process just wrote, poll the parse, not the existence.** `wait_for_pid_file` (in `tests/common/mod.rs`) checks-and-reads in one step rather than `.exists()` followed by a separate read — `std::fs::write` creates/truncates the file before its content lands, so a reader can observe it present-but-empty in between. This was also a real, repeatably-triggered flake, caught the same way as the one above: by running the suite many times in a row, not by inspection. Any future polling helper that waits on another process's file write should follow the same shape.
- **`wait_until`'s return value must always be checked (F-11's `force_stop`, a third instance of this exact class).** `force_stop` (`tests/common/mod.rs`) sent `SIGTERM` and called `wait_until(...)` but discarded its `bool` result — when the wait genuinely timed out under heavy back-to-back load (running the whole suite repeatedly, not a single normal run), the function silently returned anyway, leaking a real daemon + `caffeinate` process that no test ever reported as a failure, since nothing asserted on it. Every "all green" run still looked green. Caught only by inspecting `ps` output after test runs kept reporting success, not by any test failure — worth remembering that a clean test result and a clean process table are two different claims. Fixed the same way as the other two: assert on `wait_until`'s result rather than discard it, with a generous timeout (15s — this is a test-cleanup budget, not a product AC, so slack is fine) so genuine load doesn't cause false failures while a real hang still gets caught loudly. **The pattern to actually internalize**: any `wait_until(...)` call whose result isn't immediately `assert!`-ed is a latent version of this same bug, regardless of which helper it's in.
