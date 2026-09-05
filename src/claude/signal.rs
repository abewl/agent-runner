//! Continuation signal — prompt trailer convention + parsing.
//!
//! Every prompt Runner sends carries the identical fixed trailer
//! instruction below — one template, no PM-mode/Engineer-mode variants.
//! `next_action`/`reason` are opaque to Runner: stored, displayed, and
//! handed back as context on the next invocation — never branched on in
//! code anywhere in this crate. Runner relays the signal, it never
//! interprets it; see `PROJECT.md` §4 for why.

use std::time::Duration;

pub const TRAILER_INSTRUCTION: &str = "\n\nBefore finishing, end your response with exactly these two lines (omit the second one if it doesn't apply):\nNEXT_ACTION: <short label> — <one-line reason>\nRECHECK_AFTER: <duration, e.g. \"30m\">";

#[derive(Debug, Clone, PartialEq)]
pub struct ContinuationSignal {
    pub next_action: String,
    pub reason: String,
    pub recheck_after: Option<Duration>,
}

#[derive(Debug, PartialEq)]
pub struct SignalParseError;

/// Builds the full prompt sent to `claude`: an optional continuation
/// context line ("your own last recommendation was..."), then the
/// caller's task text, then the fixed trailer
/// instruction. The same shape for every caller — `runner run` and the
/// cron tick engine construct prompts identically; there is no branch
/// anywhere on whether this is a "manual" or "scheduled" invocation.
pub fn build_prompt(task: &str, context: Option<&str>) -> String {
    let mut prompt = String::new();
    if let Some(ctx) = context {
        prompt.push_str(ctx);
        prompt.push_str("\n\n");
    }
    prompt.push_str(task);
    prompt.push_str(TRAILER_INSTRUCTION);
    prompt
}

/// Parses the fixed `NEXT_ACTION:`/`RECHECK_AFTER:` trailer lines out of
/// `claude`'s result text. A missing `NEXT_ACTION:` line is malformed
/// output — `Err`, not silently treated as "no signal" — so it feeds into
/// the same retry-on-malformed-output path as unparseable JSON from the
/// subprocess itself.
pub fn parse_continuation_signal(
    result_text: &str,
) -> Result<ContinuationSignal, SignalParseError> {
    let mut next_action: Option<String> = None;
    let mut reason: Option<String> = None;
    let mut recheck_after: Option<Duration> = None;

    for line in result_text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("NEXT_ACTION:") {
            let (label, why) = split_label_and_reason(rest.trim());
            next_action = Some(label);
            reason = Some(why);
        } else if let Some(rest) = trimmed.strip_prefix("RECHECK_AFTER:") {
            recheck_after = Some(parse_duration(rest.trim()).ok_or(SignalParseError)?);
        }
    }

    match (next_action, reason) {
        (Some(next_action), Some(reason)) => Ok(ContinuationSignal {
            next_action,
            reason,
            recheck_after,
        }),
        _ => Err(SignalParseError),
    }
}

