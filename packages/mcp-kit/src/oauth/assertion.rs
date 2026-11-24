//! Enterprise-Managed Authorization: the JWT bearer grant (RFC 7523) for
//! identity assertion authorization grants (ID-JAG) that a trusted enterprise
//! identity provider signs for one of this server's clients.
//!
//! The library verifies the assertion's signature with the issuer's keys,
//! checks its type, audience, client, resource, lifetime and single use, and
//! issues an access token without a refresh token. The consumer configures the
//! trusted issuers, fetches their key sets and maps each enterprise user to its
//! own subject and scopes through [`IdentityAssertions`].

use super::{
    AuthorizationServer, AuthorizationServerError, MAX_REQUESTED_SCOPES,
    client::Client,
    jose::{self, Compact, Key, time},
    store::{AssertionRecord, Digest},
    token::{access_only, invalid_request, unavailable},
    wire,
};
use crate::http::{Response, auth};
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    ops::RangeInclusive,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime},
};

/// The Enterprise-Managed Authorization extension identifier. A server whose
/// authorization server accepts identity assertions may advertise it through
/// [`SessionOptions::extensions`](crate::SessionOptions::extensions).
pub const ENTERPRISE_MANAGED_AUTHORIZATION_EXTENSION: &str =
    "io.modelcontextprotocol/enterprise-managed-authorization";
/// The JWT bearer grant type (RFC 7523).
pub const JWT_BEARER_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";
/// The identity assertion grant profile, listed in the metadata's
/// `authorization_grant_profiles_supported`.
pub const ID_JAG_PROFILE: &str = "urn:ietf:params:oauth:grant-profile:id-jag";
/// The `typ` header an identity assertion must carry.
pub const ID_JAG_TYPE: &str = "oauth-id-jag+jwt";
/// The most trusted issuers.
pub const MAX_TRUSTED_ISSUERS: usize = 16;
/// The largest assertion, in bytes.
pub const MAX_ASSERTION_BYTES: usize = 8 * 1024;
/// The largest key set an issuer may serve, in bytes.
pub const MAX_KEY_SET_BYTES: usize = 64 * 1024;
/// The most keys in one key set.
pub const MAX_KEYS: usize = 32;
/// Accepted RSA modulus sizes, in bits.
pub const RSA_MODULUS_BITS: RangeInclusive<usize> = 2048..=4096;
/// The longest an assertion may remain valid from when it is presented.
pub const MAX_ASSERTION_LIFETIME: Duration = Duration::from_hours(1);
/// Clock difference tolerated for `exp`, `iat` and `nbf`.
pub const ASSERTION_CLOCK_SKEW: Duration = Duration::from_mins(1);
/// The longest `jti` or `tenant`, in bytes.
pub const MAX_ASSERTION_ID_BYTES: usize = 256;
/// The longest `email` passed to the consumer, in bytes.
pub const MAX_EMAIL_BYTES: usize = 320;

/// A signature algorithm an issuer may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SigningAlgorithm {
    /// RSASSA-PKCS1-v1_5 with SHA-256, mandatory to implement.
    Rs256,
    /// ECDSA on P-256 with SHA-256.
    Es256,
}

impl SigningAlgorithm {
    /// The JWA name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Rs256 => "RS256",
            Self::Es256 => "ES256",
        }
    }

    pub(super) fn parse(name: &str) -> Option<Self> {
        [Self::Rs256, Self::Es256]
            .into_iter()
            .find(|algorithm| algorithm.name() == name)
    }
}

/// An enterprise identity provider whose assertions this server accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedIssuer {
    /// The issuer identifier, compared exactly with the assertion's `iss`:
    /// `https`, or `http` on a loopback host, without query or fragment.
    pub issuer: String,
    /// The algorithms its assertions may use, at least one.
    pub algorithms: Vec<SigningAlgorithm>,
    /// For a multi-tenant issuer, the `tenant` claim its assertions must carry.
    pub tenant: Option<String>,
}

