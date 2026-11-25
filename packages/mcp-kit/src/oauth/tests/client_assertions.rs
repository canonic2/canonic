//! Signed client authentication at the OAuth endpoints, with real signing keys.

use super::{
    assertions::{self, T, ec_jwk, es256, key_set, merged, rs256, rsa_jwk},
    *,
};
use std::sync::atomic::{AtomicU64, Ordering};

const DOCUMENT: &str = "https://app.example.com/machine.json";
const CALLBACK: &str = "https://app.example.com/callback";
const JWKS_URI: &str = "https://keys.example.com/acme.json";

pub(crate) fn test_client_jwks() -> Vec<u8> {
    key_set(&[ec_jwk("partner-1")])
}

pub(crate) fn test_client_assertion(client: &str, audience: &str, id: &str, exp: u64) -> String {
    es256(&json!({"typ": "JWT"}), &claims(client, audience, id, exp))
}

fn claims(client: &str, audience: &str, id: &str, exp: u64) -> Value {
    json!({"iss": client, "sub": client, "aud": audience, "jti": id, "exp": exp})
}

fn fields(assertion: &str) -> [(&str, &str); 2] {
    [
        ("client_assertion_type", JWT_CLIENT_ASSERTION_TYPE),
        ("client_assertion", assertion),
    ]
}

fn machine(harness: &Harness, assertion: &str) -> Response {
    let [kind, assertion] = fields(assertion);
    harness.token(
        &[("grant_type", "client_credentials"), kind, assertion],
        &[],
    )
}

fn revoke(harness: &Harness, access: &str, assertion: &str) -> Response {
    let [kind, assertion] = fields(assertion);
    harness.post_form("/revoke", &[("token", access), kind, assertion], &[])
}

fn private_client(keys: Vec<u8>) -> AuthorizationServerOptions {
    let mut options = options();
    let machine = options
        .clients
        .iter_mut()
        .find(|client| client.client_id == MACHINE)
        .unwrap();
    machine.auth = ClientAuth::PrivateKeyJwt;
    machine.secret = None;
    options.client_jwks.insert(MACHINE.to_owned(), keys);
    options
}

fn at_test_time(mut harness: Harness) -> Harness {
    harness.server = harness
        .server
        .with_clock(Arc::new(|| SystemTime::UNIX_EPOCH + Duration::from_secs(T)));
    harness
}

fn invalid_client(response: &Response) {
    assert_eq!(response.status, 401, "{response:?}");
    assert_eq!(json_body(response)["error"], "invalid_client");
}

#[test]
fn a_configured_private_key_client_requires_a_usable_public_key_set() {
    let empty = private_client(key_set(&[]));
    let mut missing = private_client(test_client_jwks());
    missing.client_jwks.clear();
    let mut with_secret = private_client(test_client_jwks());
    with_secret.clients[2].secret = Some(MACHINE_SECRET.to_owned());
    for options in [empty, missing, with_secret] {
        let server = AuthorizationServer::new(
            options,
            &resource(),
            Arc::new(MemoryStore::new()),
            Arc::new(SignIn::default()),
        );
        assert!(matches!(server, Err(AuthorizationServerError::Client(2))));
    }
}

#[test]
fn configured_rsa_client_issues_and_revokes_with_distinct_assertions() {
    let harness = at_test_time(harness_with(
        private_client(key_set(&[rsa_jwk("idp-1")])),
        MemoryStore::new(),
    ));
    let signed = rs256(
        &json!({"typ": null}),
        &claims(MACHINE, ISSUER, "issue", T + 300),
    );
    let issued = machine(&harness, &signed);
    assert_eq!(issued.status, 200, "{issued:?}");
    let body = json_body(&issued);
    let access = body["access_token"].as_str().unwrap();
    let grant = harness.grant(access).unwrap();
    assert_eq!(
        (grant.subject(), grant.client_id()),
        (Some(MACHINE), Some(MACHINE))
    );

    invalid_client(&revoke(&harness, access, &signed));
    assert!(harness.grant(access).is_ok());
    let audience = format!("{ISSUER}/token");
    let reused_id = rs256(
        &json!({"typ": "JWT"}),
        &claims(MACHINE, &audience, "issue", T + 360),
    );
    invalid_client(&revoke(&harness, access, &reused_id));
    let fresh = rs256(
        &json!({"typ": "JWT"}),
        &claims(MACHINE, &audience, "revoke", T + 300),
    );
    assert_eq!(revoke(&harness, access, &fresh).status, 200);
    assert!(harness.grant(access).is_err());
    let recorded = harness.store.seen.lock().unwrap().join("\n");
    for secret in [&signed, &fresh, access] {
        assert!(!recorded.contains(secret));
    }
}

