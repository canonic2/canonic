//! OAuth resource-server checks for the Streamable HTTP exchange.
//!
//! A [`ResourceServer`] holds the server's canonical resource URI and its
//! prepared protected-resource metadata. The exchange uses it to serve the
//! metadata, read the bearer token before the body, and build every challenge.
//! The consumer's [`TokenVerifier`] decides whether a token is genuine; the
//! library then refuses a [`Grant`] issued for another resource, past its
//! expiry or missing a required scope. A [`ScopePolicy`] names the scopes each
//! operation needs. Nothing here stores state or performs I/O.
//!
//! ```
//! use canonic_mcp_kit::http::{Grant, ResourceOptions, ResourceServer};
//!
//! let server = ResourceServer::new(ResourceOptions::new(
//!     "https://mcp.example.com/mcp",
//!     vec!["https://auth.example.com".to_owned()],
//! ))?;
//! let grant = Grant::new(vec!["https://mcp.example.com/mcp".to_owned()]);
//! assert!(server.check_grant(Ok(grant), None).is_ok());
//! # Ok::<(), canonic_mcp_kit::http::AuthorizationError>(())
//! ```

use super::{Admitted, Field, RequestHead, Response, field};
use crate::rpc::{self, RpcError};
use serde_json::{Map, Value, json};
use std::{
    any::Any,
    borrow::Cow,
    collections::BTreeSet,
    fmt,
    net::IpAddr,
    sync::Arc,
    time::{Duration, SystemTime},
};

#[cfg(test)]
mod tests;

/// The well-known prefix of protected-resource metadata (RFC 9728).
pub const METADATA_PREFIX: &str = "/.well-known/oauth-protected-resource";
/// The longest resource, issuer or documentation URL, in bytes.
pub const MAX_URL_BYTES: usize = 2048;
/// The most authorization servers the metadata lists.
pub const MAX_AUTHORIZATION_SERVERS: usize = 8;
/// The most scopes in `scopes_supported` or the base requirement.
pub const MAX_CONFIGURED_SCOPES: usize = 64;
/// The longest scope token, in bytes.
pub const MAX_SCOPE_BYTES: usize = 256;
/// The longest `resource_name`, in bytes.
pub const MAX_RESOURCE_NAME_BYTES: usize = 256;
/// The largest encoded metadata document, in bytes.
pub const MAX_METADATA_BYTES: usize = 16 * 1024;
/// The default metadata cache interval.
pub const DEFAULT_METADATA_MAX_AGE: Duration = Duration::from_secs(3600);
/// The longest metadata cache interval.
pub const MAX_METADATA_MAX_AGE: Duration = Duration::from_hours(24);
/// The longest bearer token, in bytes.
pub const MAX_TOKEN_BYTES: usize = 8 * 1024;
/// The most audiences a grant may report.
pub const MAX_GRANT_AUDIENCES: usize = 16;
/// The most scopes a grant may carry.
pub const MAX_GRANT_SCOPES: usize = 256;
/// The longest grant subject or client ID, in bytes.
pub const MAX_GRANT_IDENTITY_BYTES: usize = 1024;
/// The most scopes one operation may require.
pub const MAX_REQUIRED_SCOPES: usize = 32;
/// The longest challenge `error_description`, in bytes.
pub const MAX_DESCRIPTION_BYTES: usize = 256;

/// What a protected server declares about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceOptions {
    /// The canonical resource URI tokens must be issued for, such as
    /// `https://mcp.example.com/mcp`: `https`, or `http` with a loopback host.
    pub resource: String,
    /// Issuer URLs of the authorization servers clients may use, 1 to
    /// [`MAX_AUTHORIZATION_SERVERS`].
    pub authorization_servers: Vec<String>,
    /// Scopes listed in the metadata; never `offline_access`.
    pub scopes_supported: Option<Vec<String>>,
    /// Scopes every request needs, checked before the body is read.
    pub required_scopes: Vec<String>,
    /// A display name for the metadata.
    pub resource_name: Option<String>,
    /// An `https` URL of human-readable documentation.
    pub resource_documentation: Option<String>,
    /// How long clients may cache the metadata.
    pub metadata_max_age: Duration,
    /// Also serve the metadata at the root well-known path.
    pub serve_root_metadata: bool,
}

