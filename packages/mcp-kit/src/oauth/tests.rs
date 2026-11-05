//! The authorization server's endpoints, driven without I/O: metadata,
//! authorization requests, sign-in binding, code and refresh grants,
//! revocation, client documents, registration and the storage contract.
use super::*;
use crate::http::{RequestHead, ResourceOptions};
use std::{
    collections::HashMap,
    sync::{Barrier, Mutex},
};

const ISSUER: &str = "https://mcp.example.com";
const RESOURCE: &str = "https://mcp.example.com/mcp";
const SIGN_IN: &str = "/oauth/sign-in";
const PUBLIC: &str = "acme-desktop";
const CONFIDENTIAL: &str = "acme-backend";
const SECRET: &str = "acme-backend-secret-0123456789abcdef";
const MACHINE: &str = "acme-nightly";
const MACHINE_SECRET: &str = "acme-nightly-secret-0123456789abcdef";
const REDIRECT: &str = "http://127.0.0.1:7777/callback";
const VERIFIER: &str = "acme-verifier-0123456789-0123456789-0123456789";

/// The headers and pending details one interaction request carried.
type Seen = (Vec<(String, Vec<u8>)>, Option<PendingAuthorization>);

/// Approves, denies, narrows or widens as the posted `decision` says, and
/// records the headers and pending details it saw.
#[derive(Default)]
struct SignIn {
    seen: Mutex<Vec<Seen>>,
}

impl Interaction for SignIn {
    fn interact(&self, request: &InteractionRequest<'_>) -> InteractionOutcome {
        self.seen
            .lock()
            .unwrap()
            .push((request.headers.to_vec(), request.pending.cloned()));
        let form = wire::form(std::str::from_utf8(request.body).unwrap()).unwrap_or_default();
        let scopes = request
            .pending
            .map(|pending| pending.scopes.clone())
            .unwrap_or_default();
        match form.get("decision").map(String::as_str) {
            Some("approve") => InteractionOutcome::Approve {
                subject: "user-1".to_owned(),
                scopes,
            },
            Some("narrow") => InteractionOutcome::Approve {
                subject: "user-1".to_owned(),
                scopes: scopes.into_iter().take(1).collect(),
            },
            Some("widen") => InteractionOutcome::Approve {
                subject: "user-1".to_owned(),
                scopes: vec!["acme:admin".to_owned()],
            },
            Some("deny") => InteractionOutcome::Deny,
            _ => InteractionOutcome::Respond(Response {
                status: 200,
                headers: vec![("Content-Type", "text/html".into())],
                body: b"<form>Sign in to Acme</form>".to_vec(),
            }),
        }
    }
}

/// A [`MemoryStore`] that records every value it is given. It can hold family
/// reads at a barrier so concurrent refreshes share one snapshot, and pause
/// family creation between two barriers so a replay can run in between.
#[derive(Default)]
struct Recording {
    inner: MemoryStore,
    seen: Mutex<Vec<String>>,
    gate: Mutex<Option<Arc<Barrier>>>,
    creation: Mutex<Option<(Arc<Barrier>, Arc<Barrier>)>>,
}

impl Recording {
    fn record(&self, value: &impl std::fmt::Debug) {
        self.seen.lock().unwrap().push(format!("{value:?}"));
    }
}

impl GrantStore for Recording {
    fn put_pending(&self, record: PendingRecord) -> Result<(), StoreError> {
        self.record(&record);
        self.inner.put_pending(record)
    }
    fn get_pending(
        &self,
        reference: &str,
        now: SystemTime,
    ) -> Result<Option<PendingRecord>, StoreError> {
        self.inner.get_pending(reference, now)
    }
    fn take_pending(
        &self,
        reference: &str,
        now: SystemTime,
    ) -> Result<Option<PendingRecord>, StoreError> {
        self.inner.take_pending(reference, now)
    }
    fn put_code(&self, record: CodeRecord) -> Result<(), StoreError> {
        self.record(&record);
        self.inner.put_code(record)
    }
    fn take_code(&self, digest: &Digest, now: SystemTime) -> Result<CodeTake, StoreError> {
        self.inner.take_code(digest, now)
    }
    fn create_family(
        &self,
        code: &Digest,
        family: FamilyRecord,
        access: AccessRecord,
    ) -> Result<bool, StoreError> {
        self.record(&(&family, &access));
        let creation = self.creation.lock().unwrap().clone();
        if let Some((paused, resume)) = creation {
            paused.wait();
            resume.wait();
        }
        self.inner.create_family(code, family, access)
    }
    fn create_client_grant(
        &self,
        family: FamilyRecord,
        access: AccessRecord,
        limit: usize,
    ) -> Result<(), StoreError> {
        self.record(&(&family, &access));
        self.inner.create_client_grant(family, access, limit)
    }
    fn get_family(&self, id: &str, now: SystemTime) -> Result<Option<FamilyRecord>, StoreError> {
        let family = self.inner.get_family(id, now);
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.wait();
        }
        family
    }
    fn rotate(&self, rotation: Rotation, now: SystemTime) -> Result<Rotated, StoreError> {
        self.record(&rotation);
        self.inner.rotate(rotation, now)
    }
    fn lookup_access(
        &self,
        digest: &Digest,
        now: SystemTime,
    ) -> Result<Option<(AccessRecord, FamilyRecord)>, StoreError> {
        self.inner.lookup_access(digest, now)
    }
    fn revoke_family(&self, id: &str) -> Result<(), StoreError> {
        self.inner.revoke_family(id)
    }
    fn purge_expired(&self, now: SystemTime, limit: usize) -> Result<usize, StoreError> {
        self.inner.purge_expired(now, limit)
    }
}

/// Serves one fixed client metadata document.
struct Documents;

impl ClientResolver for Documents {
    fn resolve(&self, client_id: &str) -> Result<Vec<u8>, String> {
        let named = |id: &str| {
            serde_json::to_vec(&json!({
                "client_id": id,
                "client_name": "Acme Web",
                "redirect_uris": ["https://app.example.com/callback"],
                "token_endpoint_auth_method": "none",
            }))
            .unwrap()
        };
        match client_id {
            "https://app.example.com/client.json" => Ok(named(client_id)),
            "https://app.example.com/other.json" => {
                Ok(named("https://elsewhere.example.com/client.json"))
            }
            _ => Err("not found".to_owned()),
        }
    }
}

/// Stores registrations in memory; refuses client names starting with `Blocked`.
#[derive(Default)]
struct Registrar(Mutex<HashMap<String, RegisteredClient>>);

impl ClientRegistrar for Registrar {
    fn register(&self, client: &RegisteredClient) -> Result<(), String> {
        if client
            .client_name
            .as_deref()
            .is_some_and(|name| name.starts_with("Blocked"))
        {
            return Err("Registration is closed for this client".to_owned());
        }
        self.0
            .lock()
            .unwrap()
            .insert(client.client_id.clone(), client.clone());
        Ok(())
    }

    fn lookup(&self, client_id: &str) -> Option<RegisteredClient> {
        self.0.lock().unwrap().get(client_id).cloned()
    }
}

fn resource() -> ResourceServer {
    ResourceServer::new(ResourceOptions::new(RESOURCE, vec![ISSUER.to_owned()])).unwrap()
}