#[test]
fn rejects_invalid_claims_headers_and_mixed_client_credentials() {
    let harness = at_test_time(harness_with(
        private_client(test_client_jwks()),
        MemoryStore::new(),
    ));
    let base = claims(MACHINE, ISSUER, "first", T + 300);
    let header = json!({"typ": "JWT"});
    for change in [
        json!({"iss": "another-client"}),
        json!({"sub": null}),
        json!({"aud": "https://elsewhere.example.com/token"}),
        json!({"aud": [ISSUER, "https://elsewhere.example.com"]}),
        json!({"exp": null}),
        json!({"exp": T - 60}),
        json!({"exp": T + 3601}),
        json!({"iat": T + 61}),
        json!({"iat": T - 3600}),
        json!({"nbf": T + 61}),
        json!({"nbf": "tomorrow"}),
        json!({"jti": null}),
        json!({"jti": ""}),
    ] {
        invalid_client(&machine(
            &harness,
            &es256(&header, &merged(base.clone(), &change)),
        ));
    }
    for change in [
        json!({"alg": "none"}),
        json!({"alg": "HS256"}),
        json!({"kid": "missing"}),
        json!({"crit": ["ext"]}),
        json!({"typ": "oauth-id-jag+jwt"}),
    ] {
        invalid_client(&machine(
            &harness,
            &es256(&merged(header.clone(), &change), &base),
        ));
    }
    let signed = es256(&header, &base);
    let corrupt = format!("{}.AAAA", signed.rsplit_once('.').unwrap().0);
    invalid_client(&machine(&harness, &corrupt));
    let [kind, assertion] = fields(&signed);
    for form in [
        vec![kind],
        vec![assertion],
        vec![("client_assertion_type", "other"), assertion],
        vec![kind, assertion, ("client_id", CONFIDENTIAL)],
        vec![kind, assertion, ("client_secret", MACHINE_SECRET)],
        vec![("client_id", MACHINE)],
        vec![("client_id", MACHINE), ("client_secret", MACHINE_SECRET)],
    ] {
        let mut fields = vec![("grant_type", "client_credentials")];
        fields.extend(form);
        invalid_client(&harness.token(&fields, &[]));
    }
    let basic = format!(
        "Basic {}",
        crate::content::base64(format!("{CONFIDENTIAL}:{SECRET}").as_bytes())
    );
    invalid_client(&harness.token(&[kind, assertion], &[("Authorization", basic.as_bytes())]));
    let signed = es256(&header, &merged(base, &json!({"aud": [ISSUER]})));
    assert_eq!(machine(&harness, &signed).status, 200);
}

struct DocumentResolver {
    registration: Value,
    keys: Option<Vec<u8>>,
    fetches: Mutex<Vec<(String, String)>>,
}

impl ClientResolver for DocumentResolver {
    fn resolve(&self, client_id: &str) -> Result<Vec<u8>, String> {
        assert_eq!(client_id, DOCUMENT);
        Ok(self.registration.to_string().into_bytes())
    }

    fn resolve_jwks(&self, client_id: &str, uri: &str) -> Result<Vec<u8>, String> {
        self.fetches
            .lock()
            .unwrap()
            .push((client_id.to_owned(), uri.to_owned()));
        self.keys.clone().ok_or_else(|| "unavailable".into())
    }
}

