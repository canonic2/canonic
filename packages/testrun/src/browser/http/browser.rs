//! Browser cookie exchange for host HTTP requests; never a copied persistent jar.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CookiePartition {
    pub top_level_site: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_cross_site_ancestor: Option<bool>,
}
impl CookiePartition {
    pub(super) fn validate(&self) -> Result<(), Error> {
        let url = absolute_url(&self.top_level_site)
            .map_err(|_| Error::from("HTTP_COOKIE_PARTITION_INVALID"))?;
        if self.top_level_site.len() > 2048
            || url.origin().ascii_serialization() != self.top_level_site
        {
            return Err("HTTP_COOKIE_PARTITION_INVALID".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartitionModel {
    Unsupported,
    Site,
    SiteAndAncestor,
}
pub(crate) trait BrowserCookies: Send + Sync {
    /// Whether an absent partition key certifies an unpartitioned cookie.
    fn partition_identity_available(&self) -> bool;
    fn read(&self, deadline: Instant) -> Result<Value, Error>;
    fn write(&self, cookies: Value, deadline: Instant) -> Result<(), Error>;
    fn partition_model(&self) -> PartitionModel {
        PartitionModel::Unsupported
    }
}
pub(crate) fn header(
    cookies: &Value,
    url: &Url,
    partition: Option<&CookiePartition>,
) -> Result<String, Error> {
    let items = cookies.as_array().ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
    let host = url.host_str().ok_or("HTTP_URL_INVALID")?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp() as f64;
    let mut pairs = Vec::new();
    let mut bytes = 0;
    for cookie in items {
        if cookie.get("partitionKeyOpaque") == Some(&json!(true)) {
            continue;
        }
        if let Some(key) = cookie.get("partitionKey").filter(|key| !key.is_null()) {
            let Some(partition) = partition else {
                continue;
            };
            if key.as_str() != Some(partition.top_level_site.as_str())
                || partition.has_cross_site_ancestor.is_some_and(|expected| {
                    cookie["_crHasCrossSiteAncestor"].as_bool() != Some(expected)
                })
            {
                continue;
            }
        }
        let Some(domain) = cookie["domain"].as_str() else {
            continue;
        };
        let matches = if let Some(domain) = domain.strip_prefix('.') {
            host == domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        } else {
            host == domain
        };
        if !matches || cookie["secure"] == true && url.scheme() != "https" {
            continue;
        }
        if cookie["expires"]
            .as_f64()
            .is_some_and(|expires| expires >= 0.0 && expires <= now)
        {
            continue;
        }
        let Some(path) = cookie["path"].as_str() else {
            continue;
        };
        if url.path() != path
            && !(url.path().starts_with(path)
                && (path.ends_with('/') || url.path().as_bytes().get(path.len()) == Some(&b'/')))
        {
            continue;
        }
        let (Some(name), Some(value)) = (cookie["name"].as_str(), cookie["value"].as_str()) else {
            continue;
        };
        if name
            .chars()
            .chain(value.chars())
            .any(|c| c == '\r' || c == '\n' || c == ';')
        {
            return Err("HTTP_COOKIE_INVALID".into());
        }
        bytes += name.len() + value.len() + 3;
        if bytes > 65536 {
            return Err("HTTP_COOKIE_HEADER_BOUNDS".into());
        }
        pairs.push(format!("{name}={value}"));
    }
    Ok(pairs.join("; "))
}
pub(crate) fn response_cookies(
    url: &Url,
    headers: &ureq::http::HeaderMap,
    partition: Option<&CookiePartition>,
) -> Value {
    let mut result = Vec::new();
    for raw in headers.get_all(ureq::http::header::SET_COOKIE) {
        let Ok(raw) = raw.to_str() else {
            continue;
        };
        if raw.len() > 4096 {
            continue;
        }
        let Ok(cookie) = cookie_store::Cookie::parse(raw.to_owned(), url) else {
            continue;
        };
        // A server cannot choose the top-level partition authority for a host client.
        if cookie.partitioned() == Some(true)
            && (partition.is_none() || cookie.secure() != Some(true) || url.scheme() != "https")
        {
            continue;
        }
        let Some(domain) = cookie.domain.as_cow() else {
            continue;
        };
        let domain = match &cookie.domain {
            cookie_store::CookieDomain::Suffix(_) => format!(".{domain}"),
            _ => domain.into_owned(),
        };
        let expires = match &cookie.expires {
            cookie_store::CookieExpiration::AtUtc(when) => when.unix_timestamp() as f64,
            cookie_store::CookieExpiration::SessionEnd => -1.0,
        };
        let mut value = json!({"name":cookie.name(),"value":cookie.value(),"domain":domain,"path":cookie.path.as_ref(),"expires":expires,"httpOnly":cookie.http_only().unwrap_or(false),"secure":cookie.secure().unwrap_or(false)});
        if let Some(same_site) = cookie.same_site() {
            value["sameSite"] = json!(same_site.to_string());
        }
        if cookie.partitioned() == Some(true) {
            let partition = partition.expect("Selected partition authority");
            value["partitionKey"] = json!(partition.top_level_site);
            if let Some(ancestor) = partition.has_cross_site_ancestor {
                value["_crHasCrossSiteAncestor"] = json!(ancestor);
            }
        }
        result.push(value);
    }
    json!(result)
}
