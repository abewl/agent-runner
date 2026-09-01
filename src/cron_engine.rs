//! Cron expression parsing/validation (F-13; the tick-evaluation half is
//! F-14). Runner accepts standard 5-field Unix cron syntax (minute hour
//! day month day-of-week).
//!
//! **The `cron` crate itself requires a leading seconds field** (6 or 7
//! fields total — verified empirically against `cron` 0.17.0, whose own
//! README example uses 7: `sec min hour day month dow year`). A bare
//! 5-field expression like `*/15 * * * *` fails to parse against it
//! directly. Sub-minute precision isn't something this system has anyway
//! — the daemon's own tick interval is fixed at 60s (see `DICT.md`) — so
//! every expression Runner accepts is rewritten to 6-field by prepending a
//! fixed `"0 "` seconds field before handing it to the `cron` crate. That
//! rewrite is entirely internal; callers only ever see or type the
//! standard 5-field form.

use std::str::FromStr;

use cron::Schedule;

/// Validates a standard 5-field cron expression (SPEC.md F-13 AC-01).
pub fn validate(expr: &str) -> Result<(), String> {
    parse(expr).map(|_| ())
}

/// The same parsing `validate` performs is what F-14's tick engine will
/// use to evaluate schedules — one parser, so an expression `runner cron
/// add` accepts is guaranteed evaluable later, never a second parser that
/// could disagree with the first.
pub(crate) fn parse(expr: &str) -> Result<Schedule, String> {
    let six_field = format!("0 {expr}");
    Schedule::from_str(&six_field).map_err(|e| format!("invalid cron expression \"{expr}\": {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_standard_five_field_expressions() {
        assert!(validate("*/15 * * * *").is_ok());
        assert!(validate("* * * * *").is_ok());
        assert!(validate("0 12 * * *").is_ok());
        assert!(validate("0 9 * * MON-FRI").is_ok());
    }

    #[test]
    fn validate_rejects_garbage() {
        assert!(validate("not a cron expression").is_err());
    }

    #[test]
    fn validate_rejects_wrong_field_count() {
        assert!(validate("* * *").is_err());
    }

    #[test]
    fn validate_error_names_the_offending_expression() {
        let err = validate("not a cron expression").unwrap_err();
        assert!(err.contains("not a cron expression"));
    }

    #[test]
    fn parse_produces_a_schedule_that_computes_upcoming_fire_times() {
        let schedule = parse("0 12 * * *").unwrap();
        // Just confirm it's genuinely usable, not just "didn't error" —
        // computing an upcoming time is what F-14 will actually need.
        assert!(schedule.upcoming(chrono::Utc).next().is_some());
    }
}
