//! Typed CRUD for the `schedules` table (F-07). Only this file builds SQL
//! against `schedules` — always parameterized (SPEC.md AC-04).

use rusqlite::{Connection, OptionalExtension, params};

use super::StoreError;

#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    pub id: String,
    pub cron_expr: String,
    pub task: String,
    pub enabled: bool,
    pub created_at: String,
    pub last_run_at: Option<String>,
}

pub struct NewSchedule<'a> {
    pub id: &'a str,
    pub cron_expr: &'a str,
    pub task: &'a str,
    pub created_at: &'a str,
}

const SELECT_COLUMNS: &str = "id, cron_expr, task, enabled, created_at, last_run_at";

fn row_to_schedule(row: &rusqlite::Row<'_>) -> rusqlite::Result<Schedule> {
    let enabled_int: i64 = row.get(3)?;
    Ok(Schedule {
        id: row.get(0)?,
        cron_expr: row.get(1)?,
        task: row.get(2)?,
        enabled: enabled_int != 0,
        created_at: row.get(4)?,
        last_run_at: row.get(5)?,
    })
}

/// Inserts a new schedule, `enabled` defaulting to true.
pub fn create(conn: &Connection, new_schedule: &NewSchedule) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO schedules (id, cron_expr, task, enabled, created_at) VALUES (?1, ?2, ?3, 1, ?4)",
        params![
            new_schedule.id,
            new_schedule.cron_expr,
            new_schedule.task,
            new_schedule.created_at
        ],
    )?;
    Ok(())
}

// `read` isn't called from production code yet — no current caller needs
// a single schedule by id (F-13's CLI only needs `list`/`create`/`delete`;
// F-14's tick engine iterates all of them via `list`).
#[allow(dead_code)]
pub fn read(conn: &Connection, id: &str) -> Result<Option<Schedule>, StoreError> {
    conn.query_row(
        &format!("SELECT {SELECT_COLUMNS} FROM schedules WHERE id = ?1"),
        params![id],
        row_to_schedule,
    )
    .optional()
    .map_err(StoreError::from)
}

pub fn list(conn: &Connection) -> Result<Vec<Schedule>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM schedules ORDER BY created_at ASC"
    ))?;
    let rows = stmt.query_map([], row_to_schedule)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

pub fn update_last_run_at(
    conn: &Connection,
    id: &str,
    last_run_at: &str,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE schedules SET last_run_at = ?2 WHERE id = ?1",
        params![id, last_run_at],
    )?;
    Ok(())
}

/// Deletes by id. Returns whether a row was actually deleted, so callers
/// (F-13's `runner cron remove`) can tell "removed" from "no such id"
/// rather than silently succeeding either way.
pub fn delete(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    let affected = conn.execute("DELETE FROM schedules WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn create_then_read_round_trips_with_enabled_defaulting_true() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewSchedule {
                id: "s1",
                cron_expr: "*/15 * * * *",
                task: "check tickets",
                created_at: "2026-09-01T00:00:00Z",
            },
        )
        .unwrap();

        let schedule = read(&conn, "s1").unwrap().unwrap();
        assert_eq!(schedule.cron_expr, "*/15 * * * *");
        assert!(schedule.enabled);
        assert_eq!(schedule.last_run_at, None);
    }

    #[test]
    fn read_returns_none_for_unknown_id() {
        let conn = open_in_memory();
        assert_eq!(read(&conn, "nope").unwrap(), None);
    }

    #[test]
    fn list_returns_all_in_creation_order() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewSchedule {
                id: "s1",
                cron_expr: "* * * * *",
                task: "a",
                created_at: "2026-09-01T00:00:00Z",
            },
        )
        .unwrap();
        create(
            &conn,
            &NewSchedule {
                id: "s2",
                cron_expr: "* * * * *",
                task: "b",
                created_at: "2026-09-01T00:01:00Z",
            },
        )
        .unwrap();

        let schedules = list(&conn).unwrap();
        assert_eq!(schedules.len(), 2);
        assert_eq!(schedules[0].id, "s1");
        assert_eq!(schedules[1].id, "s2");
    }

    #[test]
    fn update_last_run_at_sets_the_field() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewSchedule {
                id: "s1",
                cron_expr: "* * * * *",
                task: "a",
                created_at: "2026-09-01T00:00:00Z",
            },
        )
        .unwrap();

        update_last_run_at(&conn, "s1", "2026-09-01T00:05:00Z").unwrap();

        let schedule = read(&conn, "s1").unwrap().unwrap();
        assert_eq!(
            schedule.last_run_at,
            Some("2026-09-01T00:05:00Z".to_string())
        );
    }

    #[test]
    fn delete_removes_and_reports_true() {
        let conn = open_in_memory();
        create(
            &conn,
            &NewSchedule {
                id: "s1",
                cron_expr: "* * * * *",
                task: "a",
                created_at: "2026-09-01T00:00:00Z",
            },
        )
        .unwrap();

        assert!(delete(&conn, "s1").unwrap());
        assert_eq!(read(&conn, "s1").unwrap(), None);
    }

    #[test]
    fn delete_nonexistent_id_reports_false_not_an_error() {
        let conn = open_in_memory();
        assert!(!delete(&conn, "never-existed").unwrap());
    }
}
