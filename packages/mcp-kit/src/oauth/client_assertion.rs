//! RFC 7523 client authentication for the token and revocation endpoints.

use super::{
    ASSERTION_CLOCK_SKEW, AuthorizationServer, ClientAuth, MAX_ASSERTION_BYTES,
    MAX_ASSERTION_LIFETIME, SigningAlgorithm,
    assertion::identifier,
    client::{Client, ClientKeys},
    jose::{self, Compact, Key},
    store::AssertionRecord,
    wire,
};
use serde_json::Value;
use std::{collections::HashMap, time::SystemTime};

/// The form value for `client_assertion_type` when authenticating with a JWT.
pub const JWT_CLIENT_ASSERTION_TYPE: &str =
    "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

pub(super) enum Failure {
    Invalid,
    Unavailable,
}

struct Assertion<'a> {
    compact: Compact<'a>,
    algorithm: SigningAlgorithm,
    client_id: String,
    replay: AssertionRecord,
}

impl AuthorizationServer {
    pub(super) fn authenticate_assertion(
        &self,
        form: &HashMap<String, String>,
        now: SystemTime,
    ) -> Result<Client, Failure> {
        if form.get("client_assertion_type").map(String::as_str) != Some(JWT_CLIENT_ASSERTION_TYPE)
            || form.contains_key("client_secret")
        {
            return Err(Failure::Invalid);
        }
        let assertion = form
            .get("client_assertion")
            .and_then(|token| Assertion::parse(token, self.issuer(), now))
            .ok_or(Failure::Invalid)?;
        if form
            .get("client_id")
            .is_some_and(|id| *id != assertion.client_id)
        {
            return Err(Failure::Invalid);
        }
        let client = self
            .client(&assertion.client_id)
            .map_err(|_| Failure::Invalid)?;
        self.verify_client_signature(&client, &assertion)?;
        // Resolving a document or a remote key set can outlast the assertion.
        let now = self.now();
        if assertion.replay.expires_at <= now {
            return Err(Failure::Invalid);
        }
        match self
            .store
            .use_client_assertion(assertion.replay, client.assertion_pool, now)
        {
            Ok(true) => Ok(client),
            Ok(false) => Err(Failure::Invalid),
            Err(_) => Err(Failure::Unavailable),
        }
    }

    fn verify_client_signature(
        &self,
        client: &Client,
        assertion: &Assertion<'_>,
    ) -> Result<(), Failure> {
        let signed = match (client.auth, &client.keys) {
            (ClientAuth::PrivateKeyJwt, Some(ClientKeys::Inline(keys))) => {
                assertion.signed_by(keys)
            }
            (ClientAuth::PrivateKeyJwt, Some(ClientKeys::Remote(uri))) => {
                let resolver = self.resolver.as_ref().ok_or(Failure::Unavailable)?;
                let bytes = resolver
                    .resolve_jwks(&client.id, uri)
                    .map_err(|_| Failure::Unavailable)?;
                let keys = jose::key_set(&bytes).map_err(|_| Failure::Invalid)?;
                assertion.signed_by(&keys)
            }
            _ => return Err(Failure::Invalid),
        };
        signed.then_some(()).ok_or(Failure::Invalid)
    }
}

impl<'a> Assertion<'a> {
    fn parse(token: &'a str, issuer: &str, now: SystemTime) -> Option<Self> {
        let compact = jose::parse(token, MAX_ASSERTION_BYTES)?;
        let header = &compact.header;
        if header.contains_key("crit")
            || header.contains_key("b64")
            || header.get("kid").is_some_and(|kid| !kid.is_string())
            || header.get("typ").is_some_and(|typ| {
                !typ.as_str().is_some_and(|typ| {
                    typ.eq_ignore_ascii_case("JWT") || typ.eq_ignore_ascii_case("application/jwt")
                })
            })
        {
            return None;
        }
        let algorithm = SigningAlgorithm::parse(header.get("alg")?.as_str()?)?;
        let claims = &compact.claims;
        let client_id = claims.get("sub")?.as_str()?;
        let audience = jose::single_audience(claims.get("aud")?)?;
        if claims.get("iss")?.as_str()? != client_id
            || (audience != issuer
                && audience.strip_prefix(issuer.trim_end_matches('/')) != Some("/token"))
        {
            return None;
        }
        let id = claims.get("jti")?.as_str().filter(|id| identifier(id))?;
        let expires_at = assertion_expiry(claims, now)?;
        Some(Self {
            client_id: client_id.to_owned(),
            replay: AssertionRecord {
                digest: wire::digest(&format!("ca_{client_id} {id}")),
                expires_at,
            },
            compact,
            algorithm,
        })
    }

    fn signed_by(&self, keys: &[Key]) -> bool {
        let key_id = self.compact.header.get("kid").and_then(Value::as_str);
        self.compact.verify_with(keys, self.algorithm, key_id) == Some(true)
    }
}

/// Returns the replay expiry, including the clock skew allowed at redemption.
fn assertion_expiry(
    claims: &serde_json::Map<String, Value>,
    now: SystemTime,
) -> Option<SystemTime> {
    let expires_at = jose::time(claims.get("exp"))?;
    let replay_until = expires_at.checked_add(ASSERTION_CLOCK_SKEW)?;
    if replay_until <= now || expires_at > now.checked_add(MAX_ASSERTION_LIFETIME)? {
        return None;
    }
    let latest = now.checked_add(ASSERTION_CLOCK_SKEW)?.min(expires_at);
    for name in ["iat", "nbf"] {
        if let Some(value) = claims.get(name) {
            let time = jose::time(Some(value))?;
            if time > latest
                || (name == "iat" && expires_at.duration_since(time).ok()? > MAX_ASSERTION_LIFETIME)
            {
                return None;
            }
        }
    }
    Some(replay_until)
}