fn options() -> AuthorizationServerOptions {
    let mut options = AuthorizationServerOptions::new(ISSUER, SIGN_IN);
    options.default_scopes = vec!["acme:read".to_owned()];
    options.scopes_supported = Some(vec!["acme:read".to_owned(), "acme:write".to_owned()]);
    options.clients = vec![
        ClientOptions {
            client_id: PUBLIC.to_owned(),
            client_name: Some("Acme Desktop".to_owned()),
            redirect_uris: vec![
                REDIRECT.to_owned(),
                "http://localhost:7777/callback".to_owned(),
            ],
            scopes: vec!["acme:read".to_owned(), "acme:write".to_owned()],
            auth: ClientAuth::None,
            secret: None,
            client_credentials: false,
        },
        ClientOptions {
            client_id: CONFIDENTIAL.to_owned(),
            client_name: None,
            redirect_uris: vec!["https://backend.example.com/callback".to_owned()],
            scopes: vec!["acme:read".to_owned()],
            auth: ClientAuth::SecretBasic,
            secret: Some(SECRET.to_owned()),
            client_credentials: false,
        },
        ClientOptions {
            client_id: MACHINE.to_owned(),
            client_name: None,
            redirect_uris: Vec::new(),
            scopes: vec!["acme:read".to_owned(), "acme:write".to_owned()],
            auth: ClientAuth::SecretPost,
            secret: Some(MACHINE_SECRET.to_owned()),
            client_credentials: true,
        },
    ];
    options
}

struct Harness {
    server: AuthorizationServer,
    store: Arc<Recording>,
    sign_in: Arc<SignIn>,
    /// Every secret value a response delivered.
    issued: Mutex<Vec<String>>,
}

fn harness() -> Harness {
    harness_with(options(), MemoryStore::new())
}

fn harness_with(options: AuthorizationServerOptions, inner: MemoryStore) -> Harness {
    let store = Arc::new(Recording {
        inner,
        seen: Mutex::default(),
        gate: Mutex::default(),
        creation: Mutex::default(),
    });
    let sign_in = Arc::new(SignIn::default());
    let server = AuthorizationServer::new(
        options,
        &resource(),
        Arc::clone(&store) as Arc<dyn GrantStore>,
        Arc::clone(&sign_in) as Arc<dyn Interaction>,
    )
    .unwrap()
    .with_resolver(Arc::new(Documents))
    .with_registrar(Arc::new(Registrar::default()));
    Harness {
        server,
        store,
        sign_in,
        issued: Mutex::default(),
    }
}

fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(field, _)| field.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_ref())
}

/// The decoded query of a redirect.
fn location(response: &Response) -> (String, HashMap<String, String>) {
    assert_eq!(response.status, 303, "{response:?}");
    let location = header(response, "Location").unwrap();
    let (base, query) = location.split_once('?').unwrap();
    (base.to_owned(), wire::form(query).unwrap())
}

fn json_body(response: &Response) -> Value {
    serde_json::from_slice(&response.body).unwrap()
}

/// An authorization query for the public client.
fn authorization(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut pairs = vec![
        ("response_type", "code"),
        ("client_id", PUBLIC),
        ("redirect_uri", REDIRECT),
        ("code_challenge", ""),
        ("code_challenge_method", "S256"),
        ("state", "acme-state"),
        ("resource", RESOURCE),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value.to_owned()))
    .collect::<Vec<_>>();
    pairs[3].1 = wire::challenge(VERIFIER);
    for (name, value) in extra {
        pairs.retain(|(existing, _)| existing != name);
        if !value.is_empty() {
            pairs.push(((*name).to_owned(), (*value).to_owned()));
        }
    }
    pairs
}

impl Harness {
    fn send(&self, method: &str, target: &str, headers: &[(&str, &[u8])], body: &[u8]) -> Response {
        let response = self.server.handle(
            &RequestHead {
                method,
                target,
                headers,
            },
            body,
        );
        let mut issued = self.issued.lock().unwrap();
        if let Ok(reply) = serde_json::from_slice::<Value>(&response.body) {
            for key in ["access_token", "refresh_token", "client_secret"] {
                if let Some(value) = reply[key].as_str() {
                    issued.push(value.to_owned());
                }
            }
        }
        for (_, value) in &response.headers {
            issued.extend(
                value
                    .split(['=', '&', ';', '?'])
                    .filter(|part| part.len() > 30)
                    .map(str::to_owned),
            );
        }
        response
    }

    fn authorize(&self, pairs: &[(String, String)]) -> Response {
        let query = pairs
            .iter()
            .map(|(name, value)| format!("{name}={}", wire::encode_component(value)))
            .collect::<Vec<_>>()
            .join("&");
        self.send("GET", &format!("/authorize?{query}"), &[], b"")
    }

    /// Starts an authorization; returns the pending reference and its cookie.
    fn start(&self, pairs: &[(String, String)]) -> (String, String) {
        let response = self.authorize(pairs);
        let (base, query) = location(&response);
        assert_eq!(base, SIGN_IN);
        let cookie = header(&response, "Set-Cookie").unwrap();
        let pair = cookie.split(';').next().unwrap().to_owned();
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));
        assert!(cookie.starts_with("__Host-") && cookie.contains("; Secure"));
        (query["pending"].clone(), pair)
    }

    fn decide(&self, reference: &str, cookie: &str, decision: &str) -> Response {
        let body = format!("pending={reference}&decision={decision}");
        self.send(
            "POST",
            SIGN_IN,
            &[
                ("Content-Type", b"application/x-www-form-urlencoded"),
                ("Cookie", cookie.as_bytes()),
            ],
            body.as_bytes(),
        )
    }

    /// Signs in and returns the code.
    fn code(&self, pairs: &[(String, String)]) -> String {
        let (reference, cookie) = self.start(pairs);
        let (base, query) = location(&self.decide(&reference, &cookie, "approve"));
        let redirect = pairs
            .iter()
            .find(|(name, _)| name == "redirect_uri")
            .unwrap();
        assert_eq!(base, redirect.1);
        assert_eq!(query["iss"], ISSUER);
        assert_eq!(query["state"], "acme-state");
        query["code"].clone()
    }

    fn token(&self, fields: &[(&str, &str)], headers: &[(&str, &[u8])]) -> Response {
        let body = fields
            .iter()
            .map(|(name, value)| format!("{name}={}", wire::encode_component(value)))
            .collect::<Vec<_>>()
            .join("&");
        let mut all = vec![(
            "Content-Type",
            b"application/x-www-form-urlencoded".as_slice(),
        )];
        all.extend_from_slice(headers);
        self.send("POST", "/token", &all, body.as_bytes())
    }

    fn exchange(&self, code: &str) -> Response {
        self.token(
            &[
                ("grant_type", "authorization_code"),
                ("client_id", PUBLIC),
                ("code", code),
                ("redirect_uri", REDIRECT),
                ("code_verifier", VERIFIER),
                ("resource", RESOURCE),
            ],
            &[],
        )
    }

    fn refresh(&self, refresh: &str, scope: Option<&str>) -> Response {
        let mut fields = vec![
            ("grant_type", "refresh_token"),
            ("client_id", PUBLIC),
            ("refresh_token", refresh),
        ];
        if let Some(scope) = scope {
            fields.push(("scope", scope));
        }
        self.token(&fields, &[])
    }

    fn grant(&self, access: &str) -> Result<Grant, Rejection> {
        let header = format!("Bearer {access}");
        let fields = [("Authorization", header.as_bytes())];
        let token = resource()
            .bearer(&RequestHead {
                method: "POST",
                target: "/mcp",
                headers: &fields,
            })
            .unwrap();
        self.server.verify(token, RESOURCE)
    }
}

