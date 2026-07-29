use std::{fmt, sync::OnceLock};

use regex::Regex;

use crate::SecretString;

const REDACTED: &str = "[REDACTED]";

/// Central secret scrubber for any text that may leave a trusted runtime
/// boundary, including provider errors, diagnostics, exports, and crash data.
#[derive(Default)]
pub struct SensitiveDataRedactor {
    secrets: Vec<SecretString>,
}

impl SensitiveDataRedactor {
    pub fn new(secrets: impl IntoIterator<Item = SecretString>) -> Self {
        Self {
            secrets: secrets.into_iter().collect(),
        }
    }

    pub fn redact(&self, value: &str) -> String {
        let mut redacted = value.to_owned();
        let mut secrets = self
            .secrets
            .iter()
            .map(SecretString::expose_secret)
            .filter(|secret| !secret.is_empty())
            .collect::<Vec<_>>();
        secrets.sort_unstable_by_key(|secret| std::cmp::Reverse(secret.len()));
        secrets.dedup();
        for secret in secrets {
            redacted = redacted.replace(secret, REDACTED);
        }

        redacted = json_secret_pattern()
            .replace_all(&redacted, "${1}[REDACTED]${2}")
            .into_owned();
        redacted = header_secret_pattern()
            .replace_all(&redacted, "${1}${2}[REDACTED]")
            .into_owned();
        redacted = query_secret_pattern()
            .replace_all(&redacted, "${1}=[REDACTED]")
            .into_owned();
        token_pattern()
            .replace_all(&redacted, REDACTED)
            .into_owned()
    }

    pub fn contains_sensitive(&self, value: &str) -> bool {
        self.redact(value) != value
    }
}

impl fmt::Debug for SensitiveDataRedactor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SensitiveDataRedactor")
            .field("secret_count", &self.secrets.len())
            .finish()
    }
}

fn json_secret_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r#"(?i)("(?:api[_-]?key|apikey|password|refresh[_-]?token|access[_-]?token|secret|authorization)"\s*:\s*")[^"]*(")"#,
        )
        .expect("the JSON secret redaction pattern is valid")
    })
}

fn header_secret_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?im)\b(authorization|proxy-authorization|x-api-key|api-key|cookie|set-cookie)([ \t]*:[ \t]*)[^\r\n]+",
        )
        .expect("the header secret redaction pattern is valid")
    })
}

fn query_secret_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?i)([?&](?:api[_-]?key|apikey|password|refresh[_-]?token|access[_-]?token|token|secret))=([^&\s]+)",
        )
        .expect("the query secret redaction pattern is valid")
    })
}

fn token_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)\bsk-(?:proj|ant)-[A-Za-z0-9._-]+")
            .expect("the provider-token redaction pattern is valid")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_dynamic_and_structured_secrets_across_output_surfaces() {
        let redactor = SensitiveDataRedactor::new([
            SecretString::new("opaque-local-token"),
            SecretString::new("custom-header-value"),
        ]);
        let samples = [
            "provider error: opaque-local-token was rejected",
            "diagnostic: Authorization: Bearer arbitrary-value",
            r#"export: {"api_key":"sk-proj-exported"}"#,
            "crash: https://example.test/?access_token=custom-header-value",
            "response: sk-ant-crash-token",
        ];

        for sample in samples {
            let redacted = redactor.redact(sample);
            assert!(redacted.contains(REDACTED));
            assert!(!redacted.contains("opaque-local-token"));
            assert!(!redacted.contains("arbitrary-value"));
            assert!(!redacted.contains("sk-proj-exported"));
            assert!(!redacted.contains("custom-header-value"));
            assert!(!redacted.contains("sk-ant-crash-token"));
        }
    }

    #[test]
    fn debug_output_never_contains_registered_secrets() {
        let redactor = SensitiveDataRedactor::new([SecretString::new("never-print-this-secret")]);
        let debug = format!("{redactor:?}");

        assert_eq!(debug, "SensitiveDataRedactor { secret_count: 1 }");
        assert!(!debug.contains("never-print-this-secret"));
    }

    #[test]
    fn ordinary_code_assignments_are_not_mistaken_for_url_secrets() {
        let redactor = SensitiveDataRedactor::default();
        assert_eq!(redactor.redact("let token=blue;"), "let token=blue;");
    }
}
