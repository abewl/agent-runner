//! Run lifecycle: look up whether a task has a prior session to resume,
//! execute one claude turn through the retry pipeline, and persist the
//! result. A `runs` row is inserted (`status = running`, `owner_pid` =
//! this process) before the claude subprocess starts, and updated exactly
//! once on completion — never a second insert.

use std::path::Path;

use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

use crate::retry::{self, RetryExhausted, RunOutcome};
use crate::store::StoreError;
use crate::store::runs::{self, NewRun};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Continuation {
    pub session_id: Option<String>,
    pub context_line: Option<String>,
}

/// Most recent `done` run for `task_identity`, formatted as a resume
/// target. Failed/interrupted rows never qualify, even if more recent —
/// a task with no prior success gets an empty `Continuation`: no
/// `--resume`, no context line, a fresh session.
pub fn lookup(conn: &Connection, task_identity: &str) -> Result<Continuation, StoreError> {
    let Some(row) = runs::most_recent_done_for_task(conn, task_identity)? else {
        return Ok(Continuation::default());
    };

    let context_line =
        row.next_action
            .as_ref()
            .map(|next_action| match row.next_action_reason.as_deref() {
                Some(reason) if !reason.is_empty() => {
                    format!("Your own last recommendation was: {next_action} — {reason}")
                }
                _ => format!("Your own last recommendation was: {next_action}"),
            });

    Ok(Continuation {
        session_id: row.session_id,
        context_line,
    })
}

#[derive(Debug)]
pub enum PersistedRunError {
    Store(StoreError),
    Retry(RetryExhausted),
}

impl std::fmt::Display for PersistedRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PersistedRunError::Store(e) => write!(f, "{e}"),
            PersistedRunError::Retry(e) => write!(f, "{e}"),
        }
    }
}

impl From<StoreError> for PersistedRunError {
    fn from(e: StoreError) -> Self {
        PersistedRunError::Store(e)
    }
}

/// Runs one claude turn through the retry pipeline with full lifecycle
/// persistence. `task` is the raw task text stored in `runs.task`; `prompt`
/// is the already-built full prompt (trailer included) actually sent to
/// `claude` — the caller builds prompts, this function only persists.
pub fn run_and_persist(
    conn: &Connection,
    task_identity: &str,
    task: &str,
    prompt: &str,
    resume: Option<&str>,
    cwd: &Path,
) -> Result<RunOutcome, PersistedRunError> {
    let prompt = prompt.to_string();
    let resume = resume.map(String::from);
    let cwd = cwd.to_path_buf();
    run_and_persist_using(conn, task_identity, task, move || {
        retry::run_with_retry(&prompt, resume.as_deref(), &cwd)
    })
}

/// The actual persistence wiring, parameterized on the attempt itself
/// (mirrors `retry::run_with_retry_using`'s own injection pattern) so it's
/// testable with a canned `Ok`/`Err` closure instead of a real claude call.
fn run_and_persist_using(
    conn: &Connection,
    task_identity: &str,
    task: &str,
    attempt: impl FnOnce() -> Result<RunOutcome, RetryExhausted>,
) -> Result<RunOutcome, PersistedRunError> {
    let id = Uuid::new_v4().to_string();
    let started_at = Utc::now().to_rfc3339();
    let owner_pid = std::process::id() as i64;

    runs::create(
        conn,
        &NewRun {
            id: &id,
            task_identity,
            task,
            started_at: &started_at,
            owner_pid,
        },
    )?;

    match attempt() {
        Ok(outcome) => {
            let ended_at = Utc::now().to_rfc3339();
            let recheck_after = outcome
                .signal
                .recheck_after
                .map(|d| (Utc::now() + d).to_rfc3339());

            // Stored stripped of the NEXT_ACTION/RECHECK_AFTER trailer —
            // those fields already have their own columns, so keeping them
            // duplicated inside the stored text too would just be clutter,
            // and every reader of `result_text` would otherwise need to
            // strip it themselves instead of sharing one already-clean
            // value.
            let result_text = crate::claude::signal::strip_trailer(&outcome.result.result);

            runs::mark_done(
                conn,
                &id,
                outcome.result.session_id.as_deref(),
                outcome.result.cost_usd,
                &ended_at,
                outcome.retried as i64,
                &outcome.signal.next_action,
                &outcome.signal.reason,
                recheck_after.as_deref(),
                &result_text,
            )?;

            Ok(outcome)
        }
        Err(failure) => {
            let ended_at = Utc::now().to_rfc3339();
            runs::mark_failed(conn, &id, &failure.to_string(), &ended_at, 1)?;
            Err(PersistedRunError::Retry(failure))
        }
    }
}

