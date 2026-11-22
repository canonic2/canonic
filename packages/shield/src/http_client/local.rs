//! Closed grammar for requests to explicitly reviewed local development routes.

use super::{Client, Fetch, inspection_write_out, literal_url, positive, set_once};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) struct Request<'a> {
    pub(crate) transfer: Fetch<'a>,
    pub(crate) proxy_disabled: bool,
    pub(crate) fields: Value,
}

pub(crate) const METHODS: [&str; 5] = ["GET", "HEAD", "POST", "OPTIONS", "DELETE"];

pub(crate) fn requests<'a>(executable: &str, args: &[&'a str]) -> Option<Vec<Request<'a>>> {
    match super::client(executable)? {
        Client::Curl => curl(args),
        Client::Wget => wget(args).map(|request| vec![request]),
    }
}

#[derive(Default)]
struct Curl<'a> {
    urls: Vec<&'a str>,
    resolve: BTreeSet<(&'a str, u16)>,
    method: Option<&'a str>,
    body: Option<&'a str>,
    headers: BTreeSet<String>,
    skips_config: bool,
    proxy_disabled: bool,
    basic_auth: bool,
}

impl<'a> Curl<'a> {
    fn value(&mut self, flag: &str, value: &'a str) -> Option<()> {
        match flag {
            "request" => set_once(&mut self.method, value),
            "data-raw" => set_once(&mut self.body, value),
            "header" => self.header(value),
            "user"
                if !self.basic_auth
                    && value
                        .split_once(':')
                        .is_some_and(|(user, _)| !user.is_empty())
                    && plain(value) =>
            {
                self.basic_auth = true;
                Some(())
            }
            "noproxy" if value == "*" => {
                self.proxy_disabled = true;
                Some(())
            }
            "resolve" => {
                let (host, port) = value.strip_suffix(":127.0.0.1")?.split_once(':')?;
                let number: u16 = port.parse().ok()?;
                (local_name(host)
                    && host.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b)
                    })
                    && number != 0
                    && number.to_string() == port
                    && self.resolve.insert((host, number)))
                .then_some(())
            }
            "max-time" | "connect-timeout" => positive(value).then_some(()),
            "write-out" => inspection_write_out(value).then_some(()),
            _ => None,
        }
    }

    fn header(&mut self, value: &str) -> Option<()> {
        let (name, content) = value.split_once(':')?;
        let name = name.to_ascii_lowercase();
        let content = content.trim();
        if !plain(value) || content.is_empty() {
            return None;
        }
        let safe = match name.as_str() {
            "content-type" => content == "application/json",
            "origin" => local_url(content).is_some_and(|url| url.path() == "/"),
            "accept"
            | "access-control-request-method"
            | "access-control-request-headers"
            | "x-api-key"
            | "mcp-session-id" => true,
            _ => false,
        };
        (safe && self.headers.insert(name)).then_some(())
    }

    fn finish(self) -> Option<Vec<Request<'a>>> {
        let method = self
            .method
            .unwrap_or(if self.body.is_some() { "POST" } else { "GET" });
        let method = METHODS.into_iter().find(|known| *known == method)?;
        if self.urls.is_empty()
            || self.urls.len() > 64
            || (method != "POST" && self.body.is_some())
            || (matches!(method, "POST" | "DELETE") && self.urls.len() != 1)
            || (matches!(method, "GET" | "HEAD") && self.headers.is_empty() && !self.basic_auth)
        {
            return None;
        }
        let fields = fields(self.body, self.basic_auth, &self.headers)?;
        self.urls
            .into_iter()
            .map(|url| {
                let target = local_url(url)?;
                let host = literal_host(url)?;
                if host != "127.0.0.1"
                    && !(self.skips_config
                        && self.proxy_disabled
                        && Some(host) == target.host_str()
                        && self
                            .resolve
                            .contains(&(host, target.port_or_known_default()?)))
                {
                    return None;
                }
                Some(Request {
                    transfer: transfer(Client::Curl, url, method, self.skips_config),
                    proxy_disabled: self.proxy_disabled,
                    fields: fields.clone(),
                })
            })
            .collect()
    }
}

