//! Pure, terminal-free state for F-15's TUI. `handle_key` and `App::apply`
//! are plain data transformations with no I/O, so they're directly
//! unit-testable without a real terminal — the same DI-testing pattern
//! this project already uses in `retry.rs`/`persist.rs`/`cron_engine.rs`,
//! just via "keep it pure" rather than an injected closure this time.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rusqlite::Connection;

use crate::store::runs::{self, Run};

const LIST_LIMIT: i64 = 50;

/// What a key press means, decoupled from how it's drawn or read — keeps
/// `handle_key` a pure `KeyEvent -> Action` function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Up,
    Down,
    None,
}

pub struct App {
    pub runs: Vec<Run>,
    pub selected: usize,
}

impl App {
    pub fn new() -> Self {
        App {
            runs: Vec::new(),
            selected: 0,
        }
    }

    /// The only store call this whole module makes, and it's read-only —
    /// SPEC.md AC-04 requires this as a structural property (no write-path
    /// store function reachable from the TUI at all), not just a behavioral
    /// one, so there's deliberately no other function in `cli::tui` that
    /// touches `conn`.
    pub fn refresh(&mut self, conn: &Connection) -> Result<(), String> {
        self.runs = runs::list(conn, LIST_LIMIT).map_err(|e| e.to_string())?;
        if self.selected >= self.runs.len() {
            self.selected = self.runs.len().saturating_sub(1);
        }
        Ok(())
    }

    pub fn selected_run(&self) -> Option<&Run> {
        self.runs.get(self.selected)
    }

    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            Action::Down => {
                if self.selected + 1 < self.runs.len() {
                    self.selected += 1;
                }
            }
            Action::Quit | Action::None => {}
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// `q` or Ctrl+C exits (SPEC.md AC-05); up/down move the selection, which
/// drives the always-visible detail pane (AC-03's "or equivalent" — the
/// detail pane tracks the current selection live rather than requiring a
/// separate Enter-to-open step).
pub fn handle_key(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
        KeyCode::Up => Action::Up,
        KeyCode::Down => Action::Down,
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::runs::NewRun;

    fn open_in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::store::migrate(&conn).unwrap();
        conn
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn handle_key_q_quits() {
        assert_eq!(
            handle_key(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Action::Quit
        );
    }

    #[test]
    fn handle_key_ctrl_c_quits() {
        assert_eq!(
            handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Action::Quit
        );
    }

    #[test]
    fn handle_key_plain_c_does_not_quit() {
        assert_eq!(
            handle_key(key(KeyCode::Char('c'), KeyModifiers::NONE)),
            Action::None
        );
    }

    #[test]
    fn handle_key_arrows_map_to_up_and_down() {
        assert_eq!(handle_key(key(KeyCode::Up, KeyModifiers::NONE)), Action::Up);
        assert_eq!(
            handle_key(key(KeyCode::Down, KeyModifiers::NONE)),
            Action::Down
        );
    }

    #[test]
    fn handle_key_unmapped_key_is_none() {
        assert_eq!(
            handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE)),
            Action::None
        );
    }

    #[test]
    fn apply_up_does_not_go_below_zero() {
        let mut app = App::new();
        app.runs = vec![];
        app.selected = 0;
        app.apply(Action::Up);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn apply_down_does_not_pass_the_last_row() {
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

        let mut app = App::new();
        app.refresh(&conn).unwrap();
        assert_eq!(app.runs.len(), 1);

        app.apply(Action::Down);
        assert_eq!(app.selected, 0, "only one row — selection must stay put");
    }

    #[test]
    fn apply_up_and_down_move_selection_within_bounds() {
        let conn = open_in_memory();
        for (id, started_at) in [
            ("r1", "2026-09-01T00:00:00Z"),
            ("r2", "2026-09-01T00:01:00Z"),
        ] {
            runs::create(
                &conn,
                &NewRun {
                    id,
                    task_identity: "t",
                    task: "task",
                    started_at,
                    owner_pid: 1,
                },
            )
            .unwrap();
        }

        let mut app = App::new();
        app.refresh(&conn).unwrap();
        assert_eq!(app.selected, 0);

        app.apply(Action::Down);
        assert_eq!(app.selected, 1);

        app.apply(Action::Down);
        assert_eq!(app.selected, 1, "must not go past the last row");

        app.apply(Action::Up);
        assert_eq!(app.selected, 0);

        app.apply(Action::Up);
        assert_eq!(app.selected, 0, "must not go below zero");
    }

    #[test]
    fn refresh_clamps_selection_when_the_row_count_shrinks() {
        let conn = open_in_memory();
        for (id, started_at) in [
            ("r1", "2026-09-01T00:00:00Z"),
            ("r2", "2026-09-01T00:01:00Z"),
        ] {
            runs::create(
                &conn,
                &NewRun {
                    id,
                    task_identity: "t",
                    task: "task",
                    started_at,
                    owner_pid: 1,
                },
            )
            .unwrap();
        }

        let mut app = App::new();
        app.refresh(&conn).unwrap();
        app.selected = 1;

        // Simulate the row this selection pointed at disappearing between
        // polls (e.g. the store having been reset) — the real product
        // never deletes `runs` rows, but `refresh` must stay safe even if
        // the row count shrinks for any reason, rather than leaving
        // `selected` pointing past the end of the (new) `runs` vec.
        conn.execute("DELETE FROM runs WHERE id = 'r2'", [])
            .unwrap();
        app.refresh(&conn).unwrap();

        assert_eq!(app.runs.len(), 1);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn selected_run_none_when_empty() {
        let app = App::new();
        assert!(app.selected_run().is_none());
    }
}
