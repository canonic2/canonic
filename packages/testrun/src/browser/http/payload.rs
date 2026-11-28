//! Bounded HTTP entity encoding before any request can be dispatched.
use super::{Error, MAX_BODY};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Write};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormField {
    pub name: String,
    pub value: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Payload {
    Text { text: String },
    Base64 { data: String },
    Json { value: Value },
    Form { fields: Vec<FormField> },
    Multipart { fields: Vec<super::multipart::Part> },
}
#[derive(Default)]
struct JsonBody {
    bytes: Vec<u8>,
    overflow: bool,
}
impl JsonBody {
    fn encode(value: &impl Serialize) -> Result<Vec<u8>, Error> {
        let mut body = Self::default();
        serde_json::to_writer(&mut body, value).map_err(|_| {
            Error::from(if body.overflow {
                "HTTP_REQUEST_BODY_BOUNDS"
            } else {
                "HTTP_REQUEST_BODY_INVALID"
            })
        })?;
        Ok(body.bytes)
    }
}
pub(super) fn json_string_header(headers: &[super::Header]) -> bool {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .is_some_and(|header| header.value == "application/json")
}
impl Write for JsonBody {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > MAX_BODY {
            self.overflow = true;
            return Err(io::Error::other("HTTP entity exceeds its byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Payload {
    pub(super) fn encode_for_request(
        &self,
        json_string: bool,
    ) -> Result<(Vec<u8>, Option<String>), Error> {
        if let Self::Text { text } = self
            && json_string
        {
            if text.len() > MAX_BODY {
                return Err("HTTP_REQUEST_BODY_BOUNDS".into());
            }
            // IgnoredAny checks syntax without materializing values: exponent
            // overflow, escaped lone UTF-16 units and deep valid text remain
            // valid, just as JSON.parse in the pinned client. Its iterative
            // scratch stack is bounded by the already bounded input length.
            if serde_json::from_str::<serde::de::IgnoredAny>(text).is_err() {
                return Ok((JsonBody::encode(text)?, Some("application/json".into())));
            }
        }
        self.encode()
    }
    pub(super) fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Multipart { fields } => super::multipart::validate(fields),
            _ => self.bytes().map(|_| ()),
        }
    }
    pub(super) fn content_type(&self) -> Option<&'static str> {
        match self {
            Self::Json { .. } => Some("application/json"),
            Self::Form { .. } => Some("application/x-www-form-urlencoded"),
            Self::Text { .. } | Self::Base64 { .. } => Some("application/octet-stream"),
            Self::Multipart { .. } => None,
        }
    }
    pub(super) fn bytes(&self) -> Result<Vec<u8>, Error> {
        self.encode().map(|(bytes, _)| bytes)
    }
    pub(super) fn encode(&self) -> Result<(Vec<u8>, Option<String>), Error> {
        let bytes = match self {
            Self::Multipart { fields } => return super::multipart::encode(fields),
            Self::Text { text } => {
                if text.len() > MAX_BODY {
                    return Err("HTTP_REQUEST_BODY_BOUNDS".into());
                }
                text.as_bytes().to_vec()
            }
            Self::Base64 { data } => {
                if data.len() > MAX_BODY.div_ceil(3) * 4 {
                    return Err("HTTP_REQUEST_BODY_BOUNDS".into());
                }
                STANDARD
                    .decode(data)
                    .map_err(|_| Error::from("HTTP_REQUEST_BODY_INVALID"))?
            }
            Self::Json { value } => JsonBody::encode(value)?,
            Self::Form { fields } => {
                if fields.len() > 128 {
                    return Err("HTTP_REQUEST_BODY_BOUNDS".into());
                }
                let mut length = 0usize;
                for (index, field) in fields.iter().enumerate() {
                    if field.name.len() > 1024 || field.value.len() > 16384 {
                        return Err("HTTP_REQUEST_BODY_BOUNDS".into());
                    }
                    length += usize::from(index > 0) + 1;
                    length += url::form_urlencoded::byte_serialize(field.name.as_bytes())
                        .map(str::len)
                        .sum::<usize>();
                    length += url::form_urlencoded::byte_serialize(field.value.as_bytes())
                        .map(str::len)
                        .sum::<usize>();
                    if length > MAX_BODY {
                        return Err("HTTP_REQUEST_BODY_BOUNDS".into());
                    }
                }
                url::form_urlencoded::Serializer::new(String::with_capacity(length))
                    .extend_pairs(fields.iter().map(|field| (&field.name, &field.value)))
                    .finish()
                    .into_bytes()
            }
        };
        if bytes.len() > MAX_BODY {
            return Err("HTTP_REQUEST_BODY_BOUNDS".into());
        }
        Ok((bytes, self.content_type().map(str::to_owned)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn json_encoding_preserves_values_and_bounds_escaped_output() {
        let value = json!({"name":"Acme 東京","controls":"\0\n","values":[true,null,17]});
        let payload = Payload::Json {
            value: value.clone(),
        };
        assert_eq!(
            serde_json::from_slice::<Value>(&payload.bytes().unwrap()).unwrap(),
            value
        );
        assert_eq!(payload.content_type(), Some("application/json"));
        let oversized = Payload::Json {
            value: json!("\0".repeat(MAX_BODY / 6 + 1)),
        };
        assert_eq!(
            oversized.bytes().unwrap_err().code,
            "HTTP_REQUEST_BODY_BOUNDS"
        );
        assert_eq!(
            Payload::Base64 {
                data: "AP8=".into()
            }
            .bytes()
            .unwrap(),
            [0, 255]
        );
        assert_eq!(
            Payload::Base64 { data: "?".into() }
                .bytes()
                .unwrap_err()
                .code,
            "HTTP_REQUEST_BODY_INVALID"
        );
    }
    #[test]
    fn form_encoding_preserves_duplicate_names_and_bounds_expansion() {
        let payload = Payload::Form {
            fields: vec![
                FormField {
                    name: "label".into(),
                    value: "Acme 東京+&".into(),
                },
                FormField {
                    name: "label".into(),
                    value: "two".into(),
                },
            ],
        };
        assert_eq!(
            payload.bytes().unwrap(),
            b"label=Acme+%E6%9D%B1%E4%BA%AC%2B%26&label=two"
        );
        assert_eq!(
            payload.content_type(),
            Some("application/x-www-form-urlencoded")
        );
        let fields = vec![
            FormField {
                name: String::new(),
                value: "\0".repeat(16384)
            };
            128
        ];
        assert_eq!(
            Payload::Form { fields }.bytes().unwrap_err().code,
            "HTTP_REQUEST_BODY_BOUNDS"
        );
        let fields = vec![
            FormField {
                name: "Acme".into(),
                value: String::new()
            };
            129
        ];
        assert_eq!(
            Payload::Form { fields }.bytes().unwrap_err().code,
            "HTTP_REQUEST_BODY_BOUNDS"
        );
    }
}
