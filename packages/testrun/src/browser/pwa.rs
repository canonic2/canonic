//! Typed lifecycle actions for standard web apps installed by an owned browser.
use super::contract::Reference;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Install {
        page: Reference,
    },
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u64>,
    },
    Launch {
        #[serde(rename = "appId")]
        app_id: Uuid,
        #[serde(default)]
        url: Option<String>,
    },
    Uninstall {
        #[serde(rename = "appId")]
        app_id: Uuid,
    },
}

pub(super) fn schema(reference: &Value) -> Value {
    let id = json!({"type":"string","format":"uuid"});
    let url = json!({"type":"string","minLength":1,"maxLength":2048});
    json!({"type":"object","properties":{"operation":{"const":"browser_pwa"},"action":{"oneOf":[
        {"type":"object","properties":{"kind":{"const":"install"},"page":reference},"required":["kind","page"],"additionalProperties":false},
        list_schema(),
        {"type":"object","properties":{"kind":{"const":"launch"},"appId":id,"url":url},"required":["kind","appId"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"uninstall"},"appId":id},"required":["kind","appId"],"additionalProperties":false}
    ]}},"required":["operation","action"],"additionalProperties":false})
}

pub(super) fn list_schema() -> Value {
    let mut properties = crate::pagination::properties(canonic_mcp_kit::page::Limits::STANDARD);
    properties["kind"] = json!({"const":"list"});
    json!({"type":"object","properties":properties,"required":["kind"],"additionalProperties":false})
}

pub(super) fn standard_url(value: &str) -> Result<Url, &'static str> {
    let url = Url::parse(value).map_err(|_| "PWA_URL_INVALID")?;
    if value.len() > 2048 || !url.username().is_empty() || url.password().is_some() {
        return Err("PWA_URL_INVALID");
    }
    let trustworthy = url.scheme() == "https"
        || url.scheme() == "http"
            && match url.host() {
                Some(url::Host::Ipv4(address)) => address.is_loopback(),
                Some(url::Host::Ipv6(address)) => address.is_loopback(),
                Some(url::Host::Domain(name)) => name == "localhost",
                _ => false,
            };
    if !trustworthy {
        return Err("PWA_URL_INVALID");
    }
    Ok(url)
}

pub(super) fn in_scope(url: &Url, scope: &Url) -> bool {
    url.origin() == scope.origin() && url.path().starts_with(scope.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pwa_scope_is_url_origin_and_path_prefix() {
        let scope = standard_url("http://127.0.0.1:8080/acme/").unwrap();
        assert!(in_scope(
            &standard_url("http://127.0.0.1:8080/acme/home").unwrap(),
            &scope
        ));
        for url in [
            "http://127.0.0.1:8080/acme-other/",
            "http://127.0.0.1:8081/acme/",
            "https://127.0.0.1:8080/acme/",
        ] {
            assert!(!in_scope(&standard_url(url).unwrap(), &scope), "{url}");
        }
        for url in [
            "isolated-app://acme",
            "file:///tmp/acme",
            "http://example.com/acme",
            "https://user@example.com/acme",
        ] {
            assert!(standard_url(url).is_err(), "{url}");
        }
    }
}
