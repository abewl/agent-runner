//! `runner status` (alias `runner ps`) command implementation (F-11).

use chrono::{DateTime, Utc};

use crate::cli::display::truncate;
use crate::store::runs;

const DEFAULT_LIST_LIMIT: i64 = 50;
const TASK_TRUNCATE_LEN: usize = 40;

pub fn status(running_only: bool) -> Result<(), String> {
    let conn = crate::store::open().map_err(|e| e.to_string())?;

    let rows = if running_only {
        runs::list_running(&conn).map_err(|e| e.to_string())?
    } else {
        runs::list(&conn, DEFAULT_LIST_LIMIT).map_err(|e| e.to_string())?
    };

    if rows.is_empty() {
        println!("no runs yet");
        return Ok(());
    }

    for row in rows {
        let task_display = truncate(&row.task, TASK_TRUNCATE_LEN);
        let duration = format_duration_since(&row.started_at, row.ended_at.as_deref());
        let next_action = row.next_action.as_deref().unwrap_or("");

        println!(
            "{}  {:<11}  {:<width$}  {:<20}  {:<8}  {}",
            row.id,
            row.status,
            task_display,
            row.started_at,
            duration,
            next_action,
            width = TASK_TRUNCATE_LEN
        );
    }

    Ok(())
}

/// `started_at`/`ended_at` are RFC3339 strings (SPEC.md's "TEXT ISO-8601"
/// columns). Duration is `ended_at - started_at` for a terminal run, or
/// `now - started_at` (elapsed so far) for one still `running`. Falls back
/// to `"?"` rather than panicking if either timestamp somehow fails to
/// parse — display-layer defensiveness, not a claim it should ever happen.
fn format_duration_since(started_at: &str, ended_at: Option<&str>) -> String {
    let Ok(start) = DateTime::parse_from_rfc3339(started_at).map(|t| t.with_timezone(&Utc)) else {
        return "?".to_string();
    };
    let end = match ended_at {
        Some(e) => match DateTime::parse_from_rfc3339(e) {
            Ok(t) => t.with_timezone(&Utc),
            Err(_) => return "?".to_string(),
        },
        None => Utc::now(),
    };
    let seconds = (end - start).num_seconds().max(0);
    format_seconds(seconds)
}

fn format_seconds(total: i64) -> String {
    if total < 60 {
        format!("{total}s")
    } else if total < 3600 {
        format!("{}m{}s", total / 60, total % 60)
    } else {
        format!("{}h{}m", total / 3600, (total % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_seconds_under_a_minute() {
        assert_eq!(format_seconds(45), "45s");
    }

    #[test]
    fn format_seconds_minutes_and_seconds() {
        assert_eq!(format_seconds(125), "2m5s");
    }

    #[test]
    fn format_seconds_hours_and_minutes() {
        assert_eq!(format_seconds(3725), "1h2m");
    }

    #[test]
    fn format_duration_since_with_explicit_end() {
        let d = format_duration_since("2026-09-01T00:00:00Z", Some("2026-09-01T00:01:30Z"));
        assert_eq!(d, "1m30s");
    }

    #[test]
    fn format_duration_since_falls_back_on_unparseable_timestamp() {
        assert_eq!(format_duration_since("not-a-timestamp", None), "?");
    }

    #[test]
    fn format_duration_since_still_running_uses_now() {
        // Can't assert an exact value against "now," but it must not
        // panic and must not fall back to "?" for a valid start time.
        let d = format_duration_since("2026-09-01T00:00:00Z", None);
        assert_ne!(d, "?");
    }
}
