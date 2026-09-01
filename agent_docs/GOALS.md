# Goals — Long-Term Objectives for the Cloud Loop

## Purpose

A goal is the top of a three-layer scoping chain: `GOALS.md` (objective) → `EPICS.md` (2–3 epics' worth of decomposition) → `FEATURE.md` (granular backlog, with `SPEC.md` ACs). It exists so the self-chaining CLAUDE-PM/Engineer relay (`agent_docs/CLAUDE-PM-LOOP.md`, `agent_docs/CLOUD-LOOP.md`) has a bounded objective to work an entire multi-epic body of work against, on one shared branch, without a human picking each day's target by hand.

**Owner:** human, or CLAUDE-PM on the human's behalf. The relay *reads* this document; it does not author or close goals itself — CLAUDE-PM-LOOP authors epics/features under an already-active goal, but picking or closing the goal itself stays a human/CLAUDE-PM-in-conversation decision (`CLAUDE-PM-LOOP.md` §7).

---

## Format

```markdown
## G-XX: [Goal name] [Date set]
- Status: [ ] active | [x] achieved | [~] superseded
- Objective: one paragraph — what "done" looks like, and why it matters (tie back to PROJECT.md). Expected scale: 2–3 epics.
- Non-goals: explicitly out of scope, so decomposition doesn't drift into adjacent work
- Success criteria: how achievement is verified, beyond "all epics done"
```

No target-feature list here — that decomposition lives in `EPICS.md`, authored incrementally by CLAUDE-PM-LOOP as the relay runs, not written up front when the goal is set.

---

## Rules

1. Every epic under a goal, and every feature under an epic, must exist in `agent_docs/EPICS.md` / `agent_docs/FEATURE.md` with AC items in `agent_docs/SPEC.md` — a goal does not bypass that coupling contract (AGENT.md).
2. The relay decomposes and implements in listed/discovered order. Explicit epic independence (`EPICS.md`'s "Independent of" field) is documentation for now, not an active parallelisation signal — the relay is strictly serial.
3. When every epic is `[x] done`, the relay opens its one PR (goal branch → `develop`) and stops — see `CLOUD-LOOP.md` §6. It does not mark the goal achieved itself; a human confirms that after reviewing and merging the PR.
4. A goal can be superseded before completion (`[~] superseded`, with a note why) if PROJECT.md's direction changes.
5. Exactly one goal should be `active` at a time. The relay has no logic for choosing between competing active goals — more than one active is a hard-stop condition for both routines.

---

## Active goal

## G-01: Claudeworld Prototype — Grid World + Chop-Tree Loop [2026-08-30]
- Status: [ ] active
- Objective: Stand up a working end-to-end prototype that demonstrates the full Claudeworld architecture: a persistent in-memory grid world, an MCP server an LLM can connect to and act through, and a real-time React/TS web renderer a human can spectate. The prototype's complete playable action is the chop-tree → collect-wood loop: an LLM player navigates the grid, chops a tree across multiple tool calls until it fells, observes a wood log resource spawn in the world, collects it, and sees its inventory update — while a human watching the renderer sees the grid update in real time via SSE. All visual assets are SVG files authored to the monochrome Minecraft-2D design convention.
- Non-goals: authentication, multi-player session management, combat, crafting, map generation, any MCP tool beyond move/get_location/chop_tree/collect_wood, mobile support, production deployment, DB-backed persistence (in-memory for prototype).
- Success criteria: An LLM can connect to the MCP server cold, call get_location to orient itself, call move to navigate adjacent to a tree, call chop_tree three times to fell it, observe a wood_log in the get_location response, call collect_wood, and receive a confirmation that inventory.wood incremented — all while the web renderer reflects every state change in real time without a page refresh.