impl ResourceOptions {
    /// Options for `resource`, protected by `authorization_servers`.
    #[must_use]
    pub fn new(resource: impl Into<String>, authorization_servers: Vec<String>) -> Self {
        Self {
            resource: resource.into(),
            authorization_servers,
            scopes_supported: None,
            required_scopes: Vec::new(),
            resource_name: None,
            resource_documentation: None,
            metadata_max_age: DEFAULT_METADATA_MAX_AGE,
            serve_root_metadata: true,
        }
    }
}

/// Why resource options are invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizationError {
    /// The resource URI is not an absolute `https` (or loopback `http`) URI
    /// without userinfo, query, fragment or a trailing slash after its path.
    #[error(
        "the resource must be an https URI, or http on a loopback host, without query, fragment or trailing slash"
    )]
    Resource,
    /// There are no authorization servers, or more than the bound.
    #[error("between 1 and {MAX_AUTHORIZATION_SERVERS} authorization servers are required")]
    AuthorizationServers,
    /// The issuer at this index is invalid or repeated.
    #[error(
        "authorization server {0} must be a unique https issuer, or http on a loopback host, without query or fragment"
    )]
    Issuer(usize),
    /// A configured scope is invalid, or a list is too long.
    #[error(
        "scopes must be 1 to {MAX_SCOPE_BYTES} bytes of scope characters, at most {MAX_CONFIGURED_SCOPES} per list"
    )]
    Scope,
    /// `scopes_supported` lists `offline_access`.
    #[error("scopes_supported must not list offline_access")]
    OfflineAccess,
    /// The resource name is empty, too long or has control characters.
    #[error(
        "resource_name must be 1 to {MAX_RESOURCE_NAME_BYTES} bytes without control characters"
    )]
    ResourceName,
    /// The documentation URL is not an absolute `https` URL within the bound.
    #[error("resource_documentation must be an https URL")]
    Documentation,
    /// The metadata cache interval is too long.
    #[error("the metadata cache interval must be at most 24 hours")]
    MaxAge,
    /// The encoded metadata exceeds [`MAX_METADATA_BYTES`].
    #[error("the protected-resource metadata exceeds {MAX_METADATA_BYTES} bytes")]
    Metadata,
}

/// Prepared resource identity, metadata and challenges, shared by every request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceServer {
    resource: String,
    metadata_url: String,
    metadata_path: String,
    serve_root: bool,
    document: Vec<u8>,
    max_age: String,
    required: Vec<String>,
    /// The `scope` of a `401` challenge, when one applies.
    challenge_scope: Option<String>,
}

/// A bearer token read from a request. Its `Debug` output hides the value.
#[derive(Clone, Copy)]
pub struct BearerToken<'a>(&'a str);

impl<'a> BearerToken<'a> {
    /// The token, for the verifier only.
    #[must_use]
    pub fn as_str(&self) -> &'a str {
        self.0
    }
}

impl fmt::Debug for BearerToken<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BearerToken(..)")
    }
}

/// Decides whether a bearer token is genuine and what it grants.
///
/// The verifier checks signatures, issuers or revocation however it likes,
/// such as with keys it holds, by introspection or by a store lookup. It must
/// report the audiences the token was issued for: the library refuses a grant
/// whose audiences do not include this server. The HTTP driver calls it on the
/// request's own connection thread.
pub trait TokenVerifier: Send + Sync {
    /// Verifies `token` presented to `resource`, the canonical resource URI.
    ///
    /// # Errors
    /// Returns [`Rejection::Invalid`] for a token that is not valid, and
    /// [`Rejection::Unavailable`] when it cannot decide.
    fn verify(&self, token: BearerToken<'_>, resource: &str) -> Result<Grant, Rejection>;
}

/// Why a verifier refused a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    /// The token is unknown, malformed, expired, revoked or from another
    /// issuer. Answered with `401` and `invalid_token`.
    Invalid {
        /// A short reason for the challenge, in RFC 6750 characters. Never
        /// include the token.
        description: Option<String>,
    },
    /// The verifier cannot decide now, such as when introspection is
    /// unreachable. Answered with `503`.
    Unavailable,
}

impl Rejection {
    /// An invalid token without a description.
    #[must_use]
    pub fn invalid() -> Self {
        Self::Invalid { description: None }
    }
}

/// What a verified token authorizes, as reported by the verifier.
///
/// The grant never contains the token itself. Handlers read it with
/// [`Context::authorization`](crate::stdio::Context::authorization).
#[derive(Clone)]
pub struct Grant {
    audiences: Vec<String>,
    scopes: BTreeSet<String>,
    expires_at: Option<SystemTime>,
    subject: Option<String>,
    client_id: Option<String>,
    extension: Option<Arc<dyn Any + Send + Sync>>,
}

