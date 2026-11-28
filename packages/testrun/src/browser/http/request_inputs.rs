//! Immutable pinned Request initializers, independent of live page objects.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
mod schema;
pub(crate) mod store;
pub(crate) use schema::schemas;

#[derive(Clone)]
pub(crate) struct Snapshot {
    url: String,
    method: String,
    headers: Vec<Header>,
    body: Option<Arc<[u8]>>,
    body_from_route: bool,
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RequestInputSnapshot { private: true }")
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Overrides {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Option<Vec<Header>>,
    #[serde(default)]
    pub body: Option<Payload>,
    #[serde(default)]
    pub params: Option<Parameters>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_redirects: Option<u32>,
    #[serde(default)]
    pub fail_on_status_code: Option<bool>,
    #[serde(default)]
    pub ignore_https_errors: Option<bool>,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub read_body: bool,
    #[serde(default = "body_limit")]
    pub max_bytes: usize,
}
impl Default for Overrides {
    fn default() -> Self {
        Self {
            url: None,
            method: None,
            headers: None,
            body: None,
            params: None,
            timeout_ms: None,
            max_redirects: None,
            fail_on_status_code: None,
            ignore_https_errors: None,
            max_retries: 0,
            read_body: false,
            max_bytes: body_limit(),
        }
    }
}
impl Overrides {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        // Validate supplied fields before source lookup. Effective source fields
        // are validated again when the immutable initializer is merged.
        Snapshot {
            url: String::new(),
            method: "GET".into(),
            headers: vec![],
            body: None,
            body_from_route: false,
        }
        .fetch(self.clone())
        .map(|_| ())
    }
}
impl std::fmt::Debug for Overrides {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RequestInputOverrides { private: true }")
    }
}
impl Snapshot {
    pub(crate) fn pinned(input: &Value) -> Result<Self, Error> {
        let body = input
            .get("postData")
            .filter(|value| !value.is_null())
            .map(|value| decode(value.as_str().ok_or("HTTP_REQUEST_INPUT_INVALID")?))
            .transpose()?;
        Self::new(input, body)
    }
    pub(crate) fn native(input: &Value) -> Result<Self, Error> {
        // The pinned adapter uses binary entries, never the lossy postData text.
        // Entries without bytes (for example file-backed entries) are skipped.
        let mut body = None;
        if let Some(entries) = input
            .get("postDataEntries")
            .filter(|value| !value.is_null())
        {
            let entries = entries.as_array().ok_or("HTTP_REQUEST_INPUT_INVALID")?;
            let mut bytes = Vec::new();
            for entry in entries {
                let Some(value) = entry.get("bytes").filter(|value| !value.is_null()) else {
                    continue;
                };
                let value = decode(value.as_str().ok_or("HTTP_REQUEST_INPUT_INVALID")?)?;
                if value.is_empty() {
                    continue;
                }
                if bytes.len().saturating_add(value.len()) > MAX_BODY {
                    return Err("HTTP_REQUEST_INPUT_BOUNDS".into());
                }
                bytes.extend(value);
                body = Some(());
            }
            if body.is_some() {
                return Self::new(input, Some(bytes));
            }
        }
        Self::new(input, None)
    }
    fn new(input: &Value, body: Option<Vec<u8>>) -> Result<Self, Error> {
        let url = input["url"].as_str().ok_or("HTTP_REQUEST_INPUT_INVALID")?;
        let method = input["method"]
            .as_str()
            .ok_or("HTTP_REQUEST_INPUT_INVALID")?;
        let headers = if let Some(fields) = input["headers"].as_array() {
            if fields.len() > 64 {
                return Err("HTTP_REQUEST_INPUT_BOUNDS".into());
            }
            serde_json::from_value::<Vec<Header>>(json!(fields))
                .map_err(|_| Error::from("HTTP_REQUEST_INPUT_INVALID"))?
        } else if let Some(fields) = input["headers"].as_object() {
            if fields.len() > 64 {
                return Err("HTTP_REQUEST_INPUT_BOUNDS".into());
            }
            fields
                .iter()
                .map(|(name, value)| {
                    Ok(Header {
                        name: name.clone(),
                        value: value.as_str().ok_or("HTTP_REQUEST_INPUT_INVALID")?.into(),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?
        } else {
            return Err("HTTP_REQUEST_INPUT_INVALID".into());
        };
        let snapshot = Self {
            url: url.into(),
            method: method.into(),
            headers,
            body: body.map(Arc::from),
            body_from_route: false,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    fn validate(&self) -> Result<(), Error> {
        if self.url.len() > 4096
            || self.method.len() > 32
            || self.headers.len() > 64
            || self.headers.iter().any(|header| header.name.len() > 1024)
            || self
                .headers
                .iter()
                .map(|header| header.name.len() + header.value.len())
                .sum::<usize>()
                > 16384
            || self.body.as_ref().is_some_and(|body| body.len() > MAX_BODY)
        {
            return Err("HTTP_REQUEST_INPUT_BOUNDS".into());
        }
        Ok(())
    }
    /// Preserve caller intent separately from browser-controlled wire headers.
    pub(crate) fn routed(
        &self,
        changes: &crate::browser::routing::Overrides,
    ) -> Result<Self, Error> {
        changes.validate().map_err(Error::from)?;
        let snapshot = Self {
            url: changes.url.clone().unwrap_or_else(|| self.url.clone()),
            method: changes
                .method
                .clone()
                .unwrap_or_else(|| self.method.clone()),
            headers: changes
                .headers
                .as_ref()
                .map(|headers| {
                    headers
                        .iter()
                        .map(|header| Header {
                            name: header.name.clone(),
                            value: header.value.clone(),
                        })
                        .collect()
                })
                .unwrap_or_else(|| self.headers.clone()),
            body: match &changes.body_base64 {
                Some(encoded) => Some(Arc::from(decode(encoded)?)),
                None => self.body.clone(),
            },
            body_from_route: self.body_from_route || changes.body_base64.is_some(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    fn headers(&self) -> Result<Vec<Header>, Error> {
        let mut projected = json!({"headers":self.headers});
        super::details::headers_object(&mut projected)?;
        projected["headerObjectKeys"]
            .as_array()
            .ok_or("HTTP_REQUEST_INPUT_INVALID")?
            .iter()
            .map(|key| {
                let name = key.as_str().ok_or("HTTP_REQUEST_INPUT_INVALID")?;
                Ok(Header {
                    name: name.into(),
                    value: projected["headersObject"][name]
                        .as_str()
                        .ok_or("HTTP_REQUEST_INPUT_INVALID")?
                        .into(),
                })
            })
            .collect()
    }
    pub(crate) fn fetch(&self, options: Overrides) -> Result<Fetch, Error> {
        let request = Fetch {
            url: options.url.unwrap_or_else(|| self.url.clone()),
            method: options
                .method
                .filter(|method| !method.is_empty())
                .unwrap_or_else(|| self.method.clone()),
            headers: match options.headers {
                Some(headers) => headers,
                None => self.headers()?,
            },
            body: options.body.or_else(|| {
                self.body.as_ref().map(|bytes| Payload::Base64 {
                    data: STANDARD.encode(bytes.as_ref()),
                })
            }),
            params: options.params,
            timeout_ms: options.timeout_ms,
            max_redirects: options.max_redirects,
            fail_on_status_code: options.fail_on_status_code,
            ignore_https_errors: options.ignore_https_errors,
            max_retries: options.max_retries,
            read_body: options.read_body,
            max_bytes: options.max_bytes,
        };
        request.validate()?;
        Ok(request)
    }
    pub(crate) fn bytes(&self) -> usize {
        self.url.len()
            + self.method.len()
            + self
                .headers
                .iter()
                .map(|header| header.name.len() + header.value.len())
                .sum::<usize>()
            + self.body.as_ref().map_or(0, |body| body.len())
    }
    pub(crate) fn body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }
    pub(crate) fn body_fidelity(&self) -> &'static str {
        if self.body_from_route {
            "binary-caller-route-override"
        } else {
            "binary-request-initializer"
        }
    }
}
fn decode(value: &str) -> Result<Vec<u8>, Error> {
    if value.len() > MAX_BODY.div_ceil(3) * 4 {
        return Err("HTTP_REQUEST_INPUT_BOUNDS".into());
    }
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| Error::from("HTTP_REQUEST_INPUT_INVALID"))?;
    if bytes.len() > MAX_BODY {
        return Err("HTTP_REQUEST_INPUT_BOUNDS".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Snapshot {
        Snapshot::pinned(&json!({"url":"https://example.com/Acme?old=one","method":"POST","headers":[{"name":"X-Acme","value":"one"},{"name":"x-acme","value":"two"},{"name":"Cookie","value":"AcmeSource=private"}],"postData":STANDARD.encode([65,99,109,101,0,255])})).unwrap()
    }
    fn options(value: Value) -> Overrides {
        serde_json::from_value(value).unwrap()
    }
    #[test]
    fn http_request_input_route_capabilities_disclose_caller_overrides_and_remaining_limits() {
        let report = crate::browser::http::capabilities(true);
        let inputs = &report["requestInputs"];
        assert_eq!(inputs["fallbackOverrides"], true);
        assert_eq!(inputs["continueOverrides"], true);
        assert_eq!(
            inputs["routeOverrideFields"],
            json!(["url", "method", "headers", "bodyBase64"])
        );
        assert_eq!(
            inputs["bodySource"],
            "binary-initializer-or-caller-route-override"
        );
        assert_eq!(
            inputs["headers"],
            "cumulative-caller-intent-with-pinned-object-projection"
        );
        assert_eq!(inputs["defaultEnabled"], true);
        assert_eq!(inputs["maxInputs"], 512);
        assert_eq!(inputs["maxRetainedBytes"], 33554432);
        assert_eq!(inputs["fullSdkParity"], false);
    }
    #[test]
    fn http_request_input_route_body_provenance_survives_header_changes_and_empty_replacement() {
        let original = source();
        assert_eq!(original.body_fidelity(), "binary-request-initializer");
        let changed = original
            .routed(&serde_json::from_value(json!({"bodyBase64":"AP+A"})).unwrap())
            .unwrap();
        assert_eq!(changed.body(), Some([0, 255, 128].as_slice()));
        assert_eq!(changed.body_fidelity(), "binary-caller-route-override");
        let headers = changed
            .routed(&serde_json::from_value(json!({"headers":[]})).unwrap())
            .unwrap();
        assert_eq!(headers.body(), changed.body());
        assert_eq!(headers.body_fidelity(), changed.body_fidelity());
        let empty = headers
            .routed(&serde_json::from_value(json!({"bodyBase64":""})).unwrap())
            .unwrap();
        assert_eq!(empty.body(), Some([].as_slice()));
        assert_eq!(empty.body_fidelity(), "binary-caller-route-override");
        assert_eq!(original.body(), Some([65, 99, 109, 101, 0, 255].as_slice()));
        assert_eq!(original.body_fidelity(), "binary-request-initializer");
    }
    #[test]
    fn http_request_input_snapshot_defaults_preserve_binary_initializer_and_joined_headers() {
        let source = source();
        let request = source.fetch(options(json!({}))).unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "https://example.com/Acme?old=one");
        assert_eq!(
            request.body.unwrap().bytes().unwrap(),
            [65, 99, 109, 101, 0, 255]
        );
        assert_eq!(
            request.headers,
            vec![
                Header {
                    name: "x-acme".into(),
                    value: "one, two".into()
                },
                Header {
                    name: "cookie".into(),
                    value: "AcmeSource=private".into()
                }
            ]
        );
        assert!(!format!("{source:?}").contains("private\""));
        assert!(!format!("{source:?}").contains("AcmeSource"));
    }
    #[test]
    fn http_request_input_snapshot_empty_method_falls_back_and_empty_headers_body_replace() {
        let source = source();
        let request=source.fetch(options(json!({"method":"","headers":[],"body":{"kind":"text","text":""},"params":"Acme=one&Acme=two","ignoreHttpsErrors":false,"timeoutMs":0}))).unwrap();
        assert_eq!(request.method, "POST");
        assert!(request.headers.is_empty());
        assert!(request.body.unwrap().bytes().unwrap().is_empty());
        assert_eq!(request.ignore_https_errors, Some(false));
        assert_eq!(request.timeout_ms, Some(0));
        assert!(
            matches!(request.params,Some(Parameters::Encoded(value)) if value=="Acme=one&Acme=two")
        );
        assert_eq!(
            source
                .fetch(options(json!({})))
                .unwrap()
                .body
                .unwrap()
                .bytes()
                .unwrap(),
            [65, 99, 109, 101, 0, 255]
        );
    }
    #[test]
    fn http_request_input_snapshot_native_uses_binary_entries_and_never_lossy_text() {
        let input = json!({"url":"https://example.com/Acme","method":"POST","headers":{},"hasPostData":true,"postData":"Acme lossy text","postDataEntries":[{"bytes":"AP8="},{"file":"Acme omitted source"},{"bytes":"gA=="}]});
        let source = Snapshot::native(&input).unwrap();
        assert_eq!(
            source
                .fetch(options(json!({})))
                .unwrap()
                .body
                .unwrap()
                .bytes()
                .unwrap(),
            [0, 255, 128]
        );
        let mut missing = input;
        missing.as_object_mut().unwrap().remove("postDataEntries");
        assert!(
            Snapshot::native(&missing)
                .unwrap()
                .fetch(options(json!({})))
                .unwrap()
                .body
                .is_none()
        );
        missing["postDataEntries"] = json!([{"bytes":""}]);
        assert!(
            Snapshot::native(&missing)
                .unwrap()
                .fetch(options(json!({})))
                .unwrap()
                .body
                .is_none()
        );
    }
    #[test]
    fn http_request_input_snapshot_effective_validation_allows_header_replacement_without_mutating_source()
     {
        let source=Snapshot::pinned(&json!({"url":"https://example.com/Acme","method":"POST","headers":[{"name":"X-Acme","value":"bad\nheader"}]})).unwrap();
        assert!(source.fetch(options(json!({}))).is_err());
        assert!(
            source
                .fetch(options(json!({"headers":[]})))
                .unwrap()
                .headers
                .is_empty()
        );
        assert!(
            source
                .fetch(options(json!({"method":"bad method"})))
                .is_err()
        );
    }
    #[test]
    fn http_request_input_snapshot_bounds_and_owned_copies_remain_private() {
        let mut input = json!({"url":"https://example.com/Acme","method":"POST","headers":[],"postData":STANDARD.encode([0,255])});
        let source = Snapshot::pinned(&input).unwrap();
        input["postData"] = json!("Acme invalid");
        drop(input);
        assert_eq!(
            source
                .fetch(options(json!({})))
                .unwrap()
                .body
                .unwrap()
                .bytes()
                .unwrap(),
            [0, 255]
        );
        assert!(source.bytes() > 2);
        let excessive = json!({"url":"https://example.com","method":"POST","headers":[],"postData":"A".repeat(MAX_BODY.div_ceil(3)*4+1)});
        assert_eq!(
            Snapshot::pinned(&excessive).unwrap_err().code,
            "HTTP_REQUEST_INPUT_BOUNDS"
        );
    }
}
