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
  signal.rs         — F-05 continuation-signal trailer: prompt-template construction + NEXT_ACTION/RECHECK_AFTER parsing
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
| Cron parsing | `cron` crate | Standard 5-field cron expression parsing/evaluation. |
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

...where `<prompt>` is: (optional) the F-09 continuation context line, then the F-05 trailer instruction appended after the caller's actual task text. Working directory is always the F-02 target repo. Parsing follows the same shape already proven in `chili-jar/packages/harness/adapters/claude-code/index.mjs` (`runClaudeMessage`, lines ~247–346): `--output-format json` returns a JSON array of events; find the `result` event (`subtype: "success"` or `"error_max_turns"` are both usable outcomes, anything else is a typed error); if absent, fall back to concatenating `assistant` message text blocks; if neither, typed error with truncated raw output. Never a bare `claude` invocation without `--print` — there is no code path in Runner that expects or handles an interactive session.

---

## Error handling convention

No `panic!`, `unwrap()`, or `expect()` on any path reachable from a subprocess spawn, file I/O, or DB call — these are exactly the failure modes Runner exists to detect and report, per `PROJECT.md` §4's "compile-time exhaustiveness for a component whose entire job is failure detection." Use a project-wide `Result<T, RunnerError>` (a single error enum covering config/preflight/process/signal/store/cron variants) rather than ad hoc `Box<dyn Error>` per module — callers (CLI commands, the cron engine, the TUI) need to match on failure category, not just print a message.

---

## Cron tick interval

Fixed at 60 seconds (F-14 AC-01), not configurable in this batch. Don't add a config flag for it speculatively — if a use case needs finer granularity later, that's a deliberate follow-up feature, not a default to guess now. This is a different number from `recheck_after` — the tick interval is how often the daemon *checks* whether any schedule is due at all; `recheck_after` is a per-task hint for *skipping* a due tick. Don't conflate the two in implementation.

---

## Sleep prevention (`caffeinate`, F-01)

The daemon spawns `caffeinate -s -w <own-pid>` right after detaching, as a fire-and-forget child — not stored, not polled, not explicitly killed anywhere in the codebase. `-w <pid>` is what does the work: it makes `caffeinate` watch that PID and exit the instant it dies, clean shutdown or crash alike, so there is deliberately no corresponding "stop caffeinate" code path to write or maintain.

This only prevents *system* sleep (the AC power / idle-timeout trigger). It does not by itself defeat a laptop-lid-close-triggered sleep — that requires clamshell mode (external display attached, machine on power) as a separate, operator-side condition; Runner has no code involvement in that half, it's purely a macOS power-management behavior triggered by the physical hardware state.

macOS-only. If `caffeinate` isn't on `PATH` (i.e. anywhere other than macOS), this is a logged warning, not a startup failure — matches `PROJECT.md` §5's "Stage 1 targets macOS only" without turning a missing binary into a hard crash on a platform this batch was never meant to run on anyway. A future Linux target would need `systemd-inhibit`, which wraps-and-spawns the command it protects rather than attaching to an existing PID like `caffeinate -w` does — a different process-tree shape, not a one-line substitution. Not built now; noted here so it isn't re-discovered from scratch if that stage ever happens.

---

## TUI is read-only

No store-mutating function (anything in `store/runs.rs` or `store/schedules.rs` beyond read/list) is ever called from `src/tui/`. This is a structural rule, not just a behavioral one — F-15 AC-04 expects it to be verifiable by inspection of what the TUI module can reach, not just by testing.
