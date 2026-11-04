//! An optional OAuth 2.1 authorization server beside the MCP endpoint, behind
//! the `authorization-server` feature.
//!
//! [`AuthorizationServer`] serves authorization server metadata, the
//! authorization, token, revocation and optional registration endpoints, and
//! routes the consumer's sign-in pages to its [`Interaction`]. The library
//! generates every code and token and gives the [`GrantStore`] only digests.
//! The consumer authenticates users, shows consent and chooses the store. The
//! server verifies its own access tokens through [`TokenVerifier`], the same
//! boundary any external issuer uses. Nothing here opens sockets or threads.

mod client;
mod flow;
pub mod store;
mod token;
mod wire;

pub use client::{
    CLIENT_SECRET_BYTES, ClientAuth, ClientOptions, ClientRegistrar, ClientResolver,
    MAX_CLIENT_DOCUMENT_BYTES, MAX_CLIENT_NAME_BYTES, MAX_CLIENTS, MAX_REDIRECT_URIS,
    MAX_URI_BYTES, RegisteredClient,
};
pub use store::{
    AccessRecord, CodeRecord, CodeTake, Digest, FamilyRecord, GrantStore, MemoryStore,
    PendingRecord, Rotated, Rotation, StoreError,
};

use crate::http::{
    BearerToken, Grant, Rejection, RequestHead, ResourceServer, Response, TokenVerifier, auth,
};
use client::Client;
use serde_json::{Map, Value, json};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

#[cfg(test)]
mod tests;

/// The well-known prefix of authorization server metadata (RFC 8414).
pub const METADATA_PREFIX: &str = "/.well-known/oauth-authorization-server";
/// The largest authorization query, token or revocation form, or registration.
pub const MAX_FORM_BYTES: usize = 16 * 1024;
/// The largest request body routed to an [`Interaction`].
pub const MAX_INTERACTION_BODY_BYTES: usize = 64 * 1024;
/// The largest response body an [`Interaction`] may return.
pub const MAX_INTERACTION_RESPONSE_BYTES: usize = 256 * 1024;
/// The longest `state`, in bytes.
pub const MAX_STATE_BYTES: usize = 1024;
/// The most scopes one request may name.
pub const MAX_REQUESTED_SCOPES: usize = 64;
/// Expired records removed per token request.
pub const PURGE_PER_REQUEST: usize = 64;

/// How long each record lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lifetimes {
    /// A pending authorization, 1 to 30 minutes.
    pub pending: Duration,
    /// An authorization code, 10 seconds to 10 minutes.
    pub code: Duration,
    /// An access token, 1 minute to 24 hours.
    pub access: Duration,
    /// A refresh token since its issue, 1 hour to 90 days.
    pub refresh_idle: Duration,
    /// A grant family, 1 hour to 365 days and at least `refresh_idle`.
    pub family: Duration,
    /// How long a replaced refresh token may still rotate, 0 to 60 seconds.
    pub refresh_grace: Duration,
}

impl Default for Lifetimes {
    fn default() -> Self {
        Self {
            pending: Duration::from_mins(10),
            code: Duration::from_secs(60),
            access: Duration::from_hours(1),
            refresh_idle: Duration::from_hours(14 * 24),
            family: Duration::from_hours(90 * 24),
            refresh_grace: Duration::from_secs(10),
        }
    }
}

impl Lifetimes {
    fn valid(&self) -> bool {
        let within =
            |value: Duration, least: Duration, most: Duration| (least..=most).contains(&value);
        let day = Duration::from_hours(24);
        within(
            self.pending,
            Duration::from_mins(1),
            Duration::from_mins(30),
        ) && within(self.code, Duration::from_secs(10), Duration::from_mins(10))
            && within(self.access, Duration::from_mins(1), day)
            && within(self.refresh_idle, Duration::from_hours(1), day * 90)
            && within(self.family, Duration::from_hours(1), day * 365)
            && self.family >= self.refresh_idle
            && self.refresh_grace <= Duration::from_secs(60)
    }
}

