//! Typed CRUD for the `runs` table (F-07). Only this file builds SQL
//! against `runs` — always parameterized, never string-built from
//! caller-supplied values (SPEC.md AC-04).

use rusqlite::{Connection, OptionalExtension, params};

use super::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Done,
    Failed,
    Interrupted,
}

impl RunStatus {
    fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Done => "done",
            RunStatus::Failed => "failed",
            RunStatus::Interrupted => "interrupted",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "running" => Some(RunStatus::Running),
            "done" => Some(RunStatus::Done),
            "failed" => Some(RunStatus::Failed),
            "interrupted" => Some(RunStatus::Interrupted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub id: String,
    pub task_identity: String,
    pub task: String,
    pub status: RunStatus,
    pub session_id: Option<String>,
    pub cost_usd: Option<f64>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub exit_reason: Option<String>,
    pub retry_count: i64,
    pub next_action: Option<String>,
    pub next_action_reason: Option<String>,
    pub recheck_after: Option<String>,
    /// The OS pid of the `runner` process that created this row — added in
    /// F-08 (not part of F-07's original column list) specifically so
    /// startup reconciliation (SPEC.md F-08 AC-04) can tell a genuinely
    /// still-in-flight run (its owning process is alive) from one
    /// abandoned by a process that died mid-run, rather than assuming
    /// every `running` row found at startup is automatically stale — a
    /// concurrently-running daemon tick could easily leave a legitimate
    /// one for a `runner status` invocation to see. See `DICT.md`.
    pub owner_pid: i64,
}

pub struct NewRun<'a> {
    pub id: &'a str,
    pub task_identity: &'a str,
    pub task: &'a str,
    pub started_at: &'a str,
    pub owner_pid: i64,
}

const SELECT_COLUMNS: &str = "id, task_identity, task, status, session_id, cost_usd, started_at, ended_at, exit_reason, retry_count, next_action, next_action_reason, recheck_after, owner_pid";

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    let status_str: String = row.get(3)?;
    let status = RunStatus::from_str(&status_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            format!("unknown run status: {status_str}").into(),
        )
    })?;
    Ok(Run {
        id: row.get(0)?,
        task_identity: row.get(1)?,
        task: row.get(2)?,
        status,
        session_id: row.get(4)?,
        cost_usd: row.get(5)?,
        started_at: row.get(6)?,
        ended_at: row.get(7)?,
        exit_reason: row.get(8)?,
        retry_count: row.get(9)?,
        next_action: row.get(10)?,
        next_action_reason: row.get(11)?,
        recheck_after: row.get(12)?,
        owner_pid: row.get(13)?,
    })
}

/// Inserts a new `runs` row with `status = running` — a run is always
/// created in this state (F-08 AC-01 inserts before the subprocess even
/// starts); there's no code path that creates a row in any other status.
pub fn create(conn: &Connection, new_run: &NewRun) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO runs (id, task_identity, task, status, started_at, retry_count, owner_pid) VALUES (?1, ?2, ?3, 'running', ?4, 0, ?5)",
        params![
            new_run.id,
            new_run.task_identity,
            new_run.task,
            new_run.started_at,
            new_run.owner_pid
        ],
    )?;
    Ok(())
}

/// Marks a run done with its full result — session id, cost, and the
/// parsed continuation signal (already converted to an absolute
/// `recheck_after` timestamp by the caller). Updates the row exactly
/// once — never a second insert (SPEC.md AC-02/AC-03).
#[allow(clippy::too_many_arguments)]
pub fn mark_done(
    conn: &Connection,
    id: &str,
    session_id: Option<&str>,
    cost_usd: f64,
    ended_at: &str,
    retry_count: i64,
    next_action: &str,
    next_action_reason: &str,
    recheck_after: Option<&str>,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE runs SET status='done', session_id=?2, cost_usd=?3, ended_at=?4, retry_count=?5, next_action=?6, next_action_reason=?7, recheck_after=?8 WHERE id=?1",
        params![
            id,
            session_id,
            cost_usd,
            ended_at,
            retry_count,
            next_action,
            next_action_reason,
            recheck_after
        ],
    )?;
    Ok(())
}

/// Marks a run failed — both attempts (SPEC.md F-06) exhausted.
/// `next_action`/`next_action_reason`/`recheck_after` are left null, since
/// a failed run produced no valid signal to persist.
pub fn mark_failed(
    conn: &Connection,
    id: &str,
    exit_reason: &str,
    ended_at: &str,
    retry_count: i64,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE runs SET status='failed', exit_reason=?2, ended_at=?3, retry_count=?4 WHERE id=?1",
        params![id, exit_reason, ended_at, retry_count],
    )?;
    Ok(())
}

pub fn read(conn: &Connection, id: &str) -> Result<Option<Run>, StoreError> {
    conn.query_row(
        &format!("SELECT {SELECT_COLUMNS} FROM runs WHERE id = ?1"),
        params![id],
        row_to_run,
    )
    .optional()
    .map_err(StoreError::from)
}

/// Most recent runs first, capped at `limit`.
pub fn list(conn: &Connection, limit: i64) -> Result<Vec<Run>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM runs ORDER BY started_at DESC LIMIT ?1"
    ))?;
    let rows = stmt.query_map(params![limit], row_to_run)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

/// All rows currently `status = running`.
pub fn list_running(conn: &Connection) -> Result<Vec<Run>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM runs WHERE status = 'running' ORDER BY started_at DESC"
    ))?;
    let rows = stmt.query_map([], row_to_run)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