impl Grant {
    /// A grant for tokens issued to `audiences`, without scopes or expiry.
    #[must_use]
    pub fn new(audiences: Vec<String>) -> Self {
        Self {
            audiences,
            scopes: BTreeSet::new(),
            expires_at: None,
            subject: None,
            client_id: None,
            extension: None,
        }
    }

    /// Adds granted scopes.
    #[must_use]
    pub fn with_scopes<I: IntoIterator<Item = S>, S: Into<String>>(mut self, scopes: I) -> Self {
        self.scopes.extend(scopes.into_iter().map(Into::into));
        self
    }

    /// Sets when the token stops being valid.
    #[must_use]
    pub fn with_expiry(mut self, expires_at: SystemTime) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Sets the opaque identifier of the resource owner.
    #[must_use]
    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Sets the OAuth client the token was issued to.
    #[must_use]
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Attaches the server's own authority context, such as a tenant.
    #[must_use]
    pub fn with_extension<T: Any + Send + Sync>(mut self, extension: T) -> Self {
        self.extension = Some(Arc::new(extension));
        self
    }

    /// The audiences the token was issued for.
    #[must_use]
    pub fn audiences(&self) -> &[String] {
        &self.audiences
    }

    /// The granted scopes, in sorted order.
    pub fn scopes(&self) -> impl Iterator<Item = &str> {
        self.scopes.iter().map(String::as_str)
    }

    /// Whether `scope` was granted exactly.
    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.contains(scope)
    }

    /// When the token stops being valid, if it says.
    #[must_use]
    pub fn expires_at(&self) -> Option<SystemTime> {
        self.expires_at
    }

    /// The resource owner, if known.
    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    /// The OAuth client, if known.
    #[must_use]
    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    /// The attached authority context, if it has type `T`.
    #[must_use]
    pub fn extension<T: Any>(&self) -> Option<&T> {
        self.extension.as_deref()?.downcast_ref()
    }

    fn within_bounds(&self) -> bool {
        (1..=MAX_GRANT_AUDIENCES).contains(&self.audiences.len())
            && self
                .audiences
                .iter()
                .all(|audience| audience.len() <= MAX_URL_BYTES)
            && self.scopes.len() <= MAX_GRANT_SCOPES
            && self.scopes.iter().all(|scope| valid_scope(scope))
            && [&self.subject, &self.client_id]
                .into_iter()
                .flatten()
                .all(|value| !value.is_empty() && value.len() <= MAX_GRANT_IDENTITY_BYTES)
    }
}

impl fmt::Debug for Grant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Grant")
            .field("audiences", &self.audiences)
            .field("scopes", &self.scopes)
            .field("expires_at", &self.expires_at)
            .field("subject", &self.subject)
            .field("client_id", &self.client_id)
            .field("extension", &self.extension.is_some())
            .finish()
    }
}

/// The operation a scope policy decides on.
#[derive(Debug, Clone, Copy)]
pub struct Operation<'a> {
    /// The JSON-RPC method.
    pub method: &'a str,
    /// The tool or prompt name, or the resource URI for `resources/read`.
    pub name: Option<&'a str>,
    /// The request parameters, including tool arguments and listen filters.
    pub params: &'a Value,
}

