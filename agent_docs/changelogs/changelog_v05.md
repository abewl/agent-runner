# Changelog v05

**Feature:** F-05 — Continuation Signal: Prompt Convention & Parsing
**Date:** 2026-09-01
**Branch:** `feature/F-05-continuation-signal` (off `feature/F-04-claude-subprocess-runner`)

## Summary

`signal::build_prompt(task, context)` and `signal::parse_continuation_signal(result_text)` — the mechanism that makes PM/Engineer chaining an emergent property of the agent's own self-reported signal rather than orchestration logic in Runner (`PROJECT.md` §4). Every prompt gets the identical fixed trailer instruction appended (`TRAILER_INSTRUCTION`); every result is parsed for the two fixed lines it produces (`NEXT_ACTION:`, `RECHECK_AFTER:`) into a typed `ContinuationSignal`.

Not wired into any CLI command yet — same as F-03/F-04, its consumer is F-10.

## AC items validated

`SPEC.md`'s "Continuation Signal — Prompt Convention & Parsing (F-05)":

- **AC-01** — one shared trailer instruction, appended identically regardless of caller. `build_prompt_without_context_ends_with_the_trailer`, `build_prompt_with_context_puts_context_first`.
- **AC-02** — `NEXT_ACTION:` parsed via fixed-prefix line matching. `parses_full_trailer_with_em_dash_and_recheck`, `trailer_lines_found_among_surrounding_prose`.
- **AC-03** — unparseable `RECHECK_AFTER` duration is a parse error, not silently dropped. `unparseable_recheck_after_duration_is_a_parse_error`, plus 4 direct `parse_duration` unit tests.
- **AC-04** — no `RECHECK_AFTER` line is a valid `None`, not an error. `recheck_after_absent_is_a_valid_none_not_an_error`.
- **AC-05** — no code branches on `next_action`'s or `reason`'s value. Verified by grep across `src/` — see `FEATURE.md`'s completion note for the one hit found and why it doesn't count (an `Option` presence check, not a value branch).

16 new unit tests in `src/signal.rs`. `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (71 tests) re-run 5× with zero flakes.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

None required to proceed to F-06.
