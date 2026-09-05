//! Bounded retry wrapping one claude invocation plus its continuation-
//! signal parse. Exactly one automatic retry when the subprocess itself
//! fails outright, *or* when the signal parser can't find a `NEXT_ACTION`
//! line in an otherwise-successful result: both count as "didn't get a
//! usable response," one retry bucket. Not a backoff/supervisor policy —
//! just enough resilience to absorb a single flaky invocation.

use std::path::Path;

use crate::claude::process::{self, ClaudeError, ClaudeResult};
use crate::claude::signal::{self, ContinuationSignal, SignalParseError};

#[derive(Debug, PartialEq)]
pub enum RunFailure {
    Process(ClaudeError),
    SignalParse(SignalParseError),
}

impl std::fmt::Display for RunFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunFailure::Process(e) => write!(f, "{e}"),
            RunFailure::SignalParse(_) => write!(f, "response had no NEXT_ACTION trailer"),
        }
    }
}

/// Both attempts' failures — the caller sees why the *second* try failed
/// too, not just the first.
#[derive(Debug, PartialEq)]
pub struct RetryExhausted {
    pub first: RunFailure,
    pub second: RunFailure,
}

impl std::fmt::Display for RetryExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "first attempt: {}; retry also failed: {}",
            self.first, self.second
        )
    }
}

#[derive(Debug)]
pub struct RunOutcome {
    pub result: ClaudeResult,
    pub signal: ContinuationSignal,
    /// Whether the first attempt failed and this outcome came from the
    /// retry — persisted as `runs.retry_count`.
    pub retried: bool,
}

/// Real entry point — runs one claude turn and parses its continuation
/// signal, retrying exactly once if either step fails.
pub fn run_with_retry(
    prompt: &str,
    resume: Option<&str>,
    cwd: &Path,
) -> Result<RunOutcome, RetryExhausted> {
    let prompt = prompt.to_string();
    let resume = resume.map(String::from);
    let cwd = cwd.to_path_buf();
    run_with_retry_using(move || attempt(&prompt, resume.as_deref(), &cwd))
}

fn attempt(
    prompt: &str,
    resume: Option<&str>,
    cwd: &Path,
) -> Result<(ClaudeResult, ContinuationSignal), RunFailure> {
    let result = process::run(prompt, resume, cwd).map_err(RunFailure::Process)?;
    let signal =
        signal::parse_continuation_signal(&result.result).map_err(RunFailure::SignalParse)?;
    Ok((result, signal))
}

/// The actual retry policy, parameterized on the attempt itself so it's
/// testable without ever spawning a real (or fake) subprocess — a second
/// call to `attempt_fn` either succeeds (retry absorbed the flake) or
/// fails (both reasons surface to the caller).
fn run_with_retry_using(
    mut attempt_fn: impl FnMut() -> Result<(ClaudeResult, ContinuationSignal), RunFailure>,
) -> Result<RunOutcome, RetryExhausted> {
    match attempt_fn() {
        Ok((result, signal)) => Ok(RunOutcome {
            result,
            signal,
            retried: false,
        }),
        Err(first) => match attempt_fn() {
            Ok((result, signal)) => Ok(RunOutcome {
                result,
                signal,
                retried: true,
            }),
            Err(second) => Err(RetryExhausted { first, second }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn ok_outcome(label: &str) -> (ClaudeResult, ContinuationSignal) {
        (
            ClaudeResult {
                result: label.to_string(),
                session_id: Some("sess".to_string()),
                cost_usd: 0.0,
            },
            ContinuationSignal {
                next_action: "idle".to_string(),
                reason: String::new(),
                recheck_after: None,
            },
        )
    }

    fn process_failure() -> RunFailure {
        RunFailure::Process(ClaudeError::Spawn("boom".to_string()))
    }

    fn signal_failure() -> RunFailure {
        RunFailure::SignalParse(SignalParseError)
    }

    #[test]
    fn first_attempt_success_is_not_marked_retried() {
        let calls = Cell::new(0);
        let outcome = run_with_retry_using(|| {
            calls.set(calls.get() + 1);
            Ok(ok_outcome("first try"))
        })
        .unwrap();

        assert_eq!(calls.get(), 1, "should not call a second time on success");
        assert!(!outcome.retried);
        assert_eq!(outcome.result.result, "first try");
    }

    #[test]
    fn process_failure_then_success_retries_once_and_succeeds() {
        let calls = Cell::new(0);
        let outcome = run_with_retry_using(|| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Err(process_failure())
            } else {
                Ok(ok_outcome("second try"))
            }
        })
        .unwrap();

        assert_eq!(calls.get(), 2);
        assert!(outcome.retried);
        assert_eq!(outcome.result.result, "second try");
    }

    #[test]
    fn signal_parse_failure_then_success_retries_once_and_succeeds() {
        let calls = Cell::new(0);
        let outcome = run_with_retry_using(|| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Err(signal_failure())
            } else {
                Ok(ok_outcome("recovered"))
            }
        })
        .unwrap();

        assert!(outcome.retried);
        assert_eq!(outcome.result.result, "recovered");
    }

    #[test]
    fn both_attempts_failing_surfaces_both_reasons_and_stops_at_two_calls() {
        let calls = Cell::new(0);
        let err = run_with_retry_using(|| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Err(process_failure())
            } else {
                Err(signal_failure())
            }
        })
        .unwrap_err();

        assert_eq!(calls.get(), 2, "must not retry more than once");
        assert_eq!(err.first, process_failure());
        assert_eq!(err.second, signal_failure());
    }

    #[test]
    fn retry_exhausted_display_mentions_both_attempts() {
        let err = RetryExhausted {
            first: process_failure(),
            second: signal_failure(),
        };
        let msg = err.to_string();
        assert!(msg.contains("first attempt"));
        assert!(msg.contains("retry also failed"));
    }
}