/// Hard cap on how many turns a single `run_chain` call will fire —
/// a resource guard, not a judgment about the task's content, in the
/// same spirit as `retry`'s bounded single retry. Not yet configurable;
/// raise this (or add a flag) if a real chain needs more.
pub const DEFAULT_CHAIN_MAX_TURNS: usize = 10;

/// Runs `task` as a self-chaining sequence of turns: after each one, unless
/// the agent's `chain_continue` signal is `true`, the chain stops. Each
/// turn re-runs the exact same `lookup` → `build_prompt` → `run_and_persist`
/// cycle a single manual `runner run` already does — the turn just
/// completed is already `done` in the store by the time the next
/// iteration's `lookup` runs, so session/context continuity falls out of
/// the existing resume machinery for free, with no in-memory threading
/// needed between iterations. `on_turn` is called once per completed turn
/// (before checking whether to continue), so a caller can stream progress
/// rather than waiting for the whole chain to finish.
///
/// Why a chain stopped — returned explicitly rather than left for the
/// caller to infer from `outcomes.len()`/the last outcome's fields, since
/// two different reasons (stuck, and reaching `max_turns`) can otherwise
/// produce the exact same `(len, chain_continue)` shape when a stuck-stop
/// happens to land on the final allowed turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainStopReason {
    /// The agent reported `CHAIN_CONTINUE: no` (or it was missing/
    /// unparseable, which defaults to the same thing) — a clean stop.
    AgentDone,
    /// The same `next_action` label repeated `STUCK_REPEAT_THRESHOLD`
    /// times in a row while the agent still said `chain_continue: yes`.
    Stuck,
    /// `max_turns` was reached while the agent still said
    /// `chain_continue: yes` and no stuck-repeat had triggered yet.
    MaxTurnsReached,
}

/// A hard failure on any turn propagates immediately via `?`, aborting
/// the rest of the chain rather than continuing past it. Runs `task` as a
/// self-chaining sequence of turns: after each one, unless the agent's
/// `chain_continue` signal is `true`, the chain stops. Each turn re-runs
/// the exact same `lookup` → `build_prompt` → `run_and_persist` cycle a
/// single manual `runner run` already does — the turn just completed is
/// already `done` in the store by the time the next iteration's `lookup`
/// runs, so session/context continuity falls out of the existing resume
/// machinery for free, with no in-memory threading needed between
/// iterations. `on_turn` is called once per completed turn (before
/// checking whether to continue), so a caller can stream progress rather
/// than waiting for the whole chain to finish.
pub fn run_chain(
    conn: &Connection,
    task_identity: &str,
    task: &str,
    cwd: &Path,
    max_turns: usize,
    on_turn: impl FnMut(&RunOutcome),
) -> Result<(Vec<RunOutcome>, ChainStopReason), PersistedRunError> {
    let cwd = cwd.to_path_buf();
    run_chain_using(
        conn,
        task_identity,
        task,
        max_turns,
        on_turn,
        move |prompt, resume| retry::run_with_retry(prompt, resume, &cwd),
    )
}

