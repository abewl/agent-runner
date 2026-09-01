# Changelog v04

**Feature:** F-04 — Claude Subprocess Runner
**Date:** 2026-09-01
**Branch:** `feature/F-04-claude-subprocess-runner` (off `feature/F-03-ambient-auth-preflight`)

## Summary

`process::run(prompt, resume, cwd) -> Result<ClaudeResult, ClaudeError>` — the core "run one claude turn" primitive. Spawns `claude --print --output-format json [--resume <id>] "<prompt>"` as a plain piped subprocess (no PTY) with the given working directory, and parses the result out of the CLI's JSON output — success text, session ID, and cost. No git operations, no MCP config writing; a prompt in, a result out.

Not wired into any CLI command yet — its caller (F-10, `runner run`) doesn't exist yet.

## A real discrepancy found via one manual check, not by trusting secondhand code

`SPEC.md`/`PROJECT.md`'s Stage-1 scoping cited chili-jar's `adapters/claude-code/index.mjs` as the parsing shape to follow, which reads a `cost_usd` field from a JSON *array* of events. Running the actual `claude` 2.1.252 CLI once — `claude --print --output-format json "say the single word: pong"`, executed manually in `/tmp`, a single real call, not part of the automated suite or repeated — showed the current real shape is a **single flat JSON object**, and the cost field is named **`total_cost_usd`**, not `cost_usd`. The array-vs-object difference was already handled correctly by this parser's design (wraps a non-array value into a one-element list); the field name was not, and would have silently reported `0.0` cost on every real invocation. Fixed to read `total_cost_usd` first, `cost_usd` as a fallback, with a fixture test built from the real captured output. `DICT.md`'s "Claude invocation shape" section now carries this correction.

## Why no test invokes the real `claude` CLI

Deliberate, not an oversight. The JSON-parsing logic is a pure function (`parse_claude_output`), tested with hand-built and one real-captured fixture — no subprocess needed. The spawn mechanics (args, `--resume`, working directory, non-zero exit handling) are tested against a fake stand-in shell script via the same `run_with(bin, ...)` parameterization pattern F-03's preflight check already established, rather than the real binary. Invoking the actual network-calling `claude` CLI in a suite that gets re-run 5–10× per feature (this project's own testing convention, see `DICT.md`) would be slow, dependent on network/API conditions, and would burn real subscription usage on every test run for no correctness benefit the fake-script approach doesn't already provide.

## AC items validated

`SPEC.md`'s "Claude Subprocess Runner (F-04)":

- **AC-01** — correct `--print`/`--output-format json`/prompt args. `run_with_passes_print_output_format_json_and_the_prompt`.
- **AC-02** — successful completion returns result/session_id/cost_usd. `parses_a_success_result_event`, `parses_the_real_current_cli_output_shape`.
- **AC-03** — invalid JSON or no result event falls back to assistant text; neither present is a typed, truncated error. `no_result_event_falls_back_to_assistant_text_blocks`, `no_result_event_and_no_assistant_text_is_unparseable`, `invalid_json_is_unparseable_with_truncated_raw_output`, `truncate_shortens_long_output_and_marks_it`.
- **AC-04** — non-zero exit is always a typed error. `run_with_non_zero_exit_is_always_a_typed_error`.
- **AC-05** — working directory is a required parameter, not a default+override. `run_with_invokes_the_binary_in_the_given_cwd`.
- **AC-06** — no git/MCP code anywhere in this module — verified by inspection, nothing to test the absence of.

Also covered beyond the ACs' literal text, matching chili-jar's proven behavior `SPEC.md` cites: `--resume` flag passthrough (`run_with_passes_resume_flag_and_session_id_when_given`), `error_max_turns` treated as a usable result not an error (`treats_error_max_turns_as_a_usable_result_not_an_error`), other result subtypes rejected distinctly (`other_result_subtypes_are_a_typed_error_not_a_fallback`), missing `session_id` falling back to the `resume` value (`missing_session_id_on_result_event_falls_back_to_resume_value`), a nonexistent binary producing a distinct `Spawn` error (`run_with_nonexistent_binary_is_a_spawn_error`).

20 new unit tests in `src/process.rs`. `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean; full suite (58 tests) re-run 5× with zero flakes.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

None required to proceed to F-05.
