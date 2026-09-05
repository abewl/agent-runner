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
}
