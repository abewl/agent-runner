//! The core "run one claude turn" primitive. Spawns
//! `claude --print --output-format json [--resume <id>] "<prompt>"` as a
//! plain piped subprocess — no PTY, `claude` runs non-interactively — with
//! its working directory set to the configured target repo. In practice
//! this emits a single flat JSON object, not an array, but the parser
//! accepts either shape defensively.
//!
//! No git operations (clone, branch, push) and no MCP config file writing
//! happen anywhere in this module — invocation is a plain prompt string
//! in, result out; any file changes happen because `claude`'s own tools
//! write directly into the repo working tree it was launched in.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const CLAUDE_BIN: &str = "claude";
/// Result text/errors beyond this length are truncated in error messages —
/// enough to diagnose, not enough to spam a log with a full bad response.
const TRUNCATE_LEN: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeResult {
    pub result: String,
    pub session_id: Option<String>,
    pub cost_usd: f64,
}

#[derive(Debug, PartialEq)]
pub enum ClaudeError {
    /// The subprocess itself couldn't be spawned (e.g. binary not found).
    Spawn(String),
    /// The subprocess exited non-zero — always a typed error here, never
    /// a silent success, regardless of what stdout held.
    NonZeroExit {
        code: Option<i32>,
        stderr: String,
        stdout_tail: String,
    },
    /// A `result` event was present but its `subtype` was neither
    /// `"success"` nor `"error_max_turns"` — matches chili-jar's own
    /// adapter, which rejects on any other subtype rather than treating it
    /// as a soft fallback case.
    ResultError { subtype: String, detail: String },
    /// Output was not valid JSON, or had no `result` event, *and* no
    /// `assistant` message text was found to fall back to.
    Unparseable { raw_tail: String },
}

impl std::fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaudeError::Spawn(e) => write!(f, "failed to spawn claude: {e}"),
            ClaudeError::NonZeroExit {
                code,
                stderr,
                stdout_tail,
            } => write!(
                f,
                "claude exited with {code:?}: stderr={stderr} stdout={stdout_tail}"
            ),
            ClaudeError::ResultError { subtype, detail } => {
                write!(f, "claude: {subtype} — {detail}")
            }
            ClaudeError::Unparseable { raw_tail } => {
                write!(f, "claude produced unparseable output: {raw_tail}")
            }
        }
    }
}

/// Real entry point — always invokes the actual `claude` binary. `cwd` is
/// a required parameter, not a default with an override — callers resolve
/// it from `config::repo_path()` themselves and must handle "no repo
/// configured" before calling this at all.
pub fn run(prompt: &str, resume: Option<&str>, cwd: &Path) -> Result<ClaudeResult, ClaudeError> {
    run_with(CLAUDE_BIN, prompt, resume, cwd)
}

