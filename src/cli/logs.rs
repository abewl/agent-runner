//! `runner logs <run-id>` command implementation (F-12).

use crate::cli::display::format_run_detail;
use crate::store::runs;

pub fn logs(run_id: &str) -> Result<(), String> {
    let conn = crate::store::open().map_err(|e| e.to_string())?;

    let run = runs::read(&conn, run_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no such run: {run_id}"))?;

    println!("{}", format_run_detail(&run));

    Ok(())
}
