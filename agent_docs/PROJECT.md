# Runner — Project Definition

**Authoritative source:** this document; FEATURE.md/SPEC.md/DICT.md derive from it per AGENT.md.
**Last updated:** 2026-08-30
**Status:** Stage 1 scoped at both architecture and backlog level, including the single-target-repo model and the self-reported continuation-signal mechanism for PM/Engineer chaining. `FEATURE.md`/`SPEC.md`/`DICT.md` carry the revised Stage 1 batch (Batch 1 — see `agent_docs/EPICS.md`), ready for an engineering session to pick up.

---

## 1. What The Project Is

**Runner** is a thin, standalone Rust binary that acts as the compute-side execution layer for a single [chili-jar](../../chili-jar) `claude-code` harness agent deployment. It spawns and supervises a `claude` CLI subprocess, speaks chili-jar's existing sidecar wire protocol back to chili-jar's `services/runtime` (heartbeat, telemetry events, inbox pull/reply over HTTP; `ready`/`done`/`error`/`ticket`/`capabilities` JSON signal lines on stdout/stderr), and persists enough state — Claude session ID, last known status — to resume correctly across a crash or restart.

Runner owns none of the judgment. Which ticket to work next, retry vs. blocked decisions, goal achievement, the database, the MCP tools, the dashboard — all of that stays exactly where it is today, in chili-jar. Runner exists to solve one problem cleanly: **run the `claude` CLI unattended, using the correct auth mechanism for wherever it's running, without smuggling subscription credentials onto remote machines** — which is not supported by Anthropic for unattended automation and was the failure mode of an earlier, separate attempt at this exact problem.

