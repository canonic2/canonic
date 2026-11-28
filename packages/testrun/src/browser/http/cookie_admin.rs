//! Explicit cookie administration; URL selection differs from outgoing headers.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CookieUrls {
    One(String),
    Many(Vec<String>),
}
impl Default for CookieUrls {
    fn default() -> Self {
        Self::Many(Vec::new())
    }
}
impl CookieUrls {
    pub(crate) fn parse(&self) -> Result<Vec<Url>, Error> {
        let values = match self {
            Self::One(value) => std::slice::from_ref(value),
            Self::Many(values) => values.as_slice(),
        };
        if values.len() > 128 {
            return Err("HTTP_COOKIE_URL_BOUNDS".into());
        }
        values
            .iter()
            .map(|value| {
                if value.len() > 4096 {
                    return Err("HTTP_COOKIE_URL_BOUNDS".into());
                }
                Url::parse(value).map_err(|_| Error::from("HTTP_URL_INVALID"))
            })
            .collect()
    }
}
pub(crate) fn normalize(values: Vec<Value>) -> Result<Vec<Value>, Error> {
    normalize_impl(values, true)
}
pub(crate) fn normalize_browser(values: Vec<Value>) -> Result<Vec<Value>, Error> {
    normalize_impl(values, false)
}
fn normalize_impl(values: Vec<Value>, defaults: bool) -> Result<Vec<Value>, Error> {
    if values.len() > 256 {
        return Err("HTTP_STORAGE_COOKIE_BOUNDS".into());
    }
    super::super::schema::encoded_len(&json!(values), 65536)
        .map_err(|_| Error::from("HTTP_STORAGE_COOKIE_BOUNDS"))?;
    super::super::schema::validate(
        "APIRequestContextAddCookiesParams",
        Some(&json!({"cookies":values})),
        &super::super::objects::ObjectTable::default(),
    )
    .map_err(|_| Error::from("HTTP_COOKIE_INVALID"))?;
    values
        .into_iter()
        .map(|mut value| {
            let url = value["url"].as_str().filter(|value| !value.is_empty());
            let domain = value["domain"].as_str().filter(|value| !value.is_empty());
            let path = value["path"].as_str().filter(|value| !value.is_empty());
            if url.is_some() && (domain.is_some() || path.is_some())
                || url.is_none() && (domain.is_none() || path.is_none())
                || value["expires"].as_f64().is_some_and(|expires| {
                    expires != -1.0 && !(0.0..=253402300799.0).contains(&expires)
                })
            {
                return Err("HTTP_COOKIE_INVALID".into());
            }
            if let Some(url) = url {
                if url == "about:blank" || url.starts_with("data:") {
                    return Err("HTTP_COOKIE_INVALID".into());
                }
                let parsed = Url::parse(url).map_err(|_| Error::from("HTTP_COOKIE_INVALID"))?;
                let host = parsed
                    .host_str()
                    .filter(|host| !host.is_empty())
                    .ok_or("HTTP_COOKIE_INVALID")?;
                let end = parsed.path().rfind('/').map_or(0, |index| index + 1);
                value["domain"] = json!(host);
                value["path"] = json!(&parsed.path()[..end]);
                value["secure"] = json!(parsed.scheme() == "https");
            }
            let object = value.as_object_mut().expect("Validated cookie object");
            object.remove("url");
            if defaults {
                for (name, default) in [
                    ("expires", json!(-1)),
                    ("httpOnly", json!(false)),
                    ("secure", json!(false)),
                    ("sameSite", json!("Lax")),
                ] {
                    object.entry(name).or_insert(default);
                }
            }
            // Validate outbound header safety before either store can be mutated.
            if ["name", "value"].iter().any(|field| {
                value[*field]
                    .as_str()
                    .is_none_or(|value| value.chars().any(|c| c.is_control() || c == ';'))
            }) {
                return Err("HTTP_COOKIE_INVALID".into());
            }
            Ok(value)
        })
        .collect()
}
pub(crate) fn select(values: Value, urls: &[Url]) -> Result<Value, Error> {
    let values = values.as_array().ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
    let result = json!({"cookies":matching(values, urls),"secretBearing":true});
    super::super::schema::encoded_len(&result, 65536)
        .map_err(|_| Error::from("HTTP_COOKIE_READ_BOUNDS"))?;
    Ok(result)
}
/// The pinned `cookies(urls)` selection: every cookie without URLs, otherwise
/// those whose domain, path and secure attribute match one of them.
pub(crate) fn matching(values: &[Value], urls: &[Url]) -> Vec<Value> {
    values
        .iter()
        .filter(|cookie| {
            urls.is_empty()
                || urls.iter().any(|url| {
                    let (Some(host), Some(domain), Some(path)) = (
                        url.host_str(),
                        cookie["domain"].as_str(),
                        cookie["path"].as_str(),
                    ) else {
                        return false;
                    };
                    let domain = if domain.starts_with('.') {
                        domain.to_owned()
                    } else {
                        format!(".{domain}")
                    };
                    let local = host == "localhost"
                        || host.ends_with(".localhost")
                        || host == "[::1]"
                        || host
                            .parse::<std::net::Ipv4Addr>()
                            .is_ok_and(|ip| ip.octets()[0] == 127);
                    format!(".{host}").ends_with(&domain)
                        && url.path().starts_with(path)
                        && (cookie["secure"] != true || url.scheme() == "https" || local)
                })
        })
        .cloned()
        .collect()
}
impl Contexts {
    pub(crate) fn cookie_filter_snapshot(&self, reference: Reference) -> Result<Vec<Value>, Error> {
        let client = self.get(reference)?;
        self.require_idle(reference)?;
        if client.browser.is_some() {
            return Err("HTTP_BROWSER_COOKIE_CONTEXT_REQUIRED".into());
        }
        Ok(client
            .cookies
            .lock()
            .expect("HTTP cookies")
            .filter_snapshot())
    }
    pub(crate) fn cookies_remove_selected(
        &self,
        reference: Reference,
        values: &[Value],
    ) -> Result<Value, Error> {
        let client = self.get(reference)?;
        self.require_idle(reference)?;
        if client.browser.is_some() {
            return Err("HTTP_BROWSER_COOKIE_CONTEXT_REQUIRED".into());
        }
        client
            .cookies
            .lock()
            .expect("HTTP cookies")
            .remove_selected(values);
        Ok(json!({"httpContext":reference,"cookiesCleared":true,"matchedCookies":values.len()}))
    }
    pub(crate) fn cookies_add(
        &self,
        reference: Reference,
        values: Vec<Value>,
    ) -> Result<Value, Error> {
        let client = self.get(reference)?;
        if client.browser.is_some() {
            return Err("HTTP_BROWSER_COOKIE_CONTEXT_REQUIRED".into());
        }
        let values = normalize(values)?;
        client
            .cookies
            .lock()
            .expect("HTTP cookies")
            .merge(&values)?;
        Ok(json!({"httpContext":reference,"cookiesAdded":true}))
    }
    pub(crate) fn cookies_read(&self, reference: Reference, urls: &[Url]) -> Result<Value, Error> {
        let client = self.get(reference)?;
        if client.browser.is_some() {
            return Err("HTTP_BROWSER_COOKIE_CONTEXT_REQUIRED".into());
        }
        select(
            json!(client.cookies.lock().expect("HTTP cookies").snapshot()),
            urls,
        )
    }
}
/// One pinned `SetNetworkCookie` input, shared by HTTP and browser contexts.
pub(crate) fn cookie_schema() -> Value {
    json!({"type":"object","properties":{
        "name":{"type":"string"},"value":{"type":"string"},"url":{"type":"string"},
        "domain":{"type":"string"},"path":{"type":"string"},"expires":{"type":"number"},
        "httpOnly":{"type":"boolean"},"secure":{"type":"boolean"},
        "sameSite":{"enum":["Strict","Lax","None"]},"partitionKey":{"type":"string"},
        "_crHasCrossSiteAncestor":{"type":"boolean"}},"required":["name","value"],"additionalProperties":false})
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let cookie = cookie_schema();
    vec![
        json!({"type":"object","properties":{"operation":{"const":"http_context_cookies_add"},"httpContext":reference,"cookies":{"type":"array","maxItems":256,"items":cookie}},"required":["operation","httpContext","cookies"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"http_context_cookies_read"},"httpContext":reference,"urls":crate::browser::storage::cookies::urls_schema()},"required":["operation","httpContext"],"additionalProperties":false}),
    ]
}
