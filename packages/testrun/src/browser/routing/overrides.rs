//! Bounded request changes validated before a paused callback is consumed.
use super::{Header, MAX_INLINE_BODY};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Overrides {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Option<Vec<Header>>,
    #[serde(default)]
    pub body_base64: Option<String>,
}
impl Overrides {
    pub(crate) fn is_empty(&self) -> bool {
        self.url.is_none()
            && self.method.is_none()
            && self.headers.is_none()
            && self.body_base64.is_none()
    }
    pub(crate) fn overlay(&self, next: &Self) -> Self {
        Self {
            url: next.url.clone().or_else(|| self.url.clone()),
            method: next.method.clone().or_else(|| self.method.clone()),
            headers: next.headers.clone().or_else(|| self.headers.clone()),
            body_base64: next
                .body_base64
                .clone()
                .or_else(|| self.body_base64.clone()),
        }
    }
    pub(crate) fn apply(&self, source: &Value) -> Result<Value, &'static str> {
        if self.is_empty() {
            return Ok(source.clone());
        }
        let headers: Vec<Header> = serde_json::from_value(source["headers"].clone())
            .map_err(|_| "ROUTE_REQUEST_UNAVAILABLE")?;
        let changes = self.params(
            source["url"].as_str().ok_or("ROUTE_REQUEST_UNAVAILABLE")?,
            &headers,
        )?;
        let mut result = source.clone();
        for key in ["url", "method", "headers", "postData"] {
            if let Some(value) = changes.get(key) {
                result[key] = value.clone();
            }
        }
        Ok(result)
    }
    pub(crate) fn schema() -> Value {
        json!({"type":"object","properties":{"url":{"type":["string","null"],"maxLength":8192},"method":{"type":["string","null"],"minLength":1,"maxLength":32},"headers":{"anyOf":[super::headers_schema(),{"type":"null"}]},"bodyBase64":{"type":["string","null"],"maxLength":MAX_INLINE_BODY.div_ceil(3)*4}},"additionalProperties":false})
    }
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Some(value) = &self.url {
            request_url(value)?;
        }
        if let Some(method) = &self.method
            && (method.is_empty()
                || method.len() > 32
                || axum::http::Method::from_bytes(method.as_bytes()).is_err())
        {
            return Err("ROUTE_METHOD_INVALID");
        }
        if let Some(headers) = &self.headers {
            if headers.len() > 64 {
                return Err("ROUTE_HEADER_BOUNDS");
            }
            let mut size = 0;
            for header in headers {
                size += header.name.len() + header.value.len();
                if header.name.len() > 256 || size > 8192 {
                    return Err("ROUTE_HEADER_BOUNDS");
                }
                if axum::http::HeaderName::from_bytes(header.name.as_bytes()).is_err()
                    || axum::http::HeaderValue::from_str(&header.value).is_err()
                {
                    return Err("ROUTE_HEADER_INVALID");
                }
            }
        }
        if let Some(body) = &self.body_base64 {
            if body.len() > MAX_INLINE_BODY.div_ceil(3) * 4 {
                return Err("ROUTE_BODY_BOUNDS");
            }
            let bytes = STANDARD.decode(body).map_err(|_| "ROUTE_BODY_INVALID")?;
            if bytes.len() > MAX_INLINE_BODY {
                return Err("ROUTE_BODY_BOUNDS");
            }
        }
        Ok(())
    }
    pub(crate) fn params(
        &self,
        original_url: &str,
        original_headers: &[Header],
    ) -> Result<Value, &'static str> {
        self.validate()?;
        let mut params = json!({});
        if let Some(value) = &self.url {
            if request_url(value)?.scheme() != request_url(original_url)?.scheme() {
                return Err("ROUTE_URL_PROTOCOL_MISMATCH");
            }
            params["url"] = json!(value);
        }
        if let Some(method) = &self.method {
            params["method"] = json!(method);
        }
        if let Some(body) = &self.body_base64 {
            params["postData"] = json!(body);
        }
        if let Some(headers) = &self.headers {
            let mut fields = headers
                .iter()
                .filter(|h| !forbidden(h))
                .cloned()
                .collect::<Vec<_>>();
            fields.extend(original_headers.iter().filter(|h| forbidden(h)).cloned());
            // Apply the combined bound before any backend receives private bytes.
            Self {
                headers: Some(fields.clone()),
                ..Default::default()
            }
            .validate()?;
            params["headers"] = json!(fields);
        }
        Ok(params)
    }
}
fn request_url(value: &str) -> Result<url::Url, &'static str> {
    if value.len() > 8192 {
        return Err("ROUTE_URL_INVALID");
    }
    let parsed = url::Url::parse(value).map_err(|_| "ROUTE_URL_INVALID")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err("ROUTE_URL_INVALID");
    }
    Ok(parsed)
}
fn forbidden(header: &Header) -> bool {
    let name = header.name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "accept-charset"
            | "accept-encoding"
            | "access-control-request-headers"
            | "access-control-request-method"
            | "connection"
            | "content-length"
            | "cookie"
            | "date"
            | "dnt"
            | "expect"
            | "host"
            | "keep-alive"
            | "origin"
            | "referer"
            | "set-cookie"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "via"
    ) || name.starts_with("proxy-")
        || name.starts_with("sec-")
        || matches!(
            name.as_str(),
            "x-http-method" | "x-http-method-override" | "x-method-override"
        ) && matches!(
            header.value.to_ascii_uppercase().as_str(),
            "CONNECT" | "TRACE" | "TRACK"
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn h(name: &str, value: &str) -> Header {
        Header {
            name: name.into(),
            value: value.into(),
        }
    }
    #[test]
    fn fallback_overrides_compose_without_replacing_unmentioned_fields() {
        let first = Overrides {
            url: Some("https://example.com/changed".into()),
            method: Some("POST".into()),
            headers: Some(vec![h("X-Acme", "first")]),
            body_base64: Some("AP8=".into()),
        };
        let second = Overrides {
            method: Some("PUT".into()),
            ..Default::default()
        };
        let merged = first.overlay(&second);
        let source = json!({"url":"https://example.com/original","method":"GET","headers":[h("Cookie","browser"),h("X-Old","removed")],"isNavigationRequest":false});
        let changed = merged.apply(&source).unwrap();
        assert_eq!(changed["url"], "https://example.com/changed");
        assert_eq!(changed["method"], "PUT");
        assert_eq!(changed["postData"], "AP8=");
        assert_eq!(
            changed["headers"],
            json!([h("X-Acme", "first"), h("Cookie", "browser")])
        );
        assert_eq!(changed["isNavigationRequest"], false);
        assert_eq!(source["method"], "GET");
    }

    #[test]
    fn rewrites_keep_protocol_and_browser_controlled_headers() {
        let changes = Overrides {
            url: Some("https://example.com/new?private=Acme".into()),
            method: Some("POST".into()),
            headers: Some(vec![
                h("X-Acme", "new"),
                h("Cookie", "forged"),
                h("Host", "forged"),
            ]),
            body_base64: Some("AP8=".into()),
        };
        let params = changes
            .params(
                "https://example.com/old",
                &[
                    h("Cookie", "original"),
                    h("Host", "example.com"),
                    h("X-Old", "removed"),
                ],
            )
            .unwrap();
        assert_eq!(params["postData"], "AP8=");
        assert_eq!(
            params["headers"],
            json!([
                h("X-Acme", "new"),
                h("Cookie", "original"),
                h("Host", "example.com")
            ])
        );
        assert_eq!(
            changes.params("http://example.com/old", &[]).unwrap_err(),
            "ROUTE_URL_PROTOCOL_MISMATCH"
        );
    }
    #[test]
    fn invalid_or_oversized_overrides_are_refused_before_dispatch() {
        for changes in [
            Overrides {
                body_base64: Some("?".into()),
                ..Default::default()
            },
            Overrides {
                method: Some("GET\r\n".into()),
                ..Default::default()
            },
            Overrides {
                headers: Some(vec![h("X-Acme", "secret\r\n")]),
                ..Default::default()
            },
            Overrides {
                body_base64: Some(STANDARD.encode(vec![0; MAX_INLINE_BODY + 1])),
                ..Default::default()
            },
        ] {
            assert!(changes.validate().is_err());
        }
    }
}
