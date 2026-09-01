# CLAUDE-PM Loop — Planning Half of the Self-Chaining Relay

## Purpose

This is the **CLAUDE-PM** routine's protocol — the other half of the relay with `agent_docs/CLOUD-LOOP.md`. This routine scopes work; it never implements. It decomposes the active `GOALS.md` goal into `EPICS.md` entries and `FEATURE.md`/`SPEC.md` entries, hands off to the Engineer routine, and picks up again when Engineer needs the next chunk scoped.

Operationalizes AGENT.md's existing CLAUDE-PM role (`agent_docs/CLAUDE-PM.md`) for unattended, self-chaining execution. Does not override either document.

**Created:** 2026-08-20, alongside the relay restructuring in `CLOUD-LOOP.md`. Read that document's Provenance note for why the design has no per-hop PR.

---

## 1. Trigger and branch model

| Setting | Value |
|---|---|
| Mechanism | Anthropic **Routine**, combining two triggers on the same routine object: a daily schedule (`0 21 * * *` UTC = 07:00 AEST, fixed — not local Sydney time, so it doesn't shift with daylight saving) and its own API trigger (`POST .../routines/$CLAUDE_PM_TRIGGER_ID/fire`) |
| Fired by | The daily schedule (the real kickoff mechanism, added 2026-08-20 — see below), CLOUD-LOOP (Engineer) via `$CLAUDE_PM_FIRE_TOKEN` when its queue of already-scoped `[ ] todo` features runs dry, or self-fire (§5) |
| Target repo | _[configure in Routine settings — point at this repo]_ |
| Branch | Same shared goal branch as Engineer — `feature/g-<n>-<slug>`. **This routine creates it**, on its very first hop for a newly activated goal (checked out from `develop`). Every hop after that, both roles, checks it out fresh rather than recreating it. |
| Session type | Fresh, stateless cloud session per firing |

**Token naming, concretely:** the token generated for *this* routine's own API trigger (what fires *this* routine) needs to live in **Engineer's** environment, named `CLAUDE_PM_FIRE_TOKEN` — not in this routine's own environment. Symmetrically, Engineer's trigger token needs to live in *this* routine's environment, named `ENGINEER_FIRE_TOKEN`. A routine firing itself (self-chaining) still needs its own token in its own environment for that case. Practically: both tokens end up in the same `cloud-loop` cloud environment, since both routines are already pointed at it — two `.env` lines, not two environments.

**Daily kickoff, concretely:** the schedule trigger requires no special-casing — CLAUDE-PM's own bootstrap (§2–§3) already covers every state it can land on: no active goal → harmless hard-stop no-op; active goal, no goal branch yet → this *is* kickoff, identical to any other first hop; branch exists with work remaining → an ordinary hop, same as a self-fire; goal already exhausted, final PR awaiting merge → hard-stops with a "nothing to do" note. The daily fire is just another legitimate entry point into the same logic the API trigger uses — not a different code path.

One residual risk, accepted rather than engineered around: if the daily fire lands at the exact moment an internal self-chain hop is also mid-flight, that's a real concurrent-push collision on the goal branch — the losing side's `git push` fails non-fast-forward and that session hard-stops cleanly rather than corrupting anything. Given hops finish in minutes and the schedule fires once a day, exact overlap is rare, and the failure mode is a clean stop, not a mess.

Engineer never gets a schedule trigger — its bootstrap hard-stops if no goal branch exists (it never creates one), so a daily fire there would only ever be noise. It's reached exclusively via CLAUDE-PM's fire or its own self-fire mid-chain.

---

## 2. Session bootstrap (every run)

**Step zero, unconditional, before anything else:** `git fetch origin develop && git checkout develop`. A fresh clone only has the repo's default branch (`main`) checked out — `develop` doesn't exist locally, and nothing else in this document is reachable, until this runs. Do not infer `agent_docs/`, `GOALS.md`, or this document's own existence from `main` — they live on `develop`.

1. Read `agent_docs/GOALS.md` from `develop` to find the active goal and its ID (`G-XX`).
2. Check whether a goal branch already exists for it: `git ls-remote --heads origin 'feature/g-<n>-*'`. If one exists, fetch and check it out. If not (first-ever hop for this goal), create it from `develop` (currently checked out) with a slug you choose from the goal's name.
3. Read, in order, from the goal branch now checked out: `agent_docs/PROJECT.md`, `agent_docs/AGENT.md`, `agent_docs/CLAUDE-PM.md` (this routine's own role definition), `agent_docs/GOALS.md`, `agent_docs/EPICS.md`, `agent_docs/FEATURE.md`, `agent_docs/DICT.md`, `agent_docs/SPEC.md`.
4. Confirm exactly one goal in `GOALS.md` is `active`. More than one, or none, is a hard-stop (§6) — this routine does not pick a goal itself.

---

## 3. Scoping selection

1. Read `EPICS.md`. Find the active goal's epics. If none exist yet for this goal, this hop's job is decomposing the goal itself into its first epic(s) — derive from `GOALS.md`'s objective and `PROJECT.md`, per the existing CLAUDE-PM operating rules in `AGENT.md` (don't invent scope beyond what the goal names).
2. If epics already exist, find the first one not yet fully broken into `FEATURE.md` entries. That's this hop's scope.
3. If every epic is already fully decomposed into `FEATURE.md`/`SPEC.md` entries, there's nothing to scope — this hop shouldn't have been fired for scoping. Check whether it was actually fired because Engineer's queue is genuinely empty and no epics remain either: if so, the goal is exhausted and this is Engineer's job (`CLOUD-LOOP.md` §6), not this routine's — hard-stop (§6) noting the mis-fire rather than guessing at new scope.

**One epic's worth of decomposition per hop.** Same reasoning as Engineer's one-feature-per-hop: bounded diff, isolated failure mode.

---

## 4. Authoring

- Write `EPICS.md` entries for the epic (if not yet present) and `FEATURE.md` entries + `SPEC.md` AC items for its features, matching the exact entry-format conventions already used in those files.
- Update `DICT.md` only if the epic introduces a genuinely new pattern or naming convention worth capturing — not on every hop, per AGENT.md's existing DICT.md update rule.
- **Never write or modify anything under `apps/`.** This routine authors documentation only, always. If a hop finds itself wanting to write code to "clarify" a spec, that's a sign the spec itself is underspecified — write the ambiguity down instead (as part of the entry's Description, or as a hard-stop if it's a real blocker) and let Engineer surface it for real if it turns out to matter.
- **Bare commit, bare push, directly to the goal branch.** No PR — same as Engineer, same reasoning (§6 of `CLOUD-LOOP.md`).

---

## 5. Chaining

After a successful push:

- If more epics remain undecomposed for the active goal, **fire this same routine again** (self-fire, `$CLAUDE_PM_FIRE_TOKEN` against its own trigger ID) rather than round-tripping through Engineer for scoping work Engineer can't do anyway.
- If the epic(s) just scoped produced new `[ ] todo` `FEATURE.md` entries and Engineer has work to do, fire Engineer — `curl -X POST .../routines/$ENGINEER_TRIGGER_ID/fire -H "Authorization: Bearer $ENGINEER_FIRE_TOKEN" ...`.
- Empty POST body in both cases.

---

## 6. Hard-stop

Triggers on: more than one (or zero) active goals in `GOALS.md`, a goal whose objective can't be decomposed without inventing scope `PROJECT.md` doesn't support, a mis-fire (see §3.3).

- Commit a blocker note to `agent_docs/cloud-loop-runs/BLOCKERS.md` on the goal branch (date, hop, what's ambiguous, what's needed from a human) and push it.
- **Do not fire anything.** Same reasoning as Engineer's hard-stop — a human has to resolve this before either routine continues.

---

## 7. What this routine never does

- Never implements code (`CLOUD-LOOP.md`'s job entirely).
- Never opens a PR mid-relay (only the final goal-exhaustion PR exists, and that's Engineer's to open per `CLOUD-LOOP.md` §6, since Engineer is the one that reaches the exhaustion condition).
- Never marks a goal `[x] achieved` — that happens once, in `CLOUD-LOOP.md` §6, after the final PR exists.
- Never picks the next goal when one is exhausted — same as before, that's a human/CLAUDE-PM-in-conversation decision, not something either routine does unattended.