/// What the authorization server serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationServerOptions {
    /// The issuer: `https`, or `http` on a loopback host, without query or
    /// fragment. Endpoints sit under its path.
    pub issuer: String,
    /// Pre-registered clients, at most [`MAX_CLIENTS`].
    pub clients: Vec<ClientOptions>,
    /// Scopes granted when a request names none.
    pub default_scopes: Vec<String>,
    /// Scopes listed in the metadata; also what resolved and registered
    /// clients may request. Without it, they may request the default scopes.
    pub scopes_supported: Option<Vec<String>>,
    /// The consumer's sign-in path; it and the paths under it go to the
    /// [`Interaction`]. Authorization requests redirect here.
    pub interaction_path: String,
    /// Record lifetimes.
    pub lifetimes: Lifetimes,
    /// How long clients may cache the metadata, at most 24 hours.
    pub metadata_max_age: Duration,
}

impl AuthorizationServerOptions {
    /// Options for `issuer` with sign-in at `interaction_path`.
    #[must_use]
    pub fn new(issuer: impl Into<String>, interaction_path: impl Into<String>) -> Self {
        Self {
            issuer: issuer.into(),
            clients: Vec::new(),
            default_scopes: Vec::new(),
            scopes_supported: None,
            interaction_path: interaction_path.into(),
            lifetimes: Lifetimes::default(),
            metadata_max_age: auth::DEFAULT_METADATA_MAX_AGE,
        }
    }
}

/// Why authorization server options are invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizationServerError {
    /// The issuer is invalid, ends in `/` after a path, or the resource does
    /// not list it.
    #[error(
        "the issuer must be an https URL, or http on a loopback host, without a trailing slash, listed by the resource"
    )]
    Issuer,
    /// The client at this index is invalid or repeated, or there are too many.
    #[error("pre-registered client {0} is invalid or repeated")]
    Client(usize),
    /// A scope list is invalid or names `offline_access` in `scopes_supported`.
    #[error("scopes must be valid scope tokens, at most {MAX_REQUESTED_SCOPES} per list")]
    Scope,
    /// The interaction path is not an absolute path, or collides with an endpoint.
    #[error("the interaction path must be an absolute path apart from the endpoints")]
    InteractionPath,
    /// A lifetime is out of range.
    #[error("a lifetime is out of range")]
    Lifetime,
}

/// Authenticates users and records their consent for a pending authorization.
pub trait Interaction: Send + Sync {
    /// Handles a request to the interaction path or a path under it.
    fn interact(&self, request: &InteractionRequest<'_>) -> InteractionOutcome;
}

/// A request routed to the consumer's sign-in pages.
#[derive(Debug)]
pub struct InteractionRequest<'a> {
    /// The HTTP method.
    pub method: &'a str,
    /// The request path.
    pub path: &'a str,
    /// The raw query, without `?`.
    pub query: &'a str,
    /// Header fields, without the library's browser-binding cookies.
    pub headers: &'a [(String, Vec<u8>)],
    /// The request body.
    pub body: &'a [u8],
    /// The pending authorization the request names in its `pending` query or
    /// form field, when this browser started it and it has not expired.
    pub pending: Option<&'a PendingAuthorization>,
}

/// What consent must show about a pending authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAuthorization {
    /// The non-secret reference; send it back as the `pending` form field.
    pub reference: String,
    /// The client.
    pub client_id: String,
    /// The client's display name, when it has one.
    pub client_name: Option<String>,
    /// The redirect URI's host, which consent must show.
    pub redirect_host: String,
    /// The redirect goes to `localhost`, which consent should warn about.
    pub localhost_redirect: bool,
    /// The requested scopes.
    pub scopes: Vec<String>,
    /// The resource the token is for.
    pub resource: String,
}

/// The consumer's answer to an interaction request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionOutcome {
    /// Write this response, such as a sign-in page or a redirect.
    Respond(Response),
    /// The user approved: issue a code for `subject` with `scopes`, which may
    /// narrow the request but never add to it. Accepted only on a `POST` that
    /// names a pending authorization this browser started.
    Approve {
        /// The consumer's opaque user identifier.
        subject: String,
        /// The approved scopes.
        scopes: Vec<String>,
    },
    /// The user declined; the client receives `access_denied`.
    Deny,
}

