# EPICS.md — Development Epics

**Last updated:** 2026-08-30

Epics are the layer between `PROJECT.md` (the requirements) and `FEATURE.md` (the granular backlog). This batch is human-scoped directly from `PROJECT.md` §2–§4 (Stage 1), not derived from a `GOALS.md` entry — `GOALS.md`/`CLOUD-LOOP.md`'s self-chaining relay format doesn't apply to this pass, so epics here are not tagged with a `Goal: G-XX`. Owned by CLAUDE-PM.

**Revision note:** this is a pre-implementation revision of the original Batch 1 (nothing under it has reached `[~] in progress` or `[x] done`) — feature numbering was reset clean rather than appended out of dependency order, to add target-repo configuration and the self-reported continuation-signal mechanism (`PROJECT.md`'s PM/Engineer chaining design) at their correct place in the build sequence.

---

## Format

```markdown
## E-XX: [Epic name] [Date created]
- Status: [ ] scoping | [~] in progress | [x] done
- Objective: what this epic covers and why it's a distinct chunk of the batch
- Features: ordered list of F-XX entries under this epic
- Depends on: other E-XX epics this one requires to be complete first (build order)
```

`Status: [x] done` only once every listed feature is `[x] done` in `FEATURE.md`.

---

## E-01: Runtime Core [2026-08-30]
- Status: [x] done
- Objective: The foundation every other epic builds on — CLI/daemon skeleton, target-repo configuration, ambient-auth preflight, the actual `claude` subprocess invocation, the continuation-signal trailer convention, and bounded local retry. Nothing here touches persistence beyond the PID file and repo config; it's the raw "run one claude turn correctly, in the right repo, and get a structured continuation signal back" capability.
- Features: F-01, F-02, F-03, F-04, F-05, F-06
- Depends on: n/a — first epic

## E-02: Persistence [2026-08-30]
- Status: [x] done
- Objective: The local SQLite-backed state store that everything else (manual runs, scheduled runs, the TUI) reads and writes. Covers schema/init (now including the continuation-signal columns), wiring run lifecycle into it, restart reconciliation for interrupted runs, and combined session+continuation lookup so a named task's `claude --resume` continuity and its last self-reported signal both survive across separate invocations.
- Features: F-07, F-08, F-09
- Depends on: E-01 (persists the outcome of E-01's run pipeline)

## E-03: CLI Manual Control [2026-08-30]
- Status: [~] in progress
- Objective: The user-facing commands for triggering and inspecting runs by hand — `runner run`, `runner status`/`ps`, `runner logs`. These operate directly against the Stage 1 local store; none of them require the daemon to be running.
- Features: F-10, F-11, F-12
- Depends on: E-01, E-02

## E-04: Scheduler (Cron) [2026-08-30]
- Status: [ ] scoping
- Objective: Built-in cron-style scheduling — persisted schedule definitions, CLI CRUD for them, and the daemon-internal tick loop that evaluates and fires them through the same run pipeline manual commands use, guarded against overlapping runs of the same schedule and against firing a tick whose last `recheck_after` hint hasn't elapsed yet.
- Features: F-13, F-14
- Depends on: E-01, E-02, E-03 (reuses the run pipeline E-03 exercises manually)

## E-05: TUI [2026-08-30]
- Status: [ ] scoping
- Objective: A simple, read-only terminal status dashboard — run list with live status and continuation signal, per-run detail/result view — reading the same local store the CLI reads. No control actions in this batch; view only.
- Features: F-15
- Depends on: E-02 (reads the store), E-03 (mirrors the data CLI commands already expose)