fn document(metadata: &Value, keys: Option<Vec<u8>>) -> Arc<DocumentResolver> {
    Arc::new(DocumentResolver {
        registration: merged(
            json!({
                "client_id": DOCUMENT, "client_name": "Acme Machine",
                "redirect_uris": [CALLBACK],
                "token_endpoint_auth_method": "private_key_jwt",
            }),
            metadata,
        ),
        keys,
        fetches: Mutex::default(),
    })
}

fn document_request(harness: &Harness, pairs: &[(&str, &str)], jwt: Option<&str>) -> Response {
    let mut form = pairs.to_vec();
    if let Some(jwt) = jwt {
        form.extend(fields(jwt));
    } else {
        form.push(("client_id", DOCUMENT));
    }
    harness.token(&form, &[])
}

fn document_code(harness: &Harness, code: &str, jwt: Option<&str>) -> Response {
    document_request(
        harness,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", CALLBACK),
            ("code_verifier", VERIFIER),
        ],
        jwt,
    )
}

fn document_refresh(harness: &Harness, refresh: &str, jwt: Option<&str>) -> Response {
    document_request(
        harness,
        &[("grant_type", "refresh_token"), ("refresh_token", refresh)],
        jwt,
    )
}

fn document_authorize() -> Vec<(String, String)> {
    authorization(&[("client_id", DOCUMENT), ("redirect_uri", CALLBACK)])
}

#[test]
fn full_document_replay_pool_leaves_configured_and_enterprise_grants_available() {
    let (mut harness, _) = assertions::enterprise_with_store(
        private_client(test_client_jwks()),
        MemoryStore::with_capacity(2).unwrap(),
    );
    let inline = json!({"jwks": {"keys": [ec_jwk("partner-1")]}});
    harness.server = harness.server.with_resolver(document(&inline, None));
    for id in ["document-1", "document-2"] {
        let jwt = test_client_assertion(DOCUMENT, ISSUER, id, T + 300);
        assert_eq!(
            json_body(&machine(&harness, &jwt))["error"],
            "unauthorized_client"
        );
    }
    let jwt = test_client_assertion(DOCUMENT, ISSUER, "document-3", T + 300);
    assert_eq!(machine(&harness, &jwt).status, 503);

    let jwt = test_client_assertion(MACHINE, ISSUER, "configured", T + 300);
    let issued = machine(&harness, &jwt);
    assert_eq!(issued.status, 200, "{issued:?}");
    let body = json_body(&issued);
    let access = body["access_token"].as_str().unwrap();
    let jwt = test_client_assertion(MACHINE, ISSUER, "revoke", T + 300);
    assert_eq!(revoke(&harness, access, &jwt).status, 200);
    assert!(harness.grant(access).is_err());

    let identity = rs256(&json!({}), &assertions::claims(&json!({})));
    let issued = harness.present(&identity, &[]);
    assert_eq!(issued.status, 200, "{issued:?}");
    let body = json_body(&issued);
    let grant = harness
        .grant(body["access_token"].as_str().unwrap())
        .unwrap();
    assert_eq!(grant.subject(), Some("https://idp.example.com#alice"));
}

#[test]
fn an_assertion_cannot_be_reused_in_another_client_pool() {
    let store = MemoryStore::with_capacity(1).unwrap();
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(T);
    let record = AssertionRecord {
        digest: wire::digest("acme-client-assertion"),
        expires_at: now + Duration::from_mins(1),
    };
    assert_eq!(
        store.use_client_assertion(record.clone(), ClientAssertionPool::Document, now),
        Ok(true)
    );
    assert_eq!(
        store.use_client_assertion(record, ClientAssertionPool::Configured, now),
        Ok(false)
    );
}

