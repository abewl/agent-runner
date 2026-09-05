# Changelog v17

**Type:** CLI simplification (no store/schema/daemon behavior change)
**Date:** 2026-09-05
**Branch:** `refactor/cli-simplification` (off `refactor/module-consolidation-and-comment-trim`)

## Why

7 top-level verbs, 12 invocable command paths counting subcommands. Two
were genuinely simplifiable without losing anything: `runner log` (the
TUI) and `runner logs <id>` (single-run detail) differed by one letter
and did related-but-distinct things — a real, easily-mistyped pair,
flagged but not fixed when `log` was first named. `runner repo set|show`
was a two-way subcommand enum for what's really just get/set on one
config value. `daemon start|stop|status` and `cron add|list|remove` were
left untouched — both are already idiomatic lifecycle/CRUD triads: the
comparison points would be `systemctl`/`brew services` and `crontab`-style
CLIs, not something to compress further just to shave keystrokes on
commands run by muscle memory.

## What changed

- **`runner log` merged into `runner logs`.** `Commands::Log` removed;
  `Logs { run_id: String }` became `Logs { run_id: Option<String> }`.
  `runner logs <id>` behaves exactly as before. `runner logs` with no id
  now launches the TUI (previously `runner log`). One command, and the
  naming collision is gone by construction rather than managed around.
- **`runner repo set <path>` / `runner repo show` collapsed into
  `runner repo [path]`.** A path argument sets it (same validation,
  same `cli::repo::set`); omitting it shows the current value (same
  `cli::repo::show`) — the same get/set-on-optional-argument pattern as
  `git config <key> [value]`. `RepoAction` removed from `main.rs`;
  `cli::repo::set`/`cli::repo::show` themselves are unchanged, just
  dispatched directly on `Option<PathBuf>` instead of through a
  subcommand match.
- Every doc/error string referencing the old syntax updated to match:
  `cli/repo.rs`'s "not configured" message, `cli/run.rs`'s "no repo
  configured" message, `cli/tui/mod.rs`'s module doc, and the living
  reference docs (`PROJECT.md`, `SPEC.md`, `DICT.md`) — all rewritten in
  place, since they're current-state references, not historical records.
  `FEATURE.md`'s F-02 and F-15 completion notes instead got a
  **post-completion addendum** appended (matching the precedent set by
  the earlier `tui`→`log` rename note) rather than having their original
  AC/description text rewritten, since those notes are a record of what
  shipped at the time. Changelogs (`changelog_v02.md`, `changelog_v15.md`)
  were left untouched entirely — genuinely historical, never rewritten.

## Verification

- `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean.
- `tests/repo_command.rs` and `tests/run_command.rs` updated to the new
  `["repo", <path>]`/`["repo"]` argument shape — same assertions, same
  coverage, just the invocation syntax changed. `tests/logs_command.rs`
  needed no changes: `["logs", "r1"]` parses identically whether `run_id`
  is `String` or `Option<String>`, and the bare-`logs`-launches-TUI path
  has no integration test, consistent with F-15's precedent that
  daemon/terminal-level behavior not cleanly unit-testable is covered by
  real manual verification instead.
- Full suite (161 tests: 128 unit + 33 integration) re-run 5× with zero
  flakes.
- Real manual verification, not just type-checking: `runner repo` (unset)
  → correct message; `runner repo <path>` → sets and echoes the
  canonical path; `runner repo` (after set) → shows it; `runner logs
  <bad-id>` → clear error, exit 1. `runner logs` with no id verified via
  the same `expect`+pty technique from F-15's manual verification
  (`DICT.md`'s "Manually smoke-testing a raw-mode terminal app" entry) —
  confirmed genuine rendered output (the seeded row, box borders, detail
  pane) and a clean exit (code 0, correct `EnterAlternateScreen`/
  `LeaveAlternateScreen` sequence). Zero leaked processes throughout.

## Net effect

7 top-level commands → 6 (`daemon`, `repo`, `run`, `status`, `logs`,
`cron`). One fewer subcommand enum (`RepoAction` removed; `DaemonAction`
and `CronAction` kept). The one genuinely confusing command pair
(`log`/`logs`) no longer exists.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

None identified. Not merged into `develop` — this branch, and both
branches it's stacked on (`refactor/module-consolidation-and-comment-trim`,
`feature/F-15-tui-status-dashboard`), remain unmerged pending explicit
instruction, per this project's established git workflow.
