//! The token, revocation and registration endpoints and client authentication.

use super::{
    AuthorizationServer, ClientAuth, RegisteredClient, client, expiry,
    flow::form_body,
    store::{AccessRecord, CodeTake, FamilyRecord, Rotated, Rotation},
    wire,
};
use crate::{content::decode_base64, http::Response};
use serde_json::json;
use std::{collections::HashMap, time::SystemTime};

/// Fresh tokens for a family generation.
struct Issued {
    access: String,
    access_record: AccessRecord,
    refresh: String,
    refresh_digest: super::Digest,
    refresh_expires_at: SystemTime,
}

impl AuthorizationServer {
    pub(super) fn token(&self, headers: &[(&str, &[u8])], body: &[u8]) -> Response {
        let now = SystemTime::now();
        // Retention follows lifetimes, not traffic; a failed purge waits for the next request.
        let _ = self.store.purge_expired(now, super::PURGE_PER_REQUEST);
        let Some(form) = form_body(headers, body) else {
            return invalid_request("The request must be a form without repeated fields");
        };
        let client = match self.authenticate(headers, &form) {
            Ok(client) => client,
            Err(response) => return *response,
        };
        if form
            .get("resource")
            .is_some_and(|resource| !self.is_resource(resource))
        {
            return wire::oauth_error(400, "invalid_target", "The token is only for this server");
        }
        match form.get("grant_type").map(String::as_str) {
            Some("authorization_code") => self.exchange_code(&client, &form, now),
            Some("refresh_token") => self.refresh(&client, &form, now),
            _ => wire::oauth_error(
                400,
                "unsupported_grant_type",
                "Use authorization_code or refresh_token",
            ),
        }
    }

    fn exchange_code(
        &self,
        client: &client::Client,
        form: &HashMap<String, String>,
        now: SystemTime,
    ) -> Response {
        let invalid = |description| wire::oauth_error(400, "invalid_grant", description);
        let (Some(code), Some(redirect_uri), Some(verifier)) = (
            form.get("code"),
            form.get("redirect_uri"),
            form.get("code_verifier"),
        ) else {
            return invalid_request("code, redirect_uri and code_verifier are required");
        };
        let code = wire::digest(code);
        let record = match self.store.take_code(&code, now) {
            Ok(CodeTake::Taken(record)) => record,
            Ok(CodeTake::Reused(family)) => {
                // A second redemption means the code leaked: end what it produced.
                let _ = self.store.revoke_family(&family);
                return invalid("The authorization code was already used");
            }
            Ok(CodeTake::Unknown) => return invalid("The authorization code is not valid"),
            Err(_) => return unavailable(),
        };
        let pkce = (43..=128).contains(&verifier.len())
            && wire::unreserved(verifier)
            && wire::challenge(verifier) == record.challenge;
        if record.client_id != client.id || record.redirect_uri != *redirect_uri || !pkce {
            return invalid("The authorization code does not match this request");
        }
        let family_expires = now + self.lifetimes.family;
        let Ok(family_secret) = wire::secret("") else {
            return unavailable();
        };
        let Ok(issued) = self.issue(
            &record.family_id,
            1,
            &family_secret,
            &record.scopes,
            now,
            family_expires,
        ) else {
            return unavailable();
        };
        let family = FamilyRecord {
            id: record.family_id,
            subject: record.subject,
            client_id: record.client_id,
            scopes: record.scopes,
            resource: record.resource,
            created_at: now,
            expires_at: family_expires,
            proof: proof(&family_secret),
            generation: 1,
            refresh: issued.refresh_digest,
            refresh_expires_at: issued.refresh_expires_at,
            previous: None,
            access: issued.access_record.digest,
        };
        let scopes = issued.access_record.scopes.clone();
        match self
            .store
            .create_family(&code, family, issued.access_record.clone())
        {
            Ok(true) => tokens(&issued, &scopes, now),
            // A replay between redemption and creation revoked this family.
            Ok(false) => invalid("The authorization code was already used"),
            Err(_) => unavailable(),
        }
    }

