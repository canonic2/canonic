//! Secret values, digests and the URL, form, cookie and response encodings the
//! authorization server's endpoints share.

use super::store::Digest;
use crate::http::Response;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{borrow::Cow, collections::HashMap};

/// Random bytes in every secret value.
const SECRET_BYTES: usize = 32;

/// A fresh secret value: `prefix` and 256 random bits in base64url.
pub(super) fn secret(prefix: &str) -> Result<String, getrandom::Error> {
    let mut bytes = [0; SECRET_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(format!("{prefix}{}", base64url(&bytes)))
}

/// A fresh random identifier without a prefix, 128 bits in base64url.
pub(super) fn identifier() -> Result<String, getrandom::Error> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes)?;
    Ok(base64url(&bytes))
}

/// The store's digest of a secret value: SHA-256 over the whole value, whose
/// type prefix keeps digests of different kinds apart.
pub(super) fn digest(value: &str) -> Digest {
    Digest(Sha256::digest(value.as_bytes()).into())
}

/// Unpadded base64url.
pub(super) fn base64url(bytes: &[u8]) -> String {
    crate::content::base64(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

/// The S256 challenge of a PKCE verifier.
pub(super) fn challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// RFC 3986 unreserved characters only.
pub(super) fn unreserved(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
}

/// Decoded `application/x-www-form-urlencoded` pairs, refusing repeated names,
/// invalid percent-encoding and invalid UTF-8.
pub(super) fn form(text: &str) -> Option<HashMap<String, String>> {
    let mut pairs = HashMap::new();
    for pair in text.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if pairs
            .insert(decode_component(name)?, decode_component(value)?)
            .is_some()
        {
            return None;
        }
    }
    Some(pairs)
}

/// Percent-decodes a form component, with `+` as a space.
pub(super) fn decode_component(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut input = text.bytes();
    while let Some(byte) = input.next() {
        bytes.push(match byte {
            b'+' => b' ',
            b'%' => {
                let high = char::from(input.next()?).to_digit(16)?;
                let low = char::from(input.next()?).to_digit(16)?;
                u8::try_from(high * 16 + low).ok()?
            }
            byte => byte,
        });
    }
    String::from_utf8(bytes).ok()
}

/// Percent-encodes a query component, keeping unreserved characters.
pub(super) fn encode_component(text: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// `uri` with `pairs` appended to its query.
pub(super) fn with_query(uri: &str, pairs: &[(&str, &str)]) -> String {
    let query = pairs
        .iter()
        .map(|(name, value)| format!("{name}={}", encode_component(value)))
        .collect::<Vec<_>>()
        .join("&");
    let separator = if uri.contains('?') { '&' } else { '?' };
    format!("{uri}{separator}{query}")
}

/// The value of cookie `name` in the request's `Cookie` fields.
pub(super) fn cookie<'a>(headers: &[(&str, &'a [u8])], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .filter(|(field, _)| field.eq_ignore_ascii_case("cookie"))
        .filter_map(|(_, value)| std::str::from_utf8(value).ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then_some(value)
        })
}

/// `Cookie` fields without the cookies `drop` names.
pub(super) fn without_cookies(
    headers: &[(&str, &[u8])],
    drop: impl Fn(&str) -> bool,
) -> Vec<(String, Vec<u8>)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            if !name.eq_ignore_ascii_case("cookie") {
                return Some(((*name).to_owned(), value.to_vec()));
            }
            let kept = std::str::from_utf8(value)
                .ok()?
                .split(';')
                .map(str::trim)
                .filter(|pair| !pair.split('=').next().is_some_and(&drop))
                .collect::<Vec<_>>()
                .join("; ");
            (!kept.is_empty()).then(|| ((*name).to_owned(), kept.into_bytes()))
        })
        .collect()
}

/// A JSON response for a token, revocation or registration endpoint.
pub(super) fn json(status: u16, body: &Value) -> Response {
    let mut response = Response::json(status, serde_json::to_vec(body).unwrap_or_default());
    response.headers.push(("Pragma", "no-cache".into()));
    response
        .headers
        .push(("Access-Control-Allow-Origin", "*".into()));
    response
}

/// An RFC 6749 error response.
pub(super) fn oauth_error(status: u16, error: &str, description: &str) -> Response {
    json(
        status,
        &json!({"error": error, "error_description": description}),
    )
}

/// A plain-text page shown instead of redirecting to an unverified URI.
pub(super) fn page(status: u16, text: &str) -> Response {
    Response {
        status,
        headers: vec![
            ("Content-Type", "text/plain; charset=utf-8".into()),
            ("Cache-Control", "no-store".into()),
        ],
        body: text.as_bytes().to_vec(),
    }
}

/// A `303 See Other` to `location`.
pub(super) fn redirect(location: String) -> Response {
    Response {
        status: 303,
        headers: vec![
            ("Location", Cow::Owned(location)),
            ("Cache-Control", "no-store".into()),
        ],
        body: Vec::new(),
    }
}
