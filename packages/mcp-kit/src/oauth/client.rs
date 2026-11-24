//! Clients: pre-registered configuration, Client ID Metadata Documents, the
//! optional registration hook, client authentication and redirect rules.

use super::{
    AuthorizationServerError, AuthorizationServerOptions, ClientAssertionPool, jose, scope_list,
    store::Digest, wire,
};
use crate::http::auth::MAX_GRANT_IDENTITY_BYTES;
use serde_json::Value;
use std::{net::IpAddr, sync::Arc};

/// The most pre-registered clients.
pub const MAX_CLIENTS: usize = 256;
/// The most redirect URIs per client.
pub const MAX_REDIRECT_URIS: usize = 16;
/// The longest redirect URI or remote key set URI, in bytes.
pub const MAX_URI_BYTES: usize = 2048;
/// The shortest and longest client secret, in bytes.
pub const CLIENT_SECRET_BYTES: std::ops::RangeInclusive<usize> = 32..=512;
/// The largest Client ID Metadata Document, in bytes.
pub const MAX_CLIENT_DOCUMENT_BYTES: usize = 16 * 1024;
/// The longest client name, in bytes.
pub const MAX_CLIENT_NAME_BYTES: usize = 256;

/// How a client authenticates at the token and revocation endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAuth {
    /// A public client; it sends only its `client_id`.
    None,
    /// HTTP Basic authentication with the client secret.
    SecretBasic,
    /// The client secret in the form body.
    SecretPost,
    /// A signed JWT verified with the client's registered public keys.
    PrivateKeyJwt,
}

impl ClientAuth {
    /// The RFC 7591 name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::SecretBasic => "client_secret_basic",
            Self::SecretPost => "client_secret_post",
            Self::PrivateKeyJwt => "private_key_jwt",
        }
    }

    fn registration_method(name: &str) -> Option<Self> {
        [Self::None, Self::SecretBasic, Self::SecretPost]
            .into_iter()
            .find(|method| method.name() == name)
    }
}

/// A pre-registered client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientOptions {
    /// The client ID.
    pub client_id: String,
    /// Shown on consent.
    pub client_name: Option<String>,
    /// Exact redirect URIs: `https`, or `http` on a loopback host.
    pub redirect_uris: Vec<String>,
    /// Scopes the client may request.
    pub scopes: Vec<String>,
    /// How it authenticates.
    pub auth: ClientAuth,
    /// The secret for `SecretBasic` or `SecretPost`, 32 to 512 bytes; kept as a digest.
    pub secret: Option<String>,
    /// The client may use the `client_credentials` grant for machine-to-machine
    /// access, with no user. Only confidential clients may; such a client may
    /// have no redirect URIs.
    pub client_credentials: bool,
    /// The client may present enterprise identity assertions through the JWT
    /// bearer grant, once [`AuthorizationServer::with_identity_assertions`]
    /// trusts their issuers. Only confidential clients may; such a client may
    /// have no redirect URIs.
    ///
    /// [`AuthorizationServer::with_identity_assertions`]: super::AuthorizationServer::with_identity_assertions
    pub identity_assertions: bool,
}

/// Grants the server permits for a client identified by a metadata document.
/// Both grants require `private_key_jwt` client authentication.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocumentClientGrants {
    /// Issue machine access tokens to the client itself.
    pub client_credentials: bool,
    /// Accept enterprise identity assertions for this client from trusted issuers.
    pub identity_assertions: bool,
}

/// A client registered through a [`ClientRegistrar`], as the registrar stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredClient {
    /// The client ID the library generated.
    pub client_id: String,
    /// Shown on consent.
    pub client_name: Option<String>,
    /// Validated redirect URIs.
    pub redirect_uris: Vec<String>,
    /// How it authenticates.
    pub auth: ClientAuth,
    /// The digest of its generated secret, for confidential methods.
    pub secret: Option<Digest>,
}

/// Fetches Client ID Metadata Documents.
///
/// The library never opens a network connection. The resolver owns SSRF
/// protection (refusing private and loopback addresses, redirects and
/// oversized bodies), caching and any domain trust policy.
pub trait ClientResolver: Send + Sync {
    /// Returns the document at `client_id`, an `https` URL with a path.
    ///
    /// # Errors
    /// Returns a short reason when the document cannot be fetched or is not trusted.
    fn resolve(&self, client_id: &str) -> Result<Vec<u8>, String>;