fn curl<'a>(args: &[&'a str]) -> Option<Vec<Request<'a>>> {
    let mut parsed = Curl::default();
    let mut args = args.iter().copied().peekable();
    if args.next_if_eq(&"-q").is_some() {
        parsed.skips_config = true;
    }
    while let Some(arg) = args.next() {
        if arg == "--" {
            parsed.urls.extend(args);
            break;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (flag, attached) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            match (flag, attached) {
                (
                    "silent" | "show-error" | "fail" | "fail-with-body" | "include" | "globoff",
                    None,
                ) => (),
                _ => parsed.value(flag, attached.or_else(|| args.next())?)?,
            }
        } else if let Some(short) = arg.strip_prefix('-') {
            if short.is_empty() {
                return None;
            }
            for (index, flag) in short.char_indices() {
                let name = match flag {
                    's' | 'S' | 'f' | 'i' | 'g' => continue,
                    flag => curl_value_option(flag)?,
                };
                let attached = &short[index + 1..];
                parsed.value(
                    name,
                    if attached.is_empty() {
                        args.next()?
                    } else {
                        attached
                    },
                )?;
                break;
            }
        } else {
            parsed.urls.push(arg);
        }
    }
    parsed.finish()
}

fn curl_value_option(flag: char) -> Option<&'static str> {
    Some(match flag {
        'X' => "request",
        'H' => "header",
        'm' => "max-time",
        'w' => "write-out",
        'u' => "user",
        'o' => "output",
        _ => return None,
    })
}

/// Report credential operands by argument index. Evidence scanning also
/// tolerates unrelated options so a rejected request can still be redacted.
pub(crate) fn curl_credentials<'a>(
    args: impl Iterator<Item = &'a str>,
    mut found: impl FnMut(usize, &'a str),
) {
    let mut args = args.enumerate();
    while let Some((index, arg)) = args.next() {
        let (name, attached) = if arg == "--" {
            break;
        } else if let Some(long) = arg.strip_prefix("--") {
            long.split_once('=')
                .map_or((long, ""), |(name, value)| (name, value))
        } else if let Some(short) = arg.strip_prefix('-') {
            let Some((at, flag)) = short.char_indices().find(|(_, flag)| {
                !matches!(flag, 's' | 'S' | 'f' | 'i' | 'g' | 'q' | 'L' | 'I' | 'G')
            }) else {
                continue;
            };
            let Some(name) = curl_value_option(flag) else {
                continue;
            };
            (name, &short[at + 1..])
        } else {
            continue;
        };
        if !matches!(
            name,
            "user"
                | "header"
                | "request"
                | "max-time"
                | "write-out"
                | "data-raw"
                | "noproxy"
                | "resolve"
                | "connect-timeout"
                | "output"
        ) {
            continue;
        }
        let Some((index, value)) = (if attached.is_empty() && !arg.ends_with('=') {
            args.next()
        } else {
            Some((index, attached))
        }) else {
            break;
        };
        if name == "user" && !value.is_empty() {
            found(index, value);
        }
        if name == "header"
            && let Some((name, secret)) = value.split_once(':')
            && [
                "x-api-key",
                "mcp-session-id",
                "authorization",
                "proxy-authorization",
                "cookie",
            ]
            .iter()
            .any(|field| name.eq_ignore_ascii_case(field))
            && !secret.trim().is_empty()
        {
            found(index, secret.trim());
        }
    }
}

pub(crate) fn is_curl(executable: &str) -> bool {
    matches!(super::client(executable), Some(Client::Curl))
}