impl TrustedIssuer {
    /// Trusts `issuer` with RS256 and ES256 and no tenant.
    #[must_use]
    pub fn new(issuer: impl Into<String>) -> Self {
        Self {
            issuer: issuer.into(),
            algorithms: vec![SigningAlgorithm::Rs256, SigningAlgorithm::Es256],
            tenant: None,
        }
    }
}

/// Supplies trusted issuers' keys and decides which enterprise users get tokens.
///
/// The library never opens a network connection; the consumer fetches each
/// issuer's JSON Web Key Set from the `jwks_uri` in the issuer's metadata and
/// caches it. The library prepares each distinct key set once.
pub trait IdentityAssertions: Send + Sync {
    /// Returns `issuer`'s JSON Web Key Set, at most [`MAX_KEY_SET_BYTES`].
    /// `refresh` is `true` when the set last returned has no key for the
    /// assertion, as after the issuer rotates its keys; fetch it again,
    /// limiting how often an unknown key can trigger a fetch.
    ///
    /// # Errors
    /// Returns a short reason when the keys cannot be fetched; the client
    /// gets `503`.
    fn key_set(&self, issuer: &str, refresh: bool) -> Result<Vec<u8>, String>;

    /// Decides whether the verified enterprise user may have a token. Identify
    /// users by `issuer` and `subject`; use `email` only for an issuer that is
    /// authoritative for those addresses.
    fn admit(&self, assertion: &IdentityAssertion<'_>) -> AssertionDecision;
}

/// A verified identity assertion.
#[derive(Debug)]
pub struct IdentityAssertion<'a> {
    /// The trusted issuer.
    pub issuer: &'a str,
    /// The user's identifier at the issuer (`sub`).
    pub subject: &'a str,
    /// The `tenant` claim, when present.
    pub tenant: Option<&'a str>,
    /// The `email` claim, when present.
    pub email: Option<&'a str>,
    /// The authenticated client the assertion names.
    pub client_id: &'a str,
    /// The scopes the assertion requests that the client may have.
    pub scopes: &'a [String],
    /// Every claim, for consumer policy.
    pub claims: &'a Map<String, Value>,
}

/// The consumer's answer to a verified identity assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertionDecision {
    /// Issue an access token for `subject` with `scopes`, which may narrow
    /// [`IdentityAssertion::scopes`] but never add to it.
    Approve {
        /// The consumer's opaque user identifier.
        subject: String,
        /// The granted scopes.
        scopes: Vec<String>,
    },
    /// The user may not use this client; it receives `access_denied`.
    Deny,
}

/// Each issuer's last key set digest and its prepared keys.
type KeyCache = HashMap<String, (Digest, Arc<[Key]>)>;

/// The trusted issuers, the consumer's hook and each issuer's prepared keys.
pub(super) struct Assertions {
    issuers: Vec<TrustedIssuer>,
    handler: Arc<dyn IdentityAssertions>,
    keys: Mutex<KeyCache>,
    /// Key sets prepared, for the work guard in tests.
    #[cfg(test)]
    pub(super) prepared: std::sync::atomic::AtomicUsize,
}

