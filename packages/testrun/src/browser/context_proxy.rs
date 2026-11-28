//! Context proxy normalization and the pinned engines' effective initial defaults.
use super::{
    contract::{ContextOptions, Engine},
    http::{Credential, Credentials, Header},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Private HTTP(S)/SOCKS4/5 settings for one owned browser context.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextProxy {
    pub server: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bypass: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}
impl std::fmt::Debug for ContextProxy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PrivateContextProxy")
    }
}
impl ContextProxy {
    pub(crate) fn normalized(&self) -> Result<Self, &'static str> {
        if [
            Some(&self.server),
            self.bypass.as_ref(),
            self.username.as_ref(),
            self.password.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| value.len() > 4096)
        {
            return Err("HTTP_PROXY_INVALID");
        }
        let url = url::Url::parse(&self.server)
            .ok()
            .filter(|url| url.host().is_some())
            .or_else(|| url::Url::parse(&format!("http://{}", self.server)).ok())
            .filter(|url| url.host().is_some())
            .ok_or("HTTP_PROXY_INVALID")?;
        if !matches!(url.scheme(), "http" | "https" | "socks4" | "socks5") {
            return Err("HTTP_PROXY_UNSUPPORTED");
        }
        if matches!(url.scheme(), "socks4" | "socks5")
            && [self.username.as_ref(), self.password.as_ref()]
                .into_iter()
                .flatten()
                .any(|value| !value.is_empty())
        {
            return Err("HTTP_PROXY_AUTH_UNSUPPORTED");
        }
        Ok(Self {
            server: format!(
                "{}://{}",
                url.scheme(),
                &url[url::Position::BeforeHost..url::Position::AfterPort]
            ),
            bypass: self.bypass.as_ref().map(|value| {
                value
                    .split(',')
                    .map(super::http::trim_proxy)
                    .collect::<Vec<_>>()
                    .join(",")
            }),
            username: self.username.clone(),
            password: self.password.clone(),
        })
    }
    pub(crate) fn password_required(&self) -> bool {
        self.username
            .as_ref()
            .is_some_and(|value| !value.is_empty())
            && self.password.is_none()
    }
    pub(crate) fn proxies_loopback(&self) -> bool {
        let bypass = self.bypass.as_deref().unwrap_or("");
        !bypass.split(',').any(|host| {
            matches!(
                host,
                "localhost" | "127.0.0.1" | "::1" | "[::]" | "[::1]" | "<loopback>" | "<-loopback>"
            )
        })
    }
    pub(crate) fn native_fields(&self) -> Value {
        let bypass = self.bypass.as_deref().unwrap_or("");
        let mut fields = json!({"proxyServer":self.server});
        if !self.proxies_loopback() {
            if let Some(bypass) = &self.bypass {
                fields["proxyBypassList"] = json!(bypass);
            }
        } else {
            fields["proxyBypassList"] = json!(if bypass.is_empty() {
                "<-loopback>".into()
            } else {
                format!("<-loopback>,{bypass}")
            });
        }
        fields
    }
    pub(crate) fn schema() -> Value {
        let field = json!({"type":"string","maxLength":4096});
        json!({"type":"object","properties":{"server":field,"bypass":field,"username":field,"password":field},"required":["server"],"additionalProperties":false,"description":"HTTP(S), SOCKS4 or SOCKS5 browser proxy. Context options override the browser launch default; associated HTTP clients inherit unless explicitly overridden. URI userinfo is discarded. SOCKS4/5 reject nonempty structured credentials; HTTP(S) credential/header defaults follow the pinned engine, with an explicit password required for nonempty WebKit usernames. Pinned WebKit SOCKS4 navigation sends HTTP rather than SOCKS and fails against a SOCKS server; SOCKS5 has native loopback bypass. Fields are bounded to 4096 UTF-8 bytes; other schemes remain unavailable."})
    }
}
impl ContextOptions {
    pub(crate) fn effective_for(
        &self,
        engine: Engine,
        browser_proxy: Option<&ContextProxy>,
    ) -> Result<Self, &'static str> {
        let mut effective = self.clone();
        // Applied once at creation; the secret-bearing state is not retained.
        effective.storage_state = None;
        effective.proxy = self
            .proxy
            .as_ref()
            .or(browser_proxy)
            .map(ContextProxy::normalized)
            .transpose()?;
        effective.validate_for(engine)?;
        if effective.proxy.is_none() {
            return Ok(effective);
        }
        if engine != Engine::Firefox
            && let Some(proxy) = &effective.proxy
            && let Some(username) = proxy.username.as_ref().filter(|value| !value.is_empty())
        {
            let credential = Credential {
                username: username.clone(),
                password: proxy.password.clone().unwrap_or_default(),
                origin: None,
                send: None,
            };
            if engine == Engine::Webkit {
                let added = Header {
                    name: "Proxy-Authorization".into(),
                    value: credential.header(),
                };
                let mut merged: Vec<Header> = vec![];
                for header in effective
                    .extra_http_headers
                    .take()
                    .unwrap_or_default()
                    .into_iter()
                    .chain([added])
                {
                    if let Some(index) = merged
                        .iter()
                        .position(|existing| existing.name.eq_ignore_ascii_case(&header.name))
                    {
                        merged[index] = header;
                    } else {
                        merged.push(header);
                    }
                }
                effective.extra_http_headers = Some(merged);
            }
            effective.http_credentials = Some(Credentials::Single(credential));
        }
        effective.validate()?;
        Ok(effective)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_socks_normalization_and_authentication_match_pinned_creation() {
        for scheme in ["socks4", "socks5"] {
            for credentials in [json!({}), json!({"username":"","password":""})] {
                let mut value = json!({"server":format!("{scheme}://AcmeIgnored:example@127.0.0.1:1080/ignored?Acme=1"),"bypass":" localhost , 127.0.0.1 "});
                for (key, value_field) in credentials.as_object().unwrap() {
                    value[key] = value_field.clone();
                }
                let proxy: ContextProxy = serde_json::from_value(value.clone()).unwrap();
                let normalized = proxy
                    .normalized()
                    .expect("Pinned browser SOCKS must be admitted");
                assert_eq!(normalized.server, format!("{scheme}://127.0.0.1:1080"));
                assert_eq!(normalized.bypass.as_deref(), Some("localhost,127.0.0.1"));
                for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
                    let context = ContextOptions {
                        proxy: Some(proxy.clone()),
                        ..ContextOptions::default()
                    };
                    context.validate_for(engine).unwrap();
                    let launch: super::super::contract::LaunchOptions = serde_json::from_value(
                        json!({"engine":engine,"mode":"headless","proxy":value}),
                    )
                    .unwrap();
                    launch.validate().unwrap();
                    assert_eq!(launch.wire()["proxy"], value, "SDK receives original input");
                }
            }
            for credentials in [json!({"username":"Acme"}), json!({"password":"example"})] {
                let mut value = credentials;
                value["server"] = json!(format!("{scheme}://127.0.0.1:1080"));
                let proxy: ContextProxy = serde_json::from_value(value).unwrap();
                assert_eq!(
                    proxy.normalized().unwrap_err(),
                    "HTTP_PROXY_AUTH_UNSUPPORTED"
                );
            }
        }
    }
    #[test]
    fn browser_socks_whole_context_override_and_associated_http_keep_distinct_defaults() {
        let global: ContextProxy = serde_json::from_value(
            json!({"server":"socks5://AcmeIgnored:example@127.0.0.1:1080/ignored"}),
        )
        .unwrap();
        for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
            for overridden in [false, true] {
                let mut original = options(settings());
                original.proxy = None;
                if overridden {
                    original.proxy=Some(serde_json::from_value(json!({"server":"socks4://127.0.0.1:1081","bypass":" 127.0.0.1 ","username":"","password":""})).unwrap());
                }
                let effective = original.effective_for(engine, Some(&global)).unwrap();
                assert_eq!(
                    effective.http_credentials, original.http_credentials,
                    "URI userinfo must not change browser origin credentials"
                );
                assert_eq!(effective.extra_http_headers, original.extra_http_headers);
                let mut client = super::super::http::Options::default();
                client.inherit(&effective);
                assert_eq!(
                    serde_json::to_value(client.proxy).unwrap()["server"],
                    if overridden {
                        "socks4://127.0.0.1:1081"
                    } else {
                        "socks5://127.0.0.1:1080"
                    }
                );
                assert_eq!(
                    effective.proxy.unwrap().bypass.as_deref(),
                    if overridden { Some("127.0.0.1") } else { None }
                );
            }
        }
    }
    #[test]
    fn launch_proxy_validation_and_wire_preserve_the_explicit_browser_default() {
        for engine in ["chrome", "firefox", "webkit"] {
            let proxy = json!({"server":"http://AcmeIgnored:example@127.0.0.1:8080/ignored?Acme=1","bypass":" localhost , example.com ","username":"Acme global","password":"global example"});
            let options: super::super::contract::LaunchOptions =
                serde_json::from_value(json!({"engine":engine,"mode":"headless","proxy":proxy}))
                    .expect("The browser-level proxy contract must be admitted");
            options.validate().unwrap();
            assert_eq!(
                options.wire()["proxy"],
                proxy,
                "The pinned SDK owns normalization of original launch input"
            );
        }
    }
    #[test]
    fn launch_proxy_context_override_precedes_global_defaults_and_http_inheritance() {
        let global:ContextProxy=serde_json::from_value(json!({"server":"http://AcmeIgnored:example@127.0.0.1:8080/ignored","username":"Acme global","password":"global example"})).unwrap();
        let mut original = options(settings());
        original.proxy = None;
        let url = url::Url::parse("http://127.0.0.1/auth").unwrap();
        for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
            for explicit in [false, true] {
                let mut input = original.clone();
                if explicit {
                    input.proxy=Some(serde_json::from_value(json!({"server":"127.0.0.1:9090","username":"Acme context","password":"context example"})).unwrap());
                }
                let effective = input.effective_for(engine, Some(&global)).unwrap();
                let credential = effective
                    .http_credentials
                    .as_ref()
                    .unwrap()
                    .matching(&url)
                    .unwrap();
                assert_eq!(
                    credential.username,
                    if engine == Engine::Firefox {
                        "Acme origin"
                    } else if explicit {
                        "Acme context"
                    } else {
                        "Acme global"
                    }
                );
                let mut client = super::super::http::Options::default();
                client.inherit(&effective);
                assert_eq!(
                    serde_json::to_value(&client.proxy).unwrap()["server"],
                    if explicit {
                        "http://127.0.0.1:9090"
                    } else {
                        "http://127.0.0.1:8080"
                    }
                );
                assert_eq!(
                    input.wire().get("proxy").is_some(),
                    explicit,
                    "Global defaults must not become explicit context wire options"
                );
                if engine == Engine::Webkit {
                    assert!(
                        effective
                            .extra_http_headers
                            .as_ref()
                            .unwrap()
                            .iter()
                            .any(|header| header.name == "Proxy-Authorization"
                                && header.value == credential.header())
                    );
                }
            }
        }
        assert!(original.proxy.is_none());
        assert!(
            global.server.contains("AcmeIgnored"),
            "Resolving a context must not rewrite the retained launch input"
        );
    }
    #[test]
    fn launch_proxy_webkit_password_validation_uses_the_selected_context_default() {
        let value = json!({"engine":"webkit","mode":"headless","proxy":{"server":"127.0.0.1:8080","username":"Acme global"}});
        let options: super::super::contract::LaunchOptions =
            serde_json::from_value(value.clone()).unwrap();
        assert_eq!(options.validate(), Err("HTTP_PROXY_PASSWORD_REQUIRED"));
        let mut overridden = value.clone();
        overridden["context"] = json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"proxy":{"server":"127.0.0.1:9090","username":"Acme context","password":""}});
        serde_json::from_value::<super::super::contract::LaunchOptions>(overridden)
            .unwrap()
            .validate()
            .unwrap();
        let mut invalid = value;
        invalid["proxy"]["server"] = json!("socks6://127.0.0.1:8080");
        assert_eq!(
            serde_json::from_value::<super::super::contract::LaunchOptions>(invalid)
                .unwrap()
                .validate(),
            Err("HTTP_PROXY_UNSUPPORTED")
        );
    }
    fn options(proxy: Value) -> ContextOptions {
        serde_json::from_value(json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,
            "proxy":proxy,"httpCredentials":{"username":"Acme origin","password":"origin example"},
            "extraHTTPHeaders":[{"name":"X-Acme","value":"first"},{"name":"proxy-authorization","value":"caller"},{"name":"x-ACME","value":"last"}]})).unwrap()
    }
    fn settings() -> Value {
        json!({"server":"127.0.0.1:8080","username":"Acme proxy","password":"proxy example"})
    }
    #[test]
    fn context_proxy_normalization_discards_uri_credentials_and_preserves_explicit_identity() {
        let mut value = settings();
        value["server"] =
            json!("http://AcmeIgnored:example@EXAMPLE.COM:80/ignored?Acme=1#fragment");
        value["bypass"] = json!(format!(
            "{}localhost , example.com{}",
            char::from_u32(0xfeff).unwrap(),
            char::from_u32(0x3000).unwrap()
        ));
        let proxy: ContextProxy = serde_json::from_value(value).unwrap();
        let normalized = proxy.normalized().unwrap();
        assert_eq!(normalized.server, "http://example.com");
        assert_eq!(normalized.bypass.as_deref(), Some("localhost,example.com"));
        assert_eq!(normalized.username.as_deref(), Some("Acme proxy"));
        assert_eq!(normalized.password.as_deref(), Some("proxy example"));
        assert_eq!(format!("{proxy:?}"), "PrivateContextProxy");
        let proxy: ContextProxy = serde_json::from_value(
            json!({"server":"http://AcmeIgnored:bad%escape@127.0.0.1:8080/path"}),
        )
        .unwrap();
        let normalized = proxy.normalized().unwrap();
        assert!(normalized.username.is_none());
        let client = super::super::http::Options {
            proxy: Some(normalized.into()),
            ..Default::default()
        };
        client.validate().unwrap();
        let fallback: ContextProxy = serde_json::from_value(json!({"server":"http://["})).unwrap();
        assert_eq!(
            fallback.normalized().unwrap().server,
            "http://http",
            "The pinned second URL parse treats this as host http and an ignored path"
        );
    }
    #[test]
    fn context_proxy_engine_defaults_match_origin_auth_and_webkit_header_precedence() {
        let original = options(settings());
        let url = url::Url::parse("http://127.0.0.1/auth").unwrap();
        for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
            let effective = original.effective_for(engine, None).unwrap();
            let credential = effective
                .http_credentials
                .as_ref()
                .unwrap()
                .matching(&url)
                .unwrap();
            assert_eq!(
                credential.username,
                if engine == Engine::Firefox {
                    "Acme origin"
                } else {
                    "Acme proxy"
                }
            );
            if engine == Engine::Webkit {
                assert_eq!(
                    effective.extra_http_headers.as_ref().unwrap(),
                    &vec![
                        Header {
                            name: "x-ACME".into(),
                            value: "last".into()
                        },
                        Header {
                            name: "Proxy-Authorization".into(),
                            value: credential.header()
                        },
                    ]
                );
            } else {
                assert_eq!(effective.extra_http_headers, original.extra_http_headers);
            }
            assert_eq!(
                original.wire()["httpCredentials"][0]["username"],
                "Acme origin",
                "The SDK receives caller input before deriving its own defaults"
            );
        }
    }
    #[test]
    fn context_proxy_missing_and_empty_passwords_keep_the_pinned_engine_difference() {
        let original = options(json!({"server":"127.0.0.1:8080","username":"Acme proxy"}));
        let url = url::Url::parse("http://127.0.0.1/auth").unwrap();
        for engine in [Engine::Chrome, Engine::Firefox] {
            let effective = original.effective_for(engine, None).unwrap();
            assert_eq!(
                effective
                    .http_credentials
                    .as_ref()
                    .unwrap()
                    .matching(&url)
                    .unwrap()
                    .password,
                if engine == Engine::Chrome {
                    ""
                } else {
                    "origin example"
                }
            );
        }
        assert_eq!(
            original.effective_for(Engine::Webkit, None).unwrap_err(),
            "HTTP_PROXY_PASSWORD_REQUIRED"
        );
        let explicit =
            options(json!({"server":"127.0.0.1:8080","username":"Acme proxy","password":""}));
        explicit.effective_for(Engine::Webkit, None).unwrap();
        let empty = options(json!({"server":"127.0.0.1:8080","username":""}));
        assert_eq!(
            empty
                .effective_for(Engine::Webkit, None)
                .unwrap()
                .http_credentials,
            empty.http_credentials
        );
    }
    #[test]
    fn context_proxy_native_bypass_preserves_forced_loopback_and_exact_tokens() {
        for (bypass, expected) in [
            (None, "<-loopback>"),
            (Some(""), "<-loopback>"),
            (
                Some(" example.com , .acme.test "),
                "<-loopback>,example.com,.acme.test",
            ),
            (Some(" localhost , 127.0.0.1 "), "localhost,127.0.0.1"),
            (Some("[::1]"), "[::1]"),
            (Some("<loopback>"), "<loopback>"),
            (Some("<-loopback>"), "<-loopback>"),
            (Some("LOCALHOST"), "<-loopback>,LOCALHOST"),
        ] {
            let proxy = ContextProxy {
                server: "http://127.0.0.1:8080".into(),
                bypass: bypass.map(str::to_owned),
                username: None,
                password: None,
            }
            .normalized()
            .unwrap();
            assert_eq!(
                proxy.native_fields(),
                json!({"proxyServer":"http://127.0.0.1:8080","proxyBypassList":expected})
            );
        }
    }
    #[test]
    fn context_proxy_effective_headers_keep_the_existing_bounds() {
        let mut original = options(settings());
        original.extra_http_headers = Some(
            (0..64)
                .map(|index| Header {
                    name: format!("X-Acme-{index}"),
                    value: "one".into(),
                })
                .collect(),
        );
        original.validate_for(Engine::Webkit).unwrap();
        assert!(
            original.effective_for(Engine::Webkit, None).is_err(),
            "Proxy-injected defaults count toward the existing header budget"
        );
        original.effective_for(Engine::Chrome, None).unwrap();
    }
    #[test]
    fn context_proxy_http_inheritance_preserves_explicit_proxy_and_tls_false() {
        let mut original = options(settings());
        original.ignore_https_errors = Some(true);
        let effective = original.effective_for(Engine::Firefox, None).unwrap();
        let mut inherited = super::super::http::Options::default();
        inherited.inherit(&effective);
        assert_eq!(
            serde_json::to_value(&inherited.proxy).unwrap(),
            json!({"server":"http://127.0.0.1:8080","username":"Acme proxy","password":"proxy example"})
        );
        let mut explicit = super::super::http::Options {
            proxy: Some("http://Acme:example@127.0.0.1:9090".to_string().into()),
            ignore_https_errors: Some(false),
            ..Default::default()
        };
        explicit.inherit(&effective);
        assert_eq!(
            serde_json::to_value(&explicit.proxy).unwrap(),
            "http://Acme:example@127.0.0.1:9090"
        );
        assert_eq!(explicit.ignore_https_errors, Some(false));
    }
    #[test]
    fn context_proxy_normalization_refuses_unsupported_protocols_and_over_budget_fields() {
        for proxy in [
            json!({"server":"socks6://127.0.0.1:8080"}),
            json!({"server":"["}),
            json!({"server":"x".repeat(4097)}),
            json!({"server":"127.0.0.1:8080","bypass":"x".repeat(4097)}),
        ] {
            let proxy: ContextProxy = serde_json::from_value(proxy).unwrap();
            assert!(proxy.normalized().is_err());
        }
    }
}