#[test]
fn replay_capacity_visits_only_due_records() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(T);
    let due = now + Duration::from_mins(1);
    let later = now + Duration::from_mins(2);
    let document = ClientAssertionPool::Document;
    let configured = ClientAssertionPool::Configured;
    for live in [16, 4096] {
        let capacity = live + 5;
        let store = MemoryStore::with_capacity(capacity).unwrap();
        let remember = |pool, index, expires_at, now| {
            let record = AssertionRecord {
                digest: wire::digest(&format!("acme-replay-{index}")),
                expires_at,
            };
            store.use_client_assertion(record, pool, now)
        };
        for index in 0..capacity {
            let expires_at = if index < live { later } else { due };
            assert_eq!(remember(document, index, expires_at, now), Ok(true));
        }
        assert_eq!(remember(configured, capacity, later, now), Ok(true));
        assert_eq!(store.purge_work(), (0, capacity + 1));

        assert_eq!(remember(document, live, later, due), Ok(true));
        assert_eq!(store.purge_work(), (5, live + 2));
        assert_eq!(remember(configured, 0, later, due), Ok(false));
    }
}

#[test]
fn inline_document_keys_authenticate_for_machine_and_enterprise_grants() {
    let keys =
        json!({"jwks": {"keys": [ec_jwk("partner-1")]}, "grant_types": ["client_credentials"]});
    let mut harness = at_test_time(harness());
    harness.server = harness.server.with_resolver(document(&keys, None));
    let signed = test_client_assertion(DOCUMENT, ISSUER, "machine", T + 300);
    assert_eq!(
        json_body(&machine(&harness, &signed))["error"],
        "unauthorized_client"
    );

    let mut options = options();
    options.clients.retain(|client| client.client_id != MACHINE);
    options.document_client_grants.insert(
        DOCUMENT.to_owned(),
        DocumentClientGrants {
            client_credentials: true,
            identity_assertions: true,
        },
    );
    let (mut harness, _) = assertions::enterprise_with(options);
    harness.server = harness.server.with_resolver(document(&keys, None));
    let issued = machine(&harness, &signed);
    assert_eq!(issued.status, 200, "{issued:?}");
    let metadata = harness.metadata();
    assert_eq!(
        metadata["token_endpoint_auth_signing_alg_values_supported"],
        json!(["RS256", "ES256"])
    );
    assert_eq!(
        metadata["revocation_endpoint_auth_methods_supported"],
        metadata["token_endpoint_auth_methods_supported"]
    );
    assert_eq!(
        metadata["grant_types_supported"],
        json!([
            "authorization_code",
            "refresh_token",
            "client_credentials",
            JWT_BEARER_GRANT
        ])
    );

    let client = es256(
        &json!({"typ": "JWT"}),
        &merged(
            claims(DOCUMENT, ISSUER, "enterprise", T + 300),
            &json!({"padding": "x".repeat(5840)}),
        ),
    );
    let identity = rs256(
        &json!({}),
        &assertions::claims(
            &json!({"client_id": DOCUMENT, "jti": "enterprise", "padding": "x".repeat(5535)}),
        ),
    );
    assert!(client.len() <= MAX_ASSERTION_BYTES && identity.len() <= MAX_ASSERTION_BYTES);
    assert!(
        client.len() + identity.len() + JWT_BEARER_GRANT.len() + JWT_CLIENT_ASSERTION_TYPE.len()
            > MAX_FORM_BYTES
    );
    let [kind, assertion] = fields(&client);
    let issued = harness.token(
        &[
            ("grant_type", JWT_BEARER_GRANT),
            ("assertion", &identity),
            kind,
            assertion,
        ],
        &[],
    );
    assert_eq!(issued.status, 200, "{issued:?}");
    let body = json_body(&issued);
    let grant = harness
        .grant(body["access_token"].as_str().unwrap())
        .unwrap();
    assert_eq!(grant.client_id(), Some(DOCUMENT));
    assert_eq!(grant.subject(), Some("https://idp.example.com#alice"));
    assert_eq!(grant.scopes().collect::<Vec<_>>(), ["acme:read"]);
}

struct DelayedKeys {
    clock: Arc<AtomicU64>,
    resolved_at: u64,
    document: Arc<DocumentResolver>,
}

