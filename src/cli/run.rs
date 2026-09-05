//! `runner run <task>` command implementation — the command every other
//! manual-control command and the scheduler ultimately call into.
//! Standalone: does not require the daemon to be running.

use crate::claude::{preflight, signal};
use crate::persist::{self, ChainStopReason, DEFAULT_CHAIN_MAX_TURNS, STUCK_REPEAT_THRESHOLD};
use crate::retry::RunOutcome;
use crate::{config, store};

pub fn run(task: &str, once: bool) -> Result<(), String> {
    // Cheaper, more obviously-fixable setup error first — no subprocess
    // spawn needed to discover "no repo configured," unlike preflight.
    let cwd = config::repo_path()
        .ok_or_else(|| "no repo configured — run `runner repo <path>` first".to_string())?;

    preflight::check().map_err(|e| e.to_string())?;

    let conn = store::open().map_err(|e| e.to_string())?;

    // `runner run` is the CLI's own startup point, so it reconciles any
    // interrupted-looking row before accepting a new one.
    persist::reconcile_interrupted_runs(&conn).map_err(|e| e.to_string())?;

    // Task identity for a manual run is the literal, verbatim task string —
    // no transformation, no hashing, no namespacing prefix.
    let task_identity = task;

    if once {
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

        print_outcome(&outcome);
        return Ok(());
    }

    let mut turn = 0;
    let (outcomes, stop_reason) = persist::run_chain(
        &conn,
        task_identity,
        task,
        &cwd,
        DEFAULT_CHAIN_MAX_TURNS,
        |outcome| {
            turn += 1;
            if turn > 1 {
                println!("--- turn {turn} ---");
            }
            print_outcome(outcome);
        },
    )
    .map_err(|e| e.to_string())?;

    match stop_reason {
        ChainStopReason::AgentDone => {
            println!("--- chain complete: {} turn(s) ---", outcomes.len());
        }
        ChainStopReason::Stuck => {
            println!(
                "--- chain stopped: repeated the same NEXT_ACTION {STUCK_REPEAT_THRESHOLD} times with no progress ---"
            );
        }
        ChainStopReason::MaxTurnsReached => {
            println!(
                "--- chain stopped: reached the {DEFAULT_CHAIN_MAX_TURNS}-turn cap while the agent still requested to continue ---"
            );
        }
    }

    Ok(())
}

/// `outcome.result.result` still contains the NEXT_ACTION/RECHECK_AFTER/
/// CHAIN_CONTINUE trailer verbatim (parsing it out doesn't strip it from
/// the stored text). Printing both the raw text and the separately-parsed
/// signal would show the trailer twice, so strip it from the displayed
/// body and print the parsed signal once, as a status footer — the same
/// stripping function the persistence layer uses before storing
/// `result_text`, so this command's stdout and `runner logs`'s later
/// retrieval show identical, clean text.
fn print_outcome(outcome: &RunOutcome) {
    println!("{}", signal::strip_trailer(&outcome.result.result));
    println!(
        "NEXT_ACTION: {} — {}",
        outcome.signal.next_action, outcome.signal.reason
    );
    if let Some(recheck_after) = outcome.signal.recheck_after {
        println!("RECHECK_AFTER: {}s", recheck_after.as_secs());
    }
}
