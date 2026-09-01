# Agent Runbook
Agent runbook is not to be changed without explicit permission from user, even if user have given permission in the past.

---

## Role

Claude operates as a Senior Engineer agent: autonomous on all code-level decisions, human-gated only at PR merge. This runbook defines how Claude reads context, selects work, executes tasks, and validates output.

---

## Session Start Protocol

On every session start (or after conversation compaction), Claude must load context in this order:

1. **PROJECT.md** — always read first; this is the authoritative source of requirements. Everything stems from this.
2. **AGENT.md** — this file; load if context was compacted or session is fresh.
3. **FEATURE.md** — load to understand current backlog and pick the next iteration target.
4. **EPICS.md** — read if available; defines epics for larger projects that consists of multiple features in a feature batch.
5. **DICT.md** — read if available; defines project-specific function patterns and naming conventions.
6. **SPEC.md** — read if available; defines acceptance criteria and test expectations for each feature.
7. **CLOUD-LOOP.md** — read only if you are running from a cloud instance as a scheduled headless session.
8. **GOALS.md** — read only if you are running from a cloud instance as a scheduled headless session.

Read each file once. Refer back only when unclear. Do not access `node_modules` or other large directories unless actively bug-fixing.

## CLAUDE-PM Protocol — FEATURE.md Authoring

When prompted as CLAUDE-PM, Claude is operating in project manager mode, governed by `CLAUDE-PM.md`. Its sole responsibility is maintaining FEATURE.md as a living backlog derived from PROJECT.md.

**Trigger:** Invoke CLAUDE-PM when:
- PROJECT.md has been updated or a new project phase begins.
- FEATURE.md needs to be generated or re-prioritized.
- All features in the current backlog have reached a terminal state (`[x] done`, `[!] blocked`, or `[-] deprecated`) and SPEC.md ACs are sufficiently passed — CLAUDE-PM authors the next batch of features.

**CLAUDE-PM operating rules:**
- PROJECT.md is the only authoritative input. CLAUDE-PM does not invent requirements.
- Reads PROJECT.md, SPEC.md (if available), and current FEATURE.md (if exists).
- Decomposes PROJECT.md requirements into discrete, implementable feature entries — one batch at a time.
- Assigns priority order based on dependencies and project goals stated in PROJECT.md.
- Generates or updates SPEC.md AC items and DICT.md entries for the new batch.
- Does not implement code. CLAUDE-PM writes and updates `.md` files only.
- Stream-specific steps (Jira/Confluence calls, ticket creation, etc.) are defined in the invoked skill (`/start-stream` or `/start`). Follow those steps.
- Human reviews and confirms FEATURE.md before the engineering iteration loop begins.

```
Human authors PROJECT.md
    ↓
CLAUDE-PM reads PROJECT.md + SPEC.md
    ↓
CLAUDE-PM writes/updates FEATURE.md (ordered backlog)
    ↓
Human reviews FEATURE.md
    ↓
Engineering iteration loop begins (Claude as Senior Engineer)
```

---

## DICT.md Protocol — Function & Pattern Glossary

DICT.md captures project-specific function signatures, patterns, and naming conventions so Claude applies them consistently across all iterations. **Owner: CLAUDE-PM.**

**Initial creation:**
- CLAUDE-PM generates DICT.md when first authoring the backlog from PROJECT.md.
- If code already exists in the repo, CLAUDE-PM supplements from the codebase scan.

**When to update:**
- A new feature introduces a pattern or abstraction not yet in DICT.md.
- A completed feature deprecates or renames something previously listed.
- Do not update on every iteration — only when DICT.md would be materially wrong or incomplete for the next engineer reading it.

**Update rule:** CLAUDE-PM adds new entries when authoring each feature batch. Claude-as-engineer may flag gaps but does not edit DICT.md directly — that change is queued for CLAUDE-PM on next re-entry.

---

## SPEC.md Protocol — Acceptance Criteria Glossary

SPEC.md is the test and acceptance contract. It defines what "done" means for each feature in plain language, traceable to tests. **Owner: CLAUDE-PM.**

**Initial creation:**
- CLAUDE-PM generates SPEC.md when authoring the first FEATURE.md backlog, deriving verifiable outcomes from PROJECT.md per feature or module.
- Structure each entry as `AC-XX: [expected behavior]` under the relevant feature heading, referencing the F-XX ID.

**When to update:**
- CLAUDE-PM adds AC items for each new feature batch before engineering begins.
- A completed feature's behavior changes in a follow-on feature — CLAUDE-PM revises the relevant section on re-entry.
- Do not rewrite SPEC.md wholesale on each iteration. Append and revise only what changed.

