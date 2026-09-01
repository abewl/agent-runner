//! `runner logs <run-id>` command implementation (F-12).

use crate::store::runs::{self, RunStatus};

pub fn logs(run_id: &str) -> Result<(), String> {
    let conn = crate::store::open().map_err(|e| e.to_string())?;

    let run = runs::read(&conn, run_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no such run: {run_id}"))?;

    match run.status {
        RunStatus::Done => {
            println!("{}", run.result_text.as_deref().unwrap_or(""));
        }
        RunStatus::Failed => {
            println!(
                "{}",
                run.exit_reason
                    .as_deref()
                    .unwrap_or("(no failure detail recorded)")
            );
        }
        RunStatus::Running | RunStatus::Interrupted => {
            println!("status: {} — no result yet", run.status);
        }
    }

    println!(
        "next_action: {}",
        run.next_action.as_deref().unwrap_or("none")
    );
    println!(
        "next_action_reason: {}",
        run.next_action_reason.as_deref().unwrap_or("none")
    );
    println!(
        "recheck_after: {}",
        run.recheck_after.as_deref().unwrap_or("none")
    );

    Ok(())
}