fn run_with(
    bin: &str,
    prompt: &str,
    resume: Option<&str>,
    cwd: &Path,
) -> Result<ClaudeResult, ClaudeError> {
    let mut cmd = Command::new(bin);
    cmd.arg("--print").arg("--output-format").arg("json");
    // Without a permission flag, non-interactive claude blocks on any file
    // write/edit — there's no TTY to answer a permission prompt through,
    // so the call would otherwise fail on any task beyond read-only work.
    // `acceptEdits` covers the core capability Runner needs; deliberately
    // not `--dangerously-skip-permissions`/`bypassPermissions`, which
    // Anthropic's own docs recommend only for sandboxes with no internet
    // access — this runs on the user's real machine, not an isolated one.
    // A target repo can still layer its own `.claude/settings.json` for
    // finer-grained tool permissions; claude already reads settings from
    // its working directory, and this doesn't override that.
    cmd.arg("--permission-mode").arg("acceptEdits");
    if let Some(session_id) = resume {
        cmd.arg("--resume").arg(session_id);
    }
    cmd.arg(prompt);
    cmd.current_dir(cwd);

    let output = cmd
        .output()
        .map_err(|e| ClaudeError::Spawn(e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    if !output.status.success() {
        return Err(ClaudeError::NonZeroExit {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            stdout_tail: truncate(&stdout),
        });
    }

    parse_claude_output(&stdout, resume)
}

fn truncate(s: &str) -> String {
    if s.len() > TRUNCATE_LEN {
        format!("{}... (truncated)", &s[..TRUNCATE_LEN])
    } else {
        s.to_string()
    }
}

/// Parses `--output-format json`'s event array. Pure function — no
/// subprocess involved — so every branch is unit-testable with hand-built
/// JSON, matching the shape already proven in chili-jar's
/// `adapters/claude-code/index.mjs` (`runClaudeMessage`).
fn parse_claude_output(stdout: &str, resume: Option<&str>) -> Result<ClaudeResult, ClaudeError> {
    let parsed: Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(_) => {
            return fallback_to_assistant_text(stdout, resume).ok_or_else(|| {
                ClaudeError::Unparseable {
                    raw_tail: truncate(stdout),
                }
            });
        }
    };

    let events: Vec<Value> = match parsed {
        Value::Array(arr) => arr,
        other => vec![other],
    };

    if let Some(result_event) = events
        .iter()
        .find(|e| e.get("type").and_then(Value::as_str) == Some("result"))
    {
        let subtype = result_event
            .get("subtype")
            .and_then(Value::as_str)
            .unwrap_or("");

        return match subtype {
            "success" | "error_max_turns" => {
                let result_text = result_event
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let session_id = result_event
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .or_else(|| resume.map(String::from));
                // Verified against a real `claude --print --output-format
                // json` call on this machine (2.1.252): the field is
                // `total_cost_usd`, not `cost_usd`. `cost_usd` is kept as a
                // fallback in case an older/different CLI version or mode
                // still uses that name — chili-jar's adapter code (which
                // this parser otherwise follows) assumed `cost_usd`, likely
                // against an earlier CLI version.
                let cost_usd = result_event
                    .get("total_cost_usd")
                    .or_else(|| result_event.get("cost_usd"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                Ok(ClaudeResult {
                    result: result_text,
                    session_id,
                    cost_usd,
                })
            }
            other => Err(ClaudeError::ResultError {
                subtype: other.to_string(),
                detail: result_event
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or("no detail")
                    .to_string(),
            }),
        };
    }

    fallback_to_assistant_text_from_events(&events, resume).ok_or_else(|| {
        ClaudeError::Unparseable {
            raw_tail: truncate(stdout),
        }
    })
}

/// Used when `stdout` wasn't valid JSON at all — nothing to extract
/// events from, so this always returns `None` (falls straight through to
/// `Unparseable`). Kept as a named function (rather than inlined) so the
/// "not valid JSON" and "valid JSON, no result event" paths both read as
/// explicitly choosing the same fallback behavior.
fn fallback_to_assistant_text(_stdout: &str, _resume: Option<&str>) -> Option<ClaudeResult> {
    None
}

fn fallback_to_assistant_text_from_events(
    events: &[Value],
    resume: Option<&str>,
) -> Option<ClaudeResult> {
    let text: String = events
        .iter()
        .filter(|e| e.get("type").and_then(Value::as_str) == Some("assistant"))
        .filter_map(|e| {
            e.get("message")
                .and_then(|m| m.get("content"))
                .and_then(Value::as_array)
        })
        .flatten()
        .filter(|c| c.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|c| c.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");

    if text.is_empty() {
        None
    } else {
        Some(ClaudeResult {
            result: text,
            session_id: resume.map(String::from),
            cost_usd: 0.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "runner-process-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes an executable shell script standing in for `claude`, for
    /// testing spawn mechanics (args, cwd, exit code) without ever
    /// invoking the real, network-calling CLI.
    fn fake_script(label: &str, body: &str) -> std::path::PathBuf {
        let dir = temp_dir(label);
        let path = dir.join("fake_claude.sh");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    // ---- parse_claude_output: pure function, no subprocess at all ----

    #[test]
    fn parses_a_success_result_event() {
        let stdout = r#"[
            {"type":"system","subtype":"init","session_id":"sess-1"},
            {"type":"result","subtype":"success","result":"hello world","session_id":"sess-1","total_cost_usd":0.0042}
        ]"#;
        let parsed = parse_claude_output(stdout, None).unwrap();
        assert_eq!(parsed.result, "hello world");
        assert_eq!(parsed.session_id, Some("sess-1".to_string()));
        assert_eq!(parsed.cost_usd, 0.0042);
    }

    /// The real shape a current `claude --print --output-format json` call
    /// produces on this machine (2.1.252) — a single flat object, not an
    /// array, and `total_cost_usd` rather than `cost_usd`. Trimmed of
    /// fields this parser doesn't read; nothing sensitive (token counts
    /// and non-secret metadata only). Captured 2026-09-01 via
    /// `claude --print --output-format json "say the single word: pong"`.
    #[test]
    fn parses_the_real_current_cli_output_shape() {
        let stdout = r#"{"duration_api_ms":2594,"stop_reason":"end_turn","session_id":"ced2975d-e51b-4464-bedd-d046696c30fb","total_cost_usd":0.0422666,"is_error":false,"num_turns":1,"subtype":"success","result":"pong","type":"result"}"#;
        let parsed = parse_claude_output(stdout, None).unwrap();
        assert_eq!(parsed.result, "pong");
        assert_eq!(
            parsed.session_id,
            Some("ced2975d-e51b-4464-bedd-d046696c30fb".to_string())
        );
        assert_eq!(parsed.cost_usd, 0.0422666);
    }

    #[test]
    fn falls_back_to_cost_usd_field_when_total_cost_usd_is_absent() {
        // Defensive compatibility with the older field name chili-jar's
        // adapter code assumed — see the comment at the read site.
        let stdout = r#"[{"type":"result","subtype":"success","result":"ok","cost_usd":0.0042}]"#;
        let parsed = parse_claude_output(stdout, None).unwrap();
        assert_eq!(parsed.cost_usd, 0.0042);
    }

    #[test]
    fn treats_error_max_turns_as_a_usable_result_not_an_error() {
        let stdout = r#"[{"type":"result","subtype":"error_max_turns","result":"partial","session_id":"sess-2","total_cost_usd":0.01}]"#;
        let parsed = parse_claude_output(stdout, None).unwrap();
        assert_eq!(parsed.result, "partial");
    }

    #[test]
    fn other_result_subtypes_are_a_typed_error_not_a_fallback() {
        let stdout = r#"[{"type":"result","subtype":"error_during_execution","result":"boom"}]"#;
        let err = parse_claude_output(stdout, None).unwrap_err();
        match err {
            ClaudeError::ResultError { subtype, detail } => {
                assert_eq!(subtype, "error_during_execution");
                assert_eq!(detail, "boom");
            }
            other => panic!("expected ResultError, got {other:?}"),
        }
    }

    #[test]
    fn missing_session_id_on_result_event_falls_back_to_resume_value() {
        let stdout = r#"[{"type":"result","subtype":"success","result":"ok","cost_usd":0.0}]"#;
        let parsed = parse_claude_output(stdout, Some("prior-session")).unwrap();
        assert_eq!(parsed.session_id, Some("prior-session".to_string()));
    }

    #[test]
    fn missing_cost_usd_defaults_to_zero() {
        let stdout = r#"[{"type":"result","subtype":"success","result":"ok","session_id":"s1"}]"#;
        let parsed = parse_claude_output(stdout, None).unwrap();
        assert_eq!(parsed.cost_usd, 0.0);
    }

    #[test]
    fn no_result_event_falls_back_to_assistant_text_blocks() {
        let stdout = r#"[
            {"type":"assistant","message":{"content":[{"type":"text","text":"part one "}]}},
            {"type":"assistant","message":{"content":[{"type":"text","text":"part two"}]}}
        ]"#;
        let parsed = parse_claude_output(stdout, None).unwrap();
        assert_eq!(parsed.result, "part one part two");
        assert_eq!(parsed.cost_usd, 0.0);
    }

    #[test]
    fn no_result_event_and_no_assistant_text_is_unparseable() {
        let stdout = r#"[{"type":"system","subtype":"init","session_id":"s1"}]"#;
        let err = parse_claude_output(stdout, None).unwrap_err();
        assert!(matches!(err, ClaudeError::Unparseable { .. }));
    }

    #[test]
    fn invalid_json_is_unparseable_with_truncated_raw_output() {
        let stdout = "this is not json at all";
        let err = parse_claude_output(stdout, None).unwrap_err();
        match err {
            ClaudeError::Unparseable { raw_tail } => assert_eq!(raw_tail, stdout),
            other => panic!("expected Unparseable, got {other:?}"),
        }
    }

    #[test]
    fn truncate_shortens_long_output_and_marks_it() {
        let long = "x".repeat(TRUNCATE_LEN + 200);
        let err = parse_claude_output(&long, None).unwrap_err();
        match err {
            ClaudeError::Unparseable { raw_tail } => {
                assert!(raw_tail.len() < long.len());
                assert!(raw_tail.ends_with("... (truncated)"));
            }
            other => panic!("expected Unparseable, got {other:?}"),
        }
    }

    // ---- run_with: real spawn mechanics against a fake stand-in binary ----

    #[test]
    fn run_with_passes_print_output_format_json_and_the_prompt() {
        let script = fake_script(
            "args-basic",
            r#"echo "[{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"$*\",\"cost_usd\":0}]""#,
        );
        let cwd = temp_dir("args-basic-cwd");
        let result = run_with(script.to_str().unwrap(), "hello there", None, &cwd).unwrap();
        assert!(result.result.contains("--print"));
        assert!(result.result.contains("--output-format"));
        assert!(result.result.contains("json"));
        assert!(result.result.contains("hello there"));
    }

    #[test]
    fn run_with_always_passes_permission_mode_accept_edits() {
        let script = fake_script(
            "args-permission",
            r#"echo "[{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"$*\",\"cost_usd\":0}]""#,
        );
        let cwd = temp_dir("args-permission-cwd");
        let result = run_with(script.to_str().unwrap(), "task", None, &cwd).unwrap();
        assert!(result.result.contains("--permission-mode"));
        assert!(result.result.contains("acceptEdits"));
    }

    #[test]
    fn run_with_passes_resume_flag_and_session_id_when_given() {
        let script = fake_script(
            "args-resume",
            r#"echo "[{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"$*\",\"cost_usd\":0}]""#,
        );
        let cwd = temp_dir("args-resume-cwd");
        let result = run_with(script.to_str().unwrap(), "continue", Some("sess-99"), &cwd).unwrap();
        assert!(result.result.contains("--resume"));
        assert!(result.result.contains("sess-99"));
    }

    #[test]
    fn run_with_invokes_the_binary_in_the_given_cwd() {
        let script = fake_script(
            "cwd-check",
            r#"echo "[{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"$(pwd)\",\"cost_usd\":0}]""#,
        );
        let cwd = temp_dir("cwd-check-target");
        let result = run_with(script.to_str().unwrap(), "task", None, &cwd).unwrap();
        let canonical_cwd = cwd.canonicalize().unwrap();
        assert_eq!(result.result, canonical_cwd.to_str().unwrap());
    }

    #[test]
    fn run_with_non_zero_exit_is_always_a_typed_error() {
        let script = fake_script(
            "nonzero",
            r#"echo "stdout stuff"; echo "stderr stuff" >&2; exit 7"#,
        );
        let cwd = temp_dir("nonzero-cwd");
        let err = run_with(script.to_str().unwrap(), "task", None, &cwd).unwrap_err();
        match err {
            ClaudeError::NonZeroExit {
                code,
                stderr,
                stdout_tail,
            } => {
                assert_eq!(code, Some(7));
                assert!(stderr.contains("stderr stuff"));
                assert!(stdout_tail.contains("stdout stuff"));
            }
            other => panic!("expected NonZeroExit, got {other:?}"),
        }
    }

    #[test]
    fn run_with_nonexistent_binary_is_a_spawn_error() {
        let cwd = temp_dir("spawn-error-cwd");
        let err = run_with("definitely-not-a-real-binary-xyz-123", "task", None, &cwd).unwrap_err();
        assert!(matches!(err, ClaudeError::Spawn(_)));
    }
}
