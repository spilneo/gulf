const SENSITIVE_ENV_KEY_MARKERS: &[&str] = &[
    "KEY",
    "TOKEN",
    "SECRET",
    "AFFILIATE",
    "RECIPIENT",
    "PRIVATE",
    "MNEMONIC",
    "JWT",
];

/// Redact values from the body that correspond to sensitive environment
/// variables. Keys whose uppercase form contains any marker and whose value
/// length is >= 8 are matched and replaced. The body is then compacted
/// (whitespace collapsed) and truncated to 600 chars.
pub(super) fn redact_response_body(body: &str) -> String {
    redact_response_body_with_values(body, std::env::vars())
}

fn redact_response_body_with_values<I>(body: &str, values: I) -> String
where
    I: IntoIterator<Item = (String, String)>,
{
    let mut redacted = body.to_owned();
    for (key, value) in values {
        if value.len() < 8 {
            continue;
        }
        let upper_key = key.to_uppercase();
        if !SENSITIVE_ENV_KEY_MARKERS
            .iter()
            .any(|marker| upper_key.contains(marker))
        {
            continue;
        }
        redacted = redacted.replace(&value, &format!("<redacted:{key}>"));
        // Also catch lower-case occurrences (some providers embed values in
        // JSON strings that might be folded).
        redacted = redacted.replace(&value.to_lowercase(), &format!("<redacted:{key}>"));
    }
    let compact = redacted.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(600).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic env value redacted from a failure body.
    #[test]
    fn redacts_env_secret_in_failure_text() {
        let synthetic = "syn_redact_test_secret_8chars";
        let body = format!("HTTP 400: invalid key {synthetic}");
        let result = redact_response_body_with_values(
            &body,
            [("TEST_API_KEY".to_owned(), synthetic.to_owned())],
        );

        assert!(!result.contains(synthetic), "secret value must be redacted");
        assert!(
            result.contains("<redacted:TEST_API_KEY>"),
            "redacted marker expected"
        );
    }

    /// Values shorter than 8 characters must never be redacted to avoid
    /// over-redaction (e.g. short hex snippets or common substrings).
    #[test]
    fn never_redacts_short_values() {
        let body = "error abc somewhere";
        let result =
            redact_response_body_with_values(body, [("SHORT_TOKEN".to_owned(), "abc".to_owned())]);
        assert!(result.contains("abc"), "short value must not be redacted");
    }

    /// Non-sensitive env keys should not trigger redaction.
    #[test]
    fn ignores_non_sensitive_keys() {
        let body = "error safe_value_8chars_min here";
        let result = redact_response_body_with_values(
            body,
            [("MY_SAFE_ENV".to_owned(), "safe_value_8chars_min".to_owned())],
        );
        assert!(
            result.contains("safe_value_8chars_min"),
            "non-sensitive env value must remain"
        );
    }

    /// Compact + truncate behaviour is preserved after redaction.
    #[test]
    fn preserves_compact_truncate_after_redaction() {
        let body = std::iter::repeat_n("word", 200)
            .collect::<Vec<_>>()
            .join(" ");
        // Add the token somewhere so it gets redacted too.
        let body_with_secret = format!("syn_trunc_test_val_8char {body}");
        let result = redact_response_body_with_values(
            &body_with_secret,
            [(
                "TEST_TOKEN".to_owned(),
                "syn_trunc_test_val_8char".to_owned(),
            )],
        );

        assert!(result.len() <= 600, "result must be truncated to 600 chars");
        assert!(
            !result.contains("syn_trunc_test_val_8char"),
            "secret must be redacted"
        );
        // The compacted form should not have consecutive spaces
        assert!(!result.contains("  "), "output must be compacted");
    }

    /// Multiple sensitive env values are all redacted.
    #[test]
    fn redacts_multiple_sensitive_values() {
        let body = "error syn_secret_9chars with syn_token_val_9chars";
        let result = redact_response_body_with_values(
            body,
            [
                ("API_SECRET".to_owned(), "syn_secret_9chars".to_owned()),
                ("AUTH_TOKEN".to_owned(), "syn_token_val_9chars".to_owned()),
            ],
        );
        assert!(!result.contains("syn_secret_9chars"));
        assert!(!result.contains("syn_token_val_9chars"));
        assert!(result.contains("<redacted:API_SECRET>"));
        assert!(result.contains("<redacted:AUTH_TOKEN>"));
    }
}