/// Bare status update — the minimal "update" CRUD operation this feature
/// needs (SPEC.md AC-04). F-08 adds richer, purpose-specific updates
/// (marking done with the full result, reconciling interrupted runs) on
/// top of this, since those need more fields set atomically together.
pub fn update_status(conn: &Connection, id: &str, status: RunStatus) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE runs SET status = ?2 WHERE id = ?1",
        params![id, status.as_str()],
    )?;
    Ok(())
}

/// Most recent `done` row for a task identity — F-09's resume/continuation
/// lookup. `failed`/`interrupted` rows never qualify, even if more recent
/// than the last `done` one (SPEC.md F-09 AC-03) — the `WHERE status =
/// 'done'` filter applies before the ordering, not after.
pub fn most_recent_done_for_task(
    conn: &Connection,
    task_identity: &str,
) -> Result<Option<Run>, StoreError> {
    conn.query_row(
        &format!(
            "SELECT {SELECT_COLUMNS} FROM runs WHERE task_identity = ?1 AND status = 'done' ORDER BY started_at DESC LIMIT 1"
        ),
        params![task_identity],
        row_to_run,
    )
    .optional()
    .map_err(StoreError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the real (private, but visible to this child module) `migrate`
    /// rather than a hand-copied schema — no duplicate SQL to drift out of
    /// sync with the actual migration.
    fn open_in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn create_then_read_round_trips() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewRun {
                id: "r1",
                task_identity: "manual:hello",
                task: "hello",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();

        let run = read(&conn, "r1").unwrap().unwrap();
        assert_eq!(run.id, "r1");
        assert_eq!(run.task_identity, "manual:hello");
        assert_eq!(run.status, RunStatus::Running);
        assert_eq!(run.retry_count, 0);
        assert_eq!(run.session_id, None);
    }

    #[test]
    fn read_returns_none_for_unknown_id() {
        let conn = open_in_memory();
        assert_eq!(read(&conn, "does-not-exist").unwrap(), None);
    }

    #[test]
    fn list_orders_most_recent_first() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewRun {
                id: "older",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();
        create(
            &conn,
            &NewRun {
                id: "newer",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T01:00:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();

        let runs = list(&conn, 10).unwrap();
        assert_eq!(runs[0].id, "newer");
        assert_eq!(runs[1].id, "older");
    }

    #[test]
    fn list_respects_limit() {
        let conn = open_in_memory();
        for i in 0..5 {
            create(
                &conn,
                &NewRun {
                    id: &format!("r{i}"),
                    task_identity: "t",
                    task: "task",
                    started_at: &format!("2026-09-01T00:0{i}:00Z"),
                    owner_pid: 100,
                },
            )
            .unwrap();
        }
        assert_eq!(list(&conn, 2).unwrap().len(), 2);
    }

    #[test]
    fn list_running_only_returns_running_rows() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewRun {
                id: "r1",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();
        create(
            &conn,
            &NewRun {
                id: "r2",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:01:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();
        update_status(&conn, "r2", RunStatus::Done).unwrap();

        let running = list_running(&conn).unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, "r1");
    }

    #[test]
    fn update_status_changes_only_the_targeted_row() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewRun {
                id: "r1",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();
        create(
            &conn,
            &NewRun {
                id: "r2",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:01:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();

        update_status(&conn, "r1", RunStatus::Failed).unwrap();

        assert_eq!(
            read(&conn, "r1").unwrap().unwrap().status,
            RunStatus::Failed
        );
        assert_eq!(
            read(&conn, "r2").unwrap().unwrap().status,
            RunStatus::Running
        );
    }

    #[test]
    fn most_recent_done_for_task_ignores_failed_and_interrupted_even_if_newer() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewRun {
                id: "done-older",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();
        update_status(&conn, "done-older", RunStatus::Done).unwrap();

        create(
            &conn,
            &NewRun {
                id: "failed-newer",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:05:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();
        update_status(&conn, "failed-newer", RunStatus::Failed).unwrap();

        let found = most_recent_done_for_task(&conn, "t").unwrap().unwrap();
        assert_eq!(
            found.id, "done-older",
            "a newer failed row must not shadow the last done one"
        );
    }

    #[test]
    fn most_recent_done_for_task_picks_the_latest_of_several_done_rows() {
        let conn = open_in_memory();
        for (id, started_at) in [
            ("d1", "2026-09-01T00:00:00Z"),
            ("d2", "2026-09-01T00:05:00Z"),
        ] {
            create(
                &conn,
                &NewRun {
                    id,
                    task_identity: "t",
                    task: "task",
                    started_at,
                    owner_pid: 100,
                },
            )
            .unwrap();
            update_status(&conn, id, RunStatus::Done).unwrap();
        }

        let found = most_recent_done_for_task(&conn, "t").unwrap().unwrap();
        assert_eq!(found.id, "d2");
    }

    #[test]
    fn most_recent_done_for_task_none_when_never_done() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewRun {
                id: "r1",
                task_identity: "t",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();
        assert_eq!(most_recent_done_for_task(&conn, "t").unwrap(), None);
    }

    #[test]
    fn most_recent_done_for_task_scoped_to_the_given_identity() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewRun {
                id: "other-task",
                task_identity: "other",
                task: "task",
                started_at: "2026-09-01T00:00:00Z",
                owner_pid: 100,
            },
        )
        .unwrap();
        update_status(&conn, "other-task", RunStatus::Done).unwrap();

        assert_eq!(most_recent_done_for_task(&conn, "t").unwrap(), None);
    }
}