/// The OAuth 2.1 authorization server.
pub struct AuthorizationServer {
    issuer: String,
    issuer_path: String,
    resource: String,
    clients: Vec<Client>,
    default_scopes: Vec<String>,
    scopes_supported: Option<Vec<String>>,
    interaction_path: String,
    lifetimes: Lifetimes,
    max_age: String,
    secure_cookies: bool,
    store: Arc<dyn GrantStore>,
    interaction: Arc<dyn Interaction>,
    resolver: Option<Arc<dyn ClientResolver>>,
    registrar: Option<Arc<dyn ClientRegistrar>>,
    metadata: Vec<u8>,
}

impl std::fmt::Debug for AuthorizationServer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthorizationServer")
            .field("issuer", &self.issuer)
            .field("resource", &self.resource)
            .field("clients", &self.clients.len())
            .finish_non_exhaustive()
    }
}

/// The paths a server's endpoints answer on.
enum Route {
    Metadata,
    Authorize,
    Token,
    Revoke,
    Register,
    Interaction,
}

impl AuthorizationServer {
    /// Validates `options` for `resource`, whose metadata must list the issuer.
    ///
    /// # Errors
    /// Returns [`AuthorizationServerError`] for an invalid issuer, client,
    /// scope list, interaction path or lifetime.
    pub fn new(
        options: AuthorizationServerOptions,
        resource: &ResourceServer,
        store: Arc<dyn GrantStore>,
        interaction: Arc<dyn Interaction>,
    ) -> Result<Self, AuthorizationServerError> {
        // A trailing slash would put every endpoint under `//`.
        let issuer = auth::Location::parse(&options.issuer)
            .filter(|issuer| issuer.strict)
            .filter(|_| resource.authorization_servers().contains(&options.issuer))
            .ok_or(AuthorizationServerError::Issuer)?;
        if options.clients.len() > MAX_CLIENTS {
            return Err(AuthorizationServerError::Client(MAX_CLIENTS));
        }
        for (index, client) in options.clients.iter().enumerate() {
            let repeated = options.clients[..index]
                .iter()
                .any(|other| other.client_id == client.client_id);
            if repeated || !client::valid_options(client) || !scope_list(&client.scopes) {
                return Err(AuthorizationServerError::Client(index));
            }
        }
        let supported = options.scopes_supported.as_deref();
        if !scope_list(&options.default_scopes)
            || !supported.is_none_or(scope_list)
            || supported.is_some_and(|scopes| scopes.iter().any(|scope| scope == "offline_access"))
        {
            return Err(AuthorizationServerError::Scope);
        }
        if !options.lifetimes.valid() || options.metadata_max_age > auth::MAX_METADATA_MAX_AGE {
            return Err(AuthorizationServerError::Lifetime);
        }
        let issuer_path = issuer.path.clone();
        let mut server = Self {
            issuer: options.issuer,
            secure_cookies: issuer.origin.starts_with("https://"),
            issuer_path,
            resource: resource.resource().to_owned(),
            clients: options.clients.iter().map(Client::configured).collect(),
            default_scopes: options.default_scopes,
            scopes_supported: options.scopes_supported,
            interaction_path: options.interaction_path,
            lifetimes: options.lifetimes,
            max_age: format!("max-age={}", options.metadata_max_age.as_secs()),
            store,
            interaction,
            resolver: None,
            registrar: None,
            metadata: Vec::new(),
        };
        let path = &server.interaction_path;
        let colliding = server
            .endpoint_paths()
            .any(|endpoint| endpoint == *path || endpoint.starts_with(&format!("{path}/")));
        if !path.starts_with('/')
            || path.len() > crate::http::MAX_PATH_BYTES
            || path.ends_with('/')
            || !path
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && byte != b'?' && byte != b'#')
            || colliding
        {
            return Err(AuthorizationServerError::InteractionPath);
        }
        server.prepare_metadata();
        Ok(server)
    }

    /// Accepts clients identified by Client ID Metadata Document URLs.
    #[must_use]
    pub fn with_resolver(mut self, resolver: Arc<dyn ClientResolver>) -> Self {
        self.resolver = Some(resolver);
        self.prepare_metadata();
        self
    }

    /// Enables dynamic registration (RFC 7591) through `registrar`.
    #[must_use]
    pub fn with_registrar(mut self, registrar: Arc<dyn ClientRegistrar>) -> Self {
        self.registrar = Some(registrar);
        self.prepare_metadata();
        self
    }

    /// The issuer.
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Every path this server answers on, besides the paths under the
    /// interaction path.
    pub fn paths(&self) -> impl Iterator<Item = String> + '_ {
        self.endpoint_paths()
            .chain(std::iter::once(self.interaction_path.clone()))
    }

    /// Whether this server answers `target`'s path.
    #[must_use]
    pub fn routes(&self, target: &str) -> bool {
        self.route(crate::http::request_path(target)).is_some()
    }

    fn endpoint_paths(&self) -> impl Iterator<Item = String> + '_ {
        let base = &self.issuer_path;
        [
            format!("{METADATA_PREFIX}{base}"),
            format!("{base}/authorize"),
            format!("{base}/token"),
            format!("{base}/revoke"),
        ]
        .into_iter()
        .chain(self.registrar.is_some().then(|| format!("{base}/register")))
    }

    fn route(&self, path: &str) -> Option<Route> {
        let base = &self.issuer_path;
        if path == format!("{METADATA_PREFIX}{base}") {
            return Some(Route::Metadata);
        }
        if let Some(rest) = path.strip_prefix(base.as_str()) {
            match rest {
                "/authorize" => return Some(Route::Authorize),
                "/token" => return Some(Route::Token),
                "/revoke" => return Some(Route::Revoke),
                "/register" if self.registrar.is_some() => return Some(Route::Register),
                _ => {}
            }
        }
        let interaction = &self.interaction_path;
        (path == interaction
            || path
                .strip_prefix(interaction.as_str())
                .is_some_and(|rest| rest.starts_with('/')))
        .then_some(Route::Interaction)
    }

    /// Answers a request to one of this server's paths; any other path gets
    /// `404`. The body must already be read, within [`MAX_INTERACTION_BODY_BYTES`].
    #[must_use]
    pub fn handle(&self, head: &RequestHead<'_>, body: &[u8]) -> Response {
        let path = crate::http::request_path(head.target);
        let query = head
            .target
            .split_once('?')
            .map_or("", |(_, query)| query.split('#').next().unwrap_or_default());
        let Some(route) = self.route(path) else {
            return wire::page(404, "Not found");
        };
        let cross_origin = matches!(
            route,
            Route::Metadata | Route::Token | Route::Revoke | Route::Register
        );
        if cross_origin && head.method == "OPTIONS" {
            return preflight();
        }
        let expected = match route {
            Route::Metadata | Route::Authorize => "GET",
            Route::Token | Route::Revoke | Route::Register => "POST",
            Route::Interaction => head.method,
        };
        if head.method != expected {
            let mut response = wire::page(405, "Method not allowed");
            response.headers.push(("Allow", expected.to_owned().into()));
            return response;
        }
        if body.len() > MAX_FORM_BYTES && !matches!(route, Route::Interaction) {
            return wire::oauth_error(413, "invalid_request", "The request is too large");
        }
        match route {
            Route::Metadata => self.metadata_response(),
            Route::Authorize => self.authorize(query),
            Route::Token => self.token(head.headers, body),
            Route::Revoke => self.revoke(head.headers, body),
            Route::Register => self.register(head.headers, body),
            Route::Interaction => self.interact(head, path, query, body),
        }
    }

    fn prepare_metadata(&mut self) {
        let base = self.issuer.trim_end_matches('/');
        let mut methods = self
            .clients
            .iter()
            .map(|client| client.auth.name())
            .collect::<Vec<_>>();
        if self.resolver.is_some() {
            methods.push(ClientAuth::None.name());
        }
        if self.registrar.is_some() {
            methods.extend([
                ClientAuth::None.name(),
                ClientAuth::SecretBasic.name(),
                ClientAuth::SecretPost.name(),
            ]);
        }
        methods.sort_unstable();
        methods.dedup();
        let mut document = Map::new();
        document.insert("issuer".into(), json!(self.issuer));
        document.insert(
            "authorization_endpoint".into(),
            json!(format!("{base}/authorize")),
        );
        document.insert("token_endpoint".into(), json!(format!("{base}/token")));
        document.insert(
            "revocation_endpoint".into(),
            json!(format!("{base}/revoke")),
        );
        if self.registrar.is_some() {
            document.insert(
                "registration_endpoint".into(),
                json!(format!("{base}/register")),
            );
        }
        document.insert("response_types_supported".into(), json!(["code"]));
        document.insert(
            "grant_types_supported".into(),
            json!(["authorization_code", "refresh_token"]),
        );
        document.insert("code_challenge_methods_supported".into(), json!(["S256"]));
        document.insert(
            "token_endpoint_auth_methods_supported".into(),
            json!(methods),
        );
        document.insert(
            "revocation_endpoint_auth_methods_supported".into(),
            json!(methods),
        );
        if let Some(scopes) = &self.scopes_supported {
            document.insert("scopes_supported".into(), json!(scopes));
        }
        document.insert(
            "authorization_response_iss_parameter_supported".into(),
            json!(true),
        );
        if self.resolver.is_some() {
            document.insert("client_id_metadata_document_supported".into(), json!(true));
        }
        self.metadata = serde_json::to_vec(&Value::Object(document)).unwrap_or_default();
    }

    fn metadata_response(&self) -> Response {
        Response {
            status: 200,
            headers: vec![
                ("Content-Type", "application/json".into()),
                ("Cache-Control", self.max_age.clone().into()),
                ("Access-Control-Allow-Origin", "*".into()),
            ],
            body: self.metadata.clone(),
        }
    }

    /// Resolves `client_id`: pre-registered, registered or a metadata document.
    fn client(&self, client_id: &str) -> Result<Client, &'static str> {
        if let Some(client) = self.clients.iter().find(|client| client.id == client_id) {
            return Ok(client.clone());
        }
        if let Some(client) = self
            .registrar
            .as_ref()
            .and_then(|registrar| registrar.lookup(client_id))
        {
            return Ok(Client::registered(client));
        }
        match &self.resolver {
            Some(resolver) if client::is_document_id(client_id) => {
                let document = resolver
                    .resolve(client_id)
                    .map_err(|_| "The client metadata document cannot be fetched")?;
                client::from_document(client_id, &document)
            }
            _ => Err("The client is not known"),
        }
    }

    /// Scopes a client may request.
    fn allowed_scopes<'a>(&'a self, client: &'a Client) -> &'a [String] {
        client
            .scopes
            .as_deref()
            .or(self.scopes_supported.as_deref())
            .unwrap_or(&self.default_scopes)
    }
}

