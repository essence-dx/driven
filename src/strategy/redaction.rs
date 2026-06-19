use regex::{Captures, Regex};
use std::sync::OnceLock;

const REDACTED: &str = "[REDACTED]";

pub(crate) fn redact_secrets(value: &str) -> String {
    let redacted = authorization_regex()
        .replace_all(value, |captures: &Captures<'_>| {
            format!("{}{}", &captures["prefix"], REDACTED)
        })
        .to_string();
    let redacted = bearer_regex()
        .replace_all(&redacted, |captures: &Captures<'_>| {
            format!("{}{}", &captures["prefix"], REDACTED)
        })
        .to_string();
    let redacted = flag_regex()
        .replace_all(&redacted, |captures: &Captures<'_>| {
            format!("{}{}", &captures["prefix"], REDACTED)
        })
        .to_string();
    key_value_regex()
        .replace_all(&redacted, |captures: &Captures<'_>| {
            format!("{}{}", &captures["prefix"], REDACTED)
        })
        .to_string()
}

fn authorization_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?ix)
            (?P<prefix>\bauthorization\s*:\s*(?:bearer|basic)\s+)
            (?P<secret>[^"'\s&,;|)}]+)
            "#,
        )
        .expect("authorization redaction regex is valid")
    })
}

fn bearer_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?ix)
            (?P<prefix>\bbearer\s+)
            (?P<secret>[^"'\s&,;|)}]+)
            "#,
        )
        .expect("bearer redaction regex is valid")
    })
}

fn flag_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?ix)
            (?P<prefix>
                --(?:openai-api-key|openai_api_key|api-key|api_key|apikey|access-token|access_token|
                    refresh-token|refresh_token|id-token|id_token|client-secret|client_secret|
                    private-key|private_key|password|token|secret|session|cookie)
                (?:\s+|=)
                ["']?
            )
            (?P<secret>[^"'\s&,;|)}]+)
            "#,
        )
        .expect("flag redaction regex is valid")
    })
}

fn key_value_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?ix)
            (?P<prefix>
                ["']?
                (?:openai_api_key|api_key|apikey|access_token|refresh_token|id_token|
                    client_secret|private_key|password|token|secret|session|cookie)
                ["']?
                \s*[:=]\s*
                ["']?
            )
            (?P<secret>[^"'\s&,;|)}]+)
            "#,
        )
        .expect("key-value redaction regex is valid")
    })
}
