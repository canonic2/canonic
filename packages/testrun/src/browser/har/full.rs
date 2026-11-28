//! Pinned full-mode HAR fields derived from bounded capture observations.
//! Unknown values use the pinned tracer's `-1`, empty or fallback values; nothing is invented.
use super::*;

/// Pinned `FALLBACK_HTTP_VERSION`.
const FALLBACK_HTTP_VERSION: &str = "HTTP/1.1";
const MAX_TEXT: usize = 1024;

/// Collector-supplied facts beyond minimal replay metadata, merged per entry.
pub(crate) const DETAIL_KEYS: [&str; 12] = [
    "timing",
    "requestCookieHeader",
    "requestBodySize",
    "serverIPAddress",
    "serverPort",
    "securityDetails",
    "httpVersion",
    "requestHeadersSize",
    "responseHeadersSize",
    "responseBodySize",
    "transferSize",
    "responseEnd",
];

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(MAX_TEXT).collect())
}
fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|value| value.is_finite())
}
/// A whole nonnegative count, accepting CDP's whole-number floats.
fn count(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| {
            value
                .as_f64()
                .filter(|value| value.fract() == 0.0 && *value <= 9.0e15)
                .map(|value| value as i64)
        })
        .filter(|count| *count >= 0)
}
fn size(value: &Value) -> Value {
    json!(count(value).unwrap_or(-1))
}
/// Pinned `helper.millisToRoundishMillis`.
fn roundish(millis: f64) -> f64 {
    (millis * 1000.0).round() / 1000.0
}
/// Pinned `internalHttpVersion` normalization.
pub(crate) fn http_version(raw: &str) -> String {
    match raw {
        "" => FALLBACK_HTTP_VERSION.into(),
        "http/1.1" => "HTTP/1.1".into(),
        "h2" => "HTTP/2.0".into(),
        other => other.chars().take(32).collect(),
    }
}
/// The pinned SecurityDetails subset from either backend's shape.
pub(crate) fn security(value: &Value) -> Value {
    let mut details = serde_json::Map::new();
    for key in ["protocol", "subjectName", "issuer"] {
        if let Some(text) = text(&value[key]) {
            details.insert(key.into(), json!(text));
        }
    }
    for key in ["validFrom", "validTo"] {
        if let Some(number) = number(&value[key]) {
            details.insert(key.into(), json!(number));
        }
    }
    Value::Object(details)
}
fn iso(time: time::OffsetDateTime) -> Option<String> {
    time.format(&time::format_description::well_known::Rfc3339)
        .ok()
}
/// Pinned `parseCookie` attributes; unparseable dates are omitted.
fn response_cookie(header: &str) -> Value {
    let Ok(cookie) = cookie_store::RawCookie::parse(header) else {
        let (name, value) = header.split_once('=').unwrap_or((header, ""));
        let value = value.split(';').next().unwrap_or("");
        return json!({"name":name.trim(),"value":value.trim()});
    };
    let mut value = json!({"name":cookie.name(),"value":cookie.value()});
    if let Some(domain) = cookie.domain() {
        value["domain"] = json!(domain);
    }
    if let Some(path) = cookie.path() {
        value["path"] = json!(path);
    }
    if let Some(age) = cookie.max_age() {
        if let Some(expires) = time::OffsetDateTime::now_utc()
            .checked_add(age)
            .and_then(iso)
        {
            value["expires"] = json!(expires);
        }
    } else if let Some(expires) = cookie.expires_datetime().and_then(iso) {
        value["expires"] = json!(expires);
    }
    if cookie.http_only() == Some(true) {
        value["httpOnly"] = json!(true);
    }
    if cookie.secure() == Some(true) {
        value["secure"] = json!(true);
    }
    if let Some(same_site) = cookie.same_site() {
        value["sameSite"] = json!(same_site.to_string());
    }
    value
}
fn named<'a>(headers: &'a Value, name: &'a str) -> impl Iterator<Item = &'a str> {
    headers
        .as_array()
        .into_iter()
        .flatten()
        .filter(move |header| {
            header["name"]
                .as_str()
                .is_some_and(|header| header.eq_ignore_ascii_case(name))
        })
        .filter_map(|header| header["value"].as_str())
}
fn request_cookies(headers: &Value) -> Value {
    json!(
        named(headers, "cookie")
            .flat_map(|value| value.split(';'))
            .filter(|pair| !pair.trim().is_empty())
            .map(|pair| {
                let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                json!({"name":name.trim(),"value":value.trim()})
            })
            .collect::<Vec<_>>()
    )
}
/// Pinned `_onResponse` timings plus `receive` from the response end, and `time`.
fn timings(timing: &Value, end: Option<f64>) -> (Value, f64) {
    let field = |name: &str| number(&timing[name]).unwrap_or(-1.0);
    let span = |end: f64, start: f64| {
        if end == -1.0 {
            -1.0
        } else {
            roundish(end - start)
        }
    };
    let (dns, connect, ssl, wait) = if timing.is_object() {
        (
            span(field("domainLookupEnd"), field("domainLookupStart")),
            span(field("connectEnd"), field("connectStart")),
            span(field("connectEnd"), field("secureConnectionStart")),
            span(field("responseStart"), field("requestStart")),
        )
    } else {
        (-1.0, -1.0, -1.0, -1.0)
    };
    let receive = match (end, number(&timing["responseStart"])) {
        (Some(end), Some(start)) if end != -1.0 && timing.is_object() => roundish(end - start),
        _ => -1.0,
    };
    let total = [dns, connect, ssl, wait, receive]
        .into_iter()
        .filter(|value| *value > 0.0)
        .sum();
    (
        json!({"dns":dns,"connect":connect,"ssl":ssl,"send":0,"wait":wait,"receive":receive}),
        total,
    )
}
/// Rewrite one retained capture record into a pinned full-mode entry.
pub(super) fn entry(value: &mut Value, page: Option<&str>) {
    let details = value
        .as_object_mut()
        .and_then(|entry| entry.remove("_testrunDetails"))
        .unwrap_or(Value::Null);
    let observed = value["response"]
        .as_object_mut()
        .and_then(|response| response.remove("_testrunTiming"))
        .unwrap_or(Value::Null);
    let timing = if details["timing"].is_object() {
        details["timing"].clone()
    } else {
        observed
    };
    if let Some(page) = page {
        value["pageref"] = json!(page);
    }
    let version = http_version(details["httpVersion"].as_str().unwrap_or(""));
    let url = value["request"]["url"].as_str().unwrap_or("").to_owned();
    let request = &mut value["request"];
    request["httpVersion"] = json!(version);
    request["cookies"] = request_cookies(&request["headers"]);
    if request["cookies"]
        .as_array()
        .is_some_and(std::vec::Vec::is_empty)
        && let Some(header) = details["requestCookieHeader"].as_str()
    {
        // Chromium reports the sent Cookie header separately from request headers.
        request["cookies"] = request_cookies(&json!([{"name":"Cookie","value":header}]));
    }
    request["queryString"] = json!(
        url::Url::parse(&url)
            .map(|url| url
                .query_pairs()
                .map(|(name, value)| json!({"name":name,"value":value}))
                .collect::<Vec<_>>())
            .unwrap_or_default()
    );
    request["headersSize"] = size(&details["requestHeadersSize"]);
    request["bodySize"] =
        match count(&details["requestBodySize"]).or_else(|| count(&request["postData"]["size"])) {
            Some(size) => json!(size),
            _ if request.get("postData").is_none() => json!(0),
            _ => json!(-1),
        };
    let response = &mut value["response"];
    response["httpVersion"] = json!(version);
    response["cookies"] = json!(
        named(&response["headers"], "set-cookie")
            .map(response_cookie)
            .collect::<Vec<_>>()
    );
    let location = named(&response["headers"], "location")
        .next()
        .map(str::to_owned);
    response["redirectURL"] = json!(
        location
            .and_then(|location| url::Url::parse(&url)
                .and_then(|base| base.join(&location))
                .ok())
            .map(|url| url.to_string())
            .unwrap_or_default()
    );
    response["headersSize"] = size(&details["responseHeadersSize"]);
    response["bodySize"] = size(&details["responseBodySize"]);
    response["_transferSize"] = size(&details["transferSize"]);
    let mime = named(&response["headers"], "content-type")
        .next()
        .unwrap_or("x-unknown")
        .to_owned();
    response["content"]["mimeType"] = json!(mime);
    if let (Some(decoded), Some(encoded)) = (
        count(&response["content"]["size"]),
        count(&details["responseBodySize"]),
    ) {
        response["content"]["compression"] = json!((decoded - encoded).max(0));
    }
    let (timings, total) = timings(&timing, number(&details["responseEnd"]));
    value["timings"] = timings;
    value["time"] = json!(total);
    value["cache"] = json!({});
    if let Some(address) = text(&details["serverIPAddress"]) {
        value["serverIPAddress"] = json!(address);
    }
    if let Some(port) = count(&details["serverPort"]).filter(|port| (1..=65535).contains(port)) {
        value["_serverPort"] = json!(port);
    }
    if details["securityDetails"].is_object() {
        value["_securityDetails"] = security(&details["securityDetails"]);
    }
}
/// Page script that reads the pinned tracer's title and load timings.
pub(crate) const PAGE_DETAILS: &str = "()=>({title:document.title,onContentLoad:performance.timing.domContentLoadedEventStart,onLoad:performance.timing.loadEventStart})";
/// Bounded page facts read from the page's current document at stop.
pub(super) fn page_details(value: &Value) -> Option<Value> {
    let title = value["title"].as_str()?;
    let timing = |name: &str| {
        number(&value[name])
            .filter(|value| *value > 0.0)
            .unwrap_or(-1.0)
    };
    Some(
        json!({"title":title.chars().take(MAX_TEXT).collect::<String>(),"onContentLoad":timing("onContentLoad"),"onLoad":timing("onLoad")}),
    )
}
/// Pinned page entries; a closed or unreadable page keeps the initial values.
pub(super) fn page(id: &str, started: &str, details: Option<&Value>) -> Value {
    match details {
        Some(details) => {
            json!({"startedDateTime":started,"id":id,"title":details["title"],"pageTimings":{"onContentLoad":details["onContentLoad"],"onLoad":details["onLoad"]}})
        }
        None => {
            json!({"startedDateTime":started,"id":id,"title":"","pageTimings":{"onContentLoad":-1,"onLoad":-1},"_testrunTitleUnavailable":true})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_entries_follow_the_pinned_tracer_and_keep_unknowns_explicit() {
        let mut value = json!({"startedDateTime":"2026-01-01T00:00:00Z","request":{"url":"https://example.com/acme?item=one&name=Acme%20Co","method":"POST","headers":[{"name":"Cookie","value":"session=secret; theme=dark"}],"postData":{"size":4}},"response":{"status":302,"statusText":"Found","headers":[{"name":"Set-Cookie","value":"id=Acme; Path=/; Domain=example.com; Secure; HttpOnly; SameSite=Lax; Max-Age=60"},{"name":"Location","value":"/next"},{"name":"Content-Type","value":"text/plain"}],"content":{"size":12},"_testrunTiming":{"startTime":1000.0,"domainLookupStart":1.0,"domainLookupEnd":3.5,"connectStart":3.5,"secureConnectionStart":5.0,"connectEnd":9.0,"requestStart":9.0,"responseStart":20.25}},"_testrunDetails":{"httpVersion":"h2","serverIPAddress":"127.0.0.1","serverPort":443,"securityDetails":{"protocol":"TLS 1.3","subjectName":"example.com","issuer":"Acme CA","validFrom":1.0,"validTo":2.0,"keyExchange":"ignored"},"responseBodySize":5,"responseHeadersSize":40,"requestHeadersSize":30,"transferSize":45,"responseEnd":30.5}});
        entry(&mut value, Some("page@acme"));
        assert_eq!(value["pageref"], "page@acme");
        assert_eq!(value["request"]["httpVersion"], "HTTP/2.0");
        assert_eq!(
            value["request"]["cookies"],
            json!([{"name":"session","value":"secret"},{"name":"theme","value":"dark"}])
        );
        assert_eq!(
            value["request"]["queryString"],
            json!([{"name":"item","value":"one"},{"name":"name","value":"Acme Co"}])
        );
        assert_eq!(value["request"]["bodySize"], 4);
        assert_eq!(value["request"]["headersSize"], 30);
        let cookie = &value["response"]["cookies"][0];
        assert_eq!(cookie["name"], "id");
        assert_eq!(cookie["path"], "/");
        assert_eq!(cookie["domain"], "example.com");
        assert_eq!(cookie["secure"], true);
        assert_eq!(cookie["httpOnly"], true);
        assert_eq!(cookie["sameSite"], "Lax");
        assert!(cookie["expires"].is_string());
        assert_eq!(value["response"]["redirectURL"], "https://example.com/next");
        assert_eq!(value["response"]["content"]["mimeType"], "text/plain");
        assert_eq!(value["response"]["content"]["compression"], 7);
        assert_eq!(value["response"]["_transferSize"], 45);
        assert_eq!(
            value["timings"],
            json!({"dns":2.5,"connect":5.5,"ssl":4.0,"send":0,"wait":11.25,"receive":10.25})
        );
        assert_eq!(value["time"], 33.5);
        assert_eq!(value["serverIPAddress"], "127.0.0.1");
        assert_eq!(value["_serverPort"], 443);
        assert_eq!(
            value["_securityDetails"],
            json!({"protocol":"TLS 1.3","subjectName":"example.com","issuer":"Acme CA","validFrom":1.0,"validTo":2.0})
        );
        assert!(value.get("_testrunDetails").is_none());
        assert!(value["response"].get("_testrunTiming").is_none());

        let mut unknown = json!({"request":{"url":"https://example.com/","method":"GET","headers":[]},"response":{"status":200,"headers":[],"content":{"size":-1}}});
        entry(&mut unknown, None);
        assert!(unknown.get("pageref").is_none());
        assert_eq!(unknown["request"]["httpVersion"], "HTTP/1.1");
        assert_eq!(unknown["request"]["bodySize"], 0);
        assert_eq!(unknown["response"]["bodySize"], -1);
        assert_eq!(unknown["response"]["headersSize"], -1);
        assert_eq!(unknown["response"]["content"]["mimeType"], "x-unknown");
        assert!(unknown["response"]["content"].get("compression").is_none());
        assert_eq!(
            unknown["timings"],
            json!({"dns":-1.0,"connect":-1.0,"ssl":-1.0,"send":0,"wait":-1.0,"receive":-1.0})
        );
        assert_eq!(unknown["time"], 0.0);
        assert!(unknown.get("serverIPAddress").is_none());
        assert!(unknown.get("_securityDetails").is_none());
    }
}
