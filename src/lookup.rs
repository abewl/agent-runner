//! Session & continuation lookup (F-09). Given a task identity, finds the
//! most recent `done` run to `--resume` from and formats its self-reported
//! continuation signal into a one-line context string for the next prompt.

use rusqlite::Connection;

use crate::store::StoreError;
use crate::store::runs;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Continuation {
    pub session_id: Option<String>,
    pub context_line: Option<String>,
}

/// Looks up the most recent `done` run for `task_identity`. `failed`/
/// `interrupted` rows never qualify — only `done` — so a run with no prior
/// successful attempt (or only failed/interrupted ones) gets an empty
/// `Continuation`: no `--resume`, no context line, a fresh session
/// (SPEC.md AC-01/AC-03).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::runs::{NewRun, RunStatus, create, mark_done, update_status};

    fn open_in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::store::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn no_prior_run_gives_an_empty_continuation() {
        let conn = open_in_memory();
        let continuation = lookup(&conn, "never-run-before").unwrap();
        assert_eq!(continuation, Continuation::default());
    }

    #[test]
    fn only_failed_or_interrupted_rows_gives_an_empty_continuation() {
        let conn = open_in_memory();
        create(
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
        update_status(&conn, "r1", RunStatus::Failed).unwrap();

        create(
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
        update_status(&conn, "r2", RunStatus::Interrupted).unwrap();

        assert_eq!(lookup(&conn, "t").unwrap(), Continuation::default());
    }

    #[test]
    fn done_row_supplies_session_id_and_formatted_context_line() {
        let conn = open_in_memory();
        create(
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
        mark_done(
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
    fn empty_reason_omits_the_separator_rather_than_a_trailing_dash() {
        let conn = open_in_memory();
        create(
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
        mark_done(
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
    fn failed_row_after_a_done_one_does_not_shadow_it() {
        let conn = open_in_memory();
        create(
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
        mark_done(
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

        create(
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
        update_status(&conn, "failed-after", RunStatus::Failed).unwrap();

        let continuation = lookup(&conn, "t").unwrap();
        assert_eq!(continuation.session_id, Some("sess-good".to_string()));
    }

    #[test]
    fn lookup_is_scoped_to_the_given_task_identity() {
        let conn = open_in_memory();
        create(
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
        mark_done(
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
}