/// Removes the `NEXT_ACTION:`/`RECHECK_AFTER:` trailer lines from a
/// result's text, for display or storage contexts that show the parsed
/// signal separately and would otherwise show the trailer twice — once
/// verbatim in the raw text, once from the separately-parsed fields.
/// `parse_continuation_signal` itself never strips (that's not its job,
/// and the field data still needs the original text) — this is the one
/// place that does, so every consumer shows identical, clean text rather
/// than each hand-rolling it.
pub fn strip_trailer(text: &str) -> String {
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

/// Splits `"<label> — <reason>"` on the separator we ask for (an em dash),
/// falling back to a plain " - " for robustness against minor
/// reproduction variance, and finally to "whole thing is the label, empty
/// reason" if neither is present rather than failing the whole parse over
/// a missing separator — the label (`NEXT_ACTION`'s presence at all) is
/// what actually matters; the reason is display text.
fn split_label_and_reason(rest: &str) -> (String, String) {
    for sep in ["—", " - "] {
        if let Some((label, why)) = rest.split_once(sep) {
            return (label.trim().to_string(), why.trim().to_string());
        }
    }
    (rest.to_string(), String::new())
}

/// Minimal duration parser: digits followed by exactly one of `m`/`h`/`d`.
fn parse_duration(s: &str) -> Option<Duration> {
    if s.is_empty() || s.len() < 2 {
        return None;
    }
    let (digits, unit) = s.split_at(s.len() - 1);
    let n: u64 = digits.parse().ok()?;
    match unit {
        "m" => Some(Duration::from_secs(n * 60)),
        "h" => Some(Duration::from_secs(n * 3600)),
        "d" => Some(Duration::from_secs(n * 86400)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_without_context_ends_with_the_trailer() {
        let prompt = build_prompt("do the thing", None);
        assert!(prompt.starts_with("do the thing"));
        assert!(prompt.ends_with(TRAILER_INSTRUCTION));
    }

    #[test]
    fn build_prompt_with_context_puts_context_first() {
        let prompt = build_prompt("do the thing", Some("last time: idle"));
        assert!(prompt.starts_with("last time: idle"));
        assert!(prompt.contains("do the thing"));
        assert!(prompt.ends_with(TRAILER_INSTRUCTION));
        // context, task, and trailer are distinguishable, not run together
        assert!(prompt.contains("last time: idle\n\ndo the thing"));
    }

    #[test]
    fn parses_full_trailer_with_em_dash_and_recheck() {
        let text = "Did the work.\n\nNEXT_ACTION: idle — nothing left to do\nRECHECK_AFTER: 30m";
        let signal = parse_continuation_signal(text).unwrap();
        assert_eq!(signal.next_action, "idle");
        assert_eq!(signal.reason, "nothing left to do");
        assert_eq!(signal.recheck_after, Some(Duration::from_secs(1800)));
    }

    #[test]
    fn recheck_after_absent_is_a_valid_none_not_an_error() {
        let text = "NEXT_ACTION: continue_engineer — more todo items remain";
        let signal = parse_continuation_signal(text).unwrap();
        assert_eq!(signal.recheck_after, None);
    }

    #[test]
    fn accepts_ascii_hyphen_separator_as_a_fallback() {
        let text = "NEXT_ACTION: blocked - waiting on human input";
        let signal = parse_continuation_signal(text).unwrap();
        assert_eq!(signal.next_action, "blocked");
        assert_eq!(signal.reason, "waiting on human input");
    }

    #[test]
    fn missing_separator_treats_whole_remainder_as_the_label() {
        let text = "NEXT_ACTION: idle";
        let signal = parse_continuation_signal(text).unwrap();
        assert_eq!(signal.next_action, "idle");
        assert_eq!(signal.reason, "");
    }

    #[test]
    fn missing_next_action_line_is_a_parse_error() {
        let text = "Just did some work, no trailer at all.";
        assert_eq!(parse_continuation_signal(text), Err(SignalParseError));
    }

    #[test]
    fn unparseable_recheck_after_duration_is_a_parse_error() {
        let text = "NEXT_ACTION: idle — nothing to do\nRECHECK_AFTER: soon";
        assert_eq!(parse_continuation_signal(text), Err(SignalParseError));
    }

    #[test]
    fn trailer_lines_found_among_surrounding_prose() {
        let text = "Line one of the response.\nLine two.\nNEXT_ACTION: run_pm — backlog is empty\nRECHECK_AFTER: 2h\nTrailing line after (should be ignored).";
        let signal = parse_continuation_signal(text).unwrap();
        assert_eq!(signal.next_action, "run_pm");
        assert_eq!(signal.recheck_after, Some(Duration::from_secs(7200)));
    }

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86400)));
    }

    #[test]
    fn parse_duration_rejects_unknown_unit() {
        assert_eq!(parse_duration("30s"), None);
    }

    #[test]
    fn parse_duration_rejects_non_numeric() {
        assert_eq!(parse_duration("soonm"), None);
    }

    #[test]
    fn parse_duration_rejects_empty_and_too_short() {
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("m"), None);
    }

    #[test]
    fn strip_trailer_removes_both_lines() {
        let text = "Here is my answer.\n\nNEXT_ACTION: idle — done\nRECHECK_AFTER: 30m";
        assert_eq!(strip_trailer(text), "Here is my answer.");
    }

    #[test]
    fn strip_trailer_handles_next_action_only() {
        let text = "Just this.\nNEXT_ACTION: idle — done";
        assert_eq!(strip_trailer(text), "Just this.");
    }

    #[test]
    fn strip_trailer_leaves_text_without_a_trailer_untouched() {
        let text = "No trailer here at all.";
        assert_eq!(strip_trailer(text), text);
    }

    #[test]
    fn strip_trailer_only_matches_line_starts_not_substrings() {
        let text =
            "A sentence that mentions NEXT_ACTION: in passing, mid-line.\nNEXT_ACTION: idle — done";
        let stripped = strip_trailer(text);
        assert!(stripped.contains("mentions NEXT_ACTION:"));
        assert!(!stripped.contains("idle — done"));
    }
}
