//! Private bounded response facts. Missing timing uses the pinned SDK sentinels.
use serde_json::{Value, json};
pub(crate) const TIMING_FIELDS: [&str; 9] = [
    "startTime",
    "domainLookupStart",
    "domainLookupEnd",
    "connectStart",
    "secureConnectionStart",
    "connectEnd",
    "requestStart",
    "responseStart",
    "responseEnd",
];
pub(crate) fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite() && *n >= -1.0)
}
pub(crate) fn timing(source: &Value, end: Option<f64>) -> Value {
    let mut result = json!({});
    for name in TIMING_FIELDS {
        result[name] = json!(
            number(&source[name])
                .filter(|n| name != "startTime" || *n >= 0.0)
                .unwrap_or(if name == "startTime" { 0.0 } else { -1.0 })
        );
    }
    if let Some(end) = end.filter(|n| n.is_finite() && *n >= -1.0) {
        result["responseEnd"] = json!(end);
        if result["responseStart"] == -1.0 {
            result["responseStart"] = json!(end);
        }
    }
    result
}
pub(crate) fn details(
    timing_source: &Value,
    end: Option<f64>,
    address: &Value,
    security: &Value,
) -> Value {
    let address = address["ipAddress"]
        .as_str()
        .filter(|ip| {
            ip.len() <= 128
                && ip
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok()
        })
        .zip(address["port"].as_u64().filter(|port| *port <= 65535))
        .map(|(ip, port)| json!({"ipAddress":ip,"port":port}));
    let mut tls = json!({});
    for name in ["protocol", "subjectName", "issuer"] {
        if let Some(value) = security[name]
            .as_str()
            .filter(|s| s.len() <= 2048 && !s.chars().any(char::is_control))
        {
            tls[name] = json!(value);
        }
    }
    for name in ["validFrom", "validTo"] {
        if let Some(value) = security[name].as_f64().filter(|n| n.is_finite()) {
            tls[name] = json!(value);
        }
    }
    let observed = TIMING_FIELDS
        .into_iter()
        .filter(|name| {
            number(&timing_source[*name]).is_some_and(|n| *name != "startTime" || n >= 0.0)
        })
        .chain(
            end.filter(|n| n.is_finite() && *n >= -1.0)
                .map(|_| "responseEnd"),
        )
        .collect::<std::collections::BTreeSet<_>>();
    let omitted = ["protocol", "subjectName", "issuer", "validFrom", "validTo"]
        .into_iter()
        .filter(|name| !security[*name].is_null() && tls.get(*name).is_none())
        .collect::<Vec<_>>();
    json!({"serverAddr":address,"securityDetails":if tls.as_object().unwrap().is_empty(){Value::Null}else{tls},"connectionDetailsPending":false,"securityFieldsOmitted":omitted,"timing":timing(timing_source,end),"timingObservedFields":observed,"timingUnits":{"startTime":"unix-epoch-ms","offsets":"ms-from-startTime","certificateValidity":"unix-epoch-seconds"},"missingTiming":"pinned-sdk-zero-start-minus-one-offsets","private":true})
}
pub(crate) fn finish(details: &mut Value, end: Option<f64>) {
    let Some(end) = end.filter(|n| n.is_finite() && *n >= -1.0) else {
        return;
    };
    details["timing"] = timing(&details["timing"], Some(end));
    let fields = details["timingObservedFields"]
        .as_array_mut()
        .expect("Projected timing fields");
    if !fields.iter().any(|field| field == "responseEnd") {
        fields.push(json!("responseEnd"));
    }
}
pub(crate) fn cdp_timing(
    response: &Value,
    started: Option<f64>,
    wall: Option<f64>,
    cached: bool,
) -> Value {
    let raw = &response["timing"];
    let mut result = json!({});
    let request_time = number(&raw["requestTime"]);
    if let Some(wall) = wall {
        let start = if !cached && raw.is_object() {
            request_time
                .zip(started)
                .map(|(time, start)| time - start + wall)
        } else {
            Some(wall)
        };
        if let Some(start) = start.filter(|s| s.is_finite() && *s >= 0.0) {
            result["startTime"] = json!(start * 1000.0);
        }
    }
    if !cached && raw.is_object() {
        for (to, from) in [
            ("domainLookupStart", "dnsStart"),
            ("domainLookupEnd", "dnsEnd"),
            ("connectStart", "connectStart"),
            ("secureConnectionStart", "sslStart"),
            ("connectEnd", "connectEnd"),
            ("requestStart", "sendStart"),
            ("responseStart", "receiveHeadersEnd"),
        ] {
            if let Some(n) = number(&raw[from]) {
                result[to] = json!(n);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn response_details_bound_invalid_connection_facts_without_hiding_omissions() {
        let value = details(
            &json!({"startTime":-1,"connectStart":-2}),
            None,
            &json!({"ipAddress":"Acme.invalid","port":70000}),
            &json!({"issuer":"A".repeat(2049),"subjectName":"Acme\nprivate","validFrom":-10,"validTo":20,"protocol":"TLS 1.3"}),
        );
        assert!(value["serverAddr"].is_null());
        assert_eq!(
            value["securityFieldsOmitted"],
            json!(["subjectName", "issuer"])
        );
        assert_eq!(value["securityDetails"]["validFrom"], -10.0);
        assert_eq!(value["timingObservedFields"], json!([]));
        assert!(!value.to_string().contains("Acme"));
    }
    #[test]
    fn response_details_keep_units_unknowns_and_only_the_certificate_summary() {
        let value = details(
            &json!({"startTime":1000.0,"responseStart":5.0}),
            Some(8.0),
            &json!({"ipAddress":"[::1]","port":443}),
            &json!({"protocol":"TLS 1.3","subjectName":"example.com","issuer":"Acme CA","validFrom":10,"validTo":20,"signedCertificateTimestampList":["Acme-secret"]}),
        );
        assert_eq!(value["serverAddr"]["ipAddress"], "[::1]");
        assert_eq!(value["timing"]["responseEnd"], 8.0);
        assert_eq!(value["timing"]["connectStart"], -1.0);
        assert_eq!(value["securityDetails"]["validTo"], 20.0);
        assert!(!value.to_string().contains("Acme-secret"));
        let missing = details(&Value::Null, None, &Value::Null, &Value::Null);
        assert!(missing["serverAddr"].is_null() && missing["securityDetails"].is_null());
        assert_eq!(missing["timing"]["startTime"], 0.0);
        assert_eq!(missing["timingObservedFields"], json!([]));
    }
    #[test]
    fn response_details_native_timing_uses_the_pinned_wall_clock_conversion_and_cache_fallback() {
        let response = json!({"timing":{"requestTime":10.5,"dnsStart":1.0,"sendStart":2.0,"receiveHeadersEnd":3.0}});
        let raw = cdp_timing(&response, Some(10.0), Some(1000.0), false);
        assert_eq!(raw["startTime"], 1000500.0);
        assert_eq!(raw["responseStart"], 3.0);
        let cached = cdp_timing(&response, Some(10.0), Some(1000.0), true);
        assert_eq!(cached, json!({"startTime":1000000.0}));
        let finished = timing(&cached, Some(7.0));
        assert_eq!(finished["responseStart"], 7.0);
        assert_eq!(finished["responseEnd"], 7.0);
    }
}