    fn refresh(
        &self,
        client: &client::Client,
        form: &HashMap<String, String>,
        now: SystemTime,
    ) -> Response {
        let invalid = || wire::oauth_error(400, "invalid_grant", "The refresh token is not valid");
        let Some(token) = form.get("refresh_token") else {
            return invalid_request("refresh_token is required");
        };
        let Some((family_id, generation, family_secret)) = parse_refresh(token) else {
            return invalid();
        };
        let family = match self.store.get_family(family_id, now) {
            Ok(Some(family)) if family.client_id == client.id => family,
            Ok(_) => return invalid(),
            Err(_) => return unavailable(),
        };
        // A generation the family has not reached was never issued.
        let Some(next) = family
            .generation
            .checked_add(1)
            .filter(|_| generation <= family.generation)
        else {
            return invalid();
        };
        let scopes = match form.get("scope") {
            Some(scope) => {
                let scopes = scope.split(' ').map(str::to_owned).collect::<Vec<_>>();
                if !scopes.iter().all(|scope| family.scopes.contains(scope)) {
                    return wire::oauth_error(400, "invalid_scope", "Scopes may only narrow");
                }
                scopes
            }
            None => family.scopes.clone(),
        };
        // The new token carries the presented family secret; it commits only
        // when the presented token checks out, so a forged secret never does.
        let Ok(issued) = self.issue(
            family_id,
            next,
            family_secret,
            &scopes,
            now,
            family.expires_at,
        ) else {
            return unavailable();
        };
        let rotation = Rotation {
            family_id: family_id.to_owned(),
            generation,
            expected: family.generation,
            presented: wire::digest(token),
            proof: proof(family_secret),
            refresh: issued.refresh_digest,
            refresh_expires_at: issued.refresh_expires_at,
            access: issued.access_record.clone(),
            grace: self.lifetimes.refresh_grace,
        };
        match self.store.rotate(rotation, now) {
            Ok(Rotated::Rotated(_)) => tokens(&issued, &scopes, now),
            Ok(Rotated::Reused) => {
                let _ = self.store.revoke_family(family_id);
                invalid()
            }
            Ok(Rotated::Unknown) => invalid(),
            // A concurrent refresh won; the family and its tokens are unchanged.
            Ok(Rotated::Stale) | Err(_) => unavailable(),
        }
    }

    /// Generates an access token and a refresh token for `generation`.
    fn issue(
        &self,
        family_id: &str,
        generation: u64,
        family_secret: &str,
        scopes: &[String],
        now: SystemTime,
        family_expires: SystemTime,
    ) -> Result<Issued, getrandom::Error> {
        let access = wire::secret("at_")?;
        let refresh = format!(
            "rt_{family_id}.{generation}.{family_secret}.{}",
            wire::secret("")?
        );
        Ok(Issued {
            access_record: AccessRecord {
                digest: wire::digest(&access),
                family_id: family_id.to_owned(),
                scopes: scopes.to_vec(),
                expires_at: expiry(now, self.lifetimes.access, Some(family_expires)),
            },
            access,
            refresh_digest: wire::digest(&refresh),
            refresh,
            refresh_expires_at: expiry(now, self.lifetimes.refresh_idle, Some(family_expires)),
        })
    }

    pub(super) fn revoke(&self, headers: &[(&str, &[u8])], body: &[u8]) -> Response {
        let Some(form) = form_body(headers, body) else {
            return invalid_request("The request must be a form without repeated fields");
        };
        let client = match self.authenticate(headers, &form) {
            Ok(client) => client,
            Err(response) => return *response,
        };
        let Some(token) = form.get("token") else {
            return invalid_request("token is required");
        };
        let now = SystemTime::now();
        let family = if token.starts_with("at_") {
            match self.store.lookup_access(&wire::digest(token), now) {
                Ok(found) => found.map(|(_, family)| family),
                Err(_) => return unavailable(),
            }
        } else if let Some((family_id, _, _)) = parse_refresh(token) {
            let presented = wire::digest(token);
            match self.store.get_family(family_id, now) {
                Ok(family) => family.filter(|family| {
                    family.refresh.matches(&presented)
                        || family
                            .previous
                            .is_some_and(|(previous, _)| previous.matches(&presented))
                }),
                Err(_) => return unavailable(),
            }
        } else {
            None
        };
        if let Some(family) = family.filter(|family| family.client_id == client.id)
            && self.store.revoke_family(&family.id).is_err()
        {
            return unavailable();
        }
        wire::json(200, &json!({}))
    }

