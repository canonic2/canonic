//! Exact HTTP URL interception and bounded synthetic responses.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub(crate) mod ecma;
mod handlers;
mod matcher;
mod overrides;
mod regexp;
mod rules;
mod urlpattern;
#[cfg(test)]
pub(crate) use handlers::ClearBehavior;
pub use handlers::HandlerAction;
pub(crate) use handlers::{Handlers, HarHandler};
pub(crate) use matcher::Compiled;
pub use matcher::Matcher;
pub(crate) use matcher::{join as join_base, normalize as normalize_glob};
pub use overrides::Overrides;
pub(crate) use regexp::translate as translate_regex;
pub(crate) use rules::{AutoRule, HarPhase, Rules, ScopeKind as HarScope};
pub use urlpattern::UrlPattern;
pub(crate) const MAX_BINARY_BODY: usize = 1024 * 1024;
pub(crate) const MAX_INLINE_BODY: usize = 65536;

/// Pinned `route.abort(errorCode)` codes; omission means `failed`, as in the SDK.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AbortCode {
    Aborted,
    Accessdenied,
    Addressunreachable,
    Blockedbyclient,
    Blockedbyresponse,
    Connectionaborted,
    Connectionclosed,
    Connectionfailed,
    Connectionrefused,
    Connectionreset,
    Internetdisconnected,
    Namenotresolved,
    Timedout,
    #[default]
    Failed,
}
impl AbortCode {
    pub(crate) const ALL: [&'static str; 14] = [
        "aborted",
        "accessdenied",
        "addressunreachable",
        "blockedbyclient",
        "blockedbyresponse",
        "connectionaborted",
        "connectionclosed",
        "connectionfailed",
        "connectionrefused",
        "connectionreset",
        "internetdisconnected",
        "namenotresolved",
        "timedout",
        "failed",
    ];
    /// The SDK driver's `errorCode` value.
    pub(crate) fn code(self) -> &'static str {
        Self::ALL[self as usize]
    }
    /// The pinned Chromium `Fetch.failRequest` `errorReason`.
    pub(crate) fn chrome_reason(self) -> &'static str {
        [
            "Aborted",
            "AccessDenied",
            "AddressUnreachable",
            "BlockedByClient",
            "BlockedByResponse",
            "ConnectionAborted",
            "ConnectionClosed",
            "ConnectionFailed",
            "ConnectionRefused",
            "ConnectionReset",
            "InternetDisconnected",
            "NameNotResolved",
            "TimedOut",
            "Failed",
        ][self as usize]
    }
}
pub(crate) fn inline_response(
    status: u16,
    body: &str,
    headers: &[Header],
) -> Result<BinaryResponse, &'static str> {
    if body.len() > MAX_INLINE_BODY.div_ceil(3) * 4 {
        return Err("ROUTE_BODY_BOUNDS");
    }
    let response = binary_response(status, body, headers)?;
    if response.body_len > MAX_INLINE_BODY {
        return Err("ROUTE_BODY_BOUNDS");
    }
    Ok(response)
}
/// A caller `path` snapshot for `route_fulfill`, read once before dispatch.
#[derive(Clone, Debug)]
pub struct FulfillFile {
    bytes: Vec<u8>,
    mime: &'static str,
}
impl FulfillFile {
    pub(crate) fn parts(&self) -> (&[u8], &str) {
        (&self.bytes, self.mime)
    }
}
/// Pinned `getMimeTypeForPath`: the case-sensitive text after the path's last
/// dot, with `application/octet-stream` when the table has no entry.
fn fulfill_mime(path: &str) -> &'static str {
    static TYPES: std::sync::OnceLock<std::collections::BTreeMap<String, String>> =
        std::sync::OnceLock::new();
    let types = TYPES.get_or_init(|| {
        serde_json::from_str(include_str!("routing/fulfill-mime.json"))
            .expect("Pinned fulfill MIME table")
    });
    path.rfind('.')
        .and_then(|dot| types.get(&path[dot + 1..]))
        .map_or("application/octet-stream", String::as_str)
}
/// Read a frontend-bound absolute `route_fulfill` path as a bounded regular
/// file before the paused request is answered.
pub(crate) async fn read_fulfill_file(
    path: String,
    deadline: tokio::time::Instant,
) -> Result<FulfillFile, &'static str> {
    if !std::path::Path::new(&path).is_absolute() || path.len() > 4096 || path.contains('\0') {
        return Err("ROUTE_FILE_PATH_INVALID");
    }
    let read = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let file = std::fs::File::open(&path).map_err(|_| "ROUTE_FILE_IO")?;
        let metadata = file.metadata().map_err(|_| "ROUTE_FILE_IO")?;
        if !metadata.is_file() {
            return Err("ROUTE_FILE_NOT_REGULAR");
        }
        if metadata.len() > MAX_BINARY_BODY as u64 {
            return Err("ROUTE_BODY_BOUNDS");
        }
        let mut bytes = Vec::new();
        file.take(MAX_BINARY_BODY as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "ROUTE_FILE_IO")?;
        if bytes.len() > MAX_BINARY_BODY {
            return Err("ROUTE_BODY_BOUNDS");
        }
        Ok(FulfillFile {
            bytes,
            mime: fulfill_mime(&path),
        })
    });
    tokio::time::timeout_at(deadline, read)
        .await
        .map_err(|_| "ROUTE_FILE_DEADLINE")?
        .map_err(|_| "ROUTE_FILE_IO")?
}
/// Attach the snapshot of a `route_fulfill` `path`; other replies pass through.
pub(crate) async fn load_fulfill_file(
    mut response: crate::browser::contract::CallbackResponse,
    deadline: tokio::time::Instant,
) -> Result<crate::browser::contract::CallbackResponse, &'static str> {
    if let crate::browser::contract::CallbackResponse::RouteFulfill {
        path: Some(path),
        file,
        ..
    } = &mut response
        && file.is_none()
    {
        *file = Some(read_fulfill_file(path.clone(), deadline).await?);
    }
    Ok(response)
}
/// Pinned `route.fulfill` text/JSON semantics as the binary reply both backends
/// send: `(status, base64 body, headers)`. `file` carries a caller `path`
/// snapshot and its MIME type.
pub(crate) fn text_fulfill(
    status: Option<u16>,
    body: Option<&str>,
    json: Option<&Value>,
    content_type: Option<&str>,
    headers: &[Header],
    file: Option<(&[u8], &str)>,
) -> Result<(u16, String, Vec<Header>), &'static str> {
    if body.is_some() && json.is_some() {
        return Err("ROUTE_FULFILL_BODY_CONFLICT");
    }
    let bytes = match (file, json) {
        (Some((bytes, _)), _) => bytes.to_vec(),
        (None, Some(value)) => serde_json::to_vec(value).map_err(|_| "ROUTE_BODY_ENCODING")?,
        (None, None) => body.unwrap_or_default().as_bytes().to_vec(),
    };
    if bytes.len()
        > if file.is_some() {
            MAX_BINARY_BODY
        } else {
            MAX_INLINE_BODY
        }
    {
        return Err("ROUTE_BODY_BOUNDS");
    }
    // A JavaScript header object: lowercased keys keep their first position and last value.
    let mut result: Vec<Header> = Vec::new();
    for header in headers {
        let name = header.name.to_ascii_lowercase();
        match result.iter_mut().find(|existing| existing.name == name) {
            Some(existing) => existing.value.clone_from(&header.value),
            None => result.push(Header {
                name,
                value: header.value.clone(),
            }),
        }
    }
    let truthy = |value: &Value| match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    };
    let selected = content_type
        .map(str::to_owned)
        .or_else(|| {
            json.filter(|value| truthy(value))
                .map(|_| "application/json".into())
        })
        .or_else(|| file.map(|(_, mime)| mime.to_owned()));
    if let Some(value) = selected {
        match result
            .iter_mut()
            .find(|existing| existing.name == "content-type")
        {
            Some(existing) => existing.value = value,
            None => result.push(Header {
                name: "content-type".into(),
                value,
            }),
        }
    }
    let status = status.filter(|status| *status != 0).unwrap_or(200);
    Ok((status, STANDARD.encode(bytes), result))
}
pub(crate) fn headers_schema() -> Value {
    json!({"type":"array","maxItems":64,"default":[],"items":{"type":"object","properties":{"name":{"type":"string","minLength":1,"maxLength":256},"value":{"type":"string","maxLength":8192}},"required":["name","value"],"additionalProperties":false}})
}
/// The pinned WebKit delegate accepts a header dictionary. Combine fields
/// before that lossy conversion; Set-Cookie uses its upstream newline convention.
pub(crate) fn dictionary_headers(headers: Vec<Header>) -> Vec<Header> {
    let mut result: Vec<Header> = Vec::new();
    let mut positions = std::collections::BTreeMap::new();
    for header in headers {
        if let Some(index) = positions.get(&header.name).copied() {
            let previous: &mut Header = &mut result[index];
            previous.value.push_str(if header.name == "set-cookie" {
                "\n"
            } else {
                ", "
            });
            previous.value.push_str(&header.value);
        } else {
            positions.insert(header.name.clone(), result.len());
            result.push(header);
        }
    }
    result
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub name: String,
    pub value: String,
}
#[derive(Debug)]
pub(crate) struct BinaryResponse {
    status: u16,
    body_len: usize,
    headers: Vec<Header>,
}
impl BinaryResponse {
    pub(crate) fn headers_for(self, method: &str) -> Result<Vec<Header>, &'static str> {
        let metadata_length = method == "HEAD" || self.status == 304;
        let mut length = None;
        let mut headers = Vec::new();
        for header in self.headers {
            if header.name == "content-length" {
                let value = header
                    .value
                    .parse::<usize>()
                    .map_err(|_| "ROUTE_HEADER_INVALID")?;
                if self.status == 204 {
                    return Err("ROUTE_HEADER_INVALID");
                }
                if length.is_some_and(|previous| previous != value)
                    || !metadata_length && value != self.body_len
                {
                    return Err("ROUTE_CONTENT_LENGTH_MISMATCH");
                }
                length = Some(value);
            } else if header.name != "transfer-encoding" {
                headers.push(header);
            }
        }
        if self.status != 204 && (self.status != 304 || length.is_some()) {
            headers.push(Header {
                name: "content-length".into(),
                value: if metadata_length {
                    length.unwrap_or(self.body_len)
                } else {
                    self.body_len
                }
                .to_string(),
            });
        }
        Ok(headers)
    }
}

