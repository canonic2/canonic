//! Bounded private HTTP evidence and conservative header disclosure.
use super::contract::Reference;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use uuid::Uuid;
pub(crate) mod response;
pub const MAX_BODY: usize = 1024 * 1024;
/// The refusal for a request body sent during trusted sign-in entry, which
/// can carry the entered values.
pub(crate) fn body_withheld() -> super::session::Error {
    let mut error = super::session::Error::from("BODY_WITHHELD");
    error.partial = Some(
        json!({"reason":"trusted_sign_in","valuesReturned":false,"nextAction":"read the request without body"}),
    );
    error
}
pub(crate) fn decode_payload(
    body: &str,
    base64: bool,
    max: usize,
) -> Result<Vec<u8>, super::session::Error> {
    if base64 {
        use base64::{Engine, engine::general_purpose::STANDARD};
        if body.len() > max.div_ceil(3) * 4 {
            return Err("BODY_BOUNDS".into());
        }
        let bytes = STANDARD
            .decode(body)
            .map_err(|_| super::session::Error::from("BACKEND_PROTOCOL"))?;
        if bytes.len() > max {
            return Err("BODY_BOUNDS".into());
        }
        Ok(bytes)
    } else {
        if body.len() > max {
            return Err("BODY_BOUNDS".into());
        }
        Ok(body.as_bytes().to_vec())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodySide {
    Request,
    Response,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Read {
    #[serde(default)]
    pub headers: bool,
    #[serde(default)]
    pub response_details: bool,
    /// Private, unmasked request and response header arrays in observed order.
    #[serde(default)]
    pub raw_headers: bool,
    #[serde(default)]
    pub body: Option<BodySide>,
    #[serde(default = "body_limit")]
    pub max_bytes: usize,
}
fn body_limit() -> usize {
    65536
}
impl Default for Read {
    fn default() -> Self {
        Self {
            headers: false,
            response_details: false,
            raw_headers: false,
            body: None,
            max_bytes: body_limit(),
        }
    }
}
impl Read {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if !(1..=MAX_BODY).contains(&self.max_bytes) {
            Err("BODY_BOUNDS")
        } else {
            Ok(())
        }
    }
}

/// Only these values are disclosed. Unknown/custom authentication headers stay
/// masked even when their names do not resemble a credential.
pub(crate) fn headers(value: &Value) -> Value {
    let pairs: Vec<(String, String)> = if let Some(map) = value.as_object() {
        map.iter()
            .filter_map(|(name, value)| value.as_str().map(|value| (name.clone(), value.into())))
            .collect()
    } else {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                Some((
                    entry["name"].as_str()?.into(),
                    entry["value"].as_str()?.into(),
                ))
            })
            .collect()
    };
    let mut budget = 0;
    let mut entries = Vec::new();
    let mut truncated = false;
    for (name, value) in pairs {
        let name = super::diagnostics::text(&name.to_ascii_lowercase()).0;
        let allowed = matches!(
            name.as_str(),
            "content-type"
                | "content-length"
                | "content-encoding"
                | "accept"
                | "accept-language"
                | "cache-control"
                | "date"
                | "vary"
        );
        let (value, cut) = if allowed {
            super::diagnostics::text(&value)
        } else {
            ("[redacted]".into(), false)
        };
        let size = name.len() + value.len() + 64;
        if entries.len() == 64 || budget + size > 8192 {
            truncated = true;
            break;
        }
        entries.push(json!({"name":name,"value":value,"redacted":!allowed,"truncated":cut}));
        budget += size;
    }
    json!({"entries":entries,"truncated":truncated,"policy":"allowlisted-metadata-values"})
}
/// Most raw header entries and bytes one read discloses.
const MAX_RAW_HEADERS: usize = 256;
const MAX_RAW_HEADER_BYTES: usize = 65536;
/// Unmasked `{name, value}` pairs in observed order, as the SDK's
/// `headersArray()`. Object-shaped CDP headers keep their key order; a value
/// joined with newlines becomes separate entries, as Chromium joins duplicates.
pub(crate) fn raw_headers(value: &Value) -> Value {
    let pairs: Vec<(String, String)> = if let Some(map) = value.as_object() {
        map.iter()
            .filter_map(|(name, value)| value.as_str().map(|value| (name.clone(), value)))
            .flat_map(|(name, value)| {
                value
                    .split('\n')
                    .map(|part| (name.clone(), part.to_owned()))
                    .collect::<Vec<_>>()
            })
            .collect()
    } else {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                Some((
                    entry["name"].as_str()?.into(),
                    entry["value"].as_str()?.into(),
                ))
            })
            .collect()
    };
    let mut budget = 0;
    let mut entries = Vec::new();
    let mut truncated = false;
    for (name, value) in pairs {
        if entries.len() == MAX_RAW_HEADERS
            || budget + name.len() + value.len() > MAX_RAW_HEADER_BYTES
        {
            truncated = true;
            break;
        }
        budget += name.len() + value.len();
        entries.push(json!({"name":name,"value":value}));
    }
    json!({"entries":entries,"truncated":truncated,"private":true})
}
fn headers_size(headers: &Value) -> usize {
    headers["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| {
            entry["name"].as_str().map_or(0, str::len)
                + entry["value"].as_str().map_or(0, str::len)
                + 4
        })
        .sum()
}
/// Pinned SDK `requestHeadersSize`: request line plus raw headers.
pub(crate) fn request_headers_size(raw: &Value, url: &str, method: &str) -> usize {
    let path = url::Url::parse(url).map_or(0, |url| url.path().len());
    4 + method.len() + path + 8 + headers_size(raw)
}
/// Pinned SDK `responseHeadersSize`: status line plus raw headers.
pub(crate) fn response_headers_size(raw: &Value, status_text: &str) -> usize {
    4 + 8 + 3 + status_text.len() + headers_size(raw) + 2
}
/// Pinned SDK `Response.sizes()`. A reported header byte count wins over the
/// computed status line; a reported transfer size gives the encoded body size.
pub(crate) fn sizes(
    request_body: usize,
    request_headers: usize,
    raw_response: &Value,
    status_text: &str,
    reported_response_headers: Option<usize>,
    transfer: Option<usize>,
) -> Value {
    let response_headers = reported_response_headers
        .unwrap_or_else(|| response_headers_size(raw_response, status_text));
    let body = transfer.map_or_else(
        || {
            raw_response["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|entry| {
                    entry["name"]
                        .as_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case("content-length"))
                })
                .and_then(|entry| entry["value"].as_str()?.parse::<usize>().ok())
                .unwrap_or(0)
        },
        |transfer| transfer.saturating_sub(response_headers),
    );
    json!({"requestBodySize":request_body,"requestHeadersSize":request_headers,"responseBodySize":body,"responseHeadersSize":response_headers,"transferSize":transfer.unwrap_or(response_headers + body)})
}
pub(crate) fn header<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    if let Some(map) = value.as_object() {
        map.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .and_then(|(_, value)| value.as_str())
    } else {
        value
            .as_array()?
            .iter()
            .find(|entry| {
                entry["name"]
                    .as_str()
                    .is_some_and(|key| key.eq_ignore_ascii_case(name))
            })
            .and_then(|entry| entry["value"].as_str())
    }
}
pub(crate) fn bounded_identity_length(headers: &Value, max: usize) -> Result<usize, &'static str> {
    if header(headers, "content-encoding")
        .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("identity"))
    {
        return Err("BODY_DECOMPRESSION_BOUND_UNAVAILABLE");
    }
    let size = header(headers, "content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or("BODY_SIZE_UNAVAILABLE")?;
    if size > max {
        return Err("BODY_BOUNDS");
    }
    Ok(size)
}
use super::artifacts::{Artifact, Format};
#[derive(Default)]
pub(crate) struct Evidence {
    items: BTreeMap<Uuid, Artifact>,
}
impl Evidence {
    pub(crate) fn capacity(&self) -> Result<(), super::session::Error> {
        if self.items.len() >= 32 {
            Err("NETWORK_EVIDENCE_CAPACITY".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn insert(&mut self, artifact: Artifact) -> Result<Value, super::session::Error> {
        self.capacity()?;
        let id = serde_json::from_value(artifact.summary["evidenceId"].clone())
            .map_err(|_| super::session::Error::from("BACKEND_PROTOCOL"))?;
        let summary = artifact.summary.clone();
        self.items.insert(id, artifact);
        Ok(summary)
    }
    pub(crate) fn get(&self, id: Uuid) -> Result<Artifact, super::session::Error> {
        self.items
            .get(&id)
            .cloned()
            .ok_or("UNKNOWN_NETWORK_EVIDENCE".into())
    }
    pub(crate) fn forget(&mut self, id: Uuid) {
        self.items.remove(&id);
    }
    pub(crate) fn list(&self) -> Value {
        json!(
            self.items
                .values()
                .map(|item| item.summary.clone())
                .collect::<Vec<_>>()
        )
    }
}
pub(crate) fn write_body(
    directory: &Path,
    context: Reference,
    request: Uuid,
    side: BodySide,
    bytes: &[u8],
    max: usize,
) -> Result<Artifact, super::session::Error> {
    if bytes.len() > max || bytes.len() > MAX_BODY {
        return Err("BODY_BOUNDS".into());
    }
    Format{prefix:"network",extension:"bin",mime:"application/octet-stream",name:"decoded-http-payload",max_bytes:MAX_BODY}.write(directory,bytes,json!({"context":context,"requestId":request,"side":side,"redacted":false,"privacy":"private-explicit-body-read","truncated":false}))
}
pub(crate) fn remove_body(item: &Artifact) -> Result<(), super::session::Error> {
    item.remove()
}
pub(crate) fn evidence_body(item: &Artifact) -> Result<Vec<u8>, super::session::Error> {
    if item.summary["format"] != "decoded-http-payload" {
        return Err("BODY_EVIDENCE_FORMAT".into());
    }
    if item.summary["attachment"]["bytes"]
        .as_u64()
        .is_none_or(|size| size > MAX_BODY as u64)
    {
        return Err("BODY_BOUNDS".into());
    }
    item.read()
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    vec![
        json!({"type":"object","properties":{"operation":{"const":"network_read"},"context":reference,"requestId":{"type":"string","format":"uuid"},"options":{"type":"object","properties":{"headers":{"type":"boolean","default":false},"responseDetails":{"type":"boolean","default":false,"description":"Include private observed server address, TLS certificate summary, pinned request timing, HTTP version, status text and sizes; unknown fields remain explicit"},"rawHeaders":{"type":"boolean","default":false,"description":"Include private unmasked request and response headers in observed order, including cookies and credentials"},"body":{"enum":["request","response",null]},"maxBytes":{"type":"integer","minimum":1,"maximum":MAX_BODY,"default":65536}},"additionalProperties":false}},"required":["operation","context","requestId"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"network_evidence_list"}},"required":["operation"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"network_evidence_remove"},"evidenceId":{"type":"string","format":"uuid"}},"required":["operation","evidenceId"],"additionalProperties":false}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_headers_keep_order_and_secrets_and_sizes_follow_the_pinned_sdk_arithmetic() {
        // Chromium joins duplicate values with newlines; raw headers split them.
        // A parsed CDP header object is in name order.
        let raw = raw_headers(&json!({"Set-Cookie":"a=1\nb=2","Authorization":"Bearer Acme"}));
        assert_eq!(
            raw["entries"],
            json!([{"name":"Authorization","value":"Bearer Acme"},{"name":"Set-Cookie","value":"a=1"},{"name":"Set-Cookie","value":"b=2"}])
        );
        // Array-shaped driver headers keep their order.
        let ordered = raw_headers(&json!([{"name":"Z","value":"1"},{"name":"A","value":"2"}]));
        assert_eq!(ordered["entries"][0]["name"], "Z");
        assert_eq!(raw["private"], true);
        let request = raw_headers(&json!([{"name":"X-Acme","value":"one"}]));
        // 4 + "GET" + "/a" + 8 + ("X-Acme" + "one" + 4)
        assert_eq!(
            request_headers_size(&request, "https://example.com/a?q=1", "GET"),
            4 + 3 + 2 + 8 + 13
        );
        let response = raw_headers(&json!([{"name":"Content-Length","value":"10"}]));
        // 4 + 8 + 3 + "OK" + ("Content-Length" + "10" + 4) + 2
        let computed = 4 + 8 + 3 + 2 + 20 + 2;
        assert_eq!(response_headers_size(&response, "OK"), computed);
        // Without reported sizes the body follows Content-Length.
        assert_eq!(
            sizes(5, 30, &response, "OK", None, None),
            json!({"requestBodySize":5,"requestHeadersSize":30,"responseBodySize":10,"responseHeadersSize":computed,"transferSize":computed+10})
        );
        // A reported header text length and transfer size take precedence.
        assert_eq!(
            sizes(0, 30, &response, "OK", Some(50), Some(80)),
            json!({"requestBodySize":0,"requestHeadersSize":30,"responseBodySize":30,"responseHeadersSize":50,"transferSize":80})
        );
        let many = json!(
            (0..300)
                .map(|index| json!({"name":format!("x-{index}"),"value":"v"}))
                .collect::<Vec<_>>()
        );
        assert_eq!(raw_headers(&many)["truncated"], true);
        assert_eq!(
            raw_headers(&many)["entries"].as_array().unwrap().len(),
            MAX_RAW_HEADERS
        );
    }
    #[test]
    fn header_reads_mask_unknown_credentials_and_bound_decompression_before_body_calls() {
        let source = json!([{"name":"Authorization","value":"Acme-secret"},{"name":"x-custom","value":"Acme-secret"},{"name":"Content-Length","value":"12"}]);
        let summary = headers(&source);
        assert!(!summary.to_string().contains("Acme-secret"));
        assert_eq!(bounded_identity_length(&source, 32), Ok(12));
        assert_eq!(bounded_identity_length(&source, 8), Err("BODY_BOUNDS"));
        assert_eq!(
            bounded_identity_length(
                &json!({"content-length":"12","content-encoding":"gzip"}),
                32
            ),
            Err("BODY_DECOMPRESSION_BOUND_UNAVAILABLE")
        );
    }
    #[test]
    fn body_evidence_is_private_bounded_and_changed_files_cannot_be_deleted() {
        let root = tempfile::tempdir().unwrap();
        let context = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let mut store = Evidence::default();
        let artifact = write_body(
            root.path(),
            context,
            Uuid::new_v4(),
            BodySide::Response,
            b"Acme",
            8,
        )
        .unwrap();
        let body = store.insert(artifact).unwrap();
        let id = serde_json::from_value(body["evidenceId"].clone()).unwrap();
        let path = body["attachment"]["path"].as_str().unwrap();
        std::fs::write(path, b"Edit").unwrap();
        assert_eq!(
            remove_body(&store.get(id).unwrap()).unwrap_err().code,
            "EVIDENCE_CHANGED"
        );
        assert!(Path::new(path).exists());
        assert!(
            write_body(
                root.path(),
                context,
                Uuid::new_v4(),
                BodySide::Request,
                b"oversized",
                4
            )
            .is_err()
        );
    }
}
