//! `runner run <task>` command implementation (F-10) — the command every
//! other manual-control feature and the scheduler (F-14) ultimately calls
//! into. Standalone: does not require the daemon to be running.

use crate::{config, lookup, persist, preflight, signal};

pub fn run(task: &str) -> Result<(), String> {
    // Cheaper, more obviously-fixable setup error first — no subprocess
    // spawn needed to discover "no repo configured," unlike preflight.
    let cwd = config::repo_path()
        .ok_or_else(|| "no repo configured — run `runner repo set <path>` first".to_string())?;

    preflight::check().map_err(|e| e.to_string())?;

    let conn = crate::store::open().map_err(|e| e.to_string())?;

    // "CLI process startup... before any new run is accepted" (SPEC.md
    // F-08 AC-04) — `runner run` is exactly that startup point.
    persist::reconcile_interrupted_runs(&conn).map_err(|e| e.to_string())?;

    // AC-03: task identity for a manual run is the literal, verbatim task
    // string — no transformation, no hashing, no namespacing prefix.
    let task_identity = task;

    let continuation = lookup::lookup(&conn, task_identity).map_err(|e| e.to_string())?;
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

    // `outcome.result.result` is the agent's raw response text, which
    // already contains the NEXT_ACTION/RECHECK_AFTER trailer lines
    // verbatim (F-05 parses them out but never strips them from the
    // stored text — that's not its job). Printing both the raw text and
    // the separately-parsed signal would show the trailer twice; strip it
    // from the displayed body and print the parsed signal once, cleanly,
    // as a status footer instead. `signal::strip_trailer` is the same
    // function F-08's persistence layer uses before storing `result_text`,
    // so `runner run`'s immediate output and `runner logs`'s later
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