fn wget<'a>(args: &[&'a str]) -> Option<Request<'a>> {
    let (mut url, mut body, mut output) = (None, None, None);
    let mut disabled = BTreeSet::new();
    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        let (flag, attached) = if arg.starts_with("--") {
            arg.split_once('=')
                .map_or((arg, None), |(n, v)| (n, Some(v)))
        } else if let Some(value) = arg.strip_prefix("-O") {
            ("-O", (!value.is_empty()).then_some(value))
        } else {
            (arg, None)
        };
        match (flag, attached) {
            ("-q" | "-S" | "--quiet" | "--server-response", None) => (),
            ("--no-config" | "--no-hsts" | "--no-netrc" | "--no-proxy", None) => {
                disabled.insert(flag);
            }
            ("--max-redirect", _) if attached.or_else(|| args.next()) == Some("0") => {
                disabled.insert(flag);
            }
            ("--post-data", _) => {
                let value = attached.or_else(|| args.next())?;
                if value.starts_with('@') {
                    return None;
                }
                set_once(&mut body, value)?;
            }
            ("-O" | "--output-document", _) => {
                set_once(&mut output, attached.or_else(|| args.next())?)?;
            }
            _ if !arg.starts_with('-') => set_once(&mut url, arg)?,
            _ => return None,
        }
    }
    if disabled.len() != 5 || !matches!(output, Some("-" | "/dev/null")) {
        return None;
    }
    let url = url?;
    local_url(url)?;
    if literal_host(url)? != "127.0.0.1" {
        return None;
    }
    Some(Request {
        transfer: transfer(Client::Wget, url, "POST", true),
        proxy_disabled: true,
        fields: fields(Some(body?), false, &BTreeSet::new())?,
    })
}

fn transfer<'a>(
    client: Client,
    url: &'a str,
    method: &'static str,
    skips_config: bool,
) -> Fetch<'a> {
    Fetch {
        client,
        url: url.into(),
        method,
        redirects: "none",
        output: None,
        skips_config,
        reads_netrc: false,
    }
}

pub(crate) fn local_url(value: &str) -> Option<url::Url> {
    let url = url::Url::parse(literal_url(value)?).ok()?;
    let (_, authority_and_path) = value.split_once("://")?;
    let path = authority_and_path
        .find('/')
        .map_or("/", |at| &authority_and_path[at..]);
    (url.host_str()
        .is_some_and(|name| name == "127.0.0.1" || local_name(name))
        && url.query().is_none()
        && url.fragment().is_none()
        && !path.contains('%')
        && path == url.path())
    .then_some(url)
}

fn local_name(host: &str) -> bool {
    host == "localhost" || host.ends_with(".localhost")
}

fn literal_host(url: &str) -> Option<&str> {
    let (_, authority_and_path) = url.split_once("://")?;
    authority_and_path.split(['/', ':']).next()
}

fn fields(body: Option<&str>, basic_auth: bool, headers: &BTreeSet<String>) -> Option<Value> {
    let mut fields = json!({
        "client":"shell", "redirects":"none", "body":body.is_some(),
        "basic_auth":basic_auth, "headers":headers,
    });
    if let Some(body) = body {
        let payload: Value = serde_json::from_str(body).ok()?;
        if payload["jsonrpc"] == "2.0" {
            // Derived fields reject duplicate method, version, params and tool
            // name keys. A server may interpret duplicate JSON keys differently.
            #[derive(Deserialize)]
            struct Rpc {
                jsonrpc: String,
                method: String,
                params: Option<Params>,
            }
            #[derive(Deserialize)]
            struct Params {
                name: Option<String>,
            }
            let rpc: Rpc = serde_json::from_str(body).ok()?;
            if rpc.jsonrpc != "2.0" || !identifier(&rpc.method) {
                return None;
            }
            if rpc.method == "tools/call" {
                let name = rpc.params?.name.filter(|name| identifier(name))?;
                fields["rpc_tool"] = json!(name);
            }
            fields["rpc_method"] = json!(rpc.method);
        }
    }
    Some(fields)
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b))
}

fn plain(value: &str) -> bool {
    !value.chars().any(char::is_control)
}
