//! Authorization requests, pending authorizations and the consumer's sign-in
//! and consent pages.

use super::{
    AuthorizationServer, InteractionOutcome, InteractionRequest, MAX_INTERACTION_RESPONSE_BYTES,
    MAX_REQUESTED_SCOPES, MAX_STATE_BYTES, PendingAuthorization, client,
    store::{CodeRecord, PendingRecord},
    wire,
};
use crate::http::{RequestHead, Response, auth};
use std::{borrow::Cow, collections::HashMap, time::SystemTime};

/// The prefix of browser-binding cookie names; the reference follows it.
const SECURE_COOKIE: &str = "__Host-mcp-pending-";
/// The cookie prefix on a loopback `http` issuer, where `__Host-` and
/// `Secure` depend on the browser.
const LOOPBACK_COOKIE: &str = "mcp-pending-";

impl AuthorizationServer {
    pub(super) fn authorize(&self, query: &str) -> Response {
        if query.len() > super::MAX_FORM_BYTES {
            return wire::page(400, "The authorization request is too large.");
        }
        let Some(params) = wire::form(query) else {
            return wire::page(400, "The authorization request is malformed.");
        };
        let param = |name: &str| params.get(name).map(String::as_str);
        let Some(client) = param("client_id").and_then(|id| self.client(id).ok()) else {
            return wire::page(400, "The client is not known to this server.");
        };
        let Some(redirect_uri) = param("redirect_uri").filter(|uri| client.redirects_to(uri))
        else {
            return wire::page(400, "The redirect URI is not registered for this client.");
        };
        let state = param("state");
        let fail = |error: &str| {
            let mut pairs = vec![("error", error), ("iss", self.issuer.as_str())];
            if let Some(state) = state {
                pairs.push(("state", state));
            }
            wire::redirect(wire::with_query(redirect_uri, &pairs))
        };
        if param("response_type") != Some("code") {
            return fail("unsupported_response_type");
        }
        let challenge = param("code_challenge")
            .filter(|challenge| challenge.len() == 43 && wire::unreserved(challenge));
        let Some(challenge) = challenge.filter(|_| param("code_challenge_method") == Some("S256"))
        else {
            return fail("invalid_request");
        };
        if param("resource").is_some_and(|resource| !self.is_resource(resource)) {
            return fail("invalid_target");
        }
        let allowed = self.allowed_scopes(&client);
        let scopes = match param("scope") {
            Some(scope) => {
                let scopes = scope.split(' ').map(str::to_owned).collect::<Vec<_>>();
                let valid = scopes.len() <= MAX_REQUESTED_SCOPES
                    && scopes
                        .iter()
                        .all(|scope| auth::valid_scope(scope) && allowed.contains(scope));
                if !valid {
                    return fail("invalid_scope");
                }
                scopes
            }
            None => self
                .default_scopes
                .iter()
                .filter(|scope| allowed.contains(scope))
                .cloned()
                .collect(),
        };
        if state.is_some_and(|state| state.len() > MAX_STATE_BYTES) {
            return fail("invalid_request");
        }
        let (Ok(reference), Ok(binding)) = (wire::secret("pr_"), wire::secret("pb_")) else {
            return fail("server_error");
        };
        let now = SystemTime::now();
        let record = PendingRecord {
            reference: reference.clone(),
            binding: wire::digest(&binding),
            client_id: client.id,
            client_name: client.name,
            redirect_uri: redirect_uri.to_owned(),
            challenge: challenge.to_owned(),
            scopes,
            resource: self.resource.clone(),
            state: state.map(str::to_owned),
            expires_at: now + self.lifetimes.pending,
        };
        if self.store.put_pending(record).is_err() {
            return fail("temporarily_unavailable");
        }
        let mut response = wire::redirect(wire::with_query(
            &self.interaction_path,
            &[("pending", &reference)],
        ));
        response.headers.push((
            "Set-Cookie",
            Cow::Owned(self.binding_cookie(&reference, &binding, self.lifetimes.pending.as_secs())),
        ));
        response
    }