/// Which scopes each operation needs.
pub trait ScopePolicy: Send + Sync {
    /// Every scope `operation` needs, at most [`MAX_REQUIRED_SCOPES`]. The
    /// requirement may depend on arguments.
    fn required(&self, operation: &Operation<'_>) -> Vec<String>;

    /// Whether `grant` covers `scope`. The default is exact membership; a
    /// policy with scope hierarchies overrides it.
    fn satisfies(&self, grant: &Grant, scope: &str) -> bool {
        grant.has_scope(scope)
    }
}

impl ResourceServer {
    /// Validates `options` and prepares the metadata document once.
    ///
    /// # Errors
    /// Returns [`AuthorizationError`] for an invalid resource, issuer, scope,
    /// name, documentation URL, cache interval or oversized metadata.
    pub fn new(options: ResourceOptions) -> Result<Self, AuthorizationError> {
        let resource = Location::parse(&options.resource)
            .filter(|location| location.strict)
            .ok_or(AuthorizationError::Resource)?;
        if !(1..=MAX_AUTHORIZATION_SERVERS).contains(&options.authorization_servers.len()) {
            return Err(AuthorizationError::AuthorizationServers);
        }
        for (index, issuer) in options.authorization_servers.iter().enumerate() {
            let unique = !options.authorization_servers[..index].contains(issuer);
            if !unique || Location::parse(issuer).is_none() {
                return Err(AuthorizationError::Issuer(index));
            }
        }
        let scope_list = |scopes: &[String]| {
            scopes.len() <= MAX_CONFIGURED_SCOPES && scopes.iter().all(|scope| valid_scope(scope))
        };
        let supported = options.scopes_supported.as_deref();
        if !scope_list(&options.required_scopes) || !supported.is_none_or(scope_list) {
            return Err(AuthorizationError::Scope);
        }
        if supported.is_some_and(|scopes| scopes.iter().any(|scope| scope == "offline_access")) {
            return Err(AuthorizationError::OfflineAccess);
        }
        if options.resource_name.as_ref().is_some_and(|name| {
            name.is_empty()
                || name.len() > MAX_RESOURCE_NAME_BYTES
                || name.chars().any(char::is_control)
        }) {
            return Err(AuthorizationError::ResourceName);
        }
        if options
            .resource_documentation
            .as_ref()
            .is_some_and(|url| !url.starts_with("https://") || Location::parse(url).is_none())
        {
            return Err(AuthorizationError::Documentation);
        }
        if options.metadata_max_age > MAX_METADATA_MAX_AGE {
            return Err(AuthorizationError::MaxAge);
        }
        let mut document = Map::new();
        document.insert("resource".into(), json!(resource.uri));
        document.insert(
            "authorization_servers".into(),
            json!(options.authorization_servers),
        );
        document.insert("bearer_methods_supported".into(), json!(["header"]));
        if let Some(scopes) = supported {
            document.insert("scopes_supported".into(), json!(scopes));
        }
        if let Some(name) = &options.resource_name {
            document.insert("resource_name".into(), json!(name));
        }
        if let Some(url) = &options.resource_documentation {
            document.insert("resource_documentation".into(), json!(url));
        }
        let document = serde_json::to_vec(&Value::Object(document))
            .ok()
            .filter(|document| document.len() <= MAX_METADATA_BYTES)
            .ok_or(AuthorizationError::Metadata)?;
        let metadata_path = format!("{METADATA_PREFIX}{}", resource.path);
        let challenge_scope = if options.required_scopes.is_empty() {
            supported
                .filter(|scopes| !scopes.is_empty())
                .map(|scopes| scopes.join(" "))
        } else {
            Some(options.required_scopes.join(" "))
        };
        Ok(Self {
            metadata_url: format!("{}{metadata_path}", resource.origin),
            metadata_path,
            resource: resource.uri,
            serve_root: options.serve_root_metadata,
            document,
            max_age: format!("max-age={}", options.metadata_max_age.as_secs()),
            required: options.required_scopes,
            challenge_scope,
        })
    }

    /// The canonical resource URI, normalized.
    #[must_use]
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// The protected-resource metadata URL named in every challenge.
    #[must_use]
    pub fn metadata_url(&self) -> &str {
        &self.metadata_url
    }

    /// The metadata paths this server answers on.
    pub fn metadata_paths(&self) -> impl Iterator<Item = &str> {
        let root =
            (self.serve_root && self.metadata_path != METADATA_PREFIX).then_some(METADATA_PREFIX);
        std::iter::once(self.metadata_path.as_str()).chain(root)
    }

    /// The response for a request to one of [`Self::metadata_paths`]: the
    /// document for `GET`, `405` otherwise.
    #[must_use]
    pub fn metadata_response(&self, method: &str) -> Response {
        if method != "GET" {
            let mut response = Response::empty(405);
            response.headers.push(("Allow", "GET".into()));
            return response;
        }
        Response {
            status: 200,
            headers: vec![
                ("Content-Type", "application/json".into()),
                ("Cache-Control", self.max_age.clone().into()),
                ("Access-Control-Allow-Origin", "*".into()),
            ],
            body: self.document.clone(),
        }
    }

