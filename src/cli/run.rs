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
    // as a status footer instead.
    println!("{}", strip_trailer_lines(&outcome.result.result));
    println!(
        "NEXT_ACTION: {} — {}",
        outcome.signal.next_action, outcome.signal.reason
    );
    if let Some(recheck_after) = outcome.signal.recheck_after {
        println!("RECHECK_AFTER: {}s", recheck_after.as_secs());
    }

    Ok(())
}

fn strip_trailer_lines(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("NEXT_ACTION:") && !trimmed.starts_with("RECHECK_AFTER:")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_trailer_lines_removes_both_lines() {
        let text = "Here is my answer.\n\nNEXT_ACTION: idle — done\nRECHECK_AFTER: 30m";
        assert_eq!(strip_trailer_lines(text), "Here is my answer.");
    }

    #[test]
    fn strip_trailer_lines_handles_next_action_only() {
        let text = "Just this.\nNEXT_ACTION: idle — done";
        assert_eq!(strip_trailer_lines(text), "Just this.");
    }

    #[test]
    fn strip_trailer_lines_leaves_text_without_a_trailer_untouched() {
        let text = "No trailer here at all.";
        assert_eq!(strip_trailer_lines(text), text);
    }

    #[test]
    fn strip_trailer_lines_only_matches_line_starts_not_substrings() {
        let text =
            "A sentence that mentions NEXT_ACTION: in passing, mid-line.\nNEXT_ACTION: idle — done";
        let stripped = strip_trailer_lines(text);
        assert!(stripped.contains("mentions NEXT_ACTION:"));
        assert!(!stripped.contains("idle — done"));
    }
}
