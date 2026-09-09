# Runner

A thin, local-only daemon and CLI for running Claude Code tasks against your own machine's ambient subscription login — not an API key. Loosely inspired by Herdr, stripped down to four things: a persistent daemon, a cron-style scheduler, a manual-control CLI, and a read-only terminal dashboard.

Runner never makes orchestration decisions. It runs one `claude` turn at a time, persists the result, and relays whatever the agent self-reports about what to do next — it never interprets that signal itself. See [`agent_docs/PROJECT.md`](agent_docs/PROJECT.md) for the full design rationale, and [`agent_docs/DICT.md`](agent_docs/DICT.md) for implementation-level patterns and gotchas.

## Install

```
cargo install --path .
```

Installs to `~/.cargo/bin/runner`. After this, `runner` works as a plain command from **any directory** — it does not need to run from inside this repo. Ambient auth (your `claude login` session) is picked up automatically via the macOS Keychain; there's no separate credential setup.

## Usage

`runner` (this repo's own source) and the **target repo** (whatever project you want Claude actually working on) are two different things — the walkthrough below sets one of the latter.

### 1. Point Runner at a target repo

```
runner repo <path-to-a-project>
```

Machine-wide, not per-terminal — persisted in `~/Library/Application Support/runner/config.toml`, exactly one repo at a time; setting a new one replaces the old. The path must contain `agent_docs/AGENT.md` — Runner assumes the target already follows that convention, it doesn't scaffold one into a fresh repo.

```
runner repo          # no argument: show the current value, or "no repo configured"
```

### 2. Run something

**One-off, manual, synchronous — no daemon needed:**

```
runner run "fix the flaky login test"
```

Blocks until done, prints the result plus the agent's self-reported `NEXT_ACTION`/`RECHECK_AFTER` signal. Running the exact same task string again later auto-resumes the prior session rather than starting fresh.

**Recurring, on a schedule — needs the daemon (below) to actually fire:**

```
runner cron add "*/15 * * * *" "check for new PRs and review them"
runner cron list
runner cron remove <schedule-id>
```

Standard 5-field cron syntax. Adding a schedule only persists it — nothing runs until a daemon is alive to tick it.

### 3. The daemon (only needed for cron)

```
runner daemon start [--repo <path>]   # detaches, survives closing the terminal; --repo sets the target repo first
runner daemon status                  # running or not
runner daemon stop                    # SIGTERM, waits up to 5s
```

While running: ticks every 60s, fires due schedules (never overlapping the same schedule twice), and spawns `caffeinate` so the machine won't sleep out from under it. Logs land in `~/Library/Application Support/runner/logs/`. If you only ever use `runner run` manually, you never need the daemon at all.

### 4. Check what happened (works with or without the daemon running)

```
runner status [--running]   # alias: ps — tabular list of recent runs
runner logs <run-id>        # full result/failure detail + continuation signal for one run
runner logs                 # no id: interactive TUI — arrow keys to browse, q or Ctrl+C to quit, auto-refreshes every 2s
```

### Putting it together

```
runner repo ~/DEV/my-actual-project                 # point it somewhere real
runner run "summarize open TODOs"                    # try it manually first
runner status                                        # confirm it ran, see the result summary
runner cron add "0 */2 * * *" "triage new issues"    # set up a recurring task
runner daemon start                                  # start the scheduler so it actually fires
runner logs                                          # watch it live in the TUI
runner daemon stop                                   # tear the daemon down when done
```

Everything persists in `~/Library/Application Support/runner/runner.db` regardless of whether the daemon is up — `status`/`logs`/`repo` all work standalone at any point in this sequence.

## Command reference

| Command | Purpose |
|---|---|
| `runner repo [path]` | Show, or set, the configured target repo |
| `runner run <task>` | Run a task now, synchronously |
| `runner status [--running]` | List recent runs (alias: `ps`) |
| `runner logs [run-id]` | Full detail for one run, or (no id) the interactive TUI |
| `runner cron add\|list\|remove` | Manage recurring schedules |
| `runner daemon start\|stop\|status` | Manage the background scheduler process |

## Where state lives

```
~/Library/Application Support/runner/
  runner.db      — SQLite store: run history, schedules
  runner.pid     — daemon PID file, present only while the daemon is running
  config.toml    — the configured target repo path
  logs/          — daemon log output
```

Overridable via the `RUNNER_HOME` environment variable (mainly useful for running an isolated instance, e.g. in tests).
