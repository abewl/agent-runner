# Changelog v15

**Feature:** F-15 — TUI Status Dashboard
**Date:** 2026-09-04
**Branch:** `feature/F-15-tui-status-dashboard` (off `develop`)

## Summary

`runner tui` — a `ratatui` + `crossterm` read-only dashboard: a scrollable, most-recent-first run list (id, task, status, started_at, next_action) on a 2s auto-refresh, with a live detail pane tracking the current selection that shows the same result/failure/signal content `runner logs` prints. `q`/`Ctrl+C` exits cleanly. Lives at `src/cli/tui/` (`mod.rs` for terminal setup/render/event-loop, `app.rs` for state), a sibling of `cli::status`/`cli::logs` rather than a separate top-level module. **Stage 1 Batch 1 is now complete — all 15 features across all 5 epics (E-01 through E-05) are done.**

## One formatter, not two copies that could drift

`cli::logs` used to build its output via an inline sequence of `println!` calls. Extracted that logic first, into a new `cli::display::format_run_detail(&Run) -> String`, and refactored `cli::logs` to just print its result — the TUI's detail pane calls the exact same function. AC-03's requirement ("same data `runner logs` would print") is therefore true by construction, not by keeping two hand-written copies in sync. This required promoting `cli::display` from private-to-`cli` to `pub(crate)`, since `cli::tui` is a sibling module that needs to reach it — the module's own doc comment now explains why.

## `App` stays pure and terminal-free

`app.rs` holds `App { runs: Vec<Run>, selected: usize }`, a `refresh(conn)` method that is the *only* function anywhere in `cli::tui` that ever touches the store (always via `runs::list` — never a write-path function, satisfying AC-04 as a structural property, consistent with `DICT.md`'s pre-existing "TUI is read-only" note, now corrected there to point at the real `cli::tui` path), and a pure `handle_key(KeyEvent) -> Action` covering `q`/Ctrl+C (`Action::Quit`) and Up/Down. All of this is unit-tested (11 new tests) with zero terminal or subprocess dependency — the same "keep the decision logic pure, push I/O to the edges" shape this project has used throughout (`retry.rs`, `persist.rs`, `cron_engine.rs`), just via a plain struct instead of an injected closure this time, since there's no external process to fake here.

AC-03's "selecting a run... shows a detail pane" is satisfied by a continuously live detail pane that tracks whatever is currently selected via the arrow keys, rather than a separate Enter-to-open modal — read as within the AC's own "or equivalent" wording, decided during planning.

Terminal teardown (`disable_raw_mode` + `LeaveAlternateScreen`) in `run()` is unconditional — captured as a `Result` from `run_app` first, then torn down with `let _ = ...` afterward regardless of outcome — so it runs on every exit path, including an error surfacing before the very first draw.

## A real gap found during manual verification, not assumed away

This project's established pattern for anything that can't be cleanly automated (F-01/F-04/F-10/F-12/F-14) is a real manual check. That doesn't work unchanged for a raw-mode, full-screen terminal app: piping stdin gives the process no real pty at all, and `expect` — which does allocate one — defaults that pty to a **0x0 window** when `expect` itself has no controlling terminal, which is the case in this project's own verification environment. A 0x0 `Rect` makes `ratatui` lay out and draw nothing, while the process still enters/exits the alternate screen correctly and returns exit code 0 — a clean-looking false positive, only caught by checking the raw captured session bytes for actual rendered text (`strings -n 3 session.log`) rather than trusting the exit code. Fixed by explicitly sizing the pty before the first draw, using Expect's builtin `stty` against the pty slave device (`stty rows 40 columns 120 < $spawn_out(slave,name)`) — documented in full in `DICT.md` so any future raw-mode-terminal feature's manual verification starts from this, instead of rediscovering it.

After the fix, a real session against a seeded two-row store showed genuine rendered output: the list pane's `r1  done  say hello then stop  2026-09-04T00:00:00Z` row with a reverse-video selection highlight, arrow-key navigation working without error, and the detail pane showing `next_action: stop` / `next_action_reason: one-off task complete` — exactly what `runner logs r1` prints for that row. `q` exited with code 0, confirmed two independent ways: the exact expected escape sequence (`EnterAlternateScreen` + cursor-hide on each draw cycle, then cursor-show + `LeaveAlternateScreen` at the end) in the raw session log, and a `stty -a` diff of the real host shell before/after the session showing zero drift. No daemon was ever started for any of this (AC-holds: does not require the daemon running).

## AC items validated

`SPEC.md`'s "TUI Status Dashboard (F-15)":

- **AC-01** — scrollable, most-recent-first run list (id, task, status, started_at, next_action) from the same store `runner status` reads. `App::refresh` calls `runs::list` (identical to `cli::status`); verified live — real row content rendered in the manual session above.
- **AC-02** — fixed 2s auto-refresh with no flicker/full-redraw artifacts. `run_app`'s loop times refreshes off `Instant`, using `event::poll`'s timeout as both the input wait and the refresh clock rather than a separate sleep; `terminal.draw` (ratatui's own diffed backend) never does a blind full-screen clear-and-redraw.
- **AC-03** — selecting a run shows a detail pane with the same content `runner logs` would print. True by construction (`format_run_detail` shared with `cli::logs`, above); verified live — the rendered detail pane matched.
- **AC-04** — no keybinding mutates state; a structural property, not just behavioral. Verified by inspection: `App::refresh` is the only store-touching function in `cli::tui`, and it only calls `runs::list`.
- **AC-05** — `q` or `Ctrl+C` exits, fully restoring the terminal. `handle_key` unit tests cover both keys mapping to `Action::Quit`; `run()`'s unconditional teardown plus the live-session verification (escape sequence + `stty` diff) confirm the real restore.

11 new unit tests in `src/cli/tui/app.rs` (`handle_key`'s key-to-action mapping including the plain-`c`-does-not-quit negative case, `apply`'s bounds-clamping in both directions, and `refresh`'s selection-clamping when the row count shrinks). No new integration test file — consistent with F-14's precedent that daemon/terminal-level behavior not cleanly unit-testable is covered by real manual verification instead, and the manual verification here (above) is exactly that coverage. `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (161 tests: 128 unit + 33 integration) re-run 8× with zero flakes; `ps aux` confirmed zero leaked processes after both the automated suite and every manual TUI session.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

None — **Stage 1 Batch 1 (E-01 through E-05, F-01 through F-15) is complete.** Not merged into `develop` yet; awaiting explicit instruction, per this project's established git workflow (merges into `develop` are a human-directed action, not an automatic end-of-feature step).
