//! React Native's bounded protocol subset, independent of Chrome's pinned schemas.
use serde_json::Value;
pub(crate) const PROBE: &str = "(()=>{const d=Object.getOwnPropertyDescriptor(globalThis,'__TESTRUN_DIAGNOSTICS_V1__');const p=d&&d.value;const h=globalThis.HermesInternal;return JSON.stringify({schemaVersion:1,hermes:!!h&&typeof h.getRuntimeProperties==='function',hermesProperties:h&&typeof h.getRuntimeProperties==='function'?h.getRuntimeProperties():null,probe:p&&typeof p.snapshot==='function'?p.snapshot():null});})()";
pub(super) fn endpoint(endpoint: &str) -> bool {
    let Ok(url) = url::Url::parse(endpoint) else {
        return false;
    };
    let local = url
        .host_str()
        .and_then(|h| h.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    let fields: Vec<_> = url.query_pairs().collect();
    endpoint.len() <= 2048
        && url.scheme() == "ws"
        && local
        && url.port().is_some_and(|p| p > 0)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.path() == "/inspector/debug"
        && fields.len() == 2
        && ["device", "page"].iter().all(|name| {
            fields
                .iter()
                .filter(|(key, value)| {
                    key == name
                        && !value.is_empty()
                        && value.len() <= 256
                        && !value.chars().any(char::is_control)
                })
                .count()
                == 1
        })
        && fields.iter().all(|(k, v)| k != "page" || v != "-1")
}
pub(super) fn command(method: &str, params: &Value) -> bool {
    let Some(fields) = params.as_object() else {
        return false;
    };
    match method {
        "Runtime.enable"
        | "Runtime.disable"
        | "Debugger.enable"
        | "Debugger.disable"
        | "Log.enable"
        | "Log.disable"
        | "Network.enable"
        | "Network.disable"
        | "Profiler.start"
        | "Profiler.stop"
        | "HeapProfiler.stopSampling" => fields.is_empty(),
        "HeapProfiler.startSampling" => {
            params == &serde_json::json!({"samplingInterval":32768,"stackDepth":32})
        }
        "Runtime.evaluate" => {
            fields
                .keys()
                .all(|k| ["expression", "returnByValue", "contextId"].contains(&k.as_str()))
                && fields.len() == 3
                && params["expression"] == PROBE
                && params["returnByValue"] == true
                && params["contextId"]
                    .as_i64()
                    .is_some_and(|n| n > 0 && n <= i32::MAX as i64)
        }
        _ => false,
    }
}
pub(super) fn control(method: &str, params: &Value) -> bool {
    command(method, params)
        && (method.ends_with(".disable")
            || matches!(method, "Profiler.stop" | "HeapProfiler.stopSampling"))
}
