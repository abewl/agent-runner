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

use chrono::{DateTime, Utc};
use cron::Schedule;
use rusqlite::Connection;

use crate::retry::RunOutcome;
use crate::store::schedules::{self, Schedule as ScheduleRow};

/// Daemon tick poll interval — how often it *checks* whether anything is
/// due, not to be confused with `recheck_after` (a per-task hint for
/// *skipping* an otherwise-due tick — see the doc comment on
/// `process_one_schedule` and `DICT.md`'s "Cron tick interval" entry,
/// which explicitly warns against conflating the two).
pub const TICK_INTERVAL_SECS: u64 = 60;

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

/// Does `cron_expr` have a scheduled fire time in `(reference_time, now]`?
/// Deliberately **not** `Schedule::includes(now)` — that requires an exact
/// second match, and every expression Runner accepts is internally fixed
/// to `:00` seconds (see this file's top doc comment), so a tick firing
/// even a few seconds after the minute mark (normal timer behavior, not a
/// bug) would make `includes(now)` wrongly return `false`. Verified
/// empirically before writing this: `after(&reference_time).next() <= now`
/// correctly reports "due" regardless of exactly when within the tick
/// interval the check actually runs.
pub fn is_due(
    cron_expr: &str,
    reference_time: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<bool, String> {
    let schedule = parse(cron_expr)?;
    Ok(schedule
        .after(&reference_time)
        .next()
        .map(|next_fire| next_fire <= now)
        .unwrap_or(false))
}

fn parse_rfc3339(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Real entry point, called from the daemon's tick loop. Resolves the
/// configured repo and runs the ambient-auth preflight *once* for the
/// whole tick — both are system-level readiness checks, not per-schedule
/// ones, so a single failure means "not ready this tick," not "this one
/// schedule is broken." Returns the number of schedules actually
/// triggered (mainly for testability; production callers don't need it).
pub fn tick_once(conn: &Connection) -> usize {
    let Some(cwd) = crate::config::repo_path() else {
        tracing::warn!("cron tick: no repo configured, skipping this tick");
        return 0;
    };

    if let Err(e) = crate::preflight::check() {
        tracing::warn!("cron tick: preflight failed, skipping this tick: {e}");
        return 0;
    }

    tick_once_with(conn, Utc::now(), |task_identity, task, prompt, resume| {
        crate::persist::run_and_persist(conn, task_identity, task, prompt, resume, &cwd)
            .map_err(|e| e.to_string())
    })
}

/// The tick's per-schedule logic, parameterized on the trigger action
/// itself — testable with an injected closure instead of a real `claude`
/// call, the same pattern `retry::run_with_retry_using` and
/// `persist::run_and_persist_using` already established.
fn tick_once_with(
    conn: &Connection,
    now: DateTime<Utc>,
    mut trigger: impl FnMut(&str, &str, &str, Option<&str>) -> Result<RunOutcome, String>,
) -> usize {
    let schedule_rows = match schedules::list(conn) {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("cron tick: failed to list schedules: {e}");
            return 0;
        }
    };

    let mut triggered = 0;
    for schedule in schedule_rows.iter().filter(|s| s.enabled) {
        match process_one_schedule(conn, schedule, now, &mut trigger) {
            Ok(true) => triggered += 1,
            Ok(false) => {}
            Err(e) => tracing::warn!(schedule_id = %schedule.id, "cron tick: {e}"),
        }
    }
    triggered
}

/// Evaluates and, if not skipped, fires one schedule. Returns `Ok(true)`
/// if it triggered, `Ok(false)` if it was correctly skipped (not due,
/// already running, or `recheck_after` not yet elapsed — all expected,
/// routine outcomes, not errors).
fn process_one_schedule(
    conn: &Connection,
    schedule: &ScheduleRow,
    now: DateTime<Utc>,
    trigger: &mut impl FnMut(&str, &str, &str, Option<&str>) -> Result<RunOutcome, String>,
) -> Result<bool, String> {
    let reference_time = schedule
        .last_run_at
        .as_deref()
        .unwrap_or(&schedule.created_at);
    let reference_time = parse_rfc3339(reference_time)
        .ok_or_else(|| format!("unparseable reference timestamp: {reference_time}"))?;

    if !is_due(&schedule.cron_expr, reference_time, now)? {
        return Ok(false);
    }

    // AC-04: a schedule already running is skipped, independent of AC-02.
    let running = crate::store::runs::list_running(conn).map_err(|e| e.to_string())?;
    if running.iter().any(|r| r.task_identity == schedule.id) {
        tracing::info!(schedule_id = %schedule.id, "cron: skipping tick, already running");
        return Ok(false);
    }

    // AC-02/AC-03: a plain timestamp comparison against the most recent
    // done run's recheck_after — never a branch on next_action's value.
    // recheck_after is only ever set on `done` rows (F-08's mark_done);
    // failed/running/interrupted rows always have it null, which is why
    // this only needs to look at the most recent *done* run.
    if let Some(last_done) = crate::store::runs::most_recent_done_for_task(conn, &schedule.id)
        .map_err(|e| e.to_string())?
        && let Some(recheck_after_str) = &last_done.recheck_after
        && let Some(recheck_after) = parse_rfc3339(recheck_after_str)
        && recheck_after > now
    {
        tracing::info!(
            schedule_id = %schedule.id,
            recheck_after = %recheck_after_str,
            "cron: skipping tick, recheck_after not yet elapsed"
        );
        return Ok(false);
    }

    // AC-05/AC-06: trigger through the same pipeline `runner run` uses —
    // continuation lookup, then the injected trigger action.
    let continuation = crate::lookup::lookup(conn, &schedule.id).map_err(|e| e.to_string())?;
    let prompt = crate::signal::build_prompt(&schedule.task, continuation.context_line.as_deref());

    let trigger_result = trigger(
        &schedule.id,
        &schedule.task,
        &prompt,
        continuation.session_id.as_deref(),
    );
    if let Err(e) = &trigger_result {
        tracing::warn!(schedule_id = %schedule.id, "cron: triggered run failed: {e}");
    }

    // AC-05: last_run_at updates regardless of the run's outcome.
    schedules::update_last_run_at(conn, &schedule.id, &now.to_rfc3339())
        .map_err(|e| e.to_string())?;

    Ok(true)
}

/// Runs forever, ticking every `TICK_INTERVAL_SECS` — the daemon's only
/// responsibility in Stage 1 beyond staying alive. Opens its own store
/// connection fresh each tick (an embedded SQLite connection is cheap to
/// open, and this avoids holding one open — and needing it to be `Sync`
/// — across the whole daemon's lifetime for a once-a-minute need).
pub async fn run_tick_loop() {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(TICK_INTERVAL_SECS)).await;
        match crate::store::open() {
            Ok(conn) => {
                tick_once(&conn);
            }
            Err(e) => tracing::error!("cron tick: failed to open store: {e}"),
        }
    }
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

    // ---- is_due: pure, deterministic ----

    #[test]
    fn is_due_true_when_reference_is_well_in_the_past() {
        let now = chrono::Utc::now();
        let reference = now - chrono::Duration::hours(1);
        assert!(is_due("* * * * *", reference, now).unwrap());
    }

    #[test]
    fn is_due_false_when_reference_is_seconds_ago_for_an_hourly_schedule() {
        let now = chrono::Utc::now();
        let reference = now - chrono::Duration::seconds(5);
        // An hourly schedule's next fire after 5 seconds ago is almost
        // certainly more than 5 seconds away.
        assert!(!is_due("0 * * * *", reference, now).unwrap());
    }

    #[test]
    fn is_due_propagates_a_parse_error_for_an_invalid_expression() {
        assert!(is_due("garbage", chrono::Utc::now(), chrono::Utc::now()).is_err());
    }

    // ---- tick_once_with: in-memory DB + injected trigger, no subprocess ----

    mod tick_tests {
        use super::*;
        use crate::retry::RunOutcome;
        use crate::signal::ContinuationSignal;
        use crate::store::runs::{self, NewRun};
        use crate::store::schedules::NewSchedule;
        use std::cell::Cell;

        fn open_in_memory() -> Connection {
            let conn = Connection::open_in_memory().unwrap();
            crate::store::migrate(&conn).unwrap();
            conn
        }

        /// An "every minute" schedule created far enough in the past that
        /// it's always due, regardless of when `last_run_at`/`created_at`
        /// end up falling relative to "now" during a test run.
        fn add_always_due_schedule(conn: &Connection, id: &str) {
            schedules::create(
                conn,
                &NewSchedule {
                    id,
                    cron_expr: "* * * * *",
                    task: "check tickets",
                    created_at: "2020-01-01T00:00:00Z",
                },
            )
            .unwrap();
        }

        fn ok_outcome() -> RunOutcome {
            RunOutcome {
                result: crate::process::ClaudeResult {
                    result: "done".to_string(),
                    session_id: Some("sess".to_string()),
                    cost_usd: 0.0,
                },
                signal: ContinuationSignal {
                    next_action: "idle".to_string(),
                    reason: String::new(),
                    recheck_after: None,
                },
                retried: false,
            }
        }

        #[test]
        fn disabled_schedule_is_never_considered() {
            let conn = open_in_memory();
            add_always_due_schedule(&conn, "s1");
            // Disable it directly — there's no CLI toggle in this batch,
            // only a raw update, which is fine for a test setup.
            conn.execute("UPDATE schedules SET enabled = 0 WHERE id = 's1'", [])
                .unwrap();

            let calls = Cell::new(0);
            let triggered = tick_once_with(&conn, Utc::now(), |_, _, _, _| {
                calls.set(calls.get() + 1);
                Ok(ok_outcome())
            });

            assert_eq!(triggered, 0);
            assert_eq!(calls.get(), 0);
        }

        #[test]
        fn due_schedule_with_no_conflicts_triggers_and_updates_last_run_at() {
            let conn = open_in_memory();
            add_always_due_schedule(&conn, "s1");

            let calls = Cell::new(0);
            let triggered = tick_once_with(&conn, Utc::now(), |task_identity, task, _, _| {
                calls.set(calls.get() + 1);
                assert_eq!(task_identity, "s1");
                assert_eq!(task, "check tickets");
                Ok(ok_outcome())
            });

            assert_eq!(triggered, 1);
            assert_eq!(calls.get(), 1);
            let schedule = schedules::read(&conn, "s1").unwrap().unwrap();
            assert!(schedule.last_run_at.is_some());
        }

        #[test]
        fn already_running_schedule_is_skipped_ac04() {
            let conn = open_in_memory();
            add_always_due_schedule(&conn, "s1");
            // A `running` row already exists for this schedule's task
            // identity — simulating a still-in-flight prior trigger.
            runs::create(
                &conn,
                &NewRun {
                    id: "r1",
                    task_identity: "s1",
                    task: "check tickets",
                    started_at: "2026-09-01T00:00:00Z",
                    owner_pid: std::process::id() as i64,
                },
            )
            .unwrap();

            let calls = Cell::new(0);
            let triggered = tick_once_with(&conn, Utc::now(), |_, _, _, _| {
                calls.set(calls.get() + 1);
                Ok(ok_outcome())
            });

            assert_eq!(triggered, 0);
            assert_eq!(calls.get(), 0, "must not fire a second concurrent run");
        }

        #[test]
        fn future_recheck_after_skips_the_tick_ac02() {
            let conn = open_in_memory();
            add_always_due_schedule(&conn, "s1");

            let now = Utc::now();
            runs::create(
                &conn,
                &NewRun {
                    id: "r1",
                    task_identity: "s1",
                    task: "check tickets",
                    started_at: "2026-09-01T00:00:00Z",
                    owner_pid: 1,
                },
            )
            .unwrap();
            let future_recheck = (now + chrono::Duration::hours(1)).to_rfc3339();
            runs::mark_done(
                &conn,
                "r1",
                Some("sess"),
                0.0,
                "2026-09-01T00:01:00Z",
                0,
                "idle",
                "nothing to do yet",
                Some(&future_recheck),
                "result text",
            )
            .unwrap();

            let calls = Cell::new(0);
            let triggered = tick_once_with(&conn, now, |_, _, _, _| {
                calls.set(calls.get() + 1);
                Ok(ok_outcome())
            });

            assert_eq!(triggered, 0);
            assert_eq!(calls.get(), 0);
        }

        #[test]
        fn elapsed_recheck_after_allows_the_tick_to_fire() {
            let conn = open_in_memory();
            add_always_due_schedule(&conn, "s1");

            let now = Utc::now();
            runs::create(
                &conn,
                &NewRun {
                    id: "r1",
                    task_identity: "s1",
                    task: "check tickets",
                    started_at: "2026-09-01T00:00:00Z",
                    owner_pid: 1,
                },
            )
            .unwrap();
            let past_recheck = (now - chrono::Duration::hours(1)).to_rfc3339();
            runs::mark_done(
                &conn,
                "r1",
                Some("sess"),
                0.0,
                "2026-09-01T00:01:00Z",
                0,
                "idle",
                "was waiting, now elapsed",
                Some(&past_recheck),
                "result text",
            )
            .unwrap();

            let calls = Cell::new(0);
            let triggered = tick_once_with(&conn, now, |_, _, _, _| {
                calls.set(calls.get() + 1);
                Ok(ok_outcome())
            });

            assert_eq!(triggered, 1);
            assert_eq!(calls.get(), 1);
        }

        #[test]
        fn last_run_at_updates_even_when_the_triggered_run_fails() {
            let conn = open_in_memory();
            add_always_due_schedule(&conn, "s1");

            let triggered = tick_once_with(&conn, Utc::now(), |_, _, _, _| {
                Err("simulated failure".to_string())
            });

            assert_eq!(triggered, 1, "a failed trigger still counts as fired");
            let schedule = schedules::read(&conn, "s1").unwrap().unwrap();
            assert!(schedule.last_run_at.is_some());
        }

        #[test]
        fn multiple_schedules_are_each_evaluated_independently() {
            let conn = open_in_memory();
            add_always_due_schedule(&conn, "s1");
            add_always_due_schedule(&conn, "s2");

            let calls = Cell::new(0);
            let triggered = tick_once_with(&conn, Utc::now(), |_, _, _, _| {
                calls.set(calls.get() + 1);
                Ok(ok_outcome())
            });

            assert_eq!(triggered, 2);
            assert_eq!(calls.get(), 2);
        }
    }
}
