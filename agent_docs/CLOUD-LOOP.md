# Cloud Loop — Engineer Half of the Self-Chaining Relay

## Purpose

This is the **Engineer** routine's protocol — one half of a two-routine relay with `agent_docs/CLAUDE-PM-LOOP.md`. CLAUDE-PM scopes work (epics, features, ACs); this routine implements it. Neither routine authors and implements in the same run — that split is now structural, enforced by being two separate routines with two separate roles, not a rule one routine has to remember to follow.

This document operationalizes AGENT.md's Engineer role for unattended, self-chaining execution. It does not override `agent_docs/AGENT.md`.

**Provenance:** originally a daily-cron, one-feature-per-PR design (2026-08-16/17). Restructured 2026-08-20 into a self-chaining relay per the human's explicit decision: no PR-per-feature, no per-hop review — human review happens exactly once, at the relay's end, when the shared feature branch merges into `develop`.

---

## 1. Trigger and branch model

| Setting | Value |
|---|---|
| Mechanism | Anthropic **Routine**, fired via its own API trigger — `POST .../routines/<ENGINEER_TRIGGER_ID>/fire` |
| Fired by | CLAUDE-PM-LOOP, using `$ENGINEER_FIRE_TOKEN` from its own environment. This routine fires itself back for the next feature, or fires CLAUDE-PM-LOOP when its own queue of already-scoped features is empty (see §5). |
| Target repo | _[configure in Routine settings — point at this repo]_ |
| Branch | The **shared goal branch** — `feature/g-<n>-<slug>` (e.g. `feature/g-01-tech-debt-audit`), created once by CLAUDE-PM-LOOP's first hop for the active goal. This routine checks it out and pushes directly to it. Never `develop`, never `main`. |
| Session type | Fresh, stateless cloud session per firing |

No schedule trigger drives steady-state operation — the relay is entirely self-fired. A schedule trigger, if added later, is a fallback dead-man's-switch only (§8), not the primary mechanism.

---

## 2. Session bootstrap (every run)

**Step zero, unconditional, before anything else:** `git fetch origin develop && git checkout develop`. A fresh clone only has `main` checked out — `develop`, and everything on it including this document, isn't reachable until this runs.

1. Read `agent_docs/GOALS.md` from `develop` to find the active goal's ID. Find its goal branch: `git ls-remote --heads origin 'feature/g-<n>-*'`. If none exists yet, that's a hard-stop (§7) — CLAUDE-PM-LOOP creates the goal branch, this routine never does.
2. Fetch and check out that goal branch — not `develop` from here on. It carries every prior hop's work, both CLAUDE-PM's and Engineer's.
3. Read, in order, from the goal branch: `agent_docs/PROJECT.md`, `agent_docs/AGENT.md`, `agent_docs/GOALS.md`, `agent_docs/EPICS.md`, `agent_docs/FEATURE.md`, `agent_docs/DICT.md`, `agent_docs/SPEC.md`.
4. Confirm the active goal in `GOALS.md` matches what this branch is for. If it doesn't, that's a hard-stop (§7) — don't guess which goal you're implementing against.

---

## 3. Feature selection

1. Scan `FEATURE.md` for `[ ] todo` entries under the active goal's epics, in listed order.
2. Select the first one. It must already exist with AC items in `SPEC.md` — if it doesn't, that's CLAUDE-PM's job, not this routine's; hard-stop (§7) rather than author it yourself.
3. If nothing is `[ ] todo` but the active goal isn't fully covered by `EPICS.md` yet (more epics remain undecomposed), don't implement anything this hop — fire CLAUDE-PM-LOOP instead (§5) so it scopes the next epic.
4. If nothing is `[ ] todo` and every epic is fully decomposed and implemented, the goal is exhausted — go to §6 (final merge), not §5.

**Exactly one feature per hop.** Keeps each run's diff small and each run's failure mode isolated to one thing.

---

## 4. Implementation

