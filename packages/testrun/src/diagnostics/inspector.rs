//! Shared explicit loopback inspector discovery; no redirects, proxy, DNS or scan.
use crate::error::{Error, Result};
use serde_json::Value;
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

pub(super) fn endpoint(text: &str) -> Result<url::Url> {
    let u = url::Url::parse(text)
        .map_err(|_| Error::invalid("Select an explicit loopback inspector HTTP endpoint"))?;
    let local = u
        .host_str()
        .and_then(|h| h.trim_matches(['[', ']']).parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    if text.len() > 2048
        || u.scheme() != "http"
        || !local
        || u.port().is_none_or(|p| p == 0)
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.path() != "/"
    {
        return Err(Error::invalid(
            "Use http://numeric-loopback:explicit-port without credentials, path, query or fragment; establish remote forwarding separately",
        ));
    }
    Ok(u)
}
pub(super) fn address(u: &url::Url) -> Result<SocketAddr> {
    Ok(SocketAddr::new(
        u.host_str()
            .unwrap()
            .trim_matches(['[', ']'])
            .parse()
            .map_err(|_| Error::invalid("Expected literal loopback"))?,
        u.port()
            .ok_or_else(|| Error::invalid("Explicit port required"))?,
    ))
}
pub(super) async fn get(base: &url::Url, path: &str, budget: Duration) -> Result<Value> {
    if budget.is_zero() {
        return Err(Error::new("TIMEOUT", "Inspector discovery budget expired"));
    }
    let url = base
        .join(path)
        .map_err(|_| Error::invalid("Invalid inspector path"))?;
    tokio::task::spawn_blocking(move||{
        let agent:ureq::Agent=ureq::Agent::config_builder().proxy(None).max_redirects(0).http_status_as_error(false).timeout_global(Some(budget.min(Duration::from_secs(3)))).build().into();
        let mut response=agent.get(url.as_str()).call().map_err(|_|Error::new("RN_ENDPOINT_UNAVAILABLE","Selected inspector endpoint was refused, inaccessible or timed out; no other endpoint was tried"))?;
        if response.status().as_u16()!=200{return Err(Error::new("RN_DISCOVERY_HTTP","Inspector discovery requires HTTP 200; redirects are not followed"));}
        let bytes=response.body_mut().with_config().limit(1048576).read_to_vec().map_err(|_|Error::new("RN_DISCOVERY_BOUNDS","Inspector response exceeds 1 MiB or could not be read"))?;
        crate::diagnostics::analysis::apple_json(&bytes)
    }).await.map_err(|_|Error::new("RN_DISCOVERY_FAILED","Inspector discovery worker stopped"))?
}
pub(super) fn pages(base: &url::Url, value: &Value, app: &str) -> Result<Vec<Value>> {
    let entries = value
        .as_array()
        .ok_or_else(|| Error::new("RN_DISCOVERY_FORMAT", "Expected inspector /json/list array"))?;
    if entries.len() > 64 {
        return Err(Error::new(
            "RN_DISCOVERY_BOUNDS",
            "At most 64 inspector pages may be discovered",
        ));
    }
    let mut result = Vec::new();
    for p in entries.iter().filter(|p| p["appId"] == app) {
        let invalid = || {
            Error::new(
                "RN_DISCOVERY_FORMAT",
                "Selected app's inspector page lacks supported exact identity or same-origin socket metadata",
            )
        };
        let id = p["id"].as_str().ok_or_else(invalid)?;
        let logical = p["reactNative"]["logicalDeviceId"]
            .as_str()
            .ok_or_else(invalid)?;
        super::model::label(id, 256)?;
        super::model::label(logical, 256)?;
        let socket = p["webSocketDebuggerUrl"].as_str().ok_or_else(invalid)?;
        let u = url::Url::parse(socket).map_err(|_| invalid())?;
        if !crate::browser::cdp::react_native_endpoint(socket)
            || u.host_str() != base.host_str()
            || u.port() != base.port()
        {
            return Err(invalid());
        }
        let q: Vec<_> = u.query_pairs().collect();
        let device = q
            .iter()
            .find(|(k, _)| k == "device")
            .ok_or_else(invalid)?
            .1
            .as_ref();
        let page = q
            .iter()
            .find(|(k, _)| k == "page")
            .ok_or_else(invalid)?
            .1
            .as_ref();
        if device != logical || id != format!("{device}-{page}") {
            return Err(invalid());
        }
        result.push(serde_json::json!({"id":id,"appId":app,"logicalDeviceId":logical,"webSocketDebuggerUrl":socket,"capabilities":p["reactNative"]["capabilities"],"vm":p["vm"],"deviceAssociation":"logical proxy identity; not an OS device serial or process birth"}));
    }
    Ok(result)
}
