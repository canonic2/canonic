//! Apple report identity and format checks shared by ingestion and symbol matching.
use crate::error::{Error, Result};
use serde_json::Value;
pub(crate) fn uuid(value: &str) -> Option<String> {
    let s = value.replace('-', "").to_ascii_uppercase();
    (s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())).then_some(s)
}
pub(crate) fn architecture(value: &str) -> bool {
    matches!(
        value,
        "arm64" | "arm64e" | "x86_64" | "armv7" | "armv7s" | "i386"
    )
}
pub(crate) fn json(bytes: &[u8]) -> Result<(Value, Value)> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::new("INVALID_DIAGNOSTIC_FORMAT", "Apple JSON must be UTF-8"))?
        .trim_start_matches('\u{feff}');
    if let Some((header, body)) = text.split_once('\n')
        && let Ok(h) = super::super::analysis::apple_json(header.as_bytes())
        && h.get("bug_type").is_some()
        && body.trim_start().starts_with('{')
    {
        return Ok((h, super::super::analysis::apple_json(body.as_bytes())?));
    }
    Ok((
        Value::Null,
        super::super::analysis::apple_json(text.as_bytes())?,
    ))
}
pub(crate) fn app(header: &Value, body: &Value) -> Option<String> {
    body["bundleInfo"]["CFBundleIdentifier"]
        .as_str()
        .or_else(|| header["bundleID"].as_str())
        .or_else(|| body["bundleIdentifier"].as_str())
        .filter(|s| super::bundle(s))
        .map(str::to_string)
}