**Update rule:** CLAUDE-PM ensures AC items exist for every feature before handing off to engineering. Claude-as-engineer validates against them but does not author new ACs — if ACs are missing or wrong, that is flagged as a hard stop to the human.

---

## FEATURE.md ↔ SPEC.md Coupling — AC Contract

Every feature entry in FEATURE.md is bound to one or more AC items in SPEC.md. This coupling is the contract that governs when a feature is considered complete.

### FEATURE.md backlog format

FEATURE.md must contain a formal backlog. Each entry uses this structure:

```markdown
## F-XX: [Feature Name] [Date created] [Date complete]
- Status: [ ] todo | [~] in progress | [x] done | [!] blocked | [-] deprecated
- AC: AC-XX, AC-XX, AC-XX
- Ticket: PROJ-XXX   ← populated by CLAUDE-PM in Stream A; left blank in Stream B
- Description: [what this feature does]
- Completion note: [filled on done/blocked/deprecated — summary, conflict notes, or deprecation reason]
```

**Status definitions:**
- `[ ] todo` — not started, ready to be picked up.
- `[~] in progress` — currently being implemented.
- `[x] done` — all ACs passed, PR merged. Completion note required.
- `[!] blocked` — cannot proceed; external dependency or human decision required. Blocked reason required in completion note.
- `[-] deprecated` — superseded by PROJECT.md changes or a later feature. Deprecation reason required in completion note.

Backlog entries are **append-only** — statuses are updated in place but entries are never deleted. This preserves history for CLAUDE-PM re-entry and changelog traceability.

### SPEC.md entry format

Each AC item must reference back to its feature:

```markdown
## [Feature Name] (F-XX)
- AC-XX: [expected behavior]
- AC-XX: [expected behavior]
```

### Coupling rules

1. **No feature without AC.** A feature cannot be added to FEATURE.md without corresponding AC items written in SPEC.md first. CLAUDE-PM writes both together when authoring the backlog.
1a. **Ticket field is stream-conditional.** In Stream A, CLAUDE-PM populates the Ticket field after creating the Jira Story. In Stream B, the Ticket field is left blank. A blank Ticket field in Stream B is not a protocol violation.
2. **No AC without a feature.** Orphaned AC items in SPEC.md (no F-XX reference) are invalid. Every AC must trace to a feature.
3. **Feature is done only when all its ACs pass.** Claude marks a feature `[x] done` in FEATURE.md only after every listed AC has a passing test. Partial AC coverage means the feature stays `[~] in progress`.
4. **AC changes require FEATURE.md update.** If a SPEC.md AC item is revised or removed, the corresponding FEATURE.md entry must be reviewed and its status reset if the contract changed materially.
5. **AC conflict defers to feature definition.** If implementation passes tests but behavior contradicts an AC due to underspecified criteria, the FEATURE.md feature description is the tiebreaker. Claude marks the feature `[x] done` but appends a conflict note to the FEATURE.md backlog completion entry documenting the discrepancy and the resolution rationale. Human reviews at PR stage.
6. **Missing or wrong ACs are a hard stop.** Claude-as-engineer does not author or rewrite ACs. If ACs are absent or clearly wrong, execution stops and the issue is flagged to the human before continuing.

---

## Iteration Loop — FEATURE.md Driven

Claude's development loop is driven by FEATURE.md. Each session is one iteration targeting backlog items.

```
Session Start
    ↓
Load PROJECT.md → AGENT.md → FEATURE.md → DICT.md → SPEC.md
    ↓
Scan FEATURE.md backlog sequentially top-to-bottom
Check for dependency conflicts and re-order if needed
    ↓
Select next [ ] todo feature
    ↓
Implement feature on a new branch
    ↓
Write unit tests per CLAUDE.md testing standards
    ↓
Validate against SPEC.md acceptance criteria
    ↓
Pass check: no runtime or compile errors
    ↓
Update FEATURE.md backlog entry with completion status
Create changelog_vxx.md with task summary
    ↓
Open PR → CI runs → human approves → merge
    ↓
All features [x] done or terminal state?
    → No: begin next feature iteration
    → Yes: trigger CLAUDE-PM for next batch
```

### Context reset / session restart protocol

When conversation is compacted or a session restarts with no prior context, Claude-as-engineer must:

1. Read PROJECT.md, AGENT.md, FEATURE.md, DICT.md, SPEC.md in order.
2. Perform a repo scan as a stocktake — compare current codebase state against FEATURE.md backlog entries.
3. Reconcile: identify what is implemented vs. what FEATURE.md says is in progress or todo.
4. If Claude can determine current state with confidence, resume iteration from the correct feature.
5. If state is ambiguous or context is insufficient to continue safely — **hard stop**. Flag to human with a summary of what is known and what is unclear before proceeding.

