//! An isolated request context owns raw cookie state and bounded retention.
use super::*;

type Key = (String, String, String);
#[derive(Clone, Default)]
pub(super) struct Cookies {
    pub(super) store: BTreeMap<Key, Value>,
    pub(super) keys: BTreeMap<Key, (u64, usize)>,
    pub(super) order: BTreeMap<u64, Key>,
    pub(super) groups: Vec<(String, Vec<Key>)>,
    sequence: u64,
    pub(super) bytes: usize,
    pub(super) evicted: u64,
    pub(super) dropped: u64,
}
fn key(cookie: &Value) -> Key {
    (
        cookie["domain"]
            .as_str()
            .expect("Validated cookie domain")
            .into(),
        cookie["path"]
            .as_str()
            .expect("Validated cookie path")
            .into(),
        cookie["name"]
            .as_str()
            .expect("Validated cookie name")
            .into(),
    )
}
fn expired(cookie: &Value) -> bool {
    let expires = cookie["expires"]
        .as_f64()
        .expect("Validated cookie expiration");
    let now = time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
    expires != -1.0 && expires * 1000.0 < now as f64
}
impl Cookies {
    fn over_capacity(&self) -> bool {
        self.keys.len() > 256
            || self.groups.len() > 256
            || self.bytes + self.empty_name_bytes() > 65536
    }
    fn empty_name_bytes(&self) -> usize {
        self.groups
            .iter()
            .filter(|(_, keys)| keys.is_empty())
            .map(|(name, _)| name.len())
            .sum()
    }
    pub(super) fn merge(&mut self, values: &[Value]) -> Result<(), Error> {
        let mut next = self.clone();
        for value in values {
            next.insert(
                value.clone(),
                serde_json::to_vec(value)
                    .map_err(|_| Error::from("HTTP_COOKIE_INVALID"))?
                    .len(),
            );
            if next.over_capacity() {
                return Err("HTTP_STORAGE_COOKIE_BOUNDS".into());
            }
        }
        *self = next;
        Ok(())
    }
    fn forget(&mut self, key: &Key) {
        self.store.remove(key);
        if let Some((sequence, size)) = self.keys.remove(key) {
            self.order.remove(&sequence);
            self.bytes -= size;
        }
        if let Some((_, keys)) = self.groups.iter_mut().find(|(name, _)| name == &key.2) {
            keys.retain(|other| other != key);
        }
    }
    pub(super) fn prune(&mut self) {
        let expired = self
            .store
            .iter()
            .filter(|(_, cookie)| expired(cookie))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in expired {
            self.forget(&key);
        }
        self.groups.retain(|(_, keys)| !keys.is_empty());
    }
    fn insert(&mut self, cookie: Value, bytes: usize) {
        let key = key(&cookie);
        if !self.groups.iter().any(|(name, _)| name == &key.2) {
            self.groups.push((key.2.clone(), vec![]));
        }
        // SDK add prunes only this name's set. Empty name groups survive until
        // iteration or filtered removal, retaining first-insertion order.
        let expired_keys = self
            .store
            .iter()
            .filter(|(other, value)| other.2 == key.2 && expired(value))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in expired_keys {
            self.forget(&key);
        }
        if expired(&cookie) {
            self.forget(&key);
            return;
        }
        let (_, group) = self
            .groups
            .iter_mut()
            .find(|(name, _)| name == &key.2)
            .expect("Cookie name group");
        group.retain(|other| other != &key);
        group.push(key.clone());
        if let Some((sequence, size)) = self.keys.remove(&key) {
            self.order.remove(&sequence);
            self.bytes -= size;
        }
        self.sequence += 1;
        self.bytes += bytes;
        self.keys.insert(key.clone(), (self.sequence, bytes));
        self.order.insert(self.sequence, key.clone());
        self.store.insert(key, cookie);
    }
    pub(super) fn initialize(values: &[Value]) -> Result<Self, Error> {
        let mut cookies = Self::default();
        for value in values {
            // The pinned NetworkCookie shape is validated by the state owner.
            // Refuse header injection before any client or request allocation.
            if ["name", "value"].iter().any(|field| {
                value[*field]
                    .as_str()
                    .is_none_or(|value| value.chars().any(|c| c.is_control() || c == ';'))
            }) {
                return Err("HTTP_COOKIE_INVALID".into());
            }
            let bytes = serde_json::to_vec(value)
                .map_err(|_| Error::from("INVALID_STORAGE_STATE"))?
                .len();
            cookies.insert(value.clone(), bytes);
            if cookies.over_capacity() {
                return Err("HTTP_STORAGE_COOKIE_BOUNDS".into());
            }
        }
        Ok(cookies)
    }
    pub(super) fn receive(&mut self, url: &Url, headers: &ureq::http::HeaderMap) {
        for raw in headers.get_all(ureq::http::header::SET_COOKIE) {
            let Ok(raw) = raw.to_str() else {
                continue;
            };
            let Ok(cookie) = cookie_store::Cookie::parse(raw.to_owned(), url) else {
                continue;
            };
            let host = url.host_str().expect("Validated HTTP host");
            let domain = raw
                .split(';')
                .skip(1)
                .filter_map(|part| part.split_once('='))
                .filter(|(name, _)| name.trim().eq_ignore_ascii_case("domain"))
                .map(|(_, value)| value.trim().to_lowercase())
                .last()
                .filter(|value| !value.is_empty())
                .map_or_else(
                    || host.to_owned(),
                    |domain| {
                        if !domain.starts_with('.') && domain.contains('.') {
                            format!(".{domain}")
                        } else {
                            domain
                        }
                    },
                );
            if !domain_matches(host, &domain) {
                continue;
            }
            let expires = if cookie.is_expired() {
                json!(0)
            } else {
                match &cookie.expires {
                    cookie_store::CookieExpiration::AtUtc(when) => {
                        json!(when.unix_timestamp_nanos() as f64 / 1_000_000_000.0)
                    }
                    cookie_store::CookieExpiration::SessionEnd => json!(-1),
                }
            };
            let value = json!({"name":cookie.name(),"value":cookie.value(),"domain":domain,"path":cookie.path.as_ref(),"expires":expires,"httpOnly":cookie.http_only().unwrap_or(false),"secure":cookie.secure().unwrap_or(false),"sameSite":cookie.same_site().map_or_else(||"Lax".into(), |value|value.to_string())});
            if expired(&value) {
                self.insert(value, raw.len() + 2);
                continue;
            }
            if raw.len() > 4096 {
                self.dropped = self.dropped.saturating_add(1);
                continue;
            }
            self.insert(value, raw.len() + 2);
            while self.keys.len() > 256 || self.bytes > 65536 {
                let Some((_, key)) = self.order.first_key_value() else {
                    break;
                };
                let key = key.clone();
                self.forget(&key);
                self.evicted = self.evicted.saturating_add(1);
            }
        }
        while self.over_capacity() {
            // Retain the existing history bound even for expired server names.
            let Some(index) = self.groups.iter().position(|(_, keys)| keys.is_empty()) else {
                break;
            };
            self.groups.remove(index);
        }
    }
    pub(super) fn snapshot(&mut self) -> Vec<Value> {
        self.prune();
        // The SDK iterates names in first-insertion order, then each name's
        // cookies in latest-insertion order. Scope identity retains the dot.
        self.values().cloned().collect()
    }
    pub(super) fn filter_snapshot(&self) -> Vec<Value> {
        self.values().cloned().collect()
    }
    pub(super) fn remove_selected(&mut self, values: &[Value]) {
        for value in values {
            self.forget(&key(value));
        }
        self.groups.retain(|(_, keys)| !keys.is_empty());
    }
    fn values(&self) -> impl Iterator<Item = &Value> {
        self.groups
            .iter()
            .flat_map(move |(_, keys)| keys.iter().map(move |key| &self.store[key]))
    }
    pub(super) fn header(&mut self, url: &Url) -> String {
        let host = url.host_str().expect("Validated HTTP URL");
        let local = host == "localhost"
            || host.ends_with(".localhost")
            || host == "[::1]"
            || host
                .parse::<std::net::Ipv4Addr>()
                .is_ok_and(|ip| ip.octets()[0] == 127);
        self.prune();
        self.values()
            .filter(|cookie| {
                let domain = cookie["domain"].as_str().expect("Validated cookie domain");
                let path = cookie["path"].as_str().expect("Validated cookie path");
                let path_matches =
                    url.path() == path || with_slash(url.path()).starts_with(&with_slash(path));
                domain_matches(host, domain)
                    && path_matches
                    && (cookie["secure"] != true || url.scheme() == "https" || local)
            })
            .map(|cookie| {
                format!(
                    "{}={}",
                    cookie["name"].as_str().expect("Cookie name"),
                    cookie["value"].as_str().expect("Cookie value")
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}
fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain || domain.starts_with('.') && format!(".{host}").ends_with(domain)
}
fn with_slash(value: &str) -> String {
    if value.ends_with('/') {
        value.into()
    } else {
        format!("{value}/")
    }
}