    /// Returns the public JSON Web Key Set at a validated HTTPS `jwks_uri`
    /// from the document for `client_id`. Apply the same fetch protections
    /// as for the metadata document and enforce the key set size bound.
    ///
    /// # Errors
    /// Returns a short reason when the key set cannot be fetched or is untrusted.
    fn resolve_jwks(&self, _client_id: &str, _jwks_uri: &str) -> Result<Vec<u8>, String> {
        Err("Remote client key sets are unavailable".into())
    }
}

/// Approves, stores and looks up dynamically registered clients (RFC 7591).
pub trait ClientRegistrar: Send + Sync {
    /// Approves and stores a validated registration, applying the server's
    /// own policy and rate limits.
    ///
    /// # Errors
    /// Returns a short reason to send as `invalid_client_metadata`.
    fn register(&self, client: &RegisteredClient) -> Result<(), String>;

    /// Returns a client this registrar stored.
    fn lookup(&self, client_id: &str) -> Option<RegisteredClient>;
}

/// A client resolved for one request.
#[derive(Clone)]
pub(super) struct Client {
    pub(super) id: String,
    pub(super) name: Option<String>,
    pub(super) redirect_uris: Vec<String>,
    pub(super) scopes: Option<Vec<String>>,
    pub(super) auth: ClientAuth,
    pub(super) secret: Option<Digest>,
    pub(super) keys: Option<ClientKeys>,
    pub(super) assertion_pool: ClientAssertionPool,
    /// May use the `client_credentials` grant.
    pub(super) machine: bool,
    /// May present identity assertions.
    pub(super) assertions: bool,
}

#[derive(Clone)]
pub(super) enum ClientKeys {
    Inline(Arc<[jose::Key]>),
    Remote(String),
}

impl Client {
    fn configured(options: &ClientOptions, keys: Option<ClientKeys>) -> Self {
        Self {
            id: options.client_id.clone(),
            name: options.client_name.clone(),
            redirect_uris: options.redirect_uris.clone(),
            scopes: Some(options.scopes.clone()),
            auth: options.auth,
            secret: options
                .secret
                .as_deref()
                .map(|secret| wire::digest(&format!("cs_{secret}"))),
            keys,
            assertion_pool: ClientAssertionPool::Configured,
            machine: options.client_credentials,
            assertions: options.identity_assertions,
        }
    }

    /// Registered clients never get `client_credentials` or identity
    /// assertions: those are provisioned out of band, not through registration.
    pub(super) fn registered(client: RegisteredClient) -> Self {
        Self {
            id: client.client_id,
            name: client.client_name,
            redirect_uris: client.redirect_uris,
            scopes: None,
            auth: client.auth,
            secret: client.secret,
            keys: None,
            assertion_pool: ClientAssertionPool::Document,
            machine: false,
            assertions: false,
        }
    }

    /// Whether the client authenticates with a secret or a signed assertion.
    pub(super) fn confidential(&self) -> bool {
        self.auth != ClientAuth::None
    }

    pub(super) fn uses_private_key(&self) -> bool {
        self.auth == ClientAuth::PrivateKeyJwt
    }

    /// Whether `presented` matches a registered redirect URI: exactly, or with
    /// a different port for a loopback IP literal (RFC 8252).
    pub(super) fn redirects_to(&self, presented: &str) -> bool {
        self.redirect_uris.iter().any(|registered| {
            registered == presented
                || loopback_ip_parts(registered)
                    .zip(loopback_ip_parts(presented))
                    .is_some_and(|(left, right)| left == right)
        })
    }

    /// Whether `secret` is this client's secret, compared by digest.
    pub(super) fn secret_matches(&self, secret: &str) -> bool {
        self.secret
            .is_some_and(|expected| expected.matches(&wire::digest(&format!("cs_{secret}"))))
    }
}

/// Validates configured clients and prepares each of their public key sets.
pub(super) fn configured_clients(
    options: &AuthorizationServerOptions,
) -> Result<Vec<Client>, AuthorizationServerError> {
    if options.clients.len() > MAX_CLIENTS {
        return Err(AuthorizationServerError::Client(MAX_CLIENTS));
    }
    let mut clients = Vec::with_capacity(options.clients.len());
    let mut key_clients = 0;
    for (index, client) in options.clients.iter().enumerate() {
        let repeated = options.clients[..index]
            .iter()
            .any(|other| other.client_id == client.client_id);
        if repeated || !valid_options(client) || !scope_list(&client.scopes) {
            return Err(AuthorizationServerError::Client(index));
        }
        let keys = if client.auth == ClientAuth::PrivateKeyJwt {
            key_clients += 1;
            let keys = options
                .client_jwks
                .get(&client.client_id)
                .and_then(|bytes| jose::key_set(bytes).ok())
                .filter(|keys| !keys.is_empty())
                .ok_or(AuthorizationServerError::Client(index))?;
            Some(ClientKeys::Inline(keys.into()))
        } else {
            None
        };
        clients.push(Client::configured(client, keys));
    }
    if options.client_jwks.len() != key_clients {
        return Err(AuthorizationServerError::ClientKeys);
    }
    Ok(clients)
}