#[test]
fn construction_refuses_invalid_issuers_clients_paths_and_lifetimes() {
    let with = |change: &dyn Fn(&mut AuthorizationServerOptions)| {
        let mut options = options();
        change(&mut options);
        AuthorizationServer::new(
            options,
            &resource(),
            Arc::new(MemoryStore::new()),
            Arc::new(SignIn::default()),
        )
        .map(|_| ())
    };
    assert_eq!(with(&|_| {}), Ok(()));
    for issuer in [
        "https://other.example.com",
        "http://mcp.example.com",
        "https://mcp.example.com?x",
    ] {
        assert_eq!(
            with(&|options| options.issuer = issuer.to_owned()),
            Err(AuthorizationServerError::Issuer),
            "{issuer}"
        );
    }
    // A trailing slash is refused even when the resource lists it.
    let slashed = "https://mcp.example.com/oauth/";
    let listing =
        ResourceServer::new(ResourceOptions::new(RESOURCE, vec![slashed.to_owned()])).unwrap();
    assert_eq!(
        AuthorizationServer::new(
            AuthorizationServerOptions {
                issuer: slashed.to_owned(),
                ..options()
            },
            &listing,
            Arc::new(MemoryStore::new()),
            Arc::new(SignIn::default()),
        )
        .map(|_| ()),
        Err(AuthorizationServerError::Issuer)
    );
    let repeated = |options: &mut AuthorizationServerOptions| {
        let first = options.clients[0].clone();
        options.clients.push(first);
    };
    assert_eq!(with(&repeated), Err(AuthorizationServerError::Client(3)));
    for change in [
        (|client: &mut ClientOptions| client.secret = Some("short".to_owned()))
            as fn(&mut ClientOptions),
        |client| client.redirect_uris = vec!["http://app.example.com/callback".to_owned()],
        |client| client.redirect_uris = vec!["https://app.example.com/callback#x".to_owned()],
        |client| client.redirect_uris.clear(),
        |client| client.auth = ClientAuth::SecretPost,
        |client| client.scopes = vec!["a b".to_owned()],
        // A public client cannot use client credentials, and only a client
        // that does may have no redirect URIs.
        |client| client.client_credentials = true,
        |client| {
            client.client_credentials = true;
            client.redirect_uris.clear();
        },
    ] {
        assert_eq!(
            with(&|options| change(&mut options.clients[0])),
            Err(AuthorizationServerError::Client(0))
        );
    }
    assert_eq!(
        with(&|options| options.scopes_supported = Some(vec!["offline_access".to_owned()])),
        Err(AuthorizationServerError::Scope)
    );
    for path in ["oauth", "/authorize", "/oauth/", "/oauth?x"] {
        assert_eq!(
            with(&|options| options.interaction_path = path.to_owned()),
            Err(AuthorizationServerError::InteractionPath),
            "{path}"
        );
    }
    assert_eq!(
        with(&|options| options.lifetimes.code = Duration::from_secs(1)),
        Err(AuthorizationServerError::Lifetime)
    );
    assert_eq!(
        with(&|options| options.lifetimes.family = Duration::from_hours(2)),
        Err(AuthorizationServerError::Lifetime)
    );
    for grants in [0, MAX_CLIENT_GRANTS + 1] {
        assert_eq!(
            with(&|options| options.client_grants = grants),
            Err(AuthorizationServerError::ClientGrants),
            "{grants}"
        );
    }
}

#[test]
fn metadata_lists_the_issuer_and_only_supported_methods() {
    let harness = harness();
    let response = harness.send("GET", "/.well-known/oauth-authorization-server", &[], b"");
    assert_eq!(response.status, 200);
    assert_eq!(header(&response, "Access-Control-Allow-Origin"), Some("*"));
    let metadata = json_body(&response);
    assert_eq!(metadata["issuer"], ISSUER);
    assert_eq!(
        metadata["authorization_endpoint"],
        "https://mcp.example.com/authorize"
    );
    assert_eq!(
        metadata["registration_endpoint"],
        "https://mcp.example.com/register"
    );
    assert_eq!(
        metadata["code_challenge_methods_supported"],
        json!(["S256"])
    );
    assert_eq!(
        metadata["grant_types_supported"],
        json!(["authorization_code", "refresh_token", "client_credentials"])
    );
    assert_eq!(
        metadata["token_endpoint_auth_methods_supported"],
        json!(["client_secret_basic", "client_secret_post", "none"])
    );
    assert_eq!(
        metadata["authorization_response_iss_parameter_supported"],
        true
    );
    assert_eq!(metadata["client_id_metadata_document_supported"], true);

    let plain = AuthorizationServer::new(
        options(),
        &resource(),
        Arc::new(MemoryStore::new()),
        Arc::new(SignIn::default()),
    )
    .unwrap();
    let metadata: Value = serde_json::from_slice(
        &plain
            .handle(
                &RequestHead {
                    method: "GET",
                    target: "/.well-known/oauth-authorization-server",
                    headers: &[],
                },
                b"",
            )
            .body,
    )
    .unwrap();
    assert!(metadata.get("registration_endpoint").is_none());
    assert!(
        metadata
            .get("client_id_metadata_document_supported")
            .is_none()
    );
    assert_eq!(
        metadata["token_endpoint_auth_methods_supported"],
        json!(["client_secret_basic", "client_secret_post", "none"])
    );
    assert!(!plain.routes("/register"));

    let preflight = harness.send("OPTIONS", "/token", &[], b"");
    assert_eq!(preflight.status, 204);
    assert_eq!(header(&preflight, "Access-Control-Allow-Origin"), Some("*"));
    let authorize = harness.send("OPTIONS", "/authorize", &[], b"");
    assert_eq!(authorize.status, 405);
}

#[test]
fn authorization_requests_refuse_each_step_and_never_redirect_unverified() {
    let harness = harness();
    for (pairs, text) in [
        (authorization(&[("client_id", "acme-unknown")]), "client"),
        (
            authorization(&[("redirect_uri", "https://evil.example.com/callback")]),
            "redirect",
        ),
        (
            authorization(&[("redirect_uri", "http://localhost:8888/callback")]),
            "redirect",
        ),
    ] {
        let response = harness.authorize(&pairs);
        assert_eq!(response.status, 400);
        assert_eq!(header(&response, "Location"), None);
        assert!(String::from_utf8_lossy(&response.body).contains(text));
    }
    for (extra, error) in [
        (
            vec![("response_type", "token")],
            "unsupported_response_type",
        ),
        (vec![("code_challenge_method", "plain")], "invalid_request"),
        (vec![("code_challenge_method", "")], "invalid_request"),
        (vec![("code_challenge", "short")], "invalid_request"),
        (
            vec![("resource", "https://other.example.com/mcp")],
            "invalid_target",
        ),
        (vec![("scope", "acme:admin")], "invalid_scope"),
        (vec![("scope", "acme:read  acme:write")], "invalid_scope"),
    ] {
        let (base, query) = location(&harness.authorize(&authorization(&extra)));
        assert_eq!(base, REDIRECT, "{extra:?}");
        assert_eq!(query["error"], error, "{extra:?}");
        assert_eq!(query["iss"], ISSUER);
        assert_eq!(query["state"], "acme-state");
    }
    let long_state = "s".repeat(MAX_STATE_BYTES + 1);
    let (_, query) = location(&harness.authorize(&authorization(&[("state", &long_state)])));
    assert_eq!(query["error"], "invalid_request");
    // A missing resource defaults to the canonical one; another loopback IP
    // port matches the registered redirect.
    let pairs = authorization(&[
        ("resource", ""),
        ("redirect_uri", "http://127.0.0.1:9999/callback"),
    ]);
    let (reference, _) = harness.start(&pairs);
    let record = harness
        .store
        .get_pending(&reference, SystemTime::now())
        .unwrap()
        .unwrap();
    assert_eq!(record.resource, RESOURCE);
    assert_eq!(record.scopes, ["acme:read"]);
    assert_eq!(record.redirect_uri, "http://127.0.0.1:9999/callback");
}