    /// Reads the request's bearer token without verifying it.
    ///
    /// # Errors
    /// Returns `401` without an error code when no token is sent, and `400`
    /// `invalid_request` for another scheme, invalid syntax, a repeated
    /// `Authorization` field, an oversized token or an `access_token` query
    /// parameter.
    pub fn bearer<'a>(&self, head: &RequestHead<'a>) -> Result<BearerToken<'a>, Response> {
        let in_query = head.target.split_once('?').is_some_and(|(_, query)| {
            query
                .split('#')
                .next()
                .unwrap_or_default()
                .split('&')
                .any(|pair| pair.split('=').next() == Some("access_token"))
        });
        let invalid = || self.challenge(400, Some("invalid_request"), None, None);
        if in_query {
            return Err(invalid());
        }
        let value = match field(head.headers, "authorization") {
            Field::Missing => {
                return Err(self.challenge(401, None, None, self.challenge_scope.as_deref()));
            }
            Field::Repeated => return Err(invalid()),
            Field::One(value) => value,
        };
        let token = std::str::from_utf8(value)
            .ok()
            .and_then(|value| {
                let (scheme, rest) = value.split_once(' ')?;
                scheme
                    .eq_ignore_ascii_case("Bearer")
                    .then(|| rest.trim_start_matches(' '))
            })
            .filter(|token| token.len() <= MAX_TOKEN_BYTES && b64token(token))
            .ok_or_else(invalid)?;
        Ok(BearerToken(token))
    }

    /// Applies the library's checks to a verifier's outcome: bounds, audience,
    /// expiry and the base scope requirement.
    ///
    /// # Errors
    /// Returns `401` `invalid_token` for a rejected, out-of-bounds, foreign or
    /// expired grant, `503` when the verifier could not decide, and `403`
    /// `insufficient_scope` when a base scope is missing.
    pub fn check_grant(
        &self,
        outcome: Result<Grant, Rejection>,
        policy: Option<&dyn ScopePolicy>,
    ) -> Result<Grant, Response> {
        let invalid = |description: Option<&str>| {
            // Every 401 names the scopes to request, so a client replacing an
            // expired or rejected token asks for them in one round.
            self.challenge(
                401,
                Some("invalid_token"),
                description,
                self.challenge_scope.as_deref(),
            )
        };
        let grant = match outcome {
            Ok(grant) => grant,
            Err(Rejection::Invalid { description }) => {
                return Err(invalid(description.as_deref()));
            }
            Err(Rejection::Unavailable) => {
                let mut response = Response::empty(503);
                response.headers.push(("Cache-Control", "no-store".into()));
                return Err(response);
            }
        };
        if !grant.within_bounds() {
            return Err(invalid(None));
        }
        let for_us = grant
            .audiences
            .iter()
            .filter_map(|audience| Location::parse(audience))
            .any(|audience| audience.uri == self.resource);
        if !for_us {
            return Err(invalid(Some("The token was not issued for this resource")));
        }
        if grant
            .expires_at
            .is_some_and(|expires_at| expires_at <= SystemTime::now())
        {
            return Err(invalid(Some("The token has expired")));
        }
        if !Self::covers(&grant, &self.required, policy) {
            let scope = self.required.join(" ");
            return Err(self.challenge(403, Some("insufficient_scope"), None, Some(&scope)));
        }
        Ok(grant)
    }

    /// Checks an admitted request against the operation's scope requirement.
    ///
    /// # Errors
    /// Returns `403` `insufficient_scope` naming every required scope when the
    /// grant lacks one, or an internal-error reply when the policy names an
    /// invalid scope or more than [`MAX_REQUIRED_SCOPES`].
    pub fn check_scopes(
        &self,
        admitted: &Admitted,
        grant: &Grant,
        policy: &dyn ScopePolicy,
    ) -> Result<(), Response> {
        let params = &admitted.message["params"];
        let source = if admitted.method == "resources/read" {
            "uri"
        } else {
            "name"
        };
        let operation = Operation {
            method: &admitted.method,
            name: params.get(source).and_then(Value::as_str),
            params,
        };
        self.check_operation(&operation, grant, policy, || {
            let error = RpcError::new(rpc::INTERNAL_ERROR, "Scope policy failed");
            Response::failure(200, admitted.id.clone(), &error)
        })
    }

    /// Checks a notification against its scope requirement before it is
    /// accepted. The operation has the notification's method and parameters
    /// and no name.
    ///
    /// # Errors
    /// Returns `403` `insufficient_scope` naming every required scope when the
    /// grant lacks one, or `500` without a body when the policy names an
    /// invalid scope or more than [`MAX_REQUIRED_SCOPES`]; a notification gets
    /// no JSON-RPC reply.
    pub fn check_notification(
        &self,
        method: &str,
        params: &Value,
        grant: &Grant,
        policy: &dyn ScopePolicy,
    ) -> Result<(), Response> {
        let operation = Operation {
            method,
            name: None,
            params,
        };
        self.check_operation(&operation, grant, policy, || Response::empty(500))
    }

    fn check_operation(
        &self,
        operation: &Operation<'_>,
        grant: &Grant,
        policy: &dyn ScopePolicy,
        failed: impl FnOnce() -> Response,
    ) -> Result<(), Response> {
        let mut required = policy.required(operation);
        let mut seen = BTreeSet::new();
        required.retain(|scope| seen.insert(scope.clone()));
        if required.len() > MAX_REQUIRED_SCOPES || !required.iter().all(|scope| valid_scope(scope))
        {
            return Err(failed());
        }
        if Self::covers(grant, &required, Some(policy)) {
            return Ok(());
        }
        let scope = required.join(" ");
        Err(self.challenge(403, Some("insufficient_scope"), None, Some(&scope)))
    }

    fn covers(grant: &Grant, required: &[String], policy: Option<&dyn ScopePolicy>) -> bool {
        required.iter().all(|scope| match policy {
            Some(policy) => policy.satisfies(grant, scope),
            None => grant.has_scope(scope),
        })
    }

    /// A bodiless response with a `Bearer` challenge.
    fn challenge(
        &self,
        status: u16,
        error: Option<&str>,
        description: Option<&str>,
        scope: Option<&str>,
    ) -> Response {
        let mut parameters = Vec::new();
        if let Some(error) = error {
            parameters.push(format!("error=\"{error}\""));
        }
        if let Some(description) = description.filter(|text| valid_description(text)) {
            parameters.push(format!("error_description=\"{description}\""));
        }
        if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
            parameters.push(format!("scope=\"{scope}\""));
        }
        parameters.push(format!("resource_metadata=\"{}\"", self.metadata_url));
        Response {
            status,
            headers: vec![
                (
                    "WWW-Authenticate",
                    Cow::Owned(format!("Bearer {}", parameters.join(", "))),
                ),
                ("Cache-Control", "no-store".into()),
            ],
            body: Vec::new(),
        }
    }
}