/// Validates a pre-registered client.
fn valid_options(options: &ClientOptions) -> bool {
    let secret_fits = match (options.auth, &options.secret) {
        (ClientAuth::None | ClientAuth::PrivateKeyJwt, None) => true,
        (ClientAuth::SecretBasic | ClientAuth::SecretPost, Some(secret)) => {
            CLIENT_SECRET_BYTES.contains(&secret.len())
        }
        _ => false,
    };
    !options.client_id.is_empty()
        && options.client_id.len() <= MAX_GRANT_IDENTITY_BYTES
        && options
            .client_id
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
        && valid_name(options.client_name.as_deref())
        && (valid_redirects(&options.redirect_uris)
            || ((options.client_credentials || options.identity_assertions)
                && options.redirect_uris.is_empty()))
        && secret_fits
        && (!(options.client_credentials || options.identity_assertions)
            || options.auth != ClientAuth::None)
}

fn valid_name(name: Option<&str>) -> bool {
    name.is_none_or(|name| {
        !name.is_empty()
            && name.len() <= MAX_CLIENT_NAME_BYTES
            && !name.chars().any(char::is_control)
    })
}

/// 1 to [`MAX_REDIRECT_URIS`] valid, unique redirect URIs.
pub(super) fn valid_redirects(uris: &[String]) -> bool {
    (1..=MAX_REDIRECT_URIS).contains(&uris.len())
        && uris.iter().all(|uri| valid_redirect(uri))
        && uris
            .iter()
            .enumerate()
            .all(|(index, uri)| !uris[..index].contains(uri))
}

/// An absolute `https` URI, or `http` on a loopback host, without userinfo
/// or fragment.
pub(super) fn valid_redirect(uri: &str) -> bool {
    if uri.len() > MAX_URI_BYTES || iri_string::types::UriStr::new(uri).is_err() {
        return false;
    }
    let Some((scheme, rest)) = uri.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    if uri.contains('#') || authority.contains('@') || authority.is_empty() {
        return false;
    }
    match scheme.to_ascii_lowercase().as_str() {
        "https" => true,
        "http" => is_loopback(host(authority)),
        _ => false,
    }
}

/// Whether a redirect URI's host is `localhost`, for the consent warning.
pub(super) fn redirect_host(uri: &str) -> (String, bool) {
    let authority = uri.split_once("://").map_or("", |(_, rest)| {
        rest.split(['/', '?']).next().unwrap_or_default()
    });
    let host = host(authority).to_ascii_lowercase();
    let localhost = host == "localhost";
    (host, localhost)
}