/// The actual chain-loop wiring, parameterized on the attempt itself —
/// mirrors `run_and_persist`/`run_and_persist_using`'s own split, so the
/// looping/stopping logic is testable with a canned sequence of
/// `Ok`/`Err` outcomes instead of a real claude call.
fn run_chain_using(
    conn: &Connection,
    task_identity: &str,
    task: &str,
    max_turns: usize,
    mut on_turn: impl FnMut(&RunOutcome),
    mut attempt: impl FnMut(&str, Option<&str>) -> Result<RunOutcome, RetryExhausted>,
) -> Result<(Vec<RunOutcome>, ChainStopReason), PersistedRunError> {
    let mut outcomes = Vec::new();

    for _ in 0..max_turns {
        let continuation = lookup(conn, task_identity)?;
        let prompt =
            crate::claude::signal::build_prompt(task, continuation.context_line.as_deref());
        let resume = continuation.session_id.clone();

        let outcome = run_and_persist_using(conn, task_identity, task, || {
            attempt(&prompt, resume.as_deref())
        })?;

        on_turn(&outcome);
        let should_continue = outcome.signal.chain_continue;
        outcomes.push(outcome);

        if !should_continue {
            return Ok((outcomes, ChainStopReason::AgentDone));
        }

        if is_stuck(&outcomes) {
            tracing::warn!(
                task_identity,
                threshold = STUCK_REPEAT_THRESHOLD,
                "chain: stopping early, next_action repeated with no progress"
            );
            return Ok((outcomes, ChainStopReason::Stuck));
        }
    }

    tracing::warn!(
        task_identity,
        max_turns,
        "chain: reached max-turn cap while the agent still requested to continue"
    );
    Ok((outcomes, ChainStopReason::MaxTurnsReached))
}

/// How many consecutive identical `next_action` labels count as "stuck."
/// Deliberately small — three real, back-to-back repeats is already a
/// strong, low-false-positive signal that no progress is being made, and
/// a low threshold caps the real cost of a genuinely stuck chain quickly
/// rather than waiting for `max_turns`.
pub const STUCK_REPEAT_THRESHOLD: usize = 3;

/// True once the last `STUCK_REPEAT_THRESHOLD` outcomes all report the
/// exact same `next_action` *label* — a plain string-equality check,
/// never an interpretation of what the label means. Compares the label
/// only, not the `reason` text: a genuinely stuck agent's reason can
/// still drift wording turn to turn ("blocked on X" / "still blocked on
/// X" / "awaiting X") even while making zero real progress, so comparing
/// the full text would miss exactly the case this exists to catch.
fn is_stuck(outcomes: &[RunOutcome]) -> bool {
    if outcomes.len() < STUCK_REPEAT_THRESHOLD {
        return false;
    }
    let last_n = &outcomes[outcomes.len() - STUCK_REPEAT_THRESHOLD..];
    let first_label = &last_n[0].signal.next_action;
    last_n.iter().all(|o| &o.signal.next_action == first_label)
}

