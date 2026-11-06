//! Optional evidence redaction and current-clock sampling; calendar math is shared.
pub(crate) use canonic_registry_kit::timestamp::format_utc;
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
            // Most strings contain no credentials. Scan borrowed words before
            // allocating the rewritten projection, preserving safe whitespace.
            let mut next_secret = false;
            let contains_secret = text.split_whitespace().any(|word| {
                if next_secret {
                    return true;
                }
                next_secret = word.starts_with("--") && sensitive_key(word.trim_start_matches('-'));
                word.split_once('=')
                    .is_some_and(|(key, _)| sensitive_key(key.trim_start_matches('-')))
            });
            if !contains_secret {
                return false;
            }
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

/// Preserve only receipt slots selected by the core owner of this projection.
/// Callers must never derive these JSON pointers from submitted agent input.
pub(crate) fn redact_with_receipts(value: &mut Value, paths: &[String]) -> bool {
    let mut receipts = Vec::new();
    let mut changed = false;
    for path in paths {
        if let Some(slot) = value.pointer_mut(path)
            && session_authorization(slot)
        {
            let mut receipt = slot.take();
            changed |= redact(&mut receipt);
            receipts.push((path, receipt));
        }
    }
    changed |= redact(value);
    for (path, receipt) in receipts {
        if let Some(slot) = value.pointer_mut(path) {
            *slot = receipt;
        }
    }
    changed
}

// This field is a bounded receipt, not a credential-bearing header. Require
// its complete metadata shape; ordinary Authorization values stay secret.
fn session_authorization(value: &Value) -> bool {
    let Some(fields) = value.as_object() else {
        return false;
    };
    if fields
        .keys()
        .any(|k| !["kind", "grants", "truncated", "missing"].contains(&k.as_str()))
        || value["kind"] != "session_grant"
    {
        return false;
    }
    value["grants"].as_array().is_some_and(|grants| {
        grants.len() <= 16
            && grants.iter().all(|grant| {
                grant.as_object().is_some_and(|o| {
                    o.keys().all(|k| {
                        [
                            "grant_id",
                            "approval_id",
                            "operation_refs",
                            "override_denies",
                        ]
                        .contains(&k.as_str())
                    })
                }) && ["grant_id", "approval_id"]
                    .iter()
                    .all(|k| grant[*k].as_str().is_some_and(|s| s.len() <= 128))
                    && grant.get("operation_refs").is_none_or(|v| {
                        v.as_array().is_some_and(|a| {
                            a.len() <= 256
                                && a.iter().all(|s| s.as_str().is_some_and(|s| s.len() <= 128))
                        })
                    })
                    && grant
                        .get("override_denies")
                        .is_none_or(|v| v.as_array().is_some_and(|a| a.len() <= 32))
            })
    })
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
    fn session_grants_metadata_survives_redaction_but_authorization_credentials_do_not() {
        let receipt = json!({"kind":"session_grant","grants":[{"grant_id":"session-grant-acme-1","approval_id":"approval-acme-2","operation_refs":["shell:0:0-10"],"override_denies":[]}]});
        let mut value =
            json!({"authorization":receipt,"headers":{"Authorization":"Bearer private"}});
        assert!(redact_with_receipts(&mut value, &["/authorization".into()]));
        assert_eq!(value["authorization"], receipt);
        assert_eq!(value["headers"]["Authorization"], "[REDACTED]");
        let mut fake =
            json!({"authorization":{"kind":"session_grant","grants":[],"password":"private"}});
        redact(&mut fake);
        assert_eq!(fake["authorization"], "[REDACTED]");
        let mut submitted = json!({"original_input":{"authorization":receipt}});
        assert!(redact_with_receipts(
            &mut submitted,
            &["/authorization".into()]
        ));
        assert_eq!(submitted["original_input"]["authorization"], "[REDACTED]");
        let mut untrusted = json!({"authorization":receipt});
        assert!(redact(&mut untrusted));
        assert_eq!(untrusted["authorization"], "[REDACTED]");
    }

    #[test]
    fn text_redaction_preserves_safe_whitespace_and_existing_secret_projection() {
        for (source, expected, changed) in [
            ("  safe\twords\n", "  safe\twords\n", false),
            ("--token", "--token", false),
            ("--token   hidden", "--token [REDACTED]", true),
            ("safe\u{2003}words", "safe\u{2003}words", false),
            (
                "safe\u{2003}--api-key=hidden",
                "safe --api-key=[REDACTED]",
                true,
            ),
            ("--token --secret value", "--token [REDACTED] value", true),
            (
                "authorization=hidden safe",
                "authorization=[REDACTED] safe",
                true,
            ),
            (
                "--token=hidden --verbose",
                "--token=[REDACTED] --verbose",
                true,
            ),
        ] {
            let mut value = json!(source);
            assert_eq!(redact(&mut value), changed, "{source}");
            assert_eq!(value, expected);
        }
    }

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