fn host(authority: &str) -> &str {
    match authority.rfind(':') {
        Some(index) if !authority[index..].contains(']') => &authority[..index],
        _ => authority,
    }
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The scheme, host and everything after the port of a valid `http` loopback
/// IP literal URI whose port, if any, is decimal digits; `localhost` names get
/// no port exception. Userinfo is refused, so `127.0.0.1:1@evil.example`
/// cannot pass as a loopback host.
fn loopback_ip_parts(uri: &str) -> Option<(String, String, String)> {
    if !valid_redirect(uri) {
        return None;
    }
    let rest = uri.strip_prefix("http://")?;
    let split = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(split);
    let host = host(authority);
    let port = &authority[host.len()..];
    if !port.is_empty()
        && !port
            .strip_prefix(':')
            .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .ok()
        .filter(IpAddr::is_loopback)?;
    Some(("http".to_owned(), host.to_owned(), tail.to_owned()))
}

/// Whether `client_id` names a Client ID Metadata Document.
pub(super) fn is_document_id(client_id: &str) -> bool {
    client_id.len() <= MAX_GRANT_IDENTITY_BYTES
        && valid_redirect(client_id)
        && client_id
            .strip_prefix("https://")
            .and_then(|rest| rest.split_once('/'))
            .is_some_and(|(authority, path)| {
                !authority.contains('?') && !host(authority).is_empty() && !path.is_empty()
            })
}

/// Validates a fetched Client ID Metadata Document for `client_id`.
pub(super) fn from_document(
    client_id: &str,
    document: &[u8],
    grants: DocumentClientGrants,
) -> Result<Client, &'static str> {
    if document.len() > MAX_CLIENT_DOCUMENT_BYTES {
        return Err("The client metadata document is too large");
    }
    let document: Value =
        serde_json::from_slice(document).map_err(|_| "The client metadata document is not JSON")?;
    if document["client_id"].as_str() != Some(client_id) {
        return Err("The client metadata document names another client");
    }
    let name = document["client_name"]
        .as_str()
        .filter(|name| valid_name(Some(name)))
        .ok_or("The client metadata document needs a client_name")?;
    let redirect_uris = strings(&document["redirect_uris"])
        .filter(|uris| valid_redirects(uris))
        .ok_or("The client metadata document has invalid redirect_uris")?;
    let (auth, keys) = match document.get("token_endpoint_auth_method") {
        None => (ClientAuth::None, None),
        Some(method) if method == "none" => (ClientAuth::None, None),
        Some(method) if method == "private_key_jwt" => {
            (ClientAuth::PrivateKeyJwt, Some(document_keys(&document)?))
        }
        Some(_) => {
            return Err("The client metadata document has an unsupported authentication method");
        }
    };
    Ok(Client {
        id: client_id.to_owned(),
        name: Some(name.to_owned()),
        redirect_uris,
        scopes: None,
        auth,
        secret: None,
        keys,
        assertion_pool: if grants.client_credentials || grants.identity_assertions {
            ClientAssertionPool::Configured
        } else {
            ClientAssertionPool::Document
        },
        machine: auth == ClientAuth::PrivateKeyJwt && grants.client_credentials,
        assertions: auth == ClientAuth::PrivateKeyJwt && grants.identity_assertions,
    })
}

fn document_keys(document: &Value) -> Result<ClientKeys, &'static str> {
    let invalid = "The client metadata document needs one valid jwks or jwks_uri";
    match (document.get("jwks"), document.get("jwks_uri")) {
        (Some(jwks), None) => jose::key_set_value(jwks)
            .ok()
            .filter(|keys| !keys.is_empty())
            .map(|keys| ClientKeys::Inline(keys.into()))
            .ok_or(invalid),
        (None, Some(uri)) => uri
            .as_str()
            .filter(|uri| uri.starts_with("https://") && valid_redirect(uri))
            .map(|uri| ClientKeys::Remote(uri.to_owned()))
            .ok_or(invalid),
        _ => Err(invalid),
    }
}

/// A validated RFC 7591 registration request.
pub(super) struct Registration {
    pub(super) name: Option<String>,
    pub(super) redirect_uris: Vec<String>,
    pub(super) auth: ClientAuth,
}

/// Validates an RFC 7591 registration request body.
pub(super) fn registration(body: &[u8]) -> Result<Registration, &'static str> {
    let request: Value =
        serde_json::from_slice(body).map_err(|_| "The registration is not JSON")?;
    if !request.is_object() {
        return Err("The registration is not a JSON object");
    }
    let redirect_uris = strings(&request["redirect_uris"])
        .filter(|uris| valid_redirects(uris))
        .ok_or("redirect_uris must list 1 to 16 https or loopback http URIs")?;
    let name = match request.get("client_name") {
        None => None,
        Some(name) => Some(
            name.as_str()
                .filter(|name| valid_name(Some(name)))
                .ok_or("client_name is invalid")?
                .to_owned(),
        ),
    };
    let auth = match request.get("token_endpoint_auth_method") {
        None => ClientAuth::SecretBasic,
        Some(method) => method
            .as_str()
            .and_then(ClientAuth::registration_method)
            .ok_or("token_endpoint_auth_method is not supported")?,
    };
    // Every client gets both grants; the reply states that (RFC 7591 lets the
    // server replace requested metadata). Other grant types are refused.
    let grants_supported = request.get("grant_types").is_none_or(|value| {
        strings(value).is_some_and(|grants| {
            !grants.is_empty()
                && grants
                    .iter()
                    .all(|grant| grant == "authorization_code" || grant == "refresh_token")
        })
    });
    if !grants_supported {
        return Err("grant_types must be authorization_code and refresh_token");
    }
    let codes_only = match request.get("response_types") {
        None => true,
        Some(value) => strings(value).is_some_and(|types| types == ["code"]),
    };
    if !codes_only {
        return Err("response_types must be code");
    }
    Ok(Registration {
        name,
        redirect_uris,
        auth,
    })
}

fn strings(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}