/// An RFC 6749 scope token.
fn valid_scope(scope: &str) -> bool {
    (1..=MAX_SCOPE_BYTES).contains(&scope.len())
        && scope.bytes().all(|byte| {
            byte == 0x21 || (0x23..=0x5b).contains(&byte) || (0x5d..=0x7e).contains(&byte)
        })
}

/// An RFC 6750 `error_description` within its bound.
fn valid_description(text: &str) -> bool {
    text.len() <= MAX_DESCRIPTION_BYTES
        && text
            .bytes()
            .all(|byte| matches!(byte, 0x20..=0x21 | 0x23..=0x5b | 0x5d..=0x7e))
}

/// An RFC 6750 `b64token`.
fn b64token(token: &str) -> bool {
    let body = token.trim_end_matches('=');
    !body.is_empty()
        && body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~+/".contains(&byte))
}

/// A normalized `https` or loopback `http` URI.
struct Location {
    /// Lowercase scheme and host, explicit port only when not the default.
    origin: String,
    /// The path, without a lone `/`.
    path: String,
    /// `origin` followed by `path`.
    uri: String,
    /// The path has no trailing slash, as a configured resource needs.
    strict: bool,
}

impl Location {
    /// Parses an absolute URI without userinfo, query or fragment.
    fn parse(text: &str) -> Option<Self> {
        if text.len() > MAX_URL_BYTES || iri_string::types::UriStr::new(text).is_err() {
            return None;
        }
        let (scheme, rest) = text.split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        if rest.contains(['?', '#', '@']) {
            return None;
        }
        let (authority, path) = rest
            .find('/')
            .map_or((rest, ""), |index| rest.split_at(index));
        let (host, port) = match authority.rfind(':') {
            Some(index) if !authority[index..].contains(']') => {
                (&authority[..index], Some(&authority[index + 1..]))
            }
            _ => (authority, None),
        };
        let host = host.to_ascii_lowercase();
        if host.is_empty() {
            return None;
        }
        let port = match port {
            Some(port) => Some(port.parse::<u16>().ok()?),
            None => None,
        };
        let loopback = host == "localhost"
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        let default_port = match scheme.as_str() {
            "https" => 443,
            "http" if loopback => 80,
            _ => return None,
        };
        let authority = match port {
            Some(port) if port != default_port => format!("{host}:{port}"),
            _ => host,
        };
        let origin = format!("{scheme}://{authority}");
        let path = if path == "/" { "" } else { path };
        Some(Self {
            uri: format!("{origin}{path}"),
            origin,
            strict: !path.ends_with('/'),
            path: path.to_owned(),
        })
    }
}