/// Reconciles any `runs` row still `status = running` whose owning
/// process is confirmed not alive to `status = interrupted` — startup-time
/// cleanup, never automatic resumption. Deliberately does *not* assume
/// every `running` row found is stale: a concurrently active daemon tick
/// can legitimately leave one for a separate `runner status` invocation
/// to see, so each row's `owner_pid` is checked individually.
pub fn reconcile_interrupted_runs(conn: &Connection) -> Result<usize, StoreError> {
    let running = runs::list_running(conn)?;
    let mut reconciled = 0;
    for run in running {
        if !crate::pid::process_alive(run.owner_pid as i32) {
            runs::update_status(conn, &run.id, runs::RunStatus::Interrupted)?;
            reconciled += 1;
        }
    }
    Ok(reconciled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::process::ClaudeResult;
    use crate::claude::signal::ContinuationSignal;
    use crate::store::runs::RunStatus;
    use std::time::Duration;

    fn open_in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::store::migrate(&conn).unwrap();
        conn
    }

    // --- lookup ---

    #[test]
    fn lookup_no_prior_run_gives_an_empty_continuation() {
        let conn = open_in_memory();
        let continuation = lookup(&conn, "never-run-before").unwrap();
        assert_eq!(continuation, Continuation::default());
    }

    #[test]
    fn lookup_only_failed_or_interrupted_rows_gives_an_empty_continuation() {
        let conn = open_in_memory();
        runs::create(
            &conn,
            &NewRun {
                id: "r1",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 1,
            },
        )
        .unwrap();
        runs::update_status(&conn, "r1", RunStatus::Failed).unwrap();

        runs::create(
            &conn,
            &NewRun {
                id: "r2",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:01:00Z",
                owner_pid: 1,
            },
        )
        .unwrap();
        runs::update_status(&conn, "r2", RunStatus::Interrupted).unwrap();

        assert_eq!(lookup(&conn, "t").unwrap(), Continuation::default());
    }

    #[test]
    fn lookup_done_row_supplies_session_id_and_formatted_context_line() {
        let conn = open_in_memory();
        runs::create(
            &conn,
            &NewRun {
                id: "r1",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 1,
            },
        )
        .unwrap();
        runs::mark_done(
            &conn,
            "r1",
            Some("sess-abc"),
            0.01,
            "2026-09-01T00:01:00Z",
            0,
            "idle",
            "nothing left to do",
            None,
            "the result text",
        )
        .unwrap();

        let continuation = lookup(&conn, "t").unwrap();
        assert_eq!(continuation.session_id, Some("sess-abc".to_string()));
        assert_eq!(
            continuation.context_line,
            Some("Your own last recommendation was: idle — nothing left to do".to_string())
        );
    }

    #[test]
    fn lookup_empty_reason_omits_the_separator_rather_than_a_trailing_dash() {
        let conn = open_in_memory();
        runs::create(
            &conn,
            &NewRun {
                id: "r1",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 1,
            },
        )
        .unwrap();
        runs::mark_done(
            &conn,
            "r1",
            Some("sess-abc"),
            0.0,
            "2026-09-01T00:01:00Z",
            0,
            "idle",
            "",
            None,
            "the result text",
        )
        .unwrap();

        let continuation = lookup(&conn, "t").unwrap();
        assert_eq!(
            continuation.context_line,
            Some("Your own last recommendation was: idle".to_string())
        );
    }

    #[test]
    fn lookup_failed_row_after_a_done_one_does_not_shadow_it() {
        let conn = open_in_memory();
        runs::create(
            &conn,
            &NewRun {
                id: "done-first",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 1,
            },
        )
        .unwrap();
        runs::mark_done(
            &conn,
            "done-first",
            Some("sess-good"),
            0.0,
            "2026-09-01T00:01:00Z",
            0,
            "continue_engineer",
            "more to do",
            None,
            "the result text",
        )
        .unwrap();

        runs::create(
            &conn,
            &NewRun {
                id: "failed-after",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:05:00Z",
                owner_pid: 1,
            },
        )
        .unwrap();
        runs::update_status(&conn, "failed-after", RunStatus::Failed).unwrap();

        let continuation = lookup(&conn, "t").unwrap();
        assert_eq!(continuation.session_id, Some("sess-good".to_string()));
    }

    #[test]
    fn lookup_is_scoped_to_the_given_task_identity() {
        let conn = open_in_memory();
        runs::create(
            &conn,
            &NewRun {
                id: "r1",
                task_identity: "other-task",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 1,
            },
        )
        .unwrap();
        runs::mark_done(
            &conn,
            "r1",
            Some("sess-other"),
            0.0,
            "2026-09-01T00:01:00Z",
            0,
            "idle",
            "",
            None,
            "the result text",
        )
        .unwrap();

        assert_eq!(lookup(&conn, "t").unwrap(), Continuation::default());
    }

    // --- run_and_persist ---

    fn ok_outcome(next_action: &str, recheck_after: Option<Duration>) -> RunOutcome {
        RunOutcome {
            result: ClaudeResult {
                result: "some result text".to_string(),
                session_id: Some("sess-123".to_string()),
                cost_usd: 0.01,
            },
            signal: ContinuationSignal {
                next_action: next_action.to_string(),
                reason: "because".to_string(),
                recheck_after,
                chain_continue: false,
            },
            retried: false,
        }
    }

    #[test]
    fn success_creates_a_running_row_then_marks_it_done() {
        let conn = open_in_memory();
        let outcome = run_and_persist_using(&conn, "manual:hello", "hello", || {
            Ok(ok_outcome("idle", Some(Duration::from_secs(1800))))
        })
        .unwrap();

        assert_eq!(outcome.result.result, "some result text");

        let rows = runs::list(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1, "no second row should be inserted");
        let row = &rows[0];
        assert_eq!(row.status, runs::RunStatus::Done);
        assert_eq!(row.task_identity, "manual:hello");
        assert_eq!(row.task, "hello");
        assert_eq!(row.session_id, Some("sess-123".to_string()));
        assert_eq!(row.cost_usd, Some(0.01));
        assert_eq!(row.next_action, Some("idle".to_string()));
        assert_eq!(row.next_action_reason, Some("because".to_string()));
        assert!(row.recheck_after.is_some());
        assert!(row.ended_at.is_some());
        assert_eq!(row.owner_pid, std::process::id() as i64);
        assert_eq!(row.result_text, Some("some result text".to_string()));
    }

    #[test]
    fn result_text_is_stored_stripped_of_the_trailer() {
        let conn = open_in_memory();
        run_and_persist_using(&conn, "t", "task", || {
            Ok(RunOutcome {
                result: ClaudeResult {
                    result: "Actual answer.\n\nNEXT_ACTION: idle — done\nRECHECK_AFTER: 30m"
                        .to_string(),
                    session_id: Some("sess".to_string()),
                    cost_usd: 0.0,
                },
                signal: ContinuationSignal {
                    next_action: "idle".to_string(),
                    reason: "done".to_string(),
                    recheck_after: Some(Duration::from_secs(1800)),
                    chain_continue: false,
                },
                retried: false,
            })
        })
        .unwrap();

        let row = runs::list(&conn, 1).unwrap().into_iter().next().unwrap();
        assert_eq!(row.result_text, Some("Actual answer.".to_string()));
    }

    #[test]
    fn success_without_recheck_after_leaves_it_null() {
        let conn = open_in_memory();
        run_and_persist_using(&conn, "t", "task", || Ok(ok_outcome("idle", None))).unwrap();

        let row = runs::list(&conn, 1).unwrap().into_iter().next().unwrap();
        assert_eq!(row.recheck_after, None);
    }

    #[test]
    fn failure_marks_the_row_failed_not_a_second_insert() {
        let conn = open_in_memory();
        let err = run_and_persist_using(&conn, "t", "task", || {
            Err(RetryExhausted {
                first: retry::RunFailure::SignalParse(
                    crate::claude::signal::parse_continuation_signal("no trailer").unwrap_err(),
                ),
                second: retry::RunFailure::SignalParse(
                    crate::claude::signal::parse_continuation_signal("still no trailer")
                        .unwrap_err(),
                ),
            })
        })
        .unwrap_err();

        assert!(matches!(err, PersistedRunError::Retry(_)));

        let rows = runs::list(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1, "no second row should be inserted");
        let row = &rows[0];
        assert_eq!(row.status, runs::RunStatus::Failed);
        assert!(row.exit_reason.is_some());
        assert!(row.ended_at.is_some());
        assert_eq!(row.next_action, None);
        assert_eq!(row.recheck_after, None);
    }

    // --- reconcile_interrupted_runs ---

    #[test]
    fn reconcile_marks_only_rows_owned_by_dead_processes() {
        let conn = open_in_memory();

        // A row "owned" by a definitely-dead pid.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id() as i64;
        child.wait().unwrap();
        runs::create(
            &conn,
            &NewRun {
                id: "dead",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: dead_pid,
            },
        )
        .unwrap();

        // A row "owned" by this very test process — very much alive.
        runs::create(
            &conn,
            &NewRun {
                id: "alive",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: std::process::id() as i64,
            },
        )
        .unwrap();

        let reconciled = reconcile_interrupted_runs(&conn).unwrap();
        assert_eq!(reconciled, 1);

        assert_eq!(
            runs::read(&conn, "dead").unwrap().unwrap().status,
            runs::RunStatus::Interrupted
        );
        assert_eq!(
            runs::read(&conn, "alive").unwrap().unwrap().status,
            runs::RunStatus::Running,
            "a run owned by a live process must not be touched"
        );
    }

    #[test]
    fn reconcile_is_a_noop_when_nothing_is_running() {
        let conn = open_in_memory();
        assert_eq!(reconcile_interrupted_runs(&conn).unwrap(), 0);
    }

    // --- run_chain ---

    use crate::claude::signal::SignalParseError;
    use std::cell::{Cell, RefCell};

    fn chain_outcome(chain_continue: bool) -> RunOutcome {
        chain_outcome_labeled("continue_engineer", chain_continue)
    }

    fn chain_outcome_labeled(next_action: &str, chain_continue: bool) -> RunOutcome {
        RunOutcome {
            result: ClaudeResult {
                result: "turn result".to_string(),
                session_id: Some("sess-chain".to_string()),
                cost_usd: 0.01,
            },
            signal: ContinuationSignal {
                next_action: next_action.to_string(),
                reason: "more to do".to_string(),
                recheck_after: None,
                chain_continue,
            },
            retried: false,
        }
    }

    #[test]
    fn chain_stops_after_one_turn_when_chain_continue_is_false() {
        let conn = open_in_memory();
        let calls = Cell::new(0);
        let (outcomes, reason) = run_chain_using(
            &conn,
            "t",
            "task",
            20,
            |_| {},
            |_, _| {
                calls.set(calls.get() + 1);
                Ok(chain_outcome(false))
            },
        )
        .unwrap();

        assert_eq!(calls.get(), 1, "must not attempt a second turn");
        assert_eq!(outcomes.len(), 1);
        assert_eq!(reason, ChainStopReason::AgentDone);
        assert_eq!(runs::list(&conn, 10).unwrap().len(), 1);
    }

    #[test]
    fn chain_continues_across_multiple_turns_then_stops() {
        let conn = open_in_memory();
        let calls = Cell::new(0);
        let (outcomes, reason) = run_chain_using(
            &conn,
            "t",
            "task",
            20,
            |_| {},
            |_, _| {
                let n = calls.get() + 1;
                calls.set(n);
                // Distinct labels per turn so this exercises the "agent
                // says stop" path, not an accidental stuck-repeat trigger.
                Ok(chain_outcome_labeled(&format!("step-{n}"), n < 3))
            },
        )
        .unwrap();

        assert_eq!(calls.get(), 3);
        assert_eq!(outcomes.len(), 3);
        assert_eq!(reason, ChainStopReason::AgentDone);
        assert!(
            !outcomes.last().unwrap().signal.chain_continue,
            "the last turn is the one that reported done"
        );
        assert_eq!(
            runs::list(&conn, 10).unwrap().len(),
            3,
            "each turn is its own row"
        );
    }

    #[test]
    fn chain_stops_at_max_turns_cap_when_agent_keeps_requesting_continue() {
        let conn = open_in_memory();
        let calls = Cell::new(0);
        let (outcomes, reason) = run_chain_using(
            &conn,
            "t",
            "task",
            3,
            |_| {},
            |_, _| {
                let n = calls.get() + 1;
                calls.set(n);
                // Distinct labels per turn so the cap is what actually ends
                // this chain, not the stuck-detector (which would otherwise
                // trigger on turn 3 too, since 3 is also its threshold).
                Ok(chain_outcome_labeled(&format!("step-{n}"), true))
            },
        )
        .unwrap();

        assert_eq!(
            outcomes.len(),
            3,
            "capped at max_turns even though chain_continue stayed true"
        );
        assert_eq!(reason, ChainStopReason::MaxTurnsReached);
        assert!(outcomes.last().unwrap().signal.chain_continue);
    }

    #[test]
    fn chain_stops_early_when_stuck_before_reaching_max_turns() {
        let conn = open_in_memory();
        let calls = Cell::new(0);
        // max_turns is well above the stuck threshold, so a stop at
        // exactly STUCK_REPEAT_THRESHOLD turns can only be the
        // stuck-detector, not the cap.
        let (outcomes, reason) = run_chain_using(
            &conn,
            "t",
            "task",
            10,
            |_| {},
            |_, _| {
                calls.set(calls.get() + 1);
                Ok(chain_outcome_labeled("same_label_every_time", true))
            },
        )
        .unwrap();

        assert_eq!(calls.get(), STUCK_REPEAT_THRESHOLD);
        assert_eq!(outcomes.len(), STUCK_REPEAT_THRESHOLD);
        assert_eq!(reason, ChainStopReason::Stuck);
        assert!(
            outcomes.last().unwrap().signal.chain_continue,
            "the agent never said stop — the repeat-detector ended this, not a clean stop"
        );
    }

    #[test]
    fn chain_does_not_false_positive_on_fewer_than_threshold_repeats() {
        let conn = open_in_memory();
        let calls = Cell::new(0);
        // Same label twice (one less than STUCK_REPEAT_THRESHOLD), then a
        // clean stop — must not be mistaken for stuck.
        let (outcomes, reason) = run_chain_using(
            &conn,
            "t",
            "task",
            10,
            |_| {},
            |_, _| {
                let n = calls.get() + 1;
                calls.set(n);
                Ok(chain_outcome_labeled("same_label", n < 3))
            },
        )
        .unwrap();

        assert_eq!(outcomes.len(), 3);
        assert_eq!(reason, ChainStopReason::AgentDone);
    }

    #[test]
    fn chain_aborts_entirely_on_a_hard_failure_mid_chain() {
        let conn = open_in_memory();
        let calls = Cell::new(0);
        let err = run_chain_using(
            &conn,
            "t",
            "task",
            20,
            |_| {},
            |_, _| {
                let n = calls.get() + 1;
                calls.set(n);
                if n == 2 {
                    Err(RetryExhausted {
                        first: retry::RunFailure::SignalParse(SignalParseError),
                        second: retry::RunFailure::SignalParse(SignalParseError),
                    })
                } else {
                    Ok(chain_outcome(true))
                }
            },
        )
        .unwrap_err();

        assert!(matches!(err, PersistedRunError::Retry(_)));
        assert_eq!(
            calls.get(),
            2,
            "must not attempt a third turn after the second failed"
        );

        let rows = runs::list(&conn, 10).unwrap();
        assert_eq!(
            rows.len(),
            2,
            "the failed turn is still its own persisted row"
        );
        assert_eq!(
            rows.iter().filter(|r| r.status == RunStatus::Done).count(),
            1
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r.status == RunStatus::Failed)
                .count(),
            1
        );
    }

    #[test]
    fn on_turn_callback_fires_once_per_completed_turn() {
        let conn = open_in_memory();
        let seen = RefCell::new(Vec::new());
        run_chain_using(
            &conn,
            "t",
            "task",
            20,
            |outcome| seen.borrow_mut().push(outcome.signal.chain_continue),
            |_, _| {
                let n = seen.borrow().len();
                Ok(chain_outcome(n < 2))
            },
        )
        .unwrap();

        assert_eq!(*seen.borrow(), vec![true, true, false]);
    }

    #[test]
    fn chain_resumes_the_previous_turns_session_on_each_later_turn() {
        let conn = open_in_memory();
        let resumes_seen = RefCell::new(Vec::new());
        let mut turn = 0;
        run_chain_using(
            &conn,
            "t",
            "task",
            20,
            |_| {},
            |_, resume| {
                resumes_seen.borrow_mut().push(resume.map(String::from));
                turn += 1;
                Ok(RunOutcome {
                    result: ClaudeResult {
                        result: "text".to_string(),
                        session_id: Some(format!("sess-{turn}")),
                        cost_usd: 0.0,
                    },
                    signal: ContinuationSignal {
                        next_action: "continue_engineer".to_string(),
                        reason: String::new(),
                        recheck_after: None,
                        chain_continue: turn < 3,
                    },
                    retried: false,
                })
            },
        )
        .unwrap();

        let resumes = resumes_seen.borrow();
        assert_eq!(resumes[0], None, "first turn has no prior session");
        assert_eq!(
            resumes[1],
            Some("sess-1".to_string()),
            "second turn resumes the first turn's session"
        );
        assert_eq!(
            resumes[2],
            Some("sess-2".to_string()),
            "third turn resumes the second turn's session"
        );
    }
}