    pub(super) fn interact(
        &self,
        head: &RequestHead<'_>,
        path: &str,
        query: &str,
        body: &[u8],
    ) -> Response {
        if body.len() > super::MAX_INTERACTION_BODY_BYTES {
            return wire::page(413, "The request is too large.");
        }
        let form = is_form(head.headers)
            .then(|| std::str::from_utf8(body).ok().and_then(wire::form))
            .flatten();
        let reference = wire::form(query)
            .and_then(|mut query| query.remove("pending"))
            .or_else(|| form.as_ref().and_then(|form| form.get("pending").cloned()));
        let now = SystemTime::now();
        let pending = reference.as_deref().and_then(|reference| {
            let record = self.store.get_pending(reference, now).ok()??;
            let cookie = wire::cookie(head.headers, &self.cookie_name(reference))?;
            record
                .binding
                .matches(&wire::digest(cookie))
                .then_some(record)
        });
        let details = pending.as_ref().map(|record| {
            let (redirect_host, localhost_redirect) = client::redirect_host(&record.redirect_uri);
            PendingAuthorization {
                reference: record.reference.clone(),
                client_id: record.client_id.clone(),
                client_name: record.client_name.clone(),
                redirect_host,
                localhost_redirect,
                scopes: record.scopes.clone(),
                resource: record.resource.clone(),
            }
        });
        let headers = wire::without_cookies(head.headers, |name| {
            name.starts_with(SECURE_COOKIE) || name.starts_with(LOOPBACK_COOKIE)
        });
        let outcome = self.interaction.interact(&InteractionRequest {
            method: head.method,
            path,
            query,
            headers: &headers,
            body,
            pending: details.as_ref(),
        });
        let (approval, record) = match (outcome, pending) {
            (InteractionOutcome::Respond(response), _) => {
                return if response.body.len() > MAX_INTERACTION_RESPONSE_BYTES {
                    wire::page(500, "The sign-in page is too large.")
                } else {
                    response
                };
            }
            (outcome, Some(record)) if head.method == "POST" => (outcome, record),
            _ => {
                return wire::page(
                    400,
                    "This sign-in cannot be completed. Start again from your application.",
                );
            }
        };
        // Consumed once: a second completion finds nothing.
        let Ok(Some(record)) = self.store.take_pending(&record.reference, now) else {
            return wire::page(400, "This sign-in was already completed or has expired.");
        };
        let mut response = self.complete(&record, approval, now);
        response.headers.push((
            "Set-Cookie",
            Cow::Owned(self.binding_cookie(&record.reference, "", 0)),
        ));
        response
    }

    /// Redirects to the client with a code, or with the refusal.
    fn complete(
        &self,
        record: &PendingRecord,
        approval: InteractionOutcome,
        now: SystemTime,
    ) -> Response {
        let mut pairs: Vec<(&str, String)> = Vec::new();
        let finish = |mut pairs: Vec<(&str, String)>| {
            pairs.push(("iss", self.issuer.clone()));
            if let Some(state) = &record.state {
                pairs.push(("state", state.clone()));
            }
            let pairs = pairs
                .iter()
                .map(|(name, value)| (*name, value.as_str()))
                .collect::<Vec<_>>();
            wire::redirect(wire::with_query(&record.redirect_uri, &pairs))
        };
        let InteractionOutcome::Approve { subject, scopes } = approval else {
            pairs.push(("error", "access_denied".to_owned()));
            return finish(pairs);
        };
        let narrowed = scopes.iter().all(|scope| record.scopes.contains(scope));
        let subject_fits = !subject.is_empty() && subject.len() <= auth::MAX_GRANT_IDENTITY_BYTES;
        if !narrowed || !subject_fits {
            pairs.push(("error", "server_error".to_owned()));
            return finish(pairs);
        }
        let (Ok(code), Ok(family_id)) = (wire::secret("ac_"), wire::identifier()) else {
            pairs.push(("error", "server_error".to_owned()));
            return finish(pairs);
        };
        let stored = self.store.put_code(CodeRecord {
            digest: wire::digest(&code),
            family_id,
            client_id: record.client_id.clone(),
            redirect_uri: record.redirect_uri.clone(),
            challenge: record.challenge.clone(),
            scopes,
            resource: record.resource.clone(),
            subject,
            expires_at: now + self.lifetimes.code,
            consumed: false,
        });
        if stored.is_err() {
            pairs.push(("error", "temporarily_unavailable".to_owned()));
            return finish(pairs);
        }
        pairs.push(("code", code));
        finish(pairs)
    }

    fn cookie_name(&self, reference: &str) -> String {
        let prefix = if self.secure_cookies {
            SECURE_COOKIE
        } else {
            LOOPBACK_COOKIE
        };
        format!("{prefix}{reference}")
    }

    fn binding_cookie(&self, reference: &str, value: &str, max_age: u64) -> String {
        let secure = if self.secure_cookies { "; Secure" } else { "" };
        format!(
            "{}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}",
            self.cookie_name(reference)
        )
    }

    /// Whether `resource` names this server's canonical resource.
    pub(super) fn is_resource(&self, resource: &str) -> bool {
        auth::Location::parse(resource).is_some_and(|location| location.uri == self.resource)
    }
}

fn is_form(headers: &[(&str, &[u8])]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-type")
            && std::str::from_utf8(value).is_ok_and(|value| {
                value.split(';').next().is_some_and(|media| {
                    media
                        .trim()
                        .eq_ignore_ascii_case("application/x-www-form-urlencoded")
                })
            })
    })
}

/// The form fields of a token, revocation or interaction request body.
pub(super) fn form_body(headers: &[(&str, &[u8])], body: &[u8]) -> Option<HashMap<String, String>> {
    is_form(headers)
        .then(|| std::str::from_utf8(body).ok().and_then(wire::form))
        .flatten()
}
