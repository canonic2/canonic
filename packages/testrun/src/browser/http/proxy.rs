//! Private CONNECT settings and the pinned request client's destination bypass rules.
use super::*;
pub(super) mod refusal;
pub(super) mod socks;
pub(super) mod transport;

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Input {
    Url(String),
    Settings(Settings),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    server: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bypass: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    password: Option<String>,
}
impl From<String> for Input {
    fn from(value: String) -> Self {
        Self::Url(value)
    }
}
impl From<crate::browser::contract::ContextProxy> for Input {
    fn from(value: crate::browser::contract::ContextProxy) -> Self {
        Self::Settings(Settings {
            server: value.server,
            bypass: value.bypass,
            username: value.username,
            password: value.password,
        })
    }
}
impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrivateHttpProxy")
    }
}
#[derive(Clone)]
pub(super) struct Authorization(Arc<[u8]>);
impl std::fmt::Debug for Authorization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrivateProxyAuthorization")
    }
}
impl Authorization {
    pub(super) fn bytes(&self) -> &[u8] {
        &self.0
    }
}
pub(super) struct Prepared {
    pub proxy: ureq::Proxy,
    pub authorization: Option<Authorization>,
    pub socks: Option<socks::Socks>,
    bypass: Option<String>,
}
impl Prepared {
    pub(super) fn has_bypass(&self) -> bool {
        self.bypass.as_ref().is_some_and(|value| !value.is_empty())
    }
    pub(super) fn bypasses(&self, url: &Url) -> bool {
        let host = &url[url::Position::BeforeHost..url::Position::AfterHost];
        bypasses(host, self.bypass.as_deref())
    }
}
pub(crate) fn bypasses(host: &str, bypass: Option<&str>) -> bool {
    bypass
        .filter(|value| !value.is_empty())
        .is_some_and(|value| {
            value.split(',').any(|rule| {
                let rule = trim(rule);
                let rule = rule.strip_prefix('.').unwrap_or(rule);
                host.strip_suffix(rule)
                    .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('.'))
            })
        })
}
fn prepare_socks(url: &Url, bypass: Option<&String>) -> Result<Prepared, Error> {
    let version = if matches!(url.scheme(), "socks4" | "socks4a") {
        4
    } else {
        5
    };
    // The pinned SOCKS helper uses URI credentials and ignores the
    // separate object fields. HTTP CONNECT keeps its own precedence.
    let socks = socks::Socks::new(
        version,
        decode(url.username())?,
        decode(url.password().unwrap_or(""))?,
    )?;
    let protocol = if version == 4 {
        ureq::ProxyProtocol::Socks4A
    } else {
        ureq::ProxyProtocol::Socks5h
    };
    let proxy = ureq::Proxy::builder(protocol)
        .host(&url[url::Position::BeforeHost..url::Position::AfterHost])
        .port(url.port().filter(|port| *port != 0).unwrap_or(1080))
        .build()
        .map_err(|_| Error::from("HTTP_PROXY_INVALID"))?;
    Ok(Prepared {
        proxy,
        authorization: None,
        socks: Some(socks),
        bypass: bypass.cloned(),
    })
}

impl Input {
    pub(super) fn prepare(&self) -> Result<Prepared, Error> {
        let (server, bypass, username, password) = match self {
            Self::Url(server) => (server, None, None, None),
            Self::Settings(options) => (
                &options.server,
                options.bypass.as_ref(),
                options.username.as_ref(),
                options.password.as_ref(),
            ),
        };
        if [Some(server), bypass, username, password]
            .into_iter()
            .flatten()
            .any(|value| value.len() > 4096)
        {
            return Err("HTTP_PROXY_INVALID".into());
        }
        let server = trim(server);
        let explicit = server.split_once("://").is_some_and(|(scheme, _)| {
            !scheme.is_empty()
                && scheme
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        });
        let mut url = Url::parse(&if explicit {
            server.into()
        } else {
            format!("http://{server}")
        })
        .map_err(|_| Error::from("HTTP_PROXY_INVALID"))?;
        if !matches!(
            url.scheme(),
            "http" | "https" | "socks" | "socks4" | "socks4a" | "socks5" | "socks5h"
        ) {
            return Err("HTTP_PROXY_UNSUPPORTED".into());
        }
        if url.host().is_none() {
            return Err("HTTP_PROXY_INVALID".into());
        }
        if url.scheme().starts_with("socks") {
            return prepare_socks(&url, bypass);
        }
        // The pinned request helper ignores an empty object username and leaves
        // URL credentials intact. A nonempty username replaces both fields.
        if let Some(username) = username.filter(|value| !value.is_empty()) {
            url.set_username(username)
                .map_err(|_| Error::from("HTTP_PROXY_INVALID"))?;
            url.set_password(Some(password.map_or("", String::as_str)))
                .map_err(|_| Error::from("HTTP_PROXY_INVALID"))?;
        }
        let authorization = if url.username().is_empty() && url.password().is_none_or(str::is_empty)
        {
            None
        } else {
            use base64::Engine as _;
            let credentials = format!(
                "{}:{}",
                decode(url.username())?,
                decode(url.password().unwrap_or(""))?
            );
            let encoded = base64::engine::general_purpose::STANDARD.encode(credentials.as_bytes());
            Some(Authorization(
                format!("Proxy-Authorization: Basic {encoded}\r\n")
                    .into_bytes()
                    .into(),
            ))
        };
        // Credentials never enter the generic proxy URI or any origin request.
        // The private transport adds them only to the CONNECT handshake.
        let address = format!(
            "{}://{}:{}",
            url.scheme(),
            &url[url::Position::BeforeHost..url::Position::AfterHost],
            url.port_or_known_default().ok_or("HTTP_PROXY_INVALID")?
        );
        let proxy = ureq::Proxy::new(&address).map_err(|_| Error::from("HTTP_PROXY_INVALID"))?;
        Ok(Prepared {
            proxy,
            authorization,
            socks: None,
            bypass: bypass.cloned(),
        })
    }
    pub(super) fn schema() -> Value {
        let field = json!({"type":"string","maxLength":4096});
        json!({"anyOf":[field,{"type":"object","properties":{"server":field,"bypass":field,"username":field,"password":field},"required":["server"],"additionalProperties":false},{"type":"null"}],"description":"HTTP(S) CONNECT or SOCKS proxy: URL or server/bypass/username/password object. Omitted inherits an associated context's proxy. HTTP CONNECT uses object credentials; SOCKS uses URL userinfo only, as pinned (credentials and destination names at most 255 bytes, no NUL in SOCKS4 user IDs). Bypass applies per destination, including redirects. Browser creation accepts SOCKS4/5 and refuses nonempty object credentials."})
    }
}
fn decode(value: &str) -> Result<String, Error> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let high = bytes
                .get(at + 1)
                .and_then(|byte| (*byte as char).to_digit(16))
                .ok_or("HTTP_PROXY_INVALID")?;
            let low = bytes
                .get(at + 2)
                .and_then(|byte| (*byte as char).to_digit(16))
                .ok_or("HTTP_PROXY_INVALID")?;
            output.push((high * 16 + low) as u8);
            at += 3;
        } else {
            output.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(output).map_err(|_| Error::from("HTTP_PROXY_INVALID"))
}
pub(crate) fn trim(value: &str) -> &str {
    value.trim_matches(|value| {
        matches!(
            value,
            '\t' | '\n' | '\r' | '\u{b}' | '\u{c}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200a}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202f}'
                    | '\u{205f}'
                    | '\u{3000}'
                    | '\u{feff}'
        )
    })
}
