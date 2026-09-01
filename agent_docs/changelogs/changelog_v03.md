# Changelog v03

**Feature:** F-03 — Local Ambient-Auth Preflight
**Date:** 2026-09-01
**Branch:** `feature/F-03-ambient-auth-preflight` (off `feature/F-02-target-repo-configuration`)

## Summary

`preflight::check()` — a standalone capability, not yet wired into any CLI command (its consumer, F-04, doesn't exist yet). Checks, in order:

1. `claude` resolves on `PATH` — a plain directory scan against `PATH`'s entries, genuinely zero subprocess spawns for this check (not just "no spawn for the actual task").
2. A usable login session exists — macOS Keychain presence of the `"Claude Code-credentials"` generic-password entry, existence-only (`security find-generic-password -s "Claude Code-credentials"`, checking exit status; the credential value itself is never read, printed, or logged).

## A correction, not just an addition

Earlier Stage-1 scoping (this project's own `PROJECT.md`/`SPEC.md`, informed by reading chili-jar's `claude-code` adapter) assumed `claude`'s credentials live in a `~/.claude/.credentials.json` file. Checking this machine's actual, currently-logged-in Claude Code install directly — `security find-generic-password -s "Claude Code-credentials"` (found), `ls ~/.claude/` (no such file) — showed that assumption doesn't hold; real credentials are in the macOS Keychain instead. Implemented and documented against the verified mechanism. `DICT.md` now carries this correction so it doesn't get silently reintroduced by a future feature working from the old assumption.

## AC items validated

`SPEC.md`'s "Local Ambient-Auth Preflight (F-03)":

- **AC-01** — `claude` not on `PATH` fails with a message naming the missing binary; the check itself performs zero subprocess spawns. `find_on_path_locates_a_known_binary`, `find_on_path_returns_none_for_a_nonexistent_binary`, `is_executable_file_*` (4 tests), `check_with_fails_claude_not_found_when_binary_is_missing`.
- **AC-02** — distinguishes "not found" from "found but not logged in" via the Keychain check. `keychain_has_entry_false_for_a_service_that_does_not_exist`, `check_with_fails_not_logged_in_when_binary_present_but_no_keychain_entry`.
- **AC-03** — **not implementable yet.** Names a `runs` row (F-08's persistence layer), which doesn't exist. `PreflightError` is already a distinct type ready for F-08 to map into `exit_reason` once it lands. Flagged explicitly per `AGENT.md`'s AC-conflict-handling rule rather than marked done silently — see `FEATURE.md`'s F-03 completion note for the full reasoning. This is a forward dependency in how the AC was originally worded (assuming persistence would exist by the time preflight was built), not a scope cut.

9 new unit tests in `src/preflight.rs`, all parameterized (`check_with(bin_name, keychain_service)`) rather than mutating the real process-global `PATH` — deliberately avoiding the env-var-race class of bug `DICT.md`'s "Testing conventions" section already documents from F-01/F-02. `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean (the module is marked `#[allow(dead_code)]` in `main.rs` since it has no caller yet — F-04 removes that when it lands). Full suite (42 tests) re-run 5× with zero flakes.

## Conflicts

AC-03, as described above — not a behavioral conflict, a sequencing one. Feature marked `[x] done` per `AGENT.md`'s rule that a feature can close with a documented conflict note when the discrepancy traces to underspecified/forward-referencing criteria, with the human reviewing at the PR stage (no PR exists yet for this repo — see `changelog_v01.md`'s open item — so this note is that review surface for now).

## Follow-ups for next CLAUDE-PM pass

Wire `PreflightError` into `runs.exit_reason` and add F-03 AC-03's test when F-08 (Run Lifecycle Persistence) is built.
