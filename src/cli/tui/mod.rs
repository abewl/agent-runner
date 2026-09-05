//! `runner log` — a read-only `ratatui` dashboard over the same store
//! `runner status`/`runner logs` read. Terminal setup/teardown lives
//! here; all state and key-handling logic lives in `app` (kept pure and
//! terminal-free so it's unit-testable without a real terminal — see
//! `app`'s own doc comment).

pub mod app;

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Text;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use rusqlite::Connection;

use self::app::{Action, App, handle_key};
use crate::cli::display::{format_run_detail, truncate};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const TASK_TRUNCATE_LEN: usize = 30;

pub fn run() -> Result<(), String> {
    let conn = crate::store::open().map_err(|e| e.to_string())?;

    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = io::stdout();
    let entered = execute!(stdout, EnterAlternateScreen);

    let result = match entered {
        Ok(()) => run_app(&mut stdout, &conn),
        Err(e) => Err(e.to_string()),
    };

    // Guaranteed teardown — fully restores the terminal regardless of how
    // `run_app` above returned. Deliberately best-effort (`let _`) so a
    // teardown failure never masks whatever error `run_app` itself
    // produced, and both steps still run even if entering the alternate
    // screen above never succeeded.
    let _ = disable_raw_mode();
    let _ = execute!(stdout, LeaveAlternateScreen);

    result
}

fn run_app(stdout: &mut Stdout, conn: &Connection) -> Result<(), String> {
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let mut app = App::new();
    app.refresh(conn)?;
    let mut last_refresh = Instant::now();

    loop {
        terminal
            .draw(|frame| draw(frame, &app))
            .map_err(|e| e.to_string())?;

        // Refresh on a fixed poll interval without blocking key input —
        // `event::poll` doubles as both the input wait and the refresh
        // timer, rather than a separate sleep plus a non-blocking read
        // (which would either miss keys or busy-loop).
        let timeout = REFRESH_INTERVAL.saturating_sub(last_refresh.elapsed());

        if event::poll(timeout).map_err(|e| e.to_string())?
            && let Event::Key(key) = event::read().map_err(|e| e.to_string())?
        {
            match handle_key(key) {
                Action::Quit => break,
                action => app.apply(action),
            }
        }

        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            app.refresh(conn)?;
            last_refresh = Instant::now();
        }
    }

    Ok(())
}

fn draw(frame: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(frame.area());

    let items: Vec<ListItem> = if app.runs.is_empty() {
        vec![ListItem::new("no runs yet")]
    } else {
        app.runs
            .iter()
            .map(|run| {
                let line = format!(
                    "{}  {:<11}  {:<width$}  {}  {}",
                    run.id,
                    run.status,
                    truncate(&run.task, TASK_TRUNCATE_LEN),
                    run.started_at,
                    run.next_action.as_deref().unwrap_or(""),
                    width = TASK_TRUNCATE_LEN
                );
                ListItem::new(line)
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Runs (↑/↓ select, q or Ctrl+C to quit)"),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut list_state = ListState::default();
    if !app.runs.is_empty() {
        list_state.select(Some(app.selected));
    }

    frame.render_stateful_widget(list, chunks[0], &mut list_state);

    let detail_text = match app.selected_run() {
        Some(run) => format_run_detail(run),
        None => "no runs yet".to_string(),
    };
    let detail = Paragraph::new(Text::from(detail_text))
        .block(Block::default().borders(Borders::ALL).title("Detail"));

    frame.render_widget(detail, chunks[1]);
}