Runner takes architectural inspiration from [Herdr](https://github.com/herdrdev/herdr) (a Rust terminal-agent multiplexer), but is a deliberately much smaller, purpose-built rewrite. Herdr's PTY/terminal-rendering/plugin-marketplace surface exists to drive an *interactive* TUI for a human. chili-jar's `claude-code` harness already runs `claude` in non-interactive `--print` mode — there is no terminal to render and no human attached — so none of that surface applies here.

**Relationship to chili-jar:** Runner is not, right now, a replacement for chili-jar's existing `sidecar.ts` + `harness-runner.mjs` + the process-management half of `adapters/claude-code/index.mjs`. It is deployed, versioned, and released entirely separately, with its own capabilities. It is deliberately built wire-compatible with chili-jar's existing protocol so that reconciling the two later — Runner taking over the `claude-code` harness's execution path inside chili-jar — remains cheap and low-risk. That reconciliation is a possible future decision, not a commitment made by this document.

**Delivery stages:** this document, and the feature batch it backs, scopes **Stage 1** only — a fully local, macOS-only, standalone deployment: a persistent daemon on the user's own machine, driven by a CLI and a simple TUI, with built-in cron-style scheduling, using only ambient Claude subscription auth. Stage 1 has no runtime dependency on and does no integration with chili-jar's wire protocol, `ANTHROPIC_API_KEY`/remote mode, or any `ComputeProvider`. Wiring Runner into chili-jar's remote execution path is explicit future scope (Stage 2+), not addressed by this batch.

---

## 2. Scope

**In scope:**

- **Package 1 — Daemon.** A genuinely long-lived background process that survives detach and restart. This is a real improvement over chili-jar's current model, where the sidecar's process lifetime is tied 1:1 to a single deployment. In Stage 1, the daemon's reason for existing is specifically the **cron-style scheduler**: persisted schedule definitions are evaluated on a tick and trigger the same run pipeline a manual CLI invocation uses, guarded against overlapping runs of the same schedule. A manual `runner run` does not require the daemon to be running — see Package 4.
- **Package 2 — Multi-agent process + minimal topology.** Spawns and manages `claude --print --output-format ... [--resume <sessionId>]` as a plain subprocess — no PTY, no terminal emulation, matching how chili-jar's adapter already invokes `claude`. Parses the `ready`/`done`/`error`/`ticket`/`capabilities` signal protocol off stdout/stderr. Also owns the mechanical per-task setup currently living in chili-jar's `claude-code` adapter — git clone/branch/push for engineer mode, MCP config + `.claude/settings.json` writing — since that is "how to correctly invoke `claude` for this task," not orchestration judgment. Topology is intentionally minimal: chili-jar's current model is one machine : one deployment : one serial process. Whether Runner should ever support multiple agents multiplexed on a single daemon is an **open, deliberately deferred decision** — not assumed by this package split.
- **Package 3 — Persistence + OS process supervision.** Durably persists session-resume state (Claude session ID, last known status) so a crash or restart doesn't lose `--resume` continuity — a real gap in chili-jar's current adapter, where this lives only in an in-memory variable and is lost on process death. Also owns local crash/respawn handling, scoped so it doesn't fight chili-jar's PM-level retry semantics (re-reading ticket status before respawning is chili-jar's job; Runner just needs to not duplicate work after its own restart).
- **Package 4 — Headless entry point + minimal control plane.** In Stage 1, the "control plane" is deliberately not a socket/RPC protocol — it's the shared local state store (Package 3) plus the CLI. `runner run`, `runner status`, and `runner logs` operate directly against the on-disk store whether or not the daemon is running; only cron-triggered runs strictly need a live daemon, since something has to be awake to fire the tick. Building an IPC layer is explicitly deferred until there's a second process or second machine that actually needs one — see chili-jar wire-protocol integration under Out of scope.
- **CLI** — the primary manual-control surface: `runner daemon start|stop|status`, `runner repo [path]`, `runner run <task>`, `runner status` / `runner ps`, `runner cron add|list|remove`.
- **TUI** — `runner logs` with no run id launches a simple, read-only status dashboard (run list with live status, per-run detail/result view); `runner logs <run-id>` shows one run's detail directly — one command, not two, since both read the exact same store. Not Herdr's terminal multiplexer — it never renders the `claude` subprocess's raw terminal output, because there is none to render (non-interactive `--print` mode); it's a structured status viewer over the run/schedule store.
- **Auth mode — Stage 1 is local-only.** Runner assumes ambient `claude` CLI subscription login already present on the machine (no credential injection, no credential file ever written or transmitted). This is the primary path for maximizing use of an existing Pro/Max subscription's quota. Remote mode (`ANTHROPIC_API_KEY` via env, matching chili-jar's `FlyMachinesProvider`) is a later stage — see Out of scope.
- **v1 targets the `claude-code` harness path only.** chili-jar's `none` (Direct LLM) harness already calls the LLM API directly with a provided key and has no CLI-auth problem — out of scope regardless of stage.
- **Single static Rust binary**, no runtime dependency, minimal memory/boot footprint. Stage 1 ships one artifact: a local CLI/daemon for macOS.
- **Target repo & agent-docs execution.** Stage 1 operates against exactly **one** configured local repository — set once (`runner repo <path>`, or `--repo <path>` at `runner daemon start`), not per-schedule, not multi-repo. Runner does not integrate with chili-jar's MCP server or any network service to do this: it runs `claude` with its working directory set to the target repo and relies entirely on `claude`'s own file tools plus that repo's existing `agent_docs/` convention (`PROJECT.md`/`AGENT.md`/`FEATURE.md`/`SPEC.md`) — the same convention this document and chili-jar's own agent docs already follow — for what to do. This is a dependency on a *convention*, not on chili-jar as a running service.
- **Local changes only in this batch — no push.** The eventual plan is a PAT-based push/commit identity (separate from the operator's own git identity, matching chili-jar's adapter pattern of `git config user.name = <agent identity>` with its own token). That's explicitly deferred; this batch only needs `claude` to read and write files in the target repo's working tree.
- **PM/Engineer chaining via a self-reported continuation signal, not Runner-side orchestration.** Every prompt Runner sends is one fixed, generic template — Runner never branches on project content or decides whether a turn should scope work (PM) or implement it (Engineer); that judgment stays entirely inside the `claude` call, per `AGENT.md`'s own already-documented protocol. What Runner adds is memory, not judgment: every invocation is instructed to end its response with a fixed trailer —
  ```
  NEXT_ACTION: <short label> — <one-line reason>
  RECHECK_AFTER: <duration, e.g. "30m">        (omit if not applicable)
  ```
  — which Runner parses as a fixed field, the same way it already parses `ready`/`done`/`error` signal lines elsewhere in this design, never as project-specific prose. `next_action`/`reason` are opaque to Runner: stored, displayed (`status`/`logs`/TUI), and handed back as context text on the next invocation of the same task identity — never branched on in code. The **only** field Runner's control flow ever mechanically acts on is `recheck_after` (a timestamp, compared against "now") — see the cron interaction rule below.
- **One persistence concept serves both the audit log and the continuation state.** The `runs` table (already the per-invocation log, Package 3) carries the self-reported `next_action` / `reason` / `recheck_after` alongside its existing result/session/cost fields. There is no separate "agent state" table — the most recent log row for a task identity *is* its current state.
- **Cron interaction: a recheck hint can only skip a tick, never fire early.** The user-set cron expression remains the outer, authoritative cadence — that's still "built-in cron commands" as originally scoped. If the last invocation's `recheck_after` timestamp hasn't elapsed yet when a schedule comes due, that tick is skipped (logged as a skip, not silently dropped) rather than spending a `claude` call on known-idle work. A signal can never cause Runner to fire *earlier* than the cron expression's own cadence — that would need a second, dynamic timer, which is explicitly not built in this batch.

**Out of scope / deferred:**
- **Remote/API-key auth mode and chili-jar wire-protocol integration** (heartbeat/telemetry/inbox HTTP calls, `ComputeProvider.SpawnConfig` env contract) — deferred to a later stage. Stage 1 is local-only and has zero chili-jar runtime dependency.
- Replacing chili-jar's `sidecar.ts` / `harness-runner.mjs` in production now. Reconciliation is possible later, not decided now.
- Any orchestration judgment — which ticket next, retry/blocked decisions, goal achievement. Stays in chili-jar's PM agent per `chili-jar/agent_docs/GOAL_LOOP.md`.
- Database (beyond Runner's own local store), MCP tool implementations, dashboard/UI. Entirely chili-jar's.
- Multi-agent-per-daemon topology/packing — Stage 1 is strictly one task running at a time per daemon; extending Runner to run N agents concurrently is a future decision requiring explicit scope, not implied by the current package split.
- `none`/Direct-LLM harness support.
- Herdr's PTY-backed terminal rendering, keyboard/mouse pane interaction, plugin marketplace, and human-facing SSH reattach — none of this applies. Runner's own Stage 1 TUI (see In scope) is a simple read-only status dashboard over structured run/schedule data, not a terminal multiplexer, and it never renders the `claude` subprocess's raw terminal output.
- A rich, regex-manifest-based working/idle/blocked state-detection engine (Herdr's `detect/` module). v1 liveness is process exit-code + stall-timeout based, since `claude --print` is non-interactive and doesn't produce the interactive-TUI-blocked states Herdr's manifests exist to classify. Revisit only if parsing `--output-format stream-json` event-level detail (tool calls, permission requests) turns out to earn its keep.

---

## 3. Architecture

### Stage 1 (this batch) — local only

```
runner (single binary)
  ├─ CLI (clap)                  ── runner daemon start|stop|status
  │                               ── runner repo [path]
  │                               ── runner run <task>
  │                               ── runner status / ps
  │                               ── runner logs [run-id]   (no id: TUI)
  │                               ── runner cron add|list|remove
  ├─ daemon                      ── long-lived process; owns the cron tick loop only
  │    └─ cron ticker  ──on due, unless recheck_after hasn't elapsed──▶ same run pipeline as `runner run`
  └─ run pipeline
       ├─ ambient-auth preflight (claude on PATH, logged in)
       ├─ continuation lookup (last session_id + next_action/reason for this task identity)
       ├─ spawn: claude --print --output-format json [--resume <sessionId>] "<task>"
       │    cwd = configured target repo; claude reads/writes that repo's own agent_docs/*
       │    (plain subprocess, no PTY — claude runs non-interactively)
       └─ parse fixed trailer from result text: NEXT_ACTION / RECHECK_AFTER

Shared local state store (SQLite, single file under
~/Library/Application Support/runner/) is the de facto control plane:
CLI, daemon, and TUI all read/write it directly. No socket/RPC protocol
in Stage 1 — only the cron ticker strictly requires the daemon to be running;
`runner run`/`status`/`logs` work standalone. The same `runs` row is both
the audit log entry and the continuation-state record for its task identity —
no separate "agent state" table.
```

### Stage 2+ (future, not scoped by this batch)

```
Local mode (ambient subscription auth)          Remote mode (ANTHROPIC_API_KEY via env)
  runner (CLI/daemon)                              runner (daemon, image entrypoint)
    └─ claude --print --resume <id> ...               └─ claude --print --resume <id> ...

Either mode, same wire contract back to chili-jar's services/runtime:
  POST {CHILI_API_URL}/telemetry/{deploymentId}/heartbeat   (SIDECAR_TOKEN)
  POST {CHILI_API_URL}/telemetry/{deploymentId}/event       (AGENT_TOKEN)
  POST {CHILI_API_URL}/inbox/{deploymentId}/pull
  POST {CHILI_API_URL}/inbox/{deploymentId}/{messageId}/reply
  stdout/stderr signal lines: {"type":"ready"|"done"|"error"|"ticket"|"capabilities", ...}
```

chili-jar's `services/runtime/src/control-plane/providers/*` (`local-process.ts`, `fly.ts`) would decide *where* compute is provisioned and *what env it's given*; Runner would be what actually runs once that compute exists — reading env, picking an auth strategy based on what's present, executing. Not built in this batch.

---

## 4. Key Design Decisions

- **Rust over TypeScript, deliberately, despite a working TypeScript implementation already existing** (chili-jar's `sidecar.ts`, in production with no significant issues). Two reasons, both explicit project-owner decisions, not assumed: (1) a single static Rust binary has a materially smaller footprint and faster boot than a Node runtime + `node_modules` image, which matters for ephemeral, fast-cycling remote compute; (2) a deliberate investment in learning Rust via a real, bounded, low-risk project. This knowingly trades a proven, already-debugged implementation for a from-scratch rewrite — an accepted tradeoff, not an oversight.
- **No PTY, no terminal emulation.** chili-jar's `claude-code` harness already runs `claude --print` non-interactively. Herdr needs a PTY because it renders an interactive TUI for a human; Runner has no human attached and nothing to render.
- **Auth mode is not a new abstraction — it rides on chili-jar's existing `ComputeProvider` split.** `LocalProcessProvider` implies ambient subscription auth; `FlyMachinesProvider` implies env-injected API key. This directly replaces chili-jar's current `CLAUDE_CREDENTIALS`-file-injection mechanism for remote deployments — the pattern an earlier, separate attempt at this problem ran into trouble with, not for technical reasons but because Anthropic's subscription OAuth token is scoped as an interactive-user credential, not an unattended-automation one.
- **Protocol compatibility over protocol redesign.** Runner targets chili-jar's existing heartbeat/telemetry/inbox HTTP contract and signal-line format as-is, even though it isn't (yet) replacing the sidecar in production. This keeps a future reconciliation cheap instead of requiring a second migration later.
- **Responsibility boundary held deliberately narrow.** Mechanical "how to invoke `claude` correctly" logic (process spawn, git workspace setup, MCP config, session persistence) lives in Runner. Anything requiring judgment or shared state (which ticket, retry/blocked decisions, goal state) stays in chili-jar. This cuts both ways on purpose: Runner should not grow into an orchestrator (at which point off-the-shelf Herdr would be the better choice), and chili-jar should not need to absorb Runner's execution concerns to use it.
- **Stage 1's control plane is a shared local SQLite store, not a socket protocol.** CLI commands (`run`, `status`, `logs`) operate directly against the store; only cron-triggered runs strictly require a live daemon, since something has to be awake to fire the tick. This avoids building IPC infrastructure before there's a second process or second machine that actually needs one.
- **Runner's Stage 1 TUI is a status viewer, not a terminal multiplexer.** It reads the same local store the CLI reads and renders structured run/schedule state. It does not attach to or render the `claude` subprocess's own terminal output — there isn't any, since `claude` runs via `--print`.
- **Continuation is a relayed signal, not inferred state.** Rejected: Runner scanning `FEATURE.md`'s status markers itself to decide PM-vs-Engineer mode — that requires Runner to understand project-specific backlog semantics, exactly the orchestration judgment this document's responsibility boundary keeps out of Runner. Also rejected: giving every tick zero memory and relying purely on `AGENT.md`'s cold-start session protocol every time — technically workable but wasteful, discarding a decision the agent already made moments before for no reason. The adopted middle ground reuses the project's own already-established signal-protocol pattern (a fixed trailer line, parsed the same mechanical way `ready`/`done`/`error` already are) instead of inventing new project-content parsing logic. Runner's code never contains a branch on `next_action`'s value — only on whether `recheck_after` has elapsed, which is a timestamp comparison, not an interpretation.

---

## 5. Constraints

- **Stage 1 targets macOS only.** No cross-platform requirement yet — paths (`~/Library/Application Support/runner/`), signal handling, and daemonization may assume macOS conventions without an abstraction layer.
- Must not require chili-jar code changes to adopt in a future remote mode — when built, that stage reads exactly the env vars `ComputeProvider.SpawnConfig` already injects today; not relevant to Stage 1's implementation.
- Must not depend on or modify chili-jar's database, MCP server, or dashboard, at any stage.
- Ships as a single static binary; does not assume Node/npm are present in its own runtime (the `claude` CLI it spawns is separately installed/managed — Runner does not take on installing or updating it beyond invoking it).
- Local mode must never write or transmit subscription credentials off the machine they were issued on — moot for Stage 1 specifically, since no credential handling code exists yet at all; stated here as a standing constraint for when remote mode is built.

---

## 6. Agent Wayfinding

Read this document first (per AGENT.md Session Start Protocol). Then:

| Question | Read |
|---|---|
| Current backlog / AC / patterns | `agent_docs/FEATURE.md`, `agent_docs/SPEC.md`, `agent_docs/DICT.md` |
| Cloud loop goals and epics | `agent_docs/GOALS.md`, `agent_docs/EPICS.md` |
| Cloud loop relay protocol | `agent_docs/CLOUD-LOOP.md`, `agent_docs/CLAUDE-PM-LOOP.md` |
| Claude's operating rules | `agent_docs/AGENT.md` — do not edit without explicit permission |
| Past session history | `agent_docs/changelogs/` |
| chili-jar's execution-layer contract (sidecar/harness-runner/claude-code adapter, `ComputeProvider` interface) that Runner targets for wire compatibility | `../chili-jar/packages/harness/src/sidecar.ts`, `../chili-jar/packages/harness/harness-runner.mjs`, `../chili-jar/packages/harness/adapters/claude-code/index.mjs`, `../chili-jar/services/runtime/src/control-plane/providers/interface.ts` |
| chili-jar's PM/Engineer self-chaining loop that spawns the deployments Runner would eventually execute inside | `../chili-jar/agent_docs/GOAL_LOOP.md` |

**Docs not yet written:** `FEATURE.md`, `SPEC.md`, and `DICT.md` are intentionally not yet authored — CLAUDE-PM should not run a backlog-authoring pass on this document until explicitly instructed to. `GOALS.md` and `EPICS.md` in this directory currently still contain unrelated leftover content from a different prototype ("Claudeworld") and have not been touched by this pass — they'll need to be reset before any CLAUDE-PM run against this PROJECT.md, but that's follow-on work, not done here.
