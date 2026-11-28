//! Bounded Expo manifest inspection; advertised URLs are evidence, never fetched.
use super::*;
use std::time::Duration;
pub(super) fn endpoint(value: &str) -> Result<url::Url> {
    configuration::literal(value)?;
    let url = url::Url::parse(value).map_err(|_| Error::invalid("Invalid Expo endpoint"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::invalid(
            "Expo endpoint must be HTTP(S) without credentials, query or fragment",
        ));
    }
    Ok(url)
}
pub(super) fn connection(value: &str) -> Result<()> {
    configuration::literal(value)?;
    let url =
        url::Url::parse(value).map_err(|_| Error::invalid("Invalid device connection URL"))?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(Error::invalid(
            "Connection URL must not contain credentials or a fragment",
        ));
    }
    if !matches!(url.scheme(), "http" | "https" | "exp" | "exps") {
        // Custom development-client links may carry a nested server URL.
        let nested: Vec<_> = url.query_pairs().collect();
        if nested.len() != 1 || nested[0].0 != "url" {
            return Err(Error::invalid(
                "Custom client connection URL requires only one url query parameter",
            ));
        }
        endpoint(&nested[0].1)?;
    } else if url.query().is_some() || url.host_str().is_none() {
        return Err(Error::invalid(
            "Connection URL requires a host and no query except the custom client's url parameter",
        ));
    }
    Ok(())
}
fn loopback(value: &str) -> bool {
    url::Url::parse(value).ok().is_some_and(|url| {
        url.host_str().is_some_and(|h| {
            h.eq_ignore_ascii_case("localhost")
                || h.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        }) || url
            .query_pairs()
            .any(|(key, value)| key == "url" && loopback(&value))
    })
}
pub(super) async fn inspect(expo: Expo, platform: String, physical: bool) -> Value {
    let result = tokio::task::spawn_blocking(move || fetch(&expo, &platform, physical)).await;
    match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            json!({"state":"blocked","error":error,"nextAction":"Inspect the configured Expo endpoint and its execution owner's diagnostics"})
        }
        Err(_) => json!({"state":"unknown","reason":"Expo inspection worker stopped"}),
    }
}
fn fetch(expo: &Expo, platform: &str, physical: bool) -> Result<Value> {
    endpoint(&expo.url)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into();
    let mut response = agent
        .get(&expo.url)
        .header("expo-platform", platform)
        .header("accept", "application/expo+json, application/json")
        .call()
        .map_err(transport_error)?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(Error::new(
            "EXPO_HTTP",
            "Expo endpoint returned a non-success response; redirects are not followed",
        )
        .details(json!({"status":status})));
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(1_048_576)
        .read_to_vec()
        .map_err(|_| Error::new("EXPO_RESPONSE", "Cannot read Expo response within 1 MiB"))?;
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new("EXPO_PROTOCOL", "Expected an Expo JSON manifest"))?;
    summarize(expo, platform, physical, &manifest)
}
pub(super) fn transport_error(error: ureq::Error) -> Error {
    if matches!(error,ureq::Error::Io(ref io) if io.kind()==std::io::ErrorKind::ConnectionRefused) {
        Error::new(
            "EXPO_UNREACHABLE",
            "The configured Expo connection was refused",
        )
    } else {
        Error::new(
            "EXPO_INSPECTION_FAILED",
            "Expo inspection failed or timed out; this does not establish that the endpoint is unused",
        )
    }
}
pub(super) fn summarize(
    expo: &Expo,
    platform: &str,
    physical: bool,
    manifest: &Value,
) -> Result<Value> {
    let config = manifest.pointer("/extra/expoClient").unwrap_or(manifest);
    if config["slug"] != expo.slug {
        return Err(Error::new(
            "EXPO_PROJECT_MISMATCH",
            "Manifest does not identify the configured Expo slug",
        ));
    }
    let app_id = config
        .pointer(if platform == "ios" {
            "/ios/bundleIdentifier"
        } else {
            "/android/package"
        })
        .and_then(Value::as_str);
    if app_id.is_some_and(|id| {
        id.len() > 256
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    }) {
        return Err(Error::new(
            "EXPO_PROTOCOL",
            "Manifest application identity is invalid",
        ));
    }
    let bundle = manifest
        .pointer("/launchAsset/url")
        .or_else(|| manifest.get("bundleUrl"))
        .and_then(Value::as_str);
    let host = config["hostUri"].as_str();
    let candidate = expo
        .connection_url
        .clone()
        .or_else(|| host.map(|v| format!("exp://{v}")));
    let usable = candidate.as_deref().is_some_and(|v| connection(v).is_ok());
    let blocked = physical
        && expo.connection_mode.as_deref() != Some("forwarded")
        && candidate.as_deref().is_some_and(loopback);
    // Only return URLs that satisfy the credential-free projection rules.
    let bundle = bundle.filter(|v| endpoint(v).is_ok());
    Ok(
        json!({"state":if blocked {"blocked"} else {"ready"},"endpoint":expo.url,"slug":expo.slug,"manifestAppId":app_id,"platform":platform,"bundleUrl":bundle,"connectionUrl":candidate.filter(|_|usable),"connectionState":if blocked {"blocked"} else if usable {"candidate"} else {"unknown"},"owner":expo.owner,"evidence":"host_manifest_response","deviceLoaded":"unknown","nextAction":if blocked {"Configure a LAN, tunnel or explicit forwarding URL reachable by the physical phone"} else {"Open the connection URL in the intended development client; observe the selected device to establish loading and rendering"}}),
    )
}