- Code + unit tests per `agent_docs/DICT.md`.
- Non-negotiable, inherited from AGENT.md:
  - Never touch `develop` or `main` directly.
  - Never skip hooks (`--no-verify` etc.).
  - Commit messages carry no Claude/AI authorship signature, co-author line, or model identifier.
- Validate: run typecheck/tests per the feature's `SPEC.md` ACs (AGENT.md's Pass Check). On failure: **one** fix attempt, then hard-stop (§7) — no retry loop.
- Update the `FEATURE.md` entry to `[x] done` with a completion note.
- **Bare commit, bare push, directly to the goal branch.** No PR. `git add -A && git commit -m "..." && git push origin <goal-branch>`.

---

## 5. Chaining

After a successful push:

- If more `[ ] todo` features remain under already-decomposed epics, **fire this same routine again** — `curl -X POST .../routines/$ENGINEER_TRIGGER_ID/fire -H "Authorization: Bearer $ENGINEER_FIRE_TOKEN" ...` (self-fire; no need to round-trip through CLAUDE-PM for work that's already scoped).
- If no `[ ] todo` features remain and more epics need decomposing, fire CLAUDE-PM-LOOP instead — `curl -X POST .../routines/$CLAUDE_PM_TRIGGER_ID/fire -H "Authorization: Bearer $CLAUDE_PM_FIRE_TOKEN" ...`.
- If the goal is fully exhausted, don't fire anything — go to §6.

Empty POST body (`-d '{}'`) in every case — the fired routine reads fresh state itself, nothing needs to be passed in the fire payload.

---

## 6. Goal exhaustion — the one PR in this whole relay

When every epic under the active goal is decomposed and every resulting feature is `[x] done`:

1. Push the final state to the goal branch (already done per §4).
2. Open exactly one PR: `<goal-branch> → develop`. Title references the goal (`G-01`); body summarizes what the relay did across all its hops (feature list, epic list).
3. **Stop. Do not fire anything.** This is the sole human checkpoint in the entire relay, matching AGENT.md's existing rule that a human merges into `develop`. Nothing about this design changes that — it just moved from "once per feature" to "once per goal."
4. Update `GOALS.md`: mark the goal `[x] achieved` only after you've confirmed §6 completed correctly — don't mark it achieved before the PR exists.

---

## 7. Hard-stop

Triggers on: ambiguous repo/goal state, a `[ ] todo` feature with no `SPEC.md` ACs, a non-modular feature, a CI failure surviving the one fix attempt in §4.

- Action: commit a blocker note directly to the goal branch (append to `agent_docs/cloud-loop-runs/BLOCKERS.md` — date, hop, what's blocking, what's needed) and push it.
- **Do not fire anything.** The relay pauses here until a human resolves the blocker and manually re-fires either routine. Firing past an unresolved blocker risks every subsequent hop compounding the same confusion.
- No PR is opened for a hard-stop — there's no PR mechanism mid-relay by design (§1). The blocker note on the branch is the only trace; a human (or the dead-man's-switch, §8) has to notice the relay went quiet.

---

## 8. Dead-man's-switch (not yet built)

Nothing currently detects a hung run (the `.claude/`-protected-path permission-prompt class of failure, or any other unrecoverable hang) versus a legitimately paused hard-stop. `RemoteTrigger` has no cancel action — a stuck run can't be remotely killed, only noticed. A periodic schedule trigger checking "has either routine fired in the last N hours while the goal branch still has unfinished work" is the fallback worth adding once the relay's been proven — not built yet.

---

## 9. Run log archive

Same as before: every run's log (via `RemoteTrigger`'s `list_runs`/`get_run_log`) can be saved as `agent_docs/cloud-loop-runs/<date>-<session-id>.md`. Mandatory for hard-stops (§7's blocker note *is* this, effectively); optional for normal hops given there's no PR per hop to link it from anymore.

---

## 10. Run budget

Unchanged: no infrastructure-level per-run cap exists. Routines meter against the account's shared token pool. The bound is behavioral — §3's one-feature-per-hop scope and §4's one-fix-attempt rule.