/// Validate every byte before consuming a route callback. The body is decoded
/// only for its bound/length; adapters send the original validated base64.
pub(crate) fn binary_response(
    status: u16,
    body: &str,
    headers: &[Header],
) -> Result<BinaryResponse, &'static str> {
    if !(200..=599).contains(&status) {
        return Err("ROUTE_RESPONSE_BOUNDS");
    }
    if body.len() > MAX_BINARY_BODY.div_ceil(3) * 4 {
        return Err("ROUTE_BODY_BOUNDS");
    }
    let bytes = STANDARD.decode(body).map_err(|_| "ROUTE_BODY_ENCODING")?;
    if bytes.len() > MAX_BINARY_BODY {
        return Err("ROUTE_BODY_BOUNDS");
    }
    if matches!(status, 204 | 205 | 304) && !bytes.is_empty() {
        return Err("ROUTE_RESPONSE_BODY_FORBIDDEN");
    }
    if headers.len() > 64 {
        return Err("ROUTE_HEADER_BOUNDS");
    }
    let mut size = 0;
    let mut result = Vec::new();
    for header in headers {
        size += header.name.len() + header.value.len() + 32;
        if size > 8192 || header.name.len() > 256 || header.value.len() > 8192 {
            return Err("ROUTE_HEADER_BOUNDS");
        }
        let name = axum::http::HeaderName::from_bytes(header.name.as_bytes())
            .map_err(|_| "ROUTE_HEADER_INVALID")?;
        header
            .value
            .parse::<axum::http::HeaderValue>()
            .map_err(|_| "ROUTE_HEADER_INVALID")?;
        if name == axum::http::header::CONTENT_LENGTH {
            header
                .value
                .parse::<usize>()
                .map_err(|_| "ROUTE_HEADER_INVALID")?;
            if status == 204 {
                return Err("ROUTE_HEADER_INVALID");
            }
        }
        result.push(Header {
            name: name.as_str().into(),
            value: header.value.clone(),
        });
    }
    Ok(BinaryResponse {
        status,
        body_len: bytes.len(),
        headers: result,
    })
}
pub(crate) fn validate_url(url: &str) -> Result<(), &'static str> {
    if url.len() > 2048
        || !url.is_ascii()
        || url.chars().any(char::is_control)
        || url.contains(['*', '#', '\\'])
        || !(url.starts_with("http://") || url.starts_with("https://"))
    {
        return Err("ROUTE_URL_INVALID");
    }
    let uri = url
        .parse::<axum::http::Uri>()
        .map_err(|_| "ROUTE_URL_INVALID")?;
    if uri.authority().is_none_or(|a| a.as_str().contains('@')) {
        return Err("ROUTE_URL_INVALID");
    }
    Ok(())
}
pub(crate) fn schema(reference: &Value) -> Value {
    scoped_schema(reference, "page_route_set", "page")
}
pub(crate) fn context_schema(reference: &Value) -> Value {
    scoped_schema(reference, "context_route_set", "context")
}
pub(crate) fn handlers_schema(reference: &Value) -> Value {
    let mut result = json!({"type":"object","properties":{"operation":{"const":"network_route"},"scope":reference,"action":{"oneOf":[{"type":"object","properties":{"kind":{"const":"add"},"url":{"type":"string","maxLength":2048}},"required":["kind","url"],"additionalProperties":false},{"type":"object","properties":{"kind":{"const":"remove"},"handlerId":{"type":"string","format":"uuid"}},"required":["kind","handlerId"],"additionalProperties":false},{"type":"object","properties":{"kind":{"const":"list"}},"required":["kind"],"additionalProperties":false},{"type":"object","properties":{"kind":{"const":"clear"},"behavior":{"enum":["wait","default","ignoreErrors"],"description":"wait: clear after paused callbacks; default/ignoreErrors: stop matching now, paused callbacks stay answerable; omitted: refuse while paused"}},"required":["kind"],"additionalProperties":false}]}},"required":["operation","scope","action"],"additionalProperties":false});
    let matcher = Matcher::schema();
    result["properties"]["action"]["oneOf"].as_array_mut().expect("Route actions").push(json!({"type":"object","properties":{"kind":{"const":"add_matcher"},"matcher":matcher},"required":["kind","matcher"],"additionalProperties":false}));
    result["properties"]["action"]["oneOf"].as_array_mut().expect("Route actions").push(json!({"type":"object","properties":{"kind":{"const":"remove_matcher"},"matcher":matcher},"required":["kind","matcher"],"additionalProperties":false}));
    result["properties"]["action"]["oneOf"].as_array_mut().expect("Route actions").push(json!({"type":"object","properties":{"kind":{"const":"add_har"},"archiveId":{"type":"string","format":"uuid"},"urlFilter":{"anyOf":[matcher,{"type":"null"}]},"notFound":{"enum":["abort","continue","fallback"],"default":"abort"}},"required":["kind","archiveId"],"additionalProperties":false,"description":"Reply from a registered archive at this position in the scope's handler chain; Testrun answers these callbacks itself"}));
    for action in result["properties"]["action"]["oneOf"]
        .as_array_mut()
        .expect("Route actions")
        .iter_mut()
        .filter(|action| {
            ["add", "add_matcher", "add_har"]
                .iter()
                .any(|kind| action["properties"]["kind"]["const"] == *kind)
        })
    {
        action["properties"]["times"] = json!({"type":["integer","null"],"minimum":0,"maximum":u32::MAX,"description":"Invocation limit; omitted or null is unlimited. Zero, like one, permits one invocation."});
    }
    result
}
fn scoped_schema(reference: &Value, operation: &str, scope: &str) -> Value {
    json!({"type":"object","properties":{"operation":{"const":operation},(scope):reference,"url":{"type":["string","null"],"maxLength":2048,"description":"Exact ASCII HTTP(S) URL, including its query; no fragment, credentials or wildcard; null disables interception"},"authenticationChallenges":{"type":"boolean","default":false,"description":"Native Chrome authentication callbacks in the selected scope; requires a route URL"}},"required":["operation",scope,"url"],"additionalProperties":false})
}
pub(crate) fn credentials(username: &str, password: &str) -> Result<(), &'static str> {
    if username.len() > 1024
        || password.len() > 1024
        || username
            .chars()
            .chain(password.chars())
            .any(char::is_control)
    {
        return Err("AUTH_CREDENTIAL_BOUNDS");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handler_actions_accept_their_published_fields_only() {
        let reference = json!({"type":"object"});
        let schema = handlers_schema(&reference);
        let request =
            |action: Value| json!({"operation":"network_route","scope":{},"action":action});
        for action in [
            json!({"kind":"add","url":"https://example.com/acme","times":null}),
            json!({"kind":"add_matcher","matcher":{"kind":"glob","pattern":"**/acme"},"times":2}),
            json!({"kind":"add_matcher","matcher":{"kind":"url_pattern","pathname":"/acme/:id"}}),
            json!({"kind":"add_har","archiveId":"00000000-0000-4000-8000-000000000000","times":1}),
            json!({"kind":"remove_matcher","matcher":{"kind":"regex","pattern":"acme","flags":"y"}}),
            json!({"kind":"clear","behavior":"wait"}),
            json!({"kind":"clear","behavior":"default"}),
            json!({"kind":"clear","behavior":"ignoreErrors"}),
            json!({"kind":"clear"}),
            json!({"kind":"list"}),
        ] {
            crate::schema::validate(&schema, &request(action.clone()))
                .unwrap_or_else(|error| panic!("{action}: {}", error.message));
        }
        for action in [
            json!({"kind":"clear","times":1}),
            json!({"kind":"remove_matcher","matcher":{"kind":"glob","pattern":"**"},"times":1}),
            json!({"kind":"clear","behavior":"Acme"}),
        ] {
            assert!(
                crate::schema::validate(&schema, &request(action.clone())).is_err(),
                "{action}"
            );
        }
    }
    #[test]
    fn exact_url_and_response_bounds_are_checked_before_dispatch() {
        assert!(validate_url("https://example.com/acme").is_ok());
        assert!(validate_url("https://example.com/acme?item=1&name=Acme").is_ok());
        for url in [
            "https://example.com/*",
            "https://acme@example.com/",
            "file:///acme",
            "https://example.com/#acme",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
        }
        let text = |status: u16, body: &str, content_type: &str| {
            text_fulfill(
                Some(status),
                Some(body),
                None,
                Some(content_type),
                &[],
                None,
            )
            .and_then(|(status, body, headers)| inline_response(status, &body, &headers))
        };
        assert!(text(200, "Acme", "text/plain").is_ok());
        assert!(text(99, "Acme", "text/plain").is_err());
        assert!(text(200, &"x".repeat(65537), "text/plain").is_err());
        assert!(text(200, "Acme", "text/plain\r\nSet-Cookie:Acme").is_err());
        assert!(credentials("Acme", "example-password").is_ok());
        assert!(credentials("Acme\r\n", "example-password").is_err());
        assert!(credentials("Acme", &"x".repeat(1025)).is_err());
    }
    #[test]
    fn binary_fulfillment_preserves_octets_and_duplicate_headers_without_injection() {
        let headers = vec![
            Header {
                name: "Content-Type".into(),
                value: "application/octet-stream".into(),
            },
            Header {
                name: "Set-Cookie".into(),
                value: "Acme=one; Path=/".into(),
            },
            Header {
                name: "Set-Cookie".into(),
                value: "AcmeTwo=two; Path=/".into(),
            },
            Header {
                name: "Content-Length".into(),
                value: "3".into(),
            },
        ];
        let normalized = binary_response(201, "AP+A", &headers)
            .unwrap()
            .headers_for("GET")
            .unwrap();
        assert_eq!(
            normalized
                .iter()
                .filter(|header| header.name == "set-cookie")
                .count(),
            2
        );
        assert_eq!(normalized.last().unwrap().value, "3");
        assert_eq!(
            binary_response(200, "!", &[]).unwrap_err(),
            "ROUTE_BODY_ENCODING"
        );
        assert_eq!(
            binary_response(
                200,
                "AP+A",
                &[Header {
                    name: "content-length".into(),
                    value: "2".into()
                }]
            )
            .unwrap()
            .headers_for("GET")
            .unwrap_err(),
            "ROUTE_CONTENT_LENGTH_MISMATCH"
        );
        assert_eq!(
            binary_response(
                200,
                "",
                &[Header {
                    name: "x-acme".into(),
                    value: "ok\r\nSet-Cookie:Acme=bad".into()
                }]
            )
            .unwrap_err(),
            "ROUTE_HEADER_INVALID"
        );
        assert_eq!(
            binary_response(200, "", &vec![headers[0].clone(); 65]).unwrap_err(),
            "ROUTE_HEADER_BOUNDS"
        );
        assert_eq!(
            binary_response(200, &STANDARD.encode(vec![0; MAX_BINARY_BODY + 1]), &[]).unwrap_err(),
            "ROUTE_BODY_BOUNDS"
        );
    }
    #[test]
    fn synthetic_framing_distinguishes_metadata_lengths_and_no_body_statuses() {
        for (method, status) in [("HEAD", 200), ("GET", 304)] {
            let headers = binary_response(
                status,
                "",
                &[
                    Header {
                        name: "content-length".into(),
                        value: "123".into(),
                    },
                    Header {
                        name: "transfer-encoding".into(),
                        value: "chunked".into(),
                    },
                ],
            )
            .unwrap()
            .headers_for(method)
            .unwrap();
            assert_eq!(headers.len(), 1);
            assert_eq!(headers[0].value, "123");
        }
        assert!(
            binary_response(204, "", &[])
                .unwrap()
                .headers_for("GET")
                .unwrap()
                .is_empty()
        );
        assert!(
            binary_response(304, "", &[])
                .unwrap()
                .headers_for("GET")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            binary_response(204, "AP+A", &[]).unwrap_err(),
            "ROUTE_RESPONSE_BODY_FORBIDDEN"
        );
        assert_eq!(
            binary_response(
                204,
                "",
                &[Header {
                    name: "content-length".into(),
                    value: "0".into()
                }]
            )
            .unwrap_err(),
            "ROUTE_HEADER_INVALID"
        );
    }
    #[test]
    fn dictionary_conversion_preserves_duplicate_fields_and_cookie_expiry_commas() {
        let headers = dictionary_headers(vec![
            Header {
                name: "x-acme".into(),
                value: "first".into(),
            },
            Header {
                name: "set-cookie".into(),
                value: "Acme=one; Expires=Wed, 21 Oct 2037 07:28:00 GMT".into(),
            },
            Header {
                name: "x-acme".into(),
                value: "second".into(),
            },
            Header {
                name: "set-cookie".into(),
                value: "AcmeTwo=two".into(),
            },
        ]);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].value, "first, second");
        assert_eq!(
            headers[1].value,
            "Acme=one; Expires=Wed, 21 Oct 2037 07:28:00 GMT\nAcmeTwo=two"
        );
    }
    #[test]
    fn inline_bodies_fit_transport_budgets_while_evidence_retains_the_larger_bound() {
        let body = STANDARD.encode(vec![0; MAX_INLINE_BODY + 1]);
        assert_eq!(
            inline_response(200, &body, &[]).unwrap_err(),
            "ROUTE_BODY_BOUNDS"
        );
        assert!(binary_response(200, &body, &[]).is_ok());
        assert!(inline_response(200, &STANDARD.encode(vec![0; MAX_INLINE_BODY]), &[]).is_ok());
    }
    #[tokio::test]
    async fn route_fulfill_path_reads_bounded_regular_files_with_the_pinned_mime_table() {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let directory = tempfile::tempdir().unwrap();
        let file = |name: &str, bytes: &[u8]| {
            let path = directory.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path.to_str().unwrap().to_owned()
        };
        let html = read_fulfill_file(file("acme.html", b"<b>Acme</b>"), deadline)
            .await
            .unwrap();
        assert_eq!(html.parts(), (b"<b>Acme</b>".as_slice(), "text/html"));
        // Case-sensitive extension after the whole path's last dot.
        for (name, mime) in [
            ("acme.HTML", "application/octet-stream"),
            ("acme", "application/octet-stream"),
            ("acme.json", "application/json"),
        ] {
            assert_eq!(
                read_fulfill_file(file(name, b"x"), deadline)
                    .await
                    .unwrap()
                    .parts()
                    .1,
                mime,
                "{name}"
            );
        }
        assert_eq!(fulfill_mime("/acme.d/file"), "application/octet-stream");
        assert_eq!(
            read_fulfill_file(directory.path().to_str().unwrap().into(), deadline)
                .await
                .unwrap_err(),
            "ROUTE_FILE_NOT_REGULAR"
        );
        assert_eq!(
            read_fulfill_file(file("large.bin", &vec![0; MAX_BINARY_BODY + 1]), deadline)
                .await
                .unwrap_err(),
            "ROUTE_BODY_BOUNDS"
        );
        assert_eq!(
            read_fulfill_file("relative.html".into(), deadline)
                .await
                .unwrap_err(),
            "ROUTE_FILE_PATH_INVALID"
        );
        // The file replaces any body; a truthy json still selects JSON content type.
        let (_, body, headers) =
            text_fulfill(None, Some("ignored"), None, None, &[], Some(html.parts())).unwrap();
        assert_eq!(STANDARD.decode(body).unwrap(), b"<b>Acme</b>");
        assert_eq!(headers[0].value, "text/html");
        let (_, _, headers) = text_fulfill(
            None,
            None,
            Some(&json!({"a":1})),
            None,
            &[],
            Some(html.parts()),
        )
        .unwrap();
        assert_eq!(headers[0].value, "application/json");
    }
    #[test]
    fn route_fulfill_text_and_json_follow_the_pinned_client_rules() {
        let header = |name: &str, value: &str| Header {
            name: name.into(),
            value: value.into(),
        };
        let decode = |body: &str| String::from_utf8(STANDARD.decode(body).unwrap()).unwrap();
        let (status, body, headers) =
            text_fulfill(None, None, Some(&json!({"Acme":[1,true]})), None, &[], None).unwrap();
        assert_eq!(
            (status, decode(&body)),
            (200, r#"{"Acme":[1,true]}"#.into())
        );
        assert_eq!(headers[0].name, "content-type");
        assert_eq!(headers[0].value, "application/json");
        // Falsy JSON values add no content type, as `else if (options.json)`.
        for value in [json!(null), json!(false), json!(0), json!("")] {
            let (_, body, headers) =
                text_fulfill(Some(0), None, Some(&value), None, &[], None).unwrap();
            assert_eq!(decode(&body), value.to_string());
            assert!(headers.is_empty(), "{value}");
        }
        let (status, body, headers) = text_fulfill(
            Some(404),
            Some("Acme"),
            None,
            Some("text/acme"),
            &[
                header("X-Acme", "one"),
                header("Content-Type", "text/old"),
                header("x-acme", "two"),
            ],
            None,
        )
        .unwrap();
        assert_eq!((status, decode(&body)), (404, "Acme".into()));
        let pairs: Vec<_> = headers
            .iter()
            .map(|h| (h.name.as_str(), h.value.as_str()))
            .collect();
        assert_eq!(pairs, [("x-acme", "two"), ("content-type", "text/acme")]);
        assert_eq!(
            text_fulfill(None, Some("Acme"), Some(&json!(1)), None, &[], None).unwrap_err(),
            "ROUTE_FULFILL_BODY_CONFLICT"
        );
        let (status, body, headers) = text_fulfill(None, None, None, None, &[], None).unwrap();
        assert_eq!((status, body.as_str(), headers.len()), (200, "", 0));
    }
    #[test]
    fn route_abort_codes_match_the_pinned_table_and_default_to_failed() {
        use crate::browser::contract::CallbackResponse;
        let parse = |value: Value| serde_json::from_value::<CallbackResponse>(value);
        let Ok(CallbackResponse::RouteAbort { error_code }) = parse(json!({"kind":"route_abort"}))
        else {
            panic!("Omitted errorCode")
        };
        assert_eq!(error_code.unwrap_or_default(), AbortCode::Failed);
        assert_eq!(AbortCode::Failed.chrome_reason(), "Failed");
        for code in AbortCode::ALL {
            let Ok(CallbackResponse::RouteAbort {
                error_code: Some(parsed),
            }) = parse(json!({"kind":"route_abort","errorCode":code}))
            else {
                panic!("{code}")
            };
            assert_eq!(parsed.code(), code);
            // Pinned Chromium errorReasons are the same words in UpperCamelCase.
            assert_eq!(parsed.chrome_reason().to_ascii_lowercase(), code);
        }
        assert!(parse(json!({"kind":"route_abort","errorCode":"Aborted"})).is_err());
        assert!(parse(json!({"kind":"route_abort","errorCode":"acme"})).is_err());
    }
}
