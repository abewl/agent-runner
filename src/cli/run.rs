//! `runner run <task>` command implementation — the command every other
//! manual-control command and the scheduler ultimately call into.
//! Standalone: does not require the daemon to be running.

use crate::claude::{preflight, signal};
use crate::{config, persist};

pub fn run(task: &str) -> Result<(), String> {
    // Cheaper, more obviously-fixable setup error first — no subprocess
    // spawn needed to discover "no repo configured," unlike preflight.
    let cwd = config::repo_path()
        .ok_or_else(|| "no repo configured — run `runner repo <path>` first".to_string())?;

    preflight::check().map_err(|e| e.to_string())?;

    let conn = crate::store::open().map_err(|e| e.to_string())?;

    // `runner run` is the CLI's own startup point, so it reconciles any
    // interrupted-looking row before accepting a new one.
    persist::reconcile_interrupted_runs(&conn).map_err(|e| e.to_string())?;

    // Task identity for a manual run is the literal, verbatim task string —
    // no transformation, no hashing, no namespacing prefix.
    let task_identity = task;

    let continuation = persist::lookup(&conn, task_identity).map_err(|e| e.to_string())?;
    let prompt = signal::build_prompt(task, continuation.context_line.as_deref());

    let outcome = persist::run_and_persist(
        &conn,
        task_identity,
        task,
        &prompt,
        continuation.session_id.as_deref(),
        &cwd,
    )
    .map_err(|e| e.to_string())?;

    // `outcome.result.result` still contains the NEXT_ACTION/RECHECK_AFTER
    // trailer verbatim (parsing it out doesn't strip it from the stored
    // text). Printing both the raw text and the separately-parsed signal
    // would show the trailer twice, so strip it from the displayed body
    // and print the parsed signal once, as a status footer — the same
    // stripping function the persistence layer uses before storing
    // `result_text`, so this command's stdout and `runner logs`'s later
    // retrieval show identical, clean text.
    println!("{}", signal::strip_trailer(&outcome.result.result));
    println!(
        "NEXT_ACTION: {} — {}",
        outcome.signal.next_action, outcome.signal.reason
    );
    if let Some(recheck_after) = outcome.signal.recheck_after {
        println!("RECHECK_AFTER: {}s", recheck_after.as_secs());
    }

    Ok(())
}
