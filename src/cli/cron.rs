//! `runner cron add|list|remove` command implementations (F-13).

use chrono::Utc;
use uuid::Uuid;

use crate::cli::display::truncate;
use crate::cron_engine;
use crate::store::schedules::{self, NewSchedule};

const TASK_TRUNCATE_LEN: usize = 40;

pub fn add(cron_expr: &str, task: &str) -> Result<(), String> {
    // Validated before anything is written — an invalid expression must
    // leave no row inserted (SPEC.md AC-01).
    cron_engine::validate(cron_expr)?;

    let conn = crate::store::open().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();

    schedules::create(
        &conn,
        &NewSchedule {
            id: &id,
            cron_expr,
            task,
            created_at: &created_at,
        },
    )
    .map_err(|e| e.to_string())?;

    println!("schedule added: {id}");
    Ok(())
}

pub fn list() -> Result<(), String> {
    let conn = crate::store::open().map_err(|e| e.to_string())?;
    let rows = schedules::list(&conn).map_err(|e| e.to_string())?;

    if rows.is_empty() {
        println!("no schedules");
        return Ok(());
    }

    for row in rows {
        let enabled = if row.enabled { "enabled" } else { "disabled" };
        let last_run = row.last_run_at.as_deref().unwrap_or("never");
        let task_display = truncate(&row.task, TASK_TRUNCATE_LEN);
        println!(
            "{}  {:<20}  {:<width$}  {:<8}  {}",
            row.id,
            row.cron_expr,
            task_display,
            enabled,
            last_run,
            width = TASK_TRUNCATE_LEN
        );
    }

    Ok(())
}

pub fn remove(schedule_id: &str) -> Result<(), String> {
    let conn = crate::store::open().map_err(|e| e.to_string())?;
    let deleted = schedules::delete(&conn, schedule_id).map_err(|e| e.to_string())?;

    if !deleted {
        return Err(format!("no such schedule: {schedule_id}"));
    }

    println!("schedule removed: {schedule_id}");
    Ok(())
}