impl ClientResolver for DelayedKeys {
    fn resolve(&self, client_id: &str) -> Result<Vec<u8>, String> {
        self.document.resolve(client_id)
    }

    fn resolve_jwks(&self, client_id: &str, uri: &str) -> Result<Vec<u8>, String> {
        self.clock.store(self.resolved_at, Ordering::Relaxed);
        self.document.resolve_jwks(client_id, uri)
    }
}

fn delayed_keys(clock: &Arc<AtomicU64>, resolved_at: u64) -> Arc<DelayedKeys> {
    Arc::new(DelayedKeys {
        clock: Arc::clone(clock),
        resolved_at,
        document: document(&json!({"jwks_uri": JWKS_URI}), Some(test_client_jwks())),
    })
}

fn with_moving_clock(server: AuthorizationServer, clock: &Arc<AtomicU64>) -> AuthorizationServer {
    let clock = Arc::clone(clock);
    server.with_clock(Arc::new(move || {
        SystemTime::UNIX_EPOCH + Duration::from_secs(clock.load(Ordering::Relaxed))
    }))
}

#[test]
fn an_assertion_that_expires_during_key_resolution_is_refused() {
    let mut options = options();
    options.document_client_grants.insert(
        DOCUMENT.to_owned(),
        DocumentClientGrants {
            client_credentials: true,
            identity_assertions: false,
        },
    );
    let clock = Arc::new(AtomicU64::new(T));
    let mut harness = harness_with(options, MemoryStore::new());
    harness.server =
        with_moving_clock(harness.server, &clock).with_resolver(delayed_keys(&clock, T + 360));
    let jwt = test_client_assertion(DOCUMENT, ISSUER, "expired-on-fetch", T + 300);
    invalid_client(&machine(&harness, &jwt));
    assert!(harness.store.seen.lock().unwrap().is_empty());
}

#[test]
fn grant_expiry_uses_the_clock_after_client_key_resolution() {
    let mut options = options();
    options.lifetimes.refresh_idle = Duration::from_hours(1);
    let clock = Arc::new(AtomicU64::new(T));
    let mut harness = harness_with(options, MemoryStore::new());
    harness.server =
        with_moving_clock(harness.server, &clock).with_resolver(delayed_keys(&clock, T));
    let code = harness.code(&document_authorize());
    let jwt = test_client_assertion(DOCUMENT, ISSUER, "code-issue", T + 300);
    let issued = document_code(&harness, &code, Some(&jwt));
    assert_eq!(issued.status, 200, "{issued:?}");
    let body = json_body(&issued);
    let refresh = body["refresh_token"].as_str().unwrap();
    let code = harness.code(&document_authorize());

    clock.store(T + 59, Ordering::Relaxed);
    harness.server = harness.server.with_resolver(delayed_keys(&clock, T + 60));
    let jwt = test_client_assertion(DOCUMENT, ISSUER, "code-expiry", T + 300);
    assert_eq!(
        json_body(&document_code(&harness, &code, Some(&jwt)))["error"],
        "invalid_grant"
    );

    clock.store(T + 3599, Ordering::Relaxed);
    harness.server = harness.server.with_resolver(delayed_keys(&clock, T + 3600));
    let jwt = test_client_assertion(DOCUMENT, ISSUER, "refresh-expiry", T + 3900);
    assert_eq!(
        json_body(&document_refresh(&harness, refresh, Some(&jwt)))["error"],
        "invalid_grant"
    );
}

