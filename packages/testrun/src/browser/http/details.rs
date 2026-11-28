//! Private parsed response fields, retained independently of clients and body artifacts.
use super::*;

#[derive(Default)]
pub(super) struct Details {
    items: BTreeMap<Uuid, Value>,
    order: VecDeque<Uuid>,
}
impl Details {
    pub(super) fn insert(&mut self, id: Uuid, value: Value) {
        if self.items.contains_key(&id) {
            return;
        }
        if self.order.len() == 128 {
            let oldest = self.order.pop_front().expect("Response details retention");
            self.items.remove(&oldest);
        }
        self.order.push_back(id);
        self.items.insert(id, value);
    }
    pub(super) fn read(&self, id: Uuid, max_bytes: usize) -> Result<Value, Error> {
        if !(1..=65536).contains(&max_bytes) {
            return Err("HTTP_RESPONSE_RESULT_BOUNDS".into());
        }
        let value = self.items.get(&id).ok_or("UNKNOWN_HTTP_RESPONSE_DETAILS")?;
        crate::browser::schema::encoded_len(value, max_bytes)
            .map_err(|_| Error::from("HTTP_RESPONSE_RESULT_BOUNDS"))?;
        Ok(value.clone())
    }
    pub(super) fn remove(&mut self, id: Uuid) -> Value {
        let removed = self.items.remove(&id).is_some();
        if removed {
            self.order.retain(|entry| *entry != id);
        }
        json!({"requestId":id,"removed":removed,"bodyEvidenceRemoved":false,"replayed":false})
    }
}

pub(super) fn capture(
    id: Uuid,
    url: &Url,
    response: &ureq::http::Response<ureq::Body>,
    wire: Result<super::wire::Head, &'static str>,
) -> Value {
    use base64::Engine as _;
    let normalized_headers = || {
        response
            .headers()
            .iter()
            .map(|(name, value)| {
                let bytes = value.as_bytes();
                let mut header =
                    json!({"name":name.as_str(),"value":String::from_utf8_lossy(bytes)});
                if std::str::from_utf8(bytes).is_err() {
                    header["valueBase64"] =
                        json!(base64::engine::general_purpose::STANDARD.encode(bytes));
                }
                header
            })
            .collect::<Vec<_>>()
    };
    let mut result = json!({"requestId":id,"url":url.as_str(),"status":response.status().as_u16(),
        "ok":response.status().is_success(),"headerNames":"lowercase",
        "headerOrder":"per-name-values-only","statusText":null,"statusTextAvailable":false,
        "source":"rust-host-http","private":true,"replayed":false});
    match wire {
        Ok(head) => {
            let (reason, headers) = head.project();
            result["headers"] = headers;
            result["headerNames"] = json!("original");
            result["headerOrder"] = json!("wire");
            result["headerValueEncoding"] = json!("iso-8859-1");
            result["statusText"] = json!(reason);
            result["statusTextAvailable"] = json!(true);
            result["wireMetadataAvailable"] = json!(true);
        }
        Err(reason) => {
            result["headers"] = json!(normalized_headers());
            result["wireMetadataAvailable"] = json!(false);
            result["wireMetadataFailure"] = json!(reason);
        }
    }
    result
}

/// APIResponse.headers() joins duplicate values and exposes ECMAScript own-key
/// order. The original header array remains the wire-order evidence.
pub(super) fn headers_object(result: &mut Value) -> Result<(), Error> {
    use base64::Engine as _;
    let headers = result["headers"]
        .as_array()
        .ok_or("HTTP_RESPONSE_HEADERS")?;
    let mut values = BTreeMap::<String, String>::new();
    let mut keys = vec![];
    for header in headers {
        let name = header["name"]
            .as_str()
            .ok_or("HTTP_RESPONSE_HEADERS")?
            .to_ascii_lowercase();
        // The pinned SDK assigns into an ordinary object: this setter does not
        // create an own string-valued property.
        if name == "__proto__" {
            continue;
        }
        let value = if let Some(encoded) = header.get("valueBase64") {
            let encoded = encoded.as_str().ok_or("HTTP_RESPONSE_HEADERS")?;
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| Error::from("HTTP_RESPONSE_HEADERS"))?
                .into_iter()
                .map(char::from)
                .collect::<String>()
        } else {
            header["value"]
                .as_str()
                .ok_or("HTTP_RESPONSE_HEADERS")?
                .to_owned()
        };
        if let Some(previous) = values.get_mut(&name) {
            previous.push_str(if name == "set-cookie" { "\n" } else { ", " });
            previous.push_str(&value);
        } else {
            keys.push(name.clone());
            values.insert(name, value);
        }
    }
    keys.sort_by(|a, b| {
        match (
            super::ecmascript_json::array_index(&a.encode_utf16().collect::<Vec<_>>()),
            super::ecmascript_json::array_index(&b.encode_utf16().collect::<Vec<_>>()),
        ) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    result["headersObject"] = json!(values);
    result["headerObjectKeys"] = json!(keys);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn http_header_object_joins_duplicates_and_preserves_sdk_own_keys() {
        let mut value = json!({"headers":[
            {"name":"X-Acme","value":"one"},{"name":"x-acme","value":"two"},
            {"name":"Set-Cookie","value":"AcmeA=one"},{"name":"set-cookie","value":"AcmeB=two"},
            {"name":"__proto__","value":"Acme-proto"},{"name":"Constructor","value":"Acme-ctor"},
            {"name":"2","value":"two"},{"name":"1","value":"one"},
            {"name":"X-Latin","value":"replacement","valueBase64":"/w=="}
        ]});
        let original = value["headers"].clone();
        headers_object(&mut value).unwrap();
        assert_eq!(value["headers"], original);
        assert_eq!(
            value["headersObject"],
            json!({"x-acme":"one, two","set-cookie":"AcmeA=one\nAcmeB=two","constructor":"Acme-ctor","2":"two","1":"one","x-latin":"ÿ"})
        );
        assert_eq!(
            value["headerObjectKeys"],
            json!(["1", "2", "x-acme", "set-cookie", "constructor", "x-latin"])
        );
    }
    #[test]
    fn http_header_object_refuses_malformed_retained_metadata() {
        for mut value in [
            json!({}),
            json!({"headers":[{"value":"Acme"}]}),
            json!({"headers":[{"name":"Acme","valueBase64":"!"}]}),
        ] {
            assert_eq!(
                headers_object(&mut value).unwrap_err().code,
                "HTTP_RESPONSE_HEADERS"
            );
        }
    }
}