impl TokenVerifier for AuthorizationServer {
    fn verify(&self, token: BearerToken<'_>, _: &str) -> Result<Grant, Rejection> {
        let token = token.as_str();
        if !token.starts_with("at_") {
            return Err(Rejection::invalid());
        }
        match self
            .store
            .lookup_access(&wire::digest(token), SystemTime::now())
        {
            Ok(Some((access, family))) => Ok(Grant::new(vec![family.resource])
                .with_scopes(access.scopes)
                .with_expiry(access.expires_at)
                .with_subject(family.subject)
                .with_client_id(family.client_id)),
            Ok(None) => Err(Rejection::invalid()),
            Err(_) => Err(Rejection::Unavailable),
        }
    }
}

/// A CORS preflight answer for the cross-origin endpoints.
fn preflight() -> Response {
    Response {
        status: 204,
        headers: vec![
            ("Access-Control-Allow-Origin", "*".into()),
            ("Access-Control-Allow-Methods", "GET, POST".into()),
            (
                "Access-Control-Allow-Headers",
                "Authorization, Content-Type".into(),
            ),
            ("Access-Control-Max-Age", "600".into()),
        ],
        body: Vec::new(),
    }
}

/// At most [`MAX_REQUESTED_SCOPES`] valid scope tokens.
fn scope_list(scopes: &[String]) -> bool {
    scopes.len() <= MAX_REQUESTED_SCOPES && scopes.iter().all(|scope| auth::valid_scope(scope))
}

/// `now + lifetime`, capped at `limit`.
fn expiry(now: SystemTime, lifetime: Duration, limit: Option<SystemTime>) -> SystemTime {
    let expires = now + lifetime;
    limit.map_or(expires, |limit| expires.min(limit))
}