    pub(super) fn register(&self, headers: &[(&str, &[u8])], body: &[u8]) -> Response {
        let Some(registrar) = &self.registrar else {
            return wire::page(404, "Not found");
        };
        let json = headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("content-type")
                && std::str::from_utf8(value).is_ok_and(|value| {
                    value
                        .split(';')
                        .next()
                        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
                })
        });
        if !json {
            return invalid_request("The registration must be application/json");
        }
        let request = match client::registration(body) {
            Ok(request) => request,
            Err(reason) => return wire::oauth_error(400, "invalid_client_metadata", reason),
        };
        let confidential = request.auth != ClientAuth::None;
        let (Ok(client_id), Ok(secret)) = (wire::identifier(), wire::secret("cs_")) else {
            return unavailable();
        };
        let registered = RegisteredClient {
            client_id: format!("mcp_{client_id}"),
            client_name: request.name,
            redirect_uris: request.redirect_uris,
            auth: request.auth,
            secret: confidential.then(|| wire::digest(&format!("cs_{secret}"))),
        };
        if let Err(reason) = registrar.register(&registered) {
            return wire::oauth_error(400, "invalid_client_metadata", &reason);
        }
        let issued_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut reply = json!({
            "client_id": registered.client_id,
            "client_id_issued_at": issued_at,
            "redirect_uris": registered.redirect_uris,
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": registered.auth.name(),
        });
        if let Some(name) = &registered.client_name {
            reply["client_name"] = json!(name);
        }
        if confidential {
            reply["client_secret"] = json!(secret);
            reply["client_secret_expires_at"] = json!(0);
        }
        wire::json(201, &reply)
    }

    /// Authenticates the client by its registered method.
    fn authenticate(
        &self,
        headers: &[(&str, &[u8])],
        form: &HashMap<String, String>,
    ) -> Result<client::Client, Box<Response>> {
        let basic = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .and_then(|(_, value)| std::str::from_utf8(value).ok())
            .and_then(|value| {
                let (scheme, credentials) = value.split_once(' ')?;
                scheme
                    .eq_ignore_ascii_case("Basic")
                    .then_some(credentials.trim())
            });
        let refuse = |basic: bool| {
            let mut response =
                wire::oauth_error(401, "invalid_client", "Client authentication failed");
            if basic {
                response
                    .headers
                    .push(("WWW-Authenticate", "Basic realm=\"oauth\"".into()));
            }
            Box::new(response)
        };
        let (client_id, secret, method) = if let Some(credentials) = basic {
            // RFC 6749 §2.3.1: both parts are form-encoded before base64.
            let decoded = decode_base64(credentials, 2048)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok());
            let parts = decoded
                .as_deref()
                .and_then(|text| text.split_once(':'))
                .and_then(|(id, secret)| {
                    Some((wire::decode_component(id)?, wire::decode_component(secret)?))
                });
            let Some((id, secret)) = parts else {
                return Err(refuse(true));
            };
            (id, Some(secret), ClientAuth::SecretBasic)
        } else {
            let Some(id) = form.get("client_id") else {
                return Err(refuse(false));
            };
            match form.get("client_secret") {
                Some(secret) => (id.clone(), Some(secret.clone()), ClientAuth::SecretPost),
                None => (id.clone(), None, ClientAuth::None),
            }
        };
        if form
            .get("client_id")
            .is_some_and(|form_id| *form_id != client_id)
        {
            return Err(refuse(basic.is_some()));
        }
        let client = self
            .client(&client_id)
            .map_err(|_| refuse(basic.is_some()))?;
        let authenticated = client.auth == method
            && secret
                .as_deref()
                .is_none_or(|secret| client.secret_matches(secret));
        if !authenticated {
            return Err(refuse(
                basic.is_some() || client.auth == ClientAuth::SecretBasic,
            ));
        }
        Ok(client)
    }
}

/// The family ID, generation and family secret of an
/// `rt_{family}.{generation}.{secret}.{random}` token.
fn parse_refresh(token: &str) -> Option<(&str, u64, &str)> {
    let mut parts = token.strip_prefix("rt_")?.split('.');
    let (Some(family), Some(generation), Some(secret), Some(random), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return None;
    };
    let generation = generation
        .parse()
        .ok()
        .filter(|generation| *generation > 0)?;
    [family, secret, random]
        .iter()
        .all(|part| !part.is_empty())
        .then_some((family, generation, secret))
}

/// The digest of a family secret, as [`FamilyRecord::proof`] holds it.
fn proof(family_secret: &str) -> super::Digest {
    wire::digest(&format!("rf_{family_secret}"))
}

/// The successful token response.
fn tokens(issued: &Issued, scopes: &[String], now: SystemTime) -> Response {
    let expires_in = issued
        .access_record
        .expires_at
        .duration_since(now)
        .unwrap_or_default()
        .as_secs();
    wire::json(
        200,
        &json!({
            "access_token": issued.access,
            "token_type": "Bearer",
            "expires_in": expires_in,
            "refresh_token": issued.refresh,
            "scope": scopes.join(" "),
        }),
    )
}

fn invalid_request(description: &str) -> Response {
    wire::oauth_error(400, "invalid_request", description)
}

fn unavailable() -> Response {
    let mut response = Response::empty(503);
    response.headers.push(("Cache-Control", "no-store".into()));
    response
        .headers
        .push(("Access-Control-Allow-Origin", "*".into()));
    response
}