#[test]
fn document_authentication_downgrade_preserves_code_and_family_requirements() {
    let mut harness = at_test_time(harness());
    let inline = json!({"jwks": {"keys": [ec_jwk("partner-1")]}});
    harness.server = harness.server.with_resolver(document(&inline, None));
    let redeemed = harness.code(&document_authorize());
    let jwt = test_client_assertion(DOCUMENT, ISSUER, "code-redeem", T + 300);
    let issued = document_code(&harness, &redeemed, Some(&jwt));
    assert_eq!(issued.status, 200, "{issued:?}");
    let body = json_body(&issued);
    let refresh = body["refresh_token"].as_str().unwrap();
    let access = body["access_token"].as_str().unwrap();
    let (pending, cookie) = harness.start(&document_authorize());

    harness.server = harness.server.with_resolver(document(
        &json!({"token_endpoint_auth_method": "none"}),
        None,
    ));
    let (_, code) = location(&harness.decide(&pending, &cookie, "approve"));
    let code = &code["code"];
    for response in [
        document_code(&harness, code, None),
        document_code(&harness, &redeemed, None),
        document_refresh(&harness, refresh, None),
    ] {
        assert_eq!(json_body(&response)["error"], "invalid_grant");
    }
    assert_eq!(
        harness
            .post_form(
                "/revoke",
                &[("token", refresh), ("client_id", DOCUMENT)],
                &[]
            )
            .status,
        200
    );
    assert!(harness.grant(access).is_ok());

    harness.server = harness.server.with_resolver(document(&inline, None));
    let jwt = test_client_assertion(DOCUMENT, ISSUER, "code-retry", T + 300);
    assert_eq!(document_code(&harness, code, Some(&jwt)).status, 200);
    let jwt = test_client_assertion(DOCUMENT, ISSUER, "refresh-retry", T + 300);
    assert_eq!(document_refresh(&harness, refresh, Some(&jwt)).status, 200);
}

#[test]
fn only_the_registered_https_key_url_can_supply_remote_client_keys() {
    let mut options = options();
    options.document_client_grants.insert(
        DOCUMENT.to_owned(),
        DocumentClientGrants {
            client_credentials: true,
            identity_assertions: false,
        },
    );
    let mut harness = at_test_time(harness_with(options, MemoryStore::new()));
    let jwt = es256(
        &json!({"typ": "JWT", "jku": "https://other.example.com/jwks"}),
        &claims(DOCUMENT, ISSUER, "remote", T + 300),
    );
    for metadata in [
        json!({}),
        json!({"jwks_uri": "http://keys.example.com/jwks"}),
        json!({"jwks_uri": "https://user@keys.example.com/jwks"}),
        json!({"jwks_uri": JWKS_URI, "jwks": {"keys": [ec_jwk("partner-1")]}}),
    ] {
        let resolver = document(&metadata, None);
        harness.server = harness.server.with_resolver(resolver.clone());
        invalid_client(&machine(&harness, &jwt));
        assert!(resolver.fetches.lock().unwrap().is_empty());
    }
    let remote = json!({"jwks_uri": JWKS_URI});
    harness.server = harness.server.with_resolver(document(&remote, None));
    assert_eq!(machine(&harness, &jwt).status, 503);
    for bytes in [b"{".to_vec(), vec![b' '; MAX_KEY_SET_BYTES + 1]] {
        harness.server = harness.server.with_resolver(document(&remote, Some(bytes)));
        invalid_client(&machine(&harness, &jwt));
    }
    let resolver = document(&remote, Some(test_client_jwks()));
    harness.server = harness.server.with_resolver(resolver.clone());
    assert_eq!(machine(&harness, &jwt).status, 200);
    assert_eq!(
        *resolver.fetches.lock().unwrap(),
        [(DOCUMENT.to_owned(), JWKS_URI.to_owned())]
    );
}

#[test]
fn concurrent_client_assertion_use_has_one_winner_and_capacity_fails_closed() {
    let harness = at_test_time(harness_with(
        private_client(test_client_jwks()),
        MemoryStore::with_capacity(1).unwrap(),
    ));
    let signed = test_client_assertion(MACHINE, ISSUER, "race", T + 300);
    let start = Barrier::new(2);
    let mut statuses = std::thread::scope(|scope| {
        let request = || {
            start.wait();
            machine(&harness, &signed).status
        };
        let first = scope.spawn(request);
        [request(), first.join().unwrap()]
    });
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 401]);
    assert_eq!(
        machine(
            &harness,
            &test_client_assertion(MACHINE, ISSUER, "full", T + 300)
        )
        .status,
        503
    );
    invalid_client(&machine(&harness, &signed));
}
