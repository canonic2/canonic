//! Optional evidence redaction and current-clock sampling; calendar math is shared.
pub(crate) use canonic_registry::timestamp::format_utc;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn redact(value: &mut Value) -> bool {
    match value {
        Value::Object(fields) => {
            let mut changed = false;
            for (key, value) in fields {
                if sensitive_key(key) && !value.is_null() {
                    *value = json!("[REDACTED]");
                    changed = true;
                } else {
                    changed |= redact(value);
                }
            }
            changed
        }
        Value::Array(values) => {
            let mut changed = false;
            let mut next_secret = false;
            for value in values {
                if next_secret {
                    *value = json!("[REDACTED]");
                    changed = true;
                    next_secret = false;
                } else {
                    next_secret = value.as_str().is_some_and(|s| {
                        s.starts_with("--") && sensitive_key(s.trim_start_matches('-'))
                    });
                    changed |= redact(value);
                }
            }
            changed
        }
        Value::String(text) => {
            let mut changed = false;
            let mut next_secret = false;
            let words: Vec<_> = text
                .split_whitespace()
                .map(|word| {
                    if next_secret {
                        next_secret = false;
                        changed = true;
                        return "[REDACTED]".into();
                    }
                    next_secret =
                        word.starts_with("--") && sensitive_key(word.trim_start_matches('-'));
                    if let Some((key, _)) = word.split_once('=')
                        && sensitive_key(key.trim_start_matches('-'))
                    {
                        changed = true;
                        return format!("{key}=[REDACTED]");
                    }
                    word.to_owned()
                })
                .collect();
            if changed {
                *text = words.join(" ");
            }
            changed
        }
        _ => false,
    }
}

pub(crate) fn sensitive_key(key: &str) -> bool {
    // Compare the normalized suffix in place: rule validation visits every
    // option name, so allocating normalized keys and suffixes repeats per flag.
    let key = key.as_bytes();
    [
        "password",
        "passwd",
        "token",
        "secret",
        "authorization",
        "api_key",
        "access_key",
        "private_key",
    ]
    .iter()
    .any(|field| {
        let Some(start) = key.len().checked_sub(field.len()) else {
            return false;
        };
        (start == 0 || matches!(key[start - 1], b'_' | b'-'))
            && key[start..]
                .iter()
                .zip(field.bytes())
                .all(|(&byte, expected)| {
                    let normalized = if byte == b'-' {
                        b'_'
                    } else {
                        byte.to_ascii_lowercase()
                    };
                    normalized == expected
                })
    })
}

// Gregorian conversion keeps timestamp formatting independent of locale/dependencies.
pub(crate) fn now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_utc(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_names_require_exact_or_separated_suffixes() {
        for name in [
            "password",
            "PASSWD",
            "Token",
            "secret",
            "AUTHORIZATION",
            "api_key",
            "access-key",
            "private-key",
            "ACME_PASSWORD",
            "acme-passwd",
            "acme_token",
            "Acme-SeCrEt",
            "acme-AUTHORIZATION",
            "acme-API-KEY",
            "ACME_ACCESS_KEY",
            "acme_PRIVATE-key",
            "é-token",
        ] {
            assert!(sensitive_key(name), "{name}");
        }
        for name in [
            "",
            "acme-development-option-127",
            "notatoken",
            "tokenizer",
            "api__key",
            "api.key",
            "acme-token-extra",
            "acmeprivate-key",
            "étoken",
            "🔑",
        ] {
            assert!(!sensitive_key(name), "{name}");
        }
        let mut safe = json!({"flags":["--acme-development-option-127", "inspect"],
            "text":"  Acme\tinspect\n--verbose  "});
        let original = safe.clone();
        assert!(!redact(&mut safe));
        assert_eq!(safe, original);
    }

    #[test]
    fn environment_credentials_and_named_cli_values_are_redacted_without_losing_safe_context() {
        let mut value = json!({"environment":{"GITHUB_TOKEN":"private","AWS_SECRET_ACCESS_KEY":"private","PATH":"/bin"},"arguments":["--access-token","private","read"],"command":"acme --api-key=private"});
        assert!(redact(&mut value));
        assert!(!value.to_string().contains("private"));
        assert_eq!(value["environment"]["PATH"], "/bin");
        assert_eq!(value["arguments"][2], "read");
    }
}
