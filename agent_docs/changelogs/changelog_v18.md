# Changelog v18

**Feature:** F-16 — Self-Chaining Task Loop (E-06)
**Date:** 2026-09-05 to 2026-09-06
**Branch:** `feature/F-16-self-chaining-task-loop` (off `refactor/cli-simplification`)

## Summary

`runner run <task>` now chains a task across multiple `claude` turns by default — PM scopes, Engineer implements, PM reviews, and so on — until the agent itself signals it's done, instead of a human re-invoking the CLI after every turn. `--once` opts back into the exact pre-F-16 single-turn behavior. **E-06 is complete — the first epic added since Stage 1 Batch 1 closed.**

## Why this exists: a scoping correction, not a new feature bolted on

Stage 1 originally treated "one `runner run` invocation" and "one `claude` turn" as the same unit. That boundary was slightly wrong: a human-triggered task and a single turn aren't actually the same thing, and requiring a human to manually re-invoke `runner run` after every turn just to let a PM→Engineer→PM loop continue was friction the design never needed to impose. This was a deliberate, discussed flip in how "a run" is scoped — see `PROJECT.md`'s amended PM/Engineer chaining section — not a reversal of Runner's "never orchestrate" principle: Runner still never interprets what `next_action` *means*, it just now also asks the agent an explicit, separate, content-blind question — "should I keep going right now?" — and mechanically relays the answer, the same way it already mechanically relays `recheck_after` for cron.

## The mechanism: reuse everything, add almost nothing new

`persist::run_chain`/`run_chain_using` loop the *exact same* `lookup` → `build_prompt` → `run_and_persist` cycle a single manual run already did. Because the turn just completed is already `done` in the store by the time the next iteration's `lookup` runs, session/context continuity across turns falls entirely out of the pre-existing resume machinery — no in-memory state needed to be threaded between loop iterations at all. The new trailer field driving it, `CHAIN_CONTINUE: yes|no`, is deliberately a *different* field from `recheck_after`, not a repurposing of it — conflating "check again later" (cron's concern) with "keep going right now" (this epic's concern) would have been two unrelated ideas wearing one field.

The loop returns an explicit `ChainStopReason` enum (`AgentDone` / `Stuck` / `MaxTurnsReached`) rather than making the caller infer the stop reason from `outcomes.len()`/the last outcome's flags — the first draft did the latter, and it was a real, findable bug: a stuck-stop landing on exactly the last allowed turn is indistinguishable from a cap-stop under shape-inference alone. Caught while writing the dedicated "stuck stops before the cap" test, not by review.

## Three real, load-bearing gaps found via manual verification, not by inspection

This feature holds the record for most manual-verification rounds in this project's history — precisely because each round against real `claude` surfaced a genuine new gap, each one documented in full in `DICT.md`:

1. **Non-interactive `claude` blocks on any file write/edit with no permission flag** — pre-existing since F-04, affects `--once` too, not just chains. Fixed with `--permission-mode acceptEdits`; deliberately not `--dangerously-skip-permissions`/`bypassPermissions`, which Anthropic's own `claude --help` text recommends only for sandboxes with no internet access — Runner runs on the user's real, networked machine, the opposite context.
2. **A genuinely stuck task burns the entire turn cap before stopping** — even after the agent's own text explicitly recognized it was looping, it kept reporting `chain_continue: yes`. Fixed with a stuck-detector: 3 consecutive identical `next_action` *labels* (deliberately not the `reason` text, which kept drifting wording — "blocked," "still blocked," "awaiting" — even while genuinely stuck) while `chain_continue` stays `true` stops the chain immediately, logged distinguishably from both a clean stop and a cap-reached stop.
3. **`RECHECK_AFTER`'s "omit if it doesn't apply" wording let the model write a placeholder instead of omitting the line** — pre-existing since F-05. The model wrote `RECHECK_AFTER: none`, which the old variable-width parser (any digits + one unit letter) treated as a fatal error for the *entire* signal. One hiccup that used to cost one failed manual run now killed an entire chain. Fixed by tightening the shared contract rather than loosening the parser: `RECHECK_AFTER` is now a mandatory line with exactly two valid shapes — the literal token `NONE`, or a fixed-width `05m`/`02h`/`01d` duration (never a variable-width `2h`/`30m`). Anything else is still a real parse error, not silently defaulted — the fix closes an ambiguity in the contract, it doesn't make the parser more forgiving of garbage.

The general lesson from #3, worth carrying into any future trailer field: never phrase an instruction as "include X unless it doesn't apply, in which case omit it" — that asks a free-text generator to make a *structural* decision (delete a whole line) instead of choosing from a fixed set of tokens, which is exactly where a strict parser and natural language drift apart. Always give an explicit token for "doesn't apply," and always require the line.

## Real end-to-end verification, round by round

- **Round 1** (base chain mechanics): confirmed real multi-turn resume/persistence works, immediately hit gap #1 — 20 real turns burned on a permission-blocked write, real API cost incurred for nothing.
- **Round 2** (permission fix applied): confirmed the write now succeeds (file genuinely contained `1`), immediately hit gap #3 — `RECHECK_AFTER: none` aborted the whole chain via a hard signal-parse failure.
- **Round 3** (both fixes applied): a full 3-turn chain completed end-to-end for real — `steps.txt` genuinely contained `1\n2\n3`, clean `chain complete: 3 turn(s)` stop printed.
- **Round 4** (deliberately-engineered stuck task): confirmed the stuck-detector fires against a real model response after exactly 3 turns — not the 10-turn cap — with the distinguishable stop message.

Zero leaked processes after every round, verified via `ps aux` each time.

## AC items validated

`SPEC.md`'s "Self-Chaining Task Loop (F-16)", all 14:

- **AC-01/AC-02** — the `CHAIN_CONTINUE` trailer field and its safe-default-on-malformed behavior. Unit-tested (`claude/signal.rs`).
- **AC-03/AC-04** — chains by default, `--once` is byte-for-byte the old path. Verified both by code (the `--once` branch is untouched from pre-F-16) and by the real end-to-end runs above.
- **AC-05/AC-06/AC-13** — the three distinguishable stop reasons (`AgentDone`/`MaxTurnsReached`/`Stuck`). Unit-tested with dedicated tests for each, including the fewer-than-threshold negative case; AC-06 and AC-13 both verified live in rounds 3 and 4 above.
- **AC-07** — hard failure aborts the whole chain. Unit-tested (`chain_aborts_entirely_on_a_hard_failure_mid_chain`).
- **AC-08** — each turn its own row, shared `task_identity`, no schema change. Verified live via `runner status`/direct `sqlite3` inspection in earlier verification rounds (21 rows, 2 distinct task identities).
- **AC-09** — preflight once per chain, not per turn. True by construction — `cli::run::run` calls `preflight::check()` once, before entering either the `--once` or chain branch.
- **AC-10** — `strip_trailer` also strips `CHAIN_CONTINUE`. Unit-tested.
- **AC-11** — cron untouched. Verified by inspection — no changes to `cron_engine.rs` in this feature.
- **AC-12** — `--permission-mode acceptEdits` always passed. Unit-tested (fake-script arg capture) and verified live (round 3's successful write).
- **AC-14** — `RECHECK_AFTER` mandatory, fixed-width, explicit `NONE`. Unit-tested (`NONE` case-insensitivity, old-style placeholder still rejected, fixed-width rejection of the old variable-width forms).

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

None identified. Not merged into `develop` — this branch, and the two branches it's stacked on (`refactor/cli-simplification`, `refactor/module-consolidation-and-comment-trim`), remain unmerged pending explicit instruction, per this project's established git workflow.