#[test]
fn sign_in_completes_once_with_the_starting_browser_by_post() {
    let harness = harness();
    let pairs = authorization(&[("scope", "acme:read acme:write")]);
    let (reference, cookie) = harness.start(&pairs);
    let page = harness.send(
        "GET",
        &format!("{SIGN_IN}?pending={reference}"),
        &[("Cookie", format!("theme=dark; {cookie}").as_bytes())],
        b"",
    );
    assert_eq!(page.status, 200);
    {
        let seen = harness.sign_in.seen.lock().unwrap();
        let (headers, pending) = seen.last().unwrap();
        let pending = pending.as_ref().unwrap();
        assert_eq!(pending.client_name.as_deref(), Some("Acme Desktop"));
        assert_eq!(
            (pending.redirect_host.as_str(), pending.localhost_redirect),
            ("127.0.0.1", false)
        );
        assert_eq!(pending.scopes, ["acme:read", "acme:write"]);
        // The binding cookie never reaches consumer code; other cookies do.
        assert_eq!(headers, &[("Cookie".to_owned(), b"theme=dark".to_vec())]);
    }
    // A GET cannot approve, and neither can a browser without the cookie.
    let get = harness.send(
        "GET",
        &format!("{SIGN_IN}?pending={reference}&decision=approve"),
        &[("Cookie", cookie.as_bytes())],
        b"",
    );
    assert_eq!(get.status, 200);
    let foreign = harness.decide(&reference, "__Host-mcp-pending-x=y", "approve");
    assert_eq!(foreign.status, 400);
    assert!(
        harness
            .sign_in
            .seen
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .1
            .is_none()
    );
    let narrowed = harness.decide(&reference, &cookie, "narrow");
    let (_, query) = location(&narrowed);
    assert!(query.contains_key("code"));
    assert!(
        header(&narrowed, "Set-Cookie")
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(harness.decide(&reference, &cookie, "approve").status, 400);

    let (reference, cookie) = harness.start(&pairs);
    let (_, query) = location(&harness.decide(&reference, &cookie, "deny"));
    assert_eq!(query["error"], "access_denied");
    let (reference, cookie) = harness.start(&pairs);
    let (_, query) = location(&harness.decide(&reference, &cookie, "widen"));
    assert_eq!(query["error"], "server_error");
    assert!(!query.contains_key("code"));
}

#[test]
fn codes_exchange_once_with_pkce_and_tokens_verify_for_the_resource() {
    let harness = harness();
    let code = harness.code(&authorization(&[]));
    let tokens = harness.exchange(&code);
    assert_eq!(
        tokens.status,
        200,
        "{}",
        String::from_utf8_lossy(&tokens.body)
    );
    assert_eq!(header(&tokens, "Cache-Control"), Some("no-store"));
    let tokens = json_body(&tokens);
    assert_eq!(tokens["token_type"], "Bearer");
    assert_eq!(tokens["expires_in"], 3600);
    assert_eq!(tokens["scope"], "acme:read");
    let access = tokens["access_token"].as_str().unwrap();
    let grant = harness.grant(access).unwrap();
    assert_eq!(grant.subject(), Some("user-1"));
    assert_eq!(grant.client_id(), Some(PUBLIC));
    assert_eq!(grant.audiences(), [RESOURCE]);
    assert!(resource().check_grant(Ok(grant), None).is_ok());

    // A second redemption means the code leaked: the family it produced ends.
    let reused = harness.exchange(&code);
    assert_eq!(json_body(&reused)["error"], "invalid_grant");
    assert_eq!(harness.grant(access).unwrap_err(), Rejection::invalid());

    let code = harness.code(&authorization(&[]));
    let wrong = harness.token(
        &[
            ("grant_type", "authorization_code"),
            ("client_id", PUBLIC),
            ("code", &code),
            ("redirect_uri", REDIRECT),
            (
                "code_verifier",
                "acme-other-verifier-0123456789-0123456789-01234",
            ),
        ],
        &[],
    );
    assert_eq!(json_body(&wrong)["error"], "invalid_grant");
    assert_eq!(
        json_body(&harness.exchange(&code))["error"],
        "invalid_grant"
    );
    let code = harness.code(&authorization(&[]));
    let foreign = harness.token(
        &[
            ("grant_type", "authorization_code"),
            ("client_id", PUBLIC),
            ("code", &code),
            ("redirect_uri", REDIRECT),
            ("code_verifier", VERIFIER),
            ("resource", "https://other.example.com/mcp"),
        ],
        &[],
    );
    assert_eq!(json_body(&foreign)["error"], "invalid_target");
    assert_eq!(
        json_body(&harness.token(&[("grant_type", "password"), ("client_id", PUBLIC)], &[]))["error"],
        "unsupported_grant_type"
    );
    assert_eq!(harness.grant("not-ours").unwrap_err(), Rejection::invalid());
}

#[test]
fn refresh_rotates_keeps_the_resource_and_detects_reuse_at_any_age() {
    let harness = harness();
    let code = harness.code(&authorization(&[("scope", "acme:read acme:write")]));
    let first = json_body(&harness.exchange(&code));
    let (access_1, refresh_1) = (
        first["access_token"].as_str().unwrap(),
        first["refresh_token"].as_str().unwrap(),
    );
    assert!(refresh_1.starts_with("rt_") && refresh_1.contains(".1."));

    assert_eq!(
        json_body(&harness.refresh(refresh_1, Some("acme:admin")))["error"],
        "invalid_scope"
    );
    let second = json_body(&harness.refresh(refresh_1, Some("acme:read")));
    let (access_2, refresh_2) = (
        second["access_token"].as_str().unwrap(),
        second["refresh_token"].as_str().unwrap(),
    );
    assert!(refresh_2.contains(".2."));
    assert_eq!(second["scope"], "acme:read");
    // One live access token per family; the narrowed token keeps the audience.
    assert!(harness.grant(access_1).is_err());
    let grant = harness.grant(access_2).unwrap();
    assert_eq!(
        (grant.audiences(), grant.scopes().collect::<Vec<_>>()),
        ([RESOURCE.to_owned()].as_slice(), vec!["acme:read"])
    );

    // The previous token retries within the grace, as after a lost response.
    let retried = json_body(&harness.refresh(refresh_1, None));
    let refresh_3 = retried["refresh_token"].as_str().unwrap();
    assert!(refresh_3.contains(".3."));
    assert_eq!(retried["scope"], "acme:read acme:write");
    assert!(harness.grant(access_2).is_err());
    let fourth = json_body(&harness.refresh(refresh_3, None));
    let access_4 = fourth["access_token"].as_str().unwrap();
    assert!(harness.grant(access_4).is_ok());
    // Generation 1 is two behind now: reuse ends the family.
    assert_eq!(
        json_body(&harness.refresh(refresh_1, None))["error"],
        "invalid_grant"
    );
    assert!(harness.grant(access_4).is_err());
    assert_eq!(
        json_body(&harness.refresh(fourth["refresh_token"].as_str().unwrap(), None))["error"],
        "invalid_grant"
    );
    for forged in ["rt_x", "rt_a.0.b", "rt_a.1.b.c", "at_a"] {
        assert_eq!(
            json_body(&harness.refresh(forged, None))["error"],
            "invalid_grant",
            "{forged}"
        );
    }
}

#[test]
fn concurrent_refreshes_commit_once_and_leave_the_winner_valid() {
    let harness = harness();
    let tokens = json_body(&harness.exchange(&harness.code(&authorization(&[]))));
    let refresh = tokens["refresh_token"].as_str().unwrap().to_owned();
    // Both requests read generation 1 before either rotates.
    *harness.store.gate.lock().unwrap() = Some(Arc::new(Barrier::new(2)));
    let responses = std::thread::scope(|scope| {
        let first = scope.spawn(|| harness.refresh(&refresh, None));
        let second = scope.spawn(|| harness.refresh(&refresh, None));
        [first.join().unwrap(), second.join().unwrap()]
    });
    *harness.store.gate.lock().unwrap() = None;
    let (won, lost): (Vec<_>, Vec<_>) = responses
        .into_iter()
        .partition(|response| response.status == 200);
    assert_eq!((won.len(), lost.len()), (1, 1));
    assert_eq!(lost[0].status, 503);
    // The winner's tokens are the family's live ones, so the grant survives.
    let won = json_body(&won[0]);
    assert!(harness.grant(won["access_token"].as_str().unwrap()).is_ok());
    let next = json_body(&harness.refresh(won["refresh_token"].as_str().unwrap(), None));
    assert!(
        harness
            .grant(next["access_token"].as_str().unwrap())
            .is_ok()
    );
}

#[test]
fn a_replay_before_family_creation_leaves_no_live_grant() {
    let harness = harness();
    let code = harness.code(&authorization(&[]));
    let (paused, resume) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
    *harness.store.creation.lock().unwrap() = Some((Arc::clone(&paused), Arc::clone(&resume)));
    let (first, replay) = std::thread::scope(|scope| {
        let first = scope.spawn(|| harness.exchange(&code));
        // The first redemption has consumed the code and waits to create its family.
        paused.wait();
        let replay = harness.exchange(&code);
        resume.wait();
        (first.join().unwrap(), replay)
    });
    *harness.store.creation.lock().unwrap() = None;
    assert_eq!(json_body(&replay)["error"], "invalid_grant");
    assert_eq!(first.status, 400);
    assert_eq!(json_body(&first)["error"], "invalid_grant");
    assert!(json_body(&first).get("access_token").is_none());
    assert_eq!(
        json_body(&harness.exchange(&code))["error"],
        "invalid_grant"
    );
}

#[test]
fn completed_sign_ins_keep_the_pending_queue_bounded() {
    let harness = harness_with(options(), MemoryStore::with_capacity(2));
    let (waiting, _) = harness.start(&authorization(&[]));
    // No token requests run, so nothing but the store itself can trim it.
    for _ in 0..50 {
        let (reference, cookie) = harness.start(&authorization(&[]));
        let (_, query) = location(&harness.decide(&reference, &cookie, "deny"));
        assert_eq!(query["error"], "access_denied");
        assert!(harness.store.inner.queued_references() <= 4);
    }
    // The sign-in still in progress was never evicted by completed ones.
    let now = SystemTime::now();
    assert!(harness.store.get_pending(&waiting, now).unwrap().is_some());
}

#[test]
fn forged_generations_of_a_live_family_are_refused_without_change() {
    let harness = harness();
    let tokens = json_body(&harness.exchange(&harness.code(&authorization(&[]))));
    let (access, refresh) = (
        tokens["access_token"].as_str().unwrap(),
        tokens["refresh_token"].as_str().unwrap(),
    );
    let family = refresh
        .strip_prefix("rt_")
        .unwrap()
        .split('.')
        .next()
        .unwrap();
    for generation in [u64::MAX, 2] {
        let forged = format!("rt_{family}.{generation}.forged.forged");
        assert_eq!(
            json_body(&harness.refresh(&forged, None))["error"],
            "invalid_grant",
            "{generation}"
        );
    }
    // The store refuses the same generation itself, without overflowing.
    let now = SystemTime::now();
    let live = harness.store.get_family(family, now).unwrap().unwrap();
    let rotation = Rotation {
        family_id: family.to_owned(),
        generation: u64::MAX,
        expected: live.generation,
        presented: wire::digest("rt_forged"),
        proof: wire::digest("rf_forged"),
        refresh: wire::digest("rt_next"),
        refresh_expires_at: now + Duration::from_mins(1),
        access: AccessRecord {
            digest: wire::digest("at_next"),
            family_id: family.to_owned(),
            scopes: Vec::new(),
            expires_at: now + Duration::from_mins(1),
        },
        grace: Duration::from_secs(10),
    };
    assert_eq!(
        harness.store.inner.rotate(rotation, now),
        Ok(Rotated::Unknown)
    );
    // Nothing changed: the real tokens still work.
    assert!(harness.grant(access).is_ok());
    assert_eq!(harness.refresh(refresh, None).status, 200);
}

#[test]
fn only_a_token_of_the_family_can_report_reuse() {
    let harness = harness();
    let first = json_body(&harness.exchange(&harness.code(&authorization(&[]))));
    let refresh_1 = first["refresh_token"].as_str().unwrap();
    let second = json_body(&harness.refresh(refresh_1, None));
    let third = json_body(&harness.refresh(second["refresh_token"].as_str().unwrap(), None));
    let access = third["access_token"].as_str().unwrap();
    // Someone who saw only the family ID forges an old generation.
    let family = refresh_1
        .strip_prefix("rt_")
        .unwrap()
        .split('.')
        .next()
        .unwrap();
    let forged = format!("rt_{family}.1.garbage.garbage");
    assert_eq!(
        json_body(&harness.refresh(&forged, None))["error"],
        "invalid_grant"
    );
    assert!(
        harness.grant(access).is_ok(),
        "a forged token ended the sign-in"
    );
    // The real generation-1 token still proves reuse and ends it.
    assert_eq!(
        json_body(&harness.refresh(refresh_1, None))["error"],
        "invalid_grant"
    );
    assert!(harness.grant(access).is_err());
}

#[test]
fn client_credentials_issue_access_tokens_without_a_user_or_refresh() {
    let harness = harness();
    let grant = |client: &str, secret: &str, scope: Option<&str>| {
        let mut fields = vec![
            ("grant_type", "client_credentials"),
            ("client_id", client),
            ("client_secret", secret),
        ];
        if let Some(scope) = scope {
            fields.push(("scope", scope));
        }
        harness.token(&fields, &[])
    };
    let issued = grant(MACHINE, MACHINE_SECRET, None);
    assert_eq!(issued.status, 200);
    let issued = json_body(&issued);
    assert_eq!(issued["token_type"], "Bearer");
    assert_eq!(issued["expires_in"], 3600);
    assert_eq!(issued["scope"], "acme:read");
    assert!(issued.get("refresh_token").is_none());
    let verified = harness
        .grant(issued["access_token"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        (verified.subject(), verified.client_id()),
        (Some(MACHINE), Some(MACHINE))
    );
    assert_eq!(verified.audiences(), [RESOURCE]);
    assert!(resource().check_grant(Ok(verified), None).is_ok());

    let wider = json_body(&grant(
        MACHINE,
        MACHINE_SECRET,
        Some("acme:read acme:write"),
    ));
    assert_eq!(wider["scope"], "acme:read acme:write");
    for scope in ["acme:admin", "acme:read  acme:write"] {
        assert_eq!(
            json_body(&grant(MACHINE, MACHINE_SECRET, Some(scope)))["error"],
            "invalid_scope"
        );
    }
    // Only clients configured for it, and never public ones.
    assert_eq!(
        json_body(&grant(CONFIDENTIAL, SECRET, None))["error"],
        "unauthorized_client"
    );
    let public = harness.token(
        &[("grant_type", "client_credentials"), ("client_id", PUBLIC)],
        &[],
    );
    assert_eq!(json_body(&public)["error"], "unauthorized_client");
    assert_eq!(grant(MACHINE, "wrong", None).status, 401);
    let foreign = harness.token(
        &[
            ("grant_type", "client_credentials"),
            ("client_id", MACHINE),
            ("client_secret", MACHINE_SECRET),
            ("resource", "https://other.example.com/mcp"),
        ],
        &[],
    );
    assert_eq!(json_body(&foreign)["error"], "invalid_target");

    // A machine client has no sign-in: the authorization endpoint refuses it.
    let pairs = authorization(&[("client_id", MACHINE)]);
    assert_eq!(harness.authorize(&pairs).status, 400);
    // Revocation ends a client credentials token like any other.
    let revoked = grant(MACHINE, MACHINE_SECRET, None);
    let access = json_body(&revoked)["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let body = format!(
        "token={access}&client_id={MACHINE}&client_secret={}",
        wire::encode_component(MACHINE_SECRET)
    );
    let response = harness.send(
        "POST",
        "/revoke",
        &[("Content-Type", b"application/x-www-form-urlencoded")],
        body.as_bytes(),
    );
    assert_eq!(response.status, 200);
    assert!(harness.grant(&access).is_err());

    // Without default scopes, leaving `scope` out grants none, as at sign-in.
    let mut bare = options();
    bare.default_scopes = Vec::new();
    let bare = harness_with(bare, MemoryStore::new());
    let issued = json_body(&bare.token(
        &[
            ("grant_type", "client_credentials"),
            ("client_id", MACHINE),
            ("client_secret", MACHINE_SECRET),
        ],
        &[],
    ));
    assert_eq!(issued["scope"], "");
    let verified = bare
        .grant(issued["access_token"].as_str().unwrap())
        .unwrap();
    assert_eq!(verified.scopes().count(), 0);
}

#[test]
fn each_client_holds_a_bounded_number_of_client_credentials_grants() {
    // The default limit, and a raised one for many instances sharing a client.
    for limit in [DEFAULT_CLIENT_GRANTS, 40] {
        let mut options = options();
        options.client_grants = limit;
        client_grants_stay_within(
            &harness_with(options, MemoryStore::with_capacity(64)),
            limit,
        );
    }
}

fn client_grants_stay_within(harness: &Harness, limit: usize) {
    let issue = || {
        let issued = json_body(&harness.token(
            &[
                ("grant_type", "client_credentials"),
                ("client_id", MACHINE),
                ("client_secret", MACHINE_SECRET),
            ],
            &[],
        ));
        issued["access_token"].as_str().unwrap().to_owned()
    };
    let tokens = (0..=limit).map(|_| issue()).collect::<Vec<_>>();
    // The oldest gave way to the newest; the rest stay live.
    assert!(harness.grant(&tokens[0]).is_err());
    for token in &tokens[1..] {
        assert!(harness.grant(token).is_ok());
    }
    // A sign-in by another client is untouched by the machine client's churn.
    let signed_in = json_body(&harness.exchange(&harness.code(&authorization(&[]))));
    for _ in 0..(2 * limit) {
        issue();
    }
    assert!(
        harness
            .grant(signed_in["access_token"].as_str().unwrap())
            .is_ok()
    );
}

/// The server's clock, which a test moves forward.
struct Clock(Arc<Mutex<SystemTime>>);

impl Clock {
    fn install(harness: Harness) -> (Harness, Self) {
        let time = Arc::new(Mutex::new(SystemTime::now()));
        let source = Arc::clone(&time);
        let server = harness
            .server
            .with_clock(Arc::new(move || *source.lock().unwrap()));
        (Harness { server, ..harness }, Self(time))
    }

    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

#[test]
fn expired_sign_ins_and_codes_cannot_complete() {
    let (harness, clock) = Clock::install(harness());
    let (reference, cookie) = harness.start(&authorization(&[]));
    clock.advance(Lifetimes::default().pending + Duration::from_secs(1));
    assert_eq!(harness.decide(&reference, &cookie, "approve").status, 400);
    let seen = harness.sign_in.seen.lock().unwrap();
    assert!(seen.last().unwrap().1.is_none());
    drop(seen);

    let code = harness.code(&authorization(&[]));
    // The default code lifetime is one minute.
    clock.advance(Duration::from_secs(50));
    assert_eq!(harness.exchange(&code).status, 200);
    let code = harness.code(&authorization(&[]));
    clock.advance(Lifetimes::default().code + Duration::from_secs(1));
    assert_eq!(
        json_body(&harness.exchange(&code))["error"],
        "invalid_grant"
    );
}

#[test]
fn tokens_expire_by_their_lifetimes() {
    let mut options = options();
    options.lifetimes = Lifetimes {
        access: Duration::from_mins(10),
        refresh_idle: Duration::from_hours(1),
        family: Duration::from_hours(2),
        ..Lifetimes::default()
    };
    let (harness, clock) = Clock::install(harness_with(options, MemoryStore::new()));
    let second = Duration::from_secs(1);
    let sign_in = || json_body(&harness.exchange(&harness.code(&authorization(&[]))));
    let token = |tokens: &Value, key: &str| tokens[key].as_str().unwrap().to_owned();

    // An access token ends at its lifetime; its refresh token still rotates.
    let first = sign_in();
    clock.advance(Duration::from_secs(599));
    assert!(harness.grant(&token(&first, "access_token")).is_ok());
    clock.advance(2 * second);
    assert_eq!(
        harness.grant(&token(&first, "access_token")).unwrap_err(),
        Rejection::invalid()
    );
    let rotated = json_body(&harness.refresh(&token(&first, "refresh_token"), None));
    assert!(harness.grant(&token(&rotated, "access_token")).is_ok());
    // An idle refresh token ends.
    clock.advance(Duration::from_hours(1) + second);
    assert_eq!(
        json_body(&harness.refresh(&token(&rotated, "refresh_token"), None))["error"],
        "invalid_grant"
    );

    // Rotating within the idle limit cannot outlive the family.
    let mut tokens = sign_in();
    for _ in 0..2 {
        clock.advance(Duration::from_mins(50));
        tokens = json_body(&harness.refresh(&token(&tokens, "refresh_token"), None));
        assert!(tokens.get("refresh_token").is_some(), "{tokens}");
    }
    clock.advance(Duration::from_mins(25));
    assert_eq!(
        json_body(&harness.refresh(&token(&tokens, "refresh_token"), None))["error"],
        "invalid_grant"
    );

    // The replaced refresh token retries only within the grace.
    // The default grace is ten seconds.
    for (wait, retried) in [
        (Duration::from_secs(9), true),
        (Duration::from_secs(11), false),
    ] {
        let first = sign_in();
        let rotated = json_body(&harness.refresh(&token(&first, "refresh_token"), None));
        clock.advance(wait);
        let retry = json_body(&harness.refresh(&token(&first, "refresh_token"), None));
        if retried {
            assert!(harness.grant(&token(&retry, "access_token")).is_ok());
        } else {
            // Reuse after the grace ends the family.
            assert_eq!(retry["error"], "invalid_grant");
            assert_eq!(
                json_body(&harness.refresh(&token(&rotated, "refresh_token"), None))["error"],
                "invalid_grant"
            );
        }
    }

    // A client credentials token ends at the access lifetime too.
    let machine = json_body(&harness.token(
        &[
            ("grant_type", "client_credentials"),
            ("client_id", MACHINE),
            ("client_secret", MACHINE_SECRET),
        ],
        &[],
    ));
    assert!(harness.grant(&token(&machine, "access_token")).is_ok());
    clock.advance(Duration::from_mins(10) + second);
    assert!(harness.grant(&token(&machine, "access_token")).is_err());
}

#[test]
fn a_zero_grace_treats_the_previous_refresh_token_as_reuse() {
    let mut options = options();
    options.lifetimes.refresh_grace = Duration::ZERO;
    let harness = harness_with(options, MemoryStore::new());
    let tokens = json_body(&harness.exchange(&harness.code(&authorization(&[]))));
    let refresh_1 = tokens["refresh_token"].as_str().unwrap();
    let second = json_body(&harness.refresh(refresh_1, None));
    assert_eq!(
        json_body(&harness.refresh(refresh_1, None))["error"],
        "invalid_grant"
    );
    assert!(
        harness
            .grant(second["access_token"].as_str().unwrap())
            .is_err()
    );
}

#[test]
fn revocation_ends_the_family_for_its_own_client_only() {
    let harness = harness();
    let tokens = json_body(&harness.exchange(&harness.code(&authorization(&[]))));
    let (access, refresh) = (
        tokens["access_token"].as_str().unwrap(),
        tokens["refresh_token"].as_str().unwrap(),
    );
    let revoke = |token: &str, client: &[(&str, &[u8])], id: Option<&str>| {
        let mut body = format!("token={}", wire::encode_component(token));
        if let Some(id) = id {
            body = format!("{body}&client_id={id}");
        }
        let mut headers = vec![(
            "Content-Type",
            b"application/x-www-form-urlencoded".as_slice(),
        )];
        headers.extend_from_slice(client);
        harness.send("POST", "/revoke", &headers, body.as_bytes())
    };
    let basic = format!(
        "Basic {}",
        crate::content::base64(format!("{CONFIDENTIAL}:{SECRET}").as_bytes())
    );
    let other = revoke(refresh, &[("Authorization", basic.as_bytes())], None);
    assert_eq!(other.status, 200);
    assert!(harness.grant(access).is_ok());
    assert_eq!(revoke("rt_unknown.1.x", &[], Some(PUBLIC)).status, 200);
    assert_eq!(revoke(refresh, &[], Some(PUBLIC)).status, 200);
    assert!(harness.grant(access).is_err());
    assert_eq!(
        json_body(&harness.refresh(refresh, None))["error"],
        "invalid_grant"
    );
}

#[test]
fn client_authentication_follows_the_registered_method() {
    let harness = harness();
    let token = |headers: &[(&str, &[u8])], fields: &[(&str, &str)]| {
        let mut all = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", "rt_a.1.b"),
        ];
        all.extend_from_slice(fields);
        harness.token(&all, headers)
    };
    let basic = |id: &str, secret: &str| {
        format!(
            "Basic {}",
            crate::content::base64(format!("{id}:{secret}").as_bytes())
        )
    };
    let good = basic(CONFIDENTIAL, SECRET);
    // Authenticated, then refused for the unknown token. A confidential client
    // may send its secret by Basic or in the form.
    assert_eq!(
        json_body(&token(&[("Authorization", good.as_bytes())], &[]))["error"],
        "invalid_grant"
    );
    assert_eq!(
        json_body(&token(
            &[],
            &[("client_id", CONFIDENTIAL), ("client_secret", SECRET)]
        ))["error"],
        "invalid_grant"
    );
    for (headers, fields) in [
        (
            vec![("Authorization", basic(CONFIDENTIAL, "wrong").into_bytes())],
            vec![],
        ),
        (
            vec![],
            vec![("client_id", CONFIDENTIAL), ("client_secret", "wrong")],
        ),
        (vec![], vec![("client_id", CONFIDENTIAL)]),
        (
            vec![],
            vec![("client_id", PUBLIC), ("client_secret", SECRET)],
        ),
        (vec![], vec![("client_id", "acme-unknown")]),
        (vec![], vec![]),
        (
            vec![("Authorization", good.clone().into_bytes())],
            vec![("client_id", PUBLIC)],
        ),
    ] {
        let headers = headers
            .iter()
            .map(|(name, value)| (*name, value.as_slice()))
            .collect::<Vec<_>>();
        let refused = token(&headers, &fields);
        assert_eq!(refused.status, 401, "{fields:?}");
        assert_eq!(json_body(&refused)["error"], "invalid_client");
    }
    let refused = token(
        &[("Authorization", basic(CONFIDENTIAL, "wrong").as_bytes())],
        &[],
    );
    assert_eq!(
        header(&refused, "WWW-Authenticate"),
        Some("Basic realm=\"oauth\"")
    );
    let not_form = harness.send(
        "POST",
        "/token",
        &[("Content-Type", b"application/json")],
        b"{}",
    );
    assert_eq!(json_body(&not_form)["error"], "invalid_request");
    let repeated = harness.send(
        "POST",
        "/token",
        &[("Content-Type", b"application/x-www-form-urlencoded")],
        b"client_id=a&client_id=b",
    );
    assert_eq!(json_body(&repeated)["error"], "invalid_request");
}

#[test]
fn client_metadata_documents_and_registration_add_clients() {
    let harness = harness();
    let document = "https://app.example.com/client.json";
    let pairs = authorization(&[
        ("client_id", document),
        ("redirect_uri", "https://app.example.com/callback"),
    ]);
    let (reference, _) = harness.start(&pairs);
    assert!(reference.starts_with("pr_"));
    for client in [
        "https://app.example.com/other.json",
        "https://app.example.com/missing.json",
        "http://app.example.com/client.json",
    ] {
        let response = harness.authorize(&authorization(&[
            ("client_id", client),
            ("redirect_uri", "https://app.example.com/callback"),
        ]));
        assert_eq!(response.status, 400, "{client}");
    }

    let register = |body: Value| {
        harness.send(
            "POST",
            "/register",
            &[("Content-Type", b"application/json")],
            &serde_json::to_vec(&body).unwrap(),
        )
    };
    let created = register(json!({
        "client_name": "Acme CLI",
        "redirect_uris": ["http://127.0.0.1:5555/callback"],
        "grant_types": ["authorization_code"],
        "token_endpoint_auth_method": "client_secret_post",
    }));
    assert_eq!(created.status, 201);
    let created = json_body(&created);
    // The reply states the grants the client actually gets.
    assert_eq!(
        created["grant_types"],
        json!(["authorization_code", "refresh_token"])
    );
    let (client_id, secret) = (
        created["client_id"].as_str().unwrap(),
        created["client_secret"].as_str().unwrap(),
    );
    assert!(client_id.starts_with("mcp_") && secret.starts_with("cs_"));
    let code = harness.code(&authorization(&[
        ("client_id", client_id),
        ("redirect_uri", "http://127.0.0.1:5555/callback"),
    ]));
    let exchanged = harness.token(
        &[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("client_secret", secret),
            ("code", &code),
            ("redirect_uri", "http://127.0.0.1:5555/callback"),
            ("code_verifier", VERIFIER),
        ],
        &[],
    );
    assert_eq!(exchanged.status, 200);

    let public = json_body(&register(
        json!({"redirect_uris": ["http://localhost:5555/cb"], "token_endpoint_auth_method": "none"}),
    ));
    assert!(public.get("client_secret").is_none());
    for (body, reason) in [
        (
            json!({"redirect_uris": ["http://app.example.com/cb"]}),
            "redirect_uris",
        ),
        (
            json!({"redirect_uris": ["https://app.example.com/cb"], "grant_types": ["password"]}),
            "grant_types",
        ),
        (
            json!({"redirect_uris": ["https://app.example.com/cb"], "response_types": ["token"]}),
            "response_types",
        ),
        (
            json!({"redirect_uris": ["https://app.example.com/cb"], "token_endpoint_auth_method": "private_key_jwt"}),
            "token_endpoint_auth_method",
        ),
        (
            json!({"redirect_uris": ["https://app.example.com/cb"], "client_name": "Blocked Acme"}),
            "closed",
        ),
    ] {
        let refused = json_body(&register(body));
        assert_eq!(refused["error"], "invalid_client_metadata");
        assert!(
            refused["error_description"]
                .as_str()
                .unwrap()
                .contains(reason),
            "{reason}"
        );
    }
}

#[test]
fn the_store_receives_digests_never_secret_values() {
    let harness = harness();
    let code = harness.code(&authorization(&[]));
    let tokens = json_body(&harness.exchange(&code));
    let refreshed = json_body(&harness.refresh(tokens["refresh_token"].as_str().unwrap(), None));
    harness.issued.lock().unwrap().push(code);
    assert!(refreshed["access_token"].is_string());
    let registered = harness.send(
        "POST",
        "/register",
        &[("Content-Type", b"application/json")],
        br#"{"redirect_uris":["https://app.example.com/cb"],"token_endpoint_auth_method":"client_secret_basic"}"#,
    );
    assert_eq!(registered.status, 201);
    let issued = harness.issued.lock().unwrap();
    assert!(
        issued.iter().any(|value| value.starts_with("pb_")),
        "the binding was captured"
    );
    assert!(
        issued.iter().any(|value| value.starts_with("cs_")),
        "the secret was captured"
    );
    let seen = harness.store.seen.lock().unwrap().join("\n");
    assert_ne!(seen, "");
    // Pending references and family IDs are not secret; every other value is.
    let secrets = issued
        .iter()
        .filter(|value| {
            ["ac_", "at_", "rt_", "pb_", "cs_"]
                .iter()
                .any(|prefix| value.starts_with(prefix))
        })
        // A refresh token's family secret and random tail are both secret.
        .flat_map(|value| match value.strip_prefix("rt_") {
            Some(rest) => rest.split('.').skip(2).collect::<Vec<_>>(),
            None => vec![value.as_str()],
        })
        .collect::<Vec<_>>();
    assert!(secrets.len() >= 6, "{secrets:?}");
    for secret in secrets {
        assert!(!seen.contains(secret), "the store saw a secret value");
    }
}

#[test]
fn memory_store_evicts_old_pending_and_keeps_one_access_token_per_family() {
    let harness = harness_with(options(), MemoryStore::with_capacity(2));
    let (first, _) = harness.start(&authorization(&[]));
    let (second, _) = harness.start(&authorization(&[]));
    let (third, _) = harness.start(&authorization(&[]));
    let now = SystemTime::now();
    assert!(harness.store.get_pending(&first, now).unwrap().is_none());
    assert!(harness.store.get_pending(&second, now).unwrap().is_some());
    assert!(harness.store.get_pending(&third, now).unwrap().is_some());

    let tokens = json_body(&harness.exchange(&harness.code(&authorization(&[]))));
    let mut refresh = tokens["refresh_token"].as_str().unwrap().to_owned();
    for _ in 0..20 {
        let next = json_body(&harness.refresh(&refresh, None));
        refresh = next["refresh_token"]
            .as_str()
            .expect("the store never fills")
            .to_owned();
    }
    let store = MemoryStore::with_capacity(1);
    let family = |id: &str| FamilyRecord {
        id: id.to_owned(),
        subject: "user-1".to_owned(),
        client_id: PUBLIC.to_owned(),
        scopes: Vec::new(),
        resource: RESOURCE.to_owned(),
        created_at: now,
        expires_at: now + Duration::from_mins(1),
        proof: wire::digest(&format!("rf_{id}")),
        generation: 1,
        refresh: wire::digest(id),
        refresh_expires_at: now + Duration::from_mins(1),
        previous: None,
        access: wire::digest(&format!("at_{id}")),
    };
    let access = |id: &str| AccessRecord {
        digest: wire::digest(&format!("at_{id}")),
        family_id: id.to_owned(),
        scopes: Vec::new(),
        expires_at: now + Duration::from_mins(1),
    };
    let code = |id: &str| CodeRecord {
        digest: wire::digest(&format!("ac_{id}")),
        family_id: id.to_owned(),
        client_id: PUBLIC.to_owned(),
        redirect_uri: REDIRECT.to_owned(),
        challenge: wire::challenge(VERIFIER),
        scopes: Vec::new(),
        resource: RESOURCE.to_owned(),
        subject: "user-1".to_owned(),
        expires_at: now + Duration::from_secs(30),
        consumed: false,
    };
    let redeem = |id: &str| {
        store.put_code(code(id)).unwrap();
        assert!(matches!(
            store.take_code(&code(id).digest, now),
            Ok(CodeTake::Taken(_))
        ));
        code(id).digest
    };
    let a = redeem("a");
    assert_eq!(store.create_family(&a, family("a"), access("a")), Ok(true));
    assert_eq!(
        store.create_family(&a, family("b"), access("b")),
        Err(StoreError::Full)
    );
    assert_eq!(
        store.purge_expired(now + Duration::from_secs(61), 64),
        Ok(3)
    );
    // Without its consumed code, a family cannot be created.
    assert_eq!(store.create_family(&a, family("b"), access("b")), Ok(false));
    let b = redeem("b");
    assert_eq!(store.create_family(&b, family("b"), access("b")), Ok(true));
}
