//! Embedded SQLite store (F-07) — schema init/migration + shared
//! connection handling. This module tree is Runner's *only* code path
//! that touches the database (SPEC.md AC-04) — no raw SQL string-building
//! anywhere outside `store/runs.rs`/`store/schedules.rs`.

use rusqlite::Connection;

pub mod runs;
// Genuinely unused so far — F-13 (Schedule Store & CLI) is its first
// consumer, not built yet. The allow comes off then.
#[allow(dead_code)]
pub mod schedules;

/// Bumped whenever the schema changes. `CREATE TABLE IF NOT EXISTS` is what
/// actually makes `migrate` idempotent; `user_version` is set defensively
/// so a future migration has a version to branch on, not because anything
/// reads it yet. v2 (F-08) added `runs.owner_pid` directly to the `CREATE
/// TABLE` statement below rather than an `ALTER TABLE` migration path —
/// safe and correct only because Stage 1 has never been released, so no
/// real `runner.db` exists anywhere with the v1 shape to migrate from.
const SCHEMA_VERSION: i64 = 2;

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "store I/O error: {e}"),
            StoreError::Sqlite(e) => write!(f, "store error: {e}"),
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e)
    }
}

/// Opens (creating `$RUNNER_HOME` and the DB file if necessary) the store
/// at `$RUNNER_HOME/runner.db`, applying schema idempotently on every call
/// (SPEC.md AC-01).
pub fn open() -> Result<Connection, StoreError> {
    crate::paths::ensure_runner_home()?;
    let conn = Connection::open(crate::paths::db_file())?;
    migrate(&conn)?;
    Ok(conn)
}

/// `pub(crate)` (not just private-to-`store`) specifically so sibling
/// modules' tests (e.g. `persist.rs`) can set up a real, correctly-shaped
/// in-memory DB via `Connection::open_in_memory()` + this, the same way
/// `store::runs`/`store::schedules`'s own tests already do as child
/// modules — one migration function, never a second hand-copied schema.
pub(crate) fn migrate(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS runs (
            id TEXT PRIMARY KEY,
            task_identity TEXT NOT NULL,
            task TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('running','done','failed','interrupted')),
            session_id TEXT,
            cost_usd REAL,
            started_at TEXT NOT NULL,
            ended_at TEXT,
            exit_reason TEXT,
            retry_count INTEGER NOT NULL DEFAULT 0,
            next_action TEXT,
            next_action_reason TEXT,
            recheck_after TEXT,
            owner_pid INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS schedules (
            id TEXT PRIMARY KEY,
            cron_expr TEXT NOT NULL,
            task TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            last_run_at TEXT
        );
        ",
    )?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn migrate_creates_both_tables() {
        let conn = open_in_memory();
        let runs_exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='runs'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        let schedules_exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='schedules'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(runs_exists);
        assert!(schedules_exists);
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = open_in_memory();
        // Running it again must not error (SPEC.md AC-01).
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
    }

    #[test]
    fn migrate_sets_schema_version() {
        let conn = open_in_memory();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn runs_status_check_constraint_rejects_unknown_status() {
        let conn = open_in_memory();
        let result = conn.execute(
            "INSERT INTO runs (id, task_identity, task, status, started_at) VALUES ('r1', 't1', 'task', 'not-a-real-status', 'now')",
            [],
        );
        assert!(result.is_err());
    }
}
