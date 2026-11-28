//! Ordinary metadata redaction; explicit content reads retain their private scope.
use serde_json::Value;
use std::sync::OnceLock;

pub(super) fn ordinary(mut value: Value) -> Value {
    fn visit(value: &mut Value) {
        match value {
            Value::String(text) => *text = redact(text),
            Value::Array(items) => items.iter_mut().for_each(visit),
            Value::Object(object) => object.values_mut().for_each(visit),
            _ => {}
        }
    }
    visit(&mut value);
    value
}
fn redact(text: &str) -> String {
    static ENDPOINT: OnceLock<regex::Regex> = OnceLock::new();
    static CREDENTIAL: OnceLock<regex::Regex> = OnceLock::new();
    let endpoint = ENDPOINT
        .get_or_init(|| regex::Regex::new(r#"(?i)[a-z][a-z0-9+.-]*://[^\s<>"']+"#).unwrap());
    let credential=CREDENTIAL.get_or_init(||regex::Regex::new(r#"(?i)\b(?:authorization|proxy-authorization|cookie|set-cookie|password|passwd|token|access_token|refresh_token|api[-_]?key|secret|credential)\s*[:=]\s*[^\r\n,;]+"#).unwrap());
    let without_endpoints = endpoint.replace_all(text, "[redacted-endpoint]");
    credential
        .replace_all(&without_endpoints, "[redacted-credential]")
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_ordinary_metadata_omits_endpoints_and_named_credentials() {
        let raw = serde_json::json!({"question":"Inspect wss://user:password@example.com/private-token?auth=secret then token=private-fixture","binding":{"runtimeId":"http://example.com/authenticated/path"},"other":"Acme"});
        let result = ordinary(raw);
        let text = result.to_string();
        for secret in [
            "user:password",
            "private-token",
            "auth=secret",
            "private-fixture",
            "authenticated/path",
        ] {
            assert!(!text.contains(secret));
        }
        assert_eq!(result["other"], "Acme");
    }
}
