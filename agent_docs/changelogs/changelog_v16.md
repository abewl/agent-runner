# Changelog v16

**Type:** Refactor — module consolidation and comment trimming (no functional change)
**Date:** 2026-09-05
**Branch:** `refactor/module-consolidation-and-comment-trim` (off `feature/F-15-tui-status-dashboard`)

## Why

Not a feature — a retrospective pass over all 15 Stage 1 features once they
were all in place at once, rather than one at a time. Two real, related
observations prompted it: (1) several module boundaries reflected feature
*PR* boundaries (one file per feature branch) rather than natural domain
boundaries, adding indirection a reader has to trace through by hand; (2)
doc comments throughout the source consistently cited feature numbers and
SPEC.md AC items inline (`"F-06's retry pipeline"`, `"SPEC.md AC-04"`),
which is real provenance information but the wrong *place* for it — it
belongs in `changelogs/`/`FEATURE.md`, where it already lives durably, not
duplicated into source comments where it decays as soon as a reader hasn't
seen the referenced PR.

## What changed

**Module consolidation:**
- `preflight.rs`, `process.rs`, `signal.rs` (three flat top-level modules,
  related only by convention — all three about talking to the `claude`
  CLI) moved into a new `claude/` folder (`claude/mod.rs` re-exports them
  `pub(crate)`). The relationship is now visible in the tree instead of
  inferred from having read all three.
- `lookup.rs` (238 lines, but one function + its own test suite, with
  exactly one caller in `persist.rs`) folded directly into `persist.rs`.
  `persist::lookup` replaces `lookup::lookup`; both call sites
  (`cli/run.rs`, `cron_engine.rs`) updated. This shortens the call chain
  for "run one task end-to-end" by one hop.
- No other module boundaries changed — the `cli/` one-file-per-command
  split and every `#[cfg(test)]` block were left exactly as they were;
  both are good conventions, not what this pass targeted.

**Comment trimming:**
- Every doc/inline comment citing a feature number, AC item, or a
  cross-reference to `SPEC.md`/`DICT.md`/`FEATURE.md` (103 occurrences
  across 20 files) rewritten to state the invariant or rationale directly,
  without the provenance framing. Example, verbatim before/after from
  `persist.rs`:
  ```rust
  // before
  //! Wires F-06's retry pipeline to F-07's store — run lifecycle
  //! persistence (F-08).

  // after
  //! Run lifecycle: look up whether a task has a prior session to
  //! resume, execute one claude turn through the retry pipeline, and
  //! persist the result.
  ```
- One real inaccuracy caught in passing, not just trimmed: `claude/process.rs`'s
  module doc claimed the result is parsed "out of `--output-format
  json`'s event array" — the real, verified behavior (confirmed against
  the actual parsing code) is a single flat JSON object, with array
  handling kept only as a defensive fallback. Doc corrected to say so.
- `DICT.md`'s "Repo layout" section (stale — still described a `runner.rs`
  file that was never the actual name, and predated `claude/` and the
  `lookup.rs` merge) rewritten to match the real current tree.
- `store/mod.rs`'s `SCHEMA_VERSION` comment (a genuine schema changelog,
  not development-process provenance) kept its "what changed and why"
  content, with only the F-XX/AC-XX framing stripped — that one is a
  legitimate exception to "move provenance out of source," since it's a
  history of the *schema itself*, not of the feature that built it.

## Verification

No behavior was intended to change, so verification was entirely
regression-focused:
- `cargo build`, `cargo clippy --all-targets`, `cargo fmt --check` clean
  both immediately after the module moves (before touching any comments)
  and again after the full comment sweep.
- Full suite (161 tests: 128 unit + 33 integration) re-run 5× with zero
  flakes — identical pass/fail counts and per-file breakdown to the
  pre-refactor baseline, confirming the module moves and the `lookup` →
  `persist` merge changed nothing observable.
- `ps aux` confirmed zero leaked processes.

## Net effect

25 source files (unchanged count — 4 files removed/merged, 4 added: 3
moved into `claude/` plus its new `mod.rs`), 4,517 lines (down from
4,541 — a modest reduction; most of the trimming shortened comment
*prose*, not comment *line count*, so the line-count delta undersells
how much was cut). Top-level module count in `main.rs` dropped from 12
to 9. Zero test coverage lost, zero behavior changed.

## Conflicts

None.

## Follow-ups for next CLAUDE-PM pass

None identified during this pass. Not merged into `develop` — this
branch is off `feature/F-15-tui-status-dashboard`, which is itself not
yet merged; both awaiting explicit instruction before any merge, per
this project's established git workflow.