/// Why an assertion was not accepted.
enum Failure {
    Invalid(&'static str),
    Unavailable,
}

/// The checked claims of a verified assertion.
struct Verified {
    issuer: String,
    subject: String,
    tenant: Option<String>,
    email: Option<String>,
    id: String,
    expires_at: SystemTime,
    scopes: Option<Vec<String>>,
    claims: Map<String, Value>,
}

/// Validates trusted issuers for the authorization server `own`.
pub(super) fn validate(
    issuers: &[TrustedIssuer],
    own: &str,
) -> Result<(), AuthorizationServerError> {
    if issuers.is_empty() || issuers.len() > MAX_TRUSTED_ISSUERS {
        return Err(AuthorizationServerError::TrustedIssuer(
            issuers.len().min(MAX_TRUSTED_ISSUERS),
        ));
    }
    for (index, trusted) in issuers.iter().enumerate() {
        let algorithms = &trusted.algorithms;
        let valid = auth::Location::parse(&trusted.issuer).is_some()
            && trusted.issuer != own
            && !issuers[..index]
                .iter()
                .any(|other| other.issuer == trusted.issuer)
            && !algorithms.is_empty()
            && algorithms
                .iter()
                .enumerate()
                .all(|(at, algorithm)| !algorithms[..at].contains(algorithm))
            && trusted.tenant.as_deref().is_none_or(identifier);
        if !valid {
            return Err(AuthorizationServerError::TrustedIssuer(index));
        }
    }
    Ok(())
}

/// A nonempty bounded identifier without control characters.
pub(super) fn identifier(text: &str) -> bool {
    !text.is_empty() && text.len() <= MAX_ASSERTION_ID_BYTES && !text.chars().any(char::is_control)
}

impl Assertions {
    pub(super) fn new(issuers: Vec<TrustedIssuer>, handler: Arc<dyn IdentityAssertions>) -> Self {
        Self {
            issuers,
            handler,
            keys: Mutex::default(),
            #[cfg(test)]
            prepared: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Verifies `token`'s signature and claims for this server and `client_id`.
    fn verify(
        &self,
        token: &str,
        server: &AuthorizationServer,
        client_id: &str,
        now: SystemTime,
    ) -> Result<Verified, Failure> {
        let compact = jose::parse(token, MAX_ASSERTION_BYTES)
            .ok_or(Failure::Invalid("The assertion is not a signed JWT"))?;
        let issuer = compact
            .claims
            .get("iss")
            .and_then(Value::as_str)
            .and_then(|iss| self.issuers.iter().find(|trusted| trusted.issuer == iss))
            .ok_or(Failure::Invalid("The assertion's issuer is not trusted"))?;
        let header = &compact.header;
        let typed = header
            .get("typ")
            .and_then(Value::as_str)
            .is_some_and(|typ| {
                typ.eq_ignore_ascii_case(ID_JAG_TYPE)
                    || typ
                        .strip_prefix("application/")
                        .is_some_and(|typ| typ.eq_ignore_ascii_case(ID_JAG_TYPE))
            });
        if !typed {
            return Err(Failure::Invalid(
                "The assertion's typ must be oauth-id-jag+jwt",
            ));
        }
        if header.contains_key("crit") {
            return Err(Failure::Invalid(
                "The assertion has critical header parameters",
            ));
        }
        let algorithm = header
            .get("alg")
            .and_then(Value::as_str)
            .and_then(SigningAlgorithm::parse)
            .filter(|algorithm| issuer.algorithms.contains(algorithm))
            .ok_or(Failure::Invalid(
                "The assertion's algorithm is not allowed for its issuer",
            ))?;
        let key_id = match header.get("kid") {
            None => None,
            Some(Value::String(id)) => Some(id.as_str()),
            Some(_) => return Err(Failure::Invalid("The assertion's kid is not a string")),
        };
        self.check_signature(&issuer.issuer, algorithm, key_id, &compact)?;
        claims(compact.claims, issuer, server, client_id, now)
    }

    /// Verifies the signature with the issuer's keys, fetching the set again
    /// once when it has no candidate key.
    fn check_signature(
        &self,
        issuer: &str,
        algorithm: SigningAlgorithm,
        key_id: Option<&str>,
        compact: &Compact<'_>,
    ) -> Result<(), Failure> {
        for refresh in [false, true] {
            let keys = self.keys(issuer, refresh)?;
            match compact.verify_with(&keys, algorithm, key_id) {
                Some(true) => return Ok(()),
                Some(false) => {
                    return Err(Failure::Invalid("The assertion's signature is not valid"));
                }
                None => {}
            }
        }
        Err(Failure::Invalid(
            "No key of the issuer matches the assertion",
        ))
    }

    /// The issuer's prepared keys, preparing a key set only when its bytes change.
    fn keys(&self, issuer: &str, refresh: bool) -> Result<Arc<[Key]>, Failure> {
        let bytes = self
            .handler
            .key_set(issuer, refresh)
            .map_err(|_| Failure::Unavailable)?;
        let digest = wire::digest_bytes(&bytes);
        let lock = || self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((cached, keys)) = lock().get(issuer)
            && cached.matches(&digest)
        {
            return Ok(Arc::clone(keys));
        }
        // Prepared outside the lock; other issuers' requests do not wait on it.
        let keys: Arc<[Key]> = jose::key_set(&bytes)
            .map_err(|_| Failure::Unavailable)?
            .into();
        #[cfg(test)]
        self.prepared
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        lock().insert(issuer.to_owned(), (digest, Arc::clone(&keys)));
        Ok(keys)
    }
}

/// Checks the claims of an assertion whose signature is valid.
fn claims(
    claims: Map<String, Value>,
    issuer: &TrustedIssuer,
    server: &AuthorizationServer,
    client_id: &str,
    now: SystemTime,
) -> Result<Verified, Failure> {
    let invalid = |description| Err(Failure::Invalid(description));
    let text = |name: &str| claims.get(name).and_then(Value::as_str);
    let subject = text("sub")
        .filter(|subject| {
            !subject.is_empty()
                && subject.len() <= auth::MAX_GRANT_IDENTITY_BYTES
                && !subject.chars().any(char::is_control)
        })
        .ok_or(Failure::Invalid("The assertion needs a sub"))?;
    if claims.get("aud").and_then(jose::single_audience) != Some(server.issuer()) {
        return invalid("The assertion is not for this authorization server");
    }
    if text("client_id") != Some(client_id) {
        return invalid("The assertion is for another client");
    }
    let resource = match claims.get("resource") {
        None => true,
        Some(Value::String(resource)) => server.is_resource(resource),
        Some(Value::Array(resources)) => {
            !resources.is_empty()
                && resources
                    .iter()
                    .all(|resource| resource.as_str().is_some_and(|r| server.is_resource(r)))
        }
        Some(_) => false,
    };
    if !resource {
        return invalid("The assertion is for another resource");
    }
    let (Some(expires_at), Some(issued_at)) = (time(claims.get("exp")), time(claims.get("iat")))
    else {
        return invalid("The assertion needs exp and iat");
    };
    if expires_at + ASSERTION_CLOCK_SKEW <= now {
        return invalid("The assertion has expired");
    }
    if expires_at > now + MAX_ASSERTION_LIFETIME {
        return invalid("The assertion lives too long");
    }
    let not_before = match claims.get("nbf") {
        None => Some(issued_at),
        Some(value) => time(Some(value)).map(|nbf| nbf.max(issued_at)),
    };
    if not_before.is_none_or(|from| from > now + ASSERTION_CLOCK_SKEW) {
        return invalid("The assertion is not valid yet");
    }
    let Some(id) = text("jti").filter(|id| identifier(id)) else {
        return invalid("The assertion needs a jti");
    };
    let tenant = match claims.get("tenant") {
        None => None,
        Some(Value::String(tenant)) if identifier(tenant) => Some(tenant.clone()),
        Some(_) => return invalid("The assertion's tenant is invalid"),
    };
    if issuer
        .tenant
        .as_ref()
        .is_some_and(|expected| tenant.as_ref() != Some(expected))
    {
        return invalid("The assertion is for another tenant");
    }
    if claims.contains_key("cnf") {
        return invalid("Sender-constrained assertions are not supported");
    }
    let scopes = match claims.get("scope") {
        None => None,
        Some(Value::String(scope)) => {
            let scopes = scope.split(' ').map(str::to_owned).collect::<Vec<_>>();
            if scopes.len() > MAX_REQUESTED_SCOPES
                || !scopes.iter().all(|scope| auth::valid_scope(scope))
            {
                return invalid("The assertion's scope is invalid");
            }
            Some(scopes)
        }
        Some(_) => return invalid("The assertion's scope is invalid"),
    };
    let email = text("email")
        .filter(|email| email.len() <= MAX_EMAIL_BYTES && !email.chars().any(char::is_control))
        .map(str::to_owned);
    Ok(Verified {
        issuer: issuer.issuer.clone(),
        subject: subject.to_owned(),
        tenant,
        email,
        id: id.to_owned(),
        // Replay stays refused for as long as the assertion could be accepted.
        expires_at: expires_at + ASSERTION_CLOCK_SKEW,
        scopes,
        claims,
    })
}

impl AuthorizationServer {
    /// The JWT bearer grant: an access token, without a refresh token, for an
    /// enterprise user an identity assertion names.
    pub(super) fn assertion_grant(
        &self,
        client: &Client,
        form: &HashMap<String, String>,
        now: SystemTime,
    ) -> Response {
        let Some(assertions) = &self.assertions else {
            return super::token::unsupported_grant();
        };
        if !client.confidential() {
            // Neither client authentication nor a sender-constrained assertion.
            return wire::oauth_error(401, "invalid_client", "Client authentication is required");
        }
        if !client.assertions {
            return wire::oauth_error(
                400,
                "unauthorized_client",
                "This client may not present identity assertions",
            );
        }
        let Some(token) = form.get("assertion") else {
            return invalid_request("assertion is required");
        };
        let verified = match assertions.verify(token, self, &client.id, now) {
            Ok(verified) => verified,
            Err(Failure::Invalid(description)) => {
                return wire::oauth_error(400, "invalid_grant", description);
            }
            Err(Failure::Unavailable) => return unavailable(),
        };
        let replay = AssertionRecord {
            digest: wire::digest(&format!("ja_{} {}", verified.issuer, verified.id)),
            expires_at: verified.expires_at,
        };
        // A used assertion never reaches the consumer's hook; the atomic
        // check at creation still refuses a concurrent replay.
        match self.store.assertion_redeemed(&replay.digest, now) {
            Ok(false) => {}
            Ok(true) => {
                return wire::oauth_error(400, "invalid_grant", "The assertion was already used");
            }
            Err(_) => return unavailable(),
        }
        let requested = verified
            .scopes
            .clone()
            .unwrap_or_else(|| self.default_scopes.clone());
        let requested = match form.get("scope") {
            Some(scope) => {
                let narrowed = scope.split(' ').map(str::to_owned).collect::<Vec<_>>();
                if !narrowed.iter().all(|scope| requested.contains(scope)) {
                    return wire::oauth_error(
                        400,
                        "invalid_scope",
                        "Scopes may only narrow the assertion's",
                    );
                }
                narrowed
            }
            None => requested,
        };
        let allowed = self.allowed_scopes(client);
        let offered = requested
            .into_iter()
            .filter(|scope| allowed.contains(scope))
            .collect::<Vec<_>>();
        let decision = assertions.handler.admit(&IdentityAssertion {
            issuer: &verified.issuer,
            subject: &verified.subject,
            tenant: verified.tenant.as_deref(),
            email: verified.email.as_deref(),
            client_id: &client.id,
            scopes: &offered,
            claims: &verified.claims,
        });
        let AssertionDecision::Approve { subject, scopes } = decision else {
            return wire::oauth_error(
                400,
                "access_denied",
                "The enterprise user may not use this client",
            );
        };
        let narrowed = scopes.iter().all(|scope| offered.contains(scope));
        let subject_fits = !subject.is_empty() && subject.len() <= auth::MAX_GRANT_IDENTITY_BYTES;
        if !narrowed || !subject_fits {
            return wire::oauth_error(500, "server_error", "The grant could not be issued");
        }
        let Ok((access, family, access_record)) = self.single_grant(client, subject, &scopes, now)
        else {
            return unavailable();
        };
        match self.store.create_assertion_grant(
            replay,
            family,
            access_record,
            self.client_grants,
            now,
        ) {
            Ok(true) => access_only(&access, &scopes, self.lifetimes.access),
            Ok(false) => wire::oauth_error(400, "invalid_grant", "The assertion was already used"),
            Err(_) => unavailable(),
        }
    }
}
