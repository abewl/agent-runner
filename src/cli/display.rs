//! Small presentation-layer helpers shared across CLI commands that print
//! tabular output (F-11's `runner status`, F-13's `runner cron list`), and
//! — as of F-15 — across the CLI and the TUI (`crate::tui`), which is why
//! this module is `pub(crate)` rather than private to `cli`. Still not a
//! capability anything should reach for casually: it's formatting, not
//! logic.

use crate::store::runs::{Run, RunStatus};

/// Truncates `s` to at most `max` characters, appending an ellipsis when
/// it does. Character-count-based (not byte-based), safe for multi-byte
/// UTF-8 task text.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

/// The result/failure detail plus signal fields for a single run — shared
/// by F-12's `runner logs` and F-15's TUI detail pane, which SPEC.md's
/// AC-03 explicitly requires show "the same data `runner logs` would
/// print." One formatter, not two copies that could drift apart.
pub fn format_run_detail(run: &Run) -> String {
    let mut lines = Vec::new();

    match run.status {
        RunStatus::Done => {
            lines.push(run.result_text.as_deref().unwrap_or("").to_string());
        }
        RunStatus::Failed => {
            lines.push(
                run.exit_reason
                    .as_deref()
                    .unwrap_or("(no failure detail recorded)")
                    .to_string(),
            );
        }
        RunStatus::Running | RunStatus::Interrupted => {
            lines.push(format!("status: {} — no result yet", run.status));
        }
    }

    lines.push(format!(
        "next_action: {}",
        run.next_action.as_deref().unwrap_or("none")
    ));
    lines.push(format!(
        "next_action_reason: {}",
        run.next_action_reason.as_deref().unwrap_or("none")
    ));
    lines.push(format!(
        "recheck_after: {}",
        run.recheck_after.as_deref().unwrap_or("none")
    ));

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_leaves_short_strings_untouched() {
        assert_eq!(truncate("short", 40), "short");
    }

    #[test]
    fn truncate_shortens_long_strings_with_an_ellipsis() {
        let long = "x".repeat(50);
        let truncated = truncate(&long, 40);
        assert_eq!(truncated.chars().count(), 40);
        assert!(truncated.ends_with('…'));
    }
}
