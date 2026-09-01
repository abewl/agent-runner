//! Small presentation-layer helpers shared across CLI commands that print
//! tabular output (F-11's `runner status`, F-13's `runner cron list`).
//! Private to `cli` — this is formatting, not a capability anything
//! outside the CLI layer should reach for.

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
