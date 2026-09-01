//! Wires F-06's retry pipeline to F-07's store — run lifecycle persistence
//! (F-08). A `runs` row is inserted (`status = running`, `owner_pid` =
//! this process) before the claude subprocess starts, and updated exactly
//! once on completion.

use std::path::Path;

use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

use crate::retry::{self, RetryExhausted, RunOutcome};
use crate::store::StoreError;
use crate::store::runs::{self, NewRun};

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

/// Runs one claude turn (F-06's retry pipeline) with full lifecycle
/// persistence. `task` is the raw task text stored in `runs.task`; `prompt`
/// is the already-built full prompt (trailer included, per F-05) actually
/// sent to `claude` — this function doesn't build prompts itself, that's
/// the caller's job (F-10), keeping this module about persistence wiring
/// only, not prompt construction.
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

            // Stored stripped of the NEXT_ACTION/RECHECK_AFTER trailer
            // (crate::signal::strip_trailer) — those fields already have
            // their own columns; keeping them duplicated inside the
            // stored text too would just be clutter, and F-12's `runner
            // logs` and F-10's immediate stdout would otherwise need to
            // strip it themselves independently instead of sharing one
            // already-clean value.
            let result_text = crate::signal::strip_trailer(&outcome.result.result);

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
/// cleanup, never automatic resumption (SPEC.md AC-04). Deliberately does
/// *not* assume every `running` row found is stale: a concurrently active
/// daemon tick can legitimately leave one for a separate `runner status`
/// invocation to see, so each row's `owner_pid` is checked individually.
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
    use crate::process::ClaudeResult;
    use crate::signal::ContinuationSignal;
    use std::time::Duration;

    fn open_in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::store::migrate(&conn).unwrap();
        conn
    }

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
                    crate::signal::parse_continuation_signal("no trailer").unwrap_err(),
                ),
                second: retry::RunFailure::SignalParse(
                    crate::signal::parse_continuation_signal("still no trailer").unwrap_err(),
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