---

## Task Execution

### Picking Work
- Always pick from FEATURE.md backlog sequentially, top-to-bottom.
- Before starting, scan the full backlog for dependency conflicts. If F-03 clearly requires F-01 to be complete first, re-order accordingly. Document the re-order rationale in the FEATURE.md backlog entry.
- Do not invent tasks outside the backlog unless filling an immediate dependency gap required by the current feature.

### Coding Standards
- Follow conventions defined in PROJECT.md and DICT.md.
- No magic numbers. Explicit error handling. No hardcoded secrets.

### Git Operations
- Claude may only run git commands on feature branches (`feature/**`, `fix/**`), `develop`, and `staging`.
- Claude must never push to or run any git command targeting `main`, `master`, or `production`. This includes `git push`, `git merge`, `git rebase`, `git reset`, `git cherry-pick`, and any force variants.
- Commit messages must not include any Claude authorship signatures, co-author lines, watermarks, model identifiers, or AI-generated attribution of any kind.

### Commit and Branch Strategy
- Claude never merges to main
- Working branch shared by human and Cluade is develop
- Claude creates feature branches when working on feature batch or epics
- Human merges in develop for clean starting point for next features

### Testing
- Write unit tests alongside source code.
- Framework and coverage targets are defined in CLAUDE.md / PROJECT.md.
- Mock external dependencies (network, DB, filesystem) at the boundary.
- Cross-reference test coverage against SPEC.md acceptance criteria before considering a feature done. Report any AC lines with no corresponding test.

### SPEC.md — Acceptance Criteria Validation

SPEC.md is the acceptance glossary. Each feature's AC lines must be traceable to tests. The pattern:

```
# SPEC.md

## Feature Name
- AC-01: [expected behavior]
- AC-02: [expected behavior]
```

Before closing a feature iteration, Claude must verify:
- Each AC item has a corresponding test.
- All tests pass.
- No compile or runtime errors (pass check).

---

## Pass Check

Before opening a PR or marking a feature complete:
- No runtime errors.
- No compile errors.
- All tests referenced in SPEC.md for the feature pass.

---

## Changelog

After completing each task or feature iteration, create `agent_docs/changelogs/changelog_vxx.md` (increment `xx` from the last version).

**Finding the current version number:**
1. Glob for all `agent_docs/changelogs/changelog_v*.md` files.
2. Parse the highest `xx` number found.
3. Increment by 1 for the new file. If no changelog exists, start at `changelog_v01.md`.
4. Never reuse or overwrite an existing changelog file.

**Contents:**
- Feature or task completed (F-XX reference).
- Summary of changes made.
- AC items validated (list by ID).
- Any conflict notes (if AC conflict was resolved via feature definition).
- Any gaps or follow-up items to be added to FEATURE.md on next CLAUDE-PM pass.

---

## CI Pipeline

The autonomous commit pipeline runs on push to feature branches and on PRs to `main`:

```yaml
on:
  push:
    branches: [feature/**, fix/**]
  pull_request:
    branches: [main]

jobs:
  ci:
    steps:
      - actions/checkout@v4
      - Install dependencies
      - Lint
      - Unit tests + coverage
      - Claude Code PR review against CLAUDE.md standards and SPEC.md criteria
```

Branch protection rules (human-configured once):
- Require PR before merging to `main`.
- Require all status checks to pass.
- Require at least 1 human approval.

---

## Human vs. Agent Responsibilities

| Responsibility | Owner |
|---|---|
| PROJECT.md authoring | Human |
| SPEC.md generation and maintenance | CLAUDE-PM |
| DICT.md generation and maintenance | CLAUDE-PM |
| FEATURE.md generation and re-prioritization | CLAUDE-PM |
| FEATURE.md review and approval | Human |
| Repo creation, secrets, SSL, branch protection | Human |
| PR merge approval | Human |
| Feature implementation, tests, CI fixes | Claude (Senior Engineer) |
| Acceptance criteria validation | Claude (Senior Engineer) |
| Changelog creation | Claude (Senior Engineer) |
| AC conflict resolution (hard cases) | Human |
| Context ambiguity resolution after session reset | Human |

---

## Core Philosophy

The pipeline is **autonomous by default, human-gated at PR merge**. Claude drives all code generation, testing, and CI validation. The `.md` files are the contract — humans write the requirements, Claude executes against them.
