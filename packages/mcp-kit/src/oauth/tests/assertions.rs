//! Enterprise identity assertions through the token endpoint: RS256 and ES256
//! signatures, claim and header refusals, key rotation and preparation,
//! client and consumer policy, replay, per-user grant bounds and configuration.
use super::*;
use p256::ecdsa::{SigningKey as EcSigningKey, signature::Signer as _};
use rsa::{
    PrivateKey, pkcs1v15::SigningKey as RsaSigningKey, pkcs8::DecodePrivateKey as _, sha2::Sha256,
    signature::SignatureEncoding as _, traits::PublicKeyParts as _,
};
use std::sync::atomic::Ordering;

const IDP: &str = "https://idp.example.com";
/// A multi-tenant issuer that signs with ES256 only.
const PARTNER: &str = "https://partner.example.com";
const AGENT: &str = "acme-agent";
const AGENT_SECRET: &str = "acme-agent-secret-0123456789abcdef";
const RSA_KEY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/oauth/acme-idp-rs256.der"
));
const EC_SCALAR: [u8; 32] = [0x11; 32];
pub(super) const T: u64 = 1_900_000_000;

/// The subject, email and offered scopes one admission saw.
type Admitted = (String, Option<String>, Vec<String>);

/// Serves each issuer's key set, replacing it with the refreshed one on a
/// refresh, and approves users as `{issuer}#{sub}` unless told otherwise.
#[derive(Default)]
pub(super) struct Enterprise {
    current: Mutex<HashMap<String, Vec<u8>>>,
    refreshed: Mutex<HashMap<String, Vec<u8>>>,
    fetches: Mutex<Vec<(String, bool)>>,
    down: Mutex<bool>,
    decision: Mutex<Option<AssertionDecision>>,
    seen: Mutex<Vec<Admitted>>,
}

impl IdentityAssertions for Enterprise {
    fn key_set(&self, issuer: &str, refresh: bool) -> Result<Vec<u8>, String> {
        self.fetches
            .lock()
            .unwrap()
            .push((issuer.to_owned(), refresh));
        if *self.down.lock().unwrap() {
            return Err("the identity provider is unreachable".to_owned());
        }
        if refresh && let Some(set) = self.refreshed.lock().unwrap().remove(issuer) {
            self.current.lock().unwrap().insert(issuer.to_owned(), set);
        }
        Ok(self.current.lock().unwrap()[issuer].clone())
    }

    fn admit(&self, assertion: &IdentityAssertion<'_>) -> AssertionDecision {
        self.seen.lock().unwrap().push((
            assertion.subject.to_owned(),
            assertion.email.map(str::to_owned),
            assertion.scopes.to_vec(),
        ));
        self.decision
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| AssertionDecision::Approve {
                subject: format!("{}#{}", assertion.issuer, assertion.subject),
                scopes: assertion.scopes.to_vec(),
            })
    }
}

fn rsa_private() -> PrivateKey {
    PrivateKey::from_pkcs8_der(RSA_KEY).unwrap()
}

fn ec_signer() -> EcSigningKey {
    EcSigningKey::from_slice(&EC_SCALAR).unwrap()
}

pub(super) fn rsa_jwk(kid: &str) -> Value {
    let key = rsa_private();
    json!({
        "kty": "RSA",
        "kid": kid,
        "use": "sig",
        "alg": "RS256",
        "n": wire::base64url(&key.n_bytes()),
        "e": wire::base64url(&key.e().to_be_bytes_trimmed_vartime()),
    })
}

pub(super) fn ec_jwk(kid: &str) -> Value {
    let point = ec_signer().verifying_key().to_sec1_point(false);
    let bytes = point.as_bytes();
    json!({
        "kty": "EC",
        "crv": "P-256",
        "kid": kid,
        "x": wire::base64url(&bytes[1..33]),
        "y": wire::base64url(&bytes[33..65]),
    })
}

pub(super) fn key_set(keys: &[Value]) -> Vec<u8> {
    serde_json::to_vec(&json!({ "keys": keys })).unwrap()
}

/// `base` with `changes` applied; a `null` change removes the member.
pub(super) fn merged(base: Value, changes: &Value) -> Value {
    let (Value::Object(mut base), Value::Object(changes)) = (base, changes.clone()) else {
        panic!("objects only");
    };
    for (name, value) in changes {
        if value.is_null() {
            base.remove(&name);
        } else {
            base.insert(name, value);
        }
    }
    Value::Object(base)
}

pub(super) fn claims(changes: &Value) -> Value {
    merged(
        json!({
            "iss": IDP,
            "sub": "alice",
            "aud": ISSUER,
            "client_id": AGENT,
            "resource": RESOURCE,
            "scope": "acme:read",
            "iat": T,
            "exp": T + 300,
            "jti": "jag-1",
            "email": "alice@example.com",
        }),
        changes,
    )
}

/// A compact JWS of `header` and `claims`, signed by `sign`.
fn jwt(header: &Value, claims: &Value, sign: impl Fn(&[u8]) -> Vec<u8>) -> String {
    let input = format!(
        "{}.{}",
        wire::base64url(&serde_json::to_vec(header).unwrap()),
        wire::base64url(&serde_json::to_vec(claims).unwrap())
    );
    format!("{input}.{}", wire::base64url(&sign(input.as_bytes())))
}

pub(super) fn rs256(header: &Value, claims: &Value) -> String {
    let signer = RsaSigningKey::<Sha256>::new(rsa_private());
    let header = merged(
        json!({"alg": "RS256", "typ": "oauth-id-jag+jwt", "kid": "idp-1"}),
        header,
    );
    jwt(&header, claims, |input| signer.sign(input).to_vec())
}

pub(super) fn es256(header: &Value, claims: &Value) -> String {
    let signer = ec_signer();
    let header = merged(
        json!({"alg": "ES256", "typ": "oauth-id-jag+jwt", "kid": "partner-1"}),
        header,
    );
    jwt(&header, claims, |input| {
        let signature: p256::ecdsa::Signature = signer.sign(input);
        signature.to_bytes().to_vec()
    })
}

fn trusted() -> Vec<TrustedIssuer> {
    vec![
        TrustedIssuer {
            algorithms: vec![SigningAlgorithm::Rs256],
            ..TrustedIssuer::new(IDP)
        },
        TrustedIssuer {
            algorithms: vec![SigningAlgorithm::Es256],
            tenant: Some("acme".to_owned()),
            ..TrustedIssuer::new(PARTNER)
        },
    ]
}

fn agent() -> ClientOptions {
    ClientOptions {
        client_id: AGENT.to_owned(),
        client_name: None,
        redirect_uris: Vec::new(),
        scopes: vec!["acme:read".to_owned(), "acme:write".to_owned()],
        auth: ClientAuth::SecretBasic,
        secret: Some(AGENT_SECRET.to_owned()),
        client_credentials: false,
        identity_assertions: true,
    }
}

/// A harness at time `T` whose server trusts [`trusted`] issuers.
pub(super) fn enterprise_with(options: AuthorizationServerOptions) -> (Harness, Arc<Enterprise>) {
    enterprise_with_store(options, MemoryStore::new())
}

pub(super) fn enterprise_with_store(
    mut options: AuthorizationServerOptions,
    memory: MemoryStore,
) -> (Harness, Arc<Enterprise>) {
    options.clients.push(agent());
    let enterprise = Arc::new(Enterprise::default());
    enterprise.current.lock().unwrap().extend([
        (IDP.to_owned(), key_set(&[rsa_jwk("idp-1")])),
        (PARTNER.to_owned(), key_set(&[ec_jwk("partner-1")])),
    ]);
    let Harness {
        server,
        store,
        sign_in,
        issued,
    } = harness_with(options, memory);
    let server = server
        .with_clock(Arc::new(|| SystemTime::UNIX_EPOCH + Duration::from_secs(T)))
        .with_identity_assertions(
            trusted(),
            Arc::clone(&enterprise) as Arc<dyn IdentityAssertions>,
        )
        .unwrap();
    let harness = Harness {
        server,
        store,
        sign_in,
        issued,
    };
    (harness, enterprise)
}

fn enterprise() -> (Harness, Arc<Enterprise>) {
    enterprise_with(options())
}

impl Harness {
    pub(super) fn present(&self, assertion: &str, extra: &[(&str, &str)]) -> Response {
        let mut fields = vec![
            ("grant_type", JWT_BEARER_GRANT),
            ("assertion", assertion),
            ("client_id", AGENT),
            ("client_secret", AGENT_SECRET),
        ];
        fields.extend_from_slice(extra);
        self.token(&fields, &[])
    }

    pub(super) fn metadata(&self) -> Value {
        json_body(&self.send("GET", METADATA_PREFIX, &[], b""))
    }
}

fn error(response: &Response) -> (u16, String) {
    (
        response.status,
        json_body(response)["error"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    )
}

#[test]
fn identity_assertions_issue_access_tokens_without_refresh_tokens() {
    let (harness, enterprise) = enterprise();
    let metadata = harness.metadata();
    assert!(
        metadata["grant_types_supported"]
            .as_array()
            .unwrap()
            .contains(&json!(JWT_BEARER_GRANT))
    );
    assert_eq!(
        metadata["authorization_grant_profiles_supported"],
        json!([ID_JAG_PROFILE])
    );

    let assertion = rs256(&json!({}), &claims(&json!({})));
    let response = harness.present(&assertion, &[]);
    assert_eq!(response.status, 200, "{response:?}");
    let body = json_body(&response);
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(body["scope"], "acme:read");
    assert_eq!(body["expires_in"], 3600);
    assert!(body.get("refresh_token").is_none());
    assert_eq!(header(&response, "Cache-Control"), Some("no-store"));

    let grant = harness
        .grant(body["access_token"].as_str().unwrap())
        .unwrap();
    assert_eq!(grant.subject(), Some("https://idp.example.com#alice"));
    assert_eq!(grant.client_id(), Some(AGENT));
    assert_eq!(grant.scopes().collect::<Vec<_>>(), ["acme:read"]);
    assert_eq!(
        enterprise.seen.lock().unwrap()[0],
        (
            "alice".to_owned(),
            Some("alice@example.com".to_owned()),
            vec!["acme:read".to_owned()]
        )
    );

    // The same assertion is accepted once, and a replay never reaches admission.
    assert_eq!(
        error(&harness.present(&assertion, &[])),
        (400, "invalid_grant".to_owned())
    );
    assert_eq!(enterprise.seen.lock().unwrap().len(), 1);

    // A multi-tenant ES256 issuer, with an array audience and resource.
    let partner = es256(
        &json!({}),
        &claims(&json!({
            "iss": PARTNER,
            "aud": [ISSUER],
            "resource": [RESOURCE],
            "tenant": "acme",
        })),
    );
    assert_eq!(harness.present(&partner, &[]).status, 200);

    // The store holds digests only: no assertion, key or token value.
    let seen = harness.store.seen.lock().unwrap().join("\n");
    let issued = harness.issued.lock().unwrap();
    for secret in
        issued
            .iter()
            .map(String::as_str)
            .chain([assertion.as_str(), partner.as_str(), "jag-1"])
    {
        assert!(!seen.contains(secret), "{secret} reached the store");
    }
}

#[test]
fn identity_assertions_refuse_invalid_signatures_headers_and_claims() {
    let (harness, _) = enterprise();
    for (case, assertion) in refused_assertions().into_iter().chain(refused_claims()) {
        assert_eq!(
            error(&harness.present(&assertion, &[])),
            (400, "invalid_grant".to_owned()),
            "{case}"
        );
    }
    // Nothing was admitted or stored.
    assert!(harness.store.seen.lock().unwrap().is_empty());
}

/// Assertions whose form, issuer, header or signature the server must refuse.
fn refused_assertions() -> Vec<(&'static str, String)> {
    let forged = {
        let valid = rs256(&json!({}), &claims(&json!({})));
        let (input, _) = valid.rsplit_once('.').unwrap();
        let (header, _) = input.split_once('.').unwrap();
        let payload =
            wire::base64url(&serde_json::to_vec(&claims(&json!({"sub": "mallory"}))).unwrap());
        let signature = valid.rsplit_once('.').unwrap().1;
        format!("{header}.{payload}.{signature}")
    };
    vec![
        ("tampered payload", forged),
        ("not a JWT", "acme".to_owned()),
        (
            "encrypted form",
            format!("{}.e.i.c.t", rs256(&json!({}), &claims(&json!({})))),
        ),
        (
            "untrusted issuer",
            rs256(
                &json!({}),
                &claims(&json!({"iss": "https://evil.example.com"})),
            ),
        ),
        (
            "ordinary JWT type",
            rs256(&json!({"typ": "JWT"}), &claims(&json!({}))),
        ),
        (
            "missing type",
            rs256(&json!({"typ": null}), &claims(&json!({}))),
        ),
        (
            "critical header",
            rs256(&json!({"crit": ["exp"]}), &claims(&json!({}))),
        ),
        (
            "algorithm the issuer does not use",
            es256(&json!({"kid": "idp-1"}), &claims(&json!({}))),
        ),
        (
            "unsigned",
            jwt(
                &json!({"alg": "none", "typ": "oauth-id-jag+jwt"}),
                &claims(&json!({})),
                |_| Vec::new(),
            ),
        ),
    ]
}

/// Validly signed assertions whose claims the server must refuse.
fn refused_claims() -> Vec<(&'static str, String)> {
    vec![
        (
            "another authorization server",
            rs256(
                &json!({}),
                &claims(&json!({"aud": "https://other.example.com"})),
            ),
        ),
        (
            "two audiences",
            rs256(&json!({}), &claims(&json!({"aud": [ISSUER, ISSUER]}))),
        ),
        (
            "another client",
            rs256(&json!({}), &claims(&json!({"client_id": CONFIDENTIAL}))),
        ),
        (
            "another resource",
            rs256(
                &json!({}),
                &claims(&json!({"resource": "https://mcp.example.com/other"})),
            ),
        ),
        (
            "expired",
            rs256(&json!({}), &claims(&json!({"iat": T - 600, "exp": T - 61}))),
        ),
        (
            "too long-lived",
            rs256(&json!({}), &claims(&json!({"exp": T + 3601}))),
        ),
        (
            "issued in the future",
            rs256(&json!({}), &claims(&json!({"iat": T + 61}))),
        ),
        (
            "not yet valid",
            rs256(&json!({}), &claims(&json!({"nbf": T + 61}))),
        ),
        (
            "missing exp",
            rs256(&json!({}), &claims(&json!({"exp": null}))),
        ),
        (
            "missing jti",
            rs256(&json!({}), &claims(&json!({"jti": null}))),
        ),
        (
            "missing sub",
            rs256(&json!({}), &claims(&json!({"sub": ""}))),
        ),
        (
            "invalid scope",
            rs256(&json!({}), &claims(&json!({"scope": "acme read\n"}))),
        ),
        (
            "sender-constrained",
            rs256(&json!({}), &claims(&json!({"cnf": {"jkt": "acme"}}))),
        ),
        (
            "wrong tenant",
            es256(
                &json!({}),
                &claims(&json!({"iss": PARTNER, "tenant": "globex"})),
            ),
        ),
        (
            "missing tenant",
            es256(&json!({}), &claims(&json!({"iss": PARTNER}))),
        ),
    ]
}

#[test]
fn identity_assertions_follow_rotated_keys_and_prepare_each_key_set_once() {
    let (harness, enterprise) = enterprise();
    // The provider rotated to a key the cached set does not have yet.
    enterprise
        .current
        .lock()
        .unwrap()
        .insert(IDP.to_owned(), key_set(&[ec_jwk("idp-0")]));
    enterprise
        .refreshed
        .lock()
        .unwrap()
        .insert(IDP.to_owned(), key_set(&[rsa_jwk("idp-1")]));
    for jti in ["jag-1", "jag-2", "jag-3", "jag-4"] {
        let assertion = rs256(&json!({}), &claims(&json!({"jti": jti})));
        assert_eq!(harness.present(&assertion, &[]).status, 200, "{jti}");
    }
    assert_eq!(
        enterprise.fetches.lock().unwrap()[..3],
        [
            (IDP.to_owned(), false),
            (IDP.to_owned(), true),
            (IDP.to_owned(), false),
        ]
    );
    // The old and the rotated set, each once, however many assertions follow.
    let assertions = harness.server.assertions.as_ref().unwrap();
    assert_eq!(assertions.prepared.load(Ordering::Relaxed), 2);

    // A key the refreshed set still lacks, a signature by another key, and
    // an unreachable provider.
    let unknown = rs256(&json!({"kid": "idp-9"}), &claims(&json!({"jti": "jag-5"})));
    assert_eq!(
        error(&harness.present(&unknown, &[])),
        (400, "invalid_grant".to_owned())
    );
    let signer = ec_signer();
    let wrong_key = jwt(
        &json!({"alg": "RS256", "typ": "oauth-id-jag+jwt", "kid": "idp-1"}),
        &claims(&json!({"jti": "jag-6"})),
        |input| {
            let signature: p256::ecdsa::Signature = signer.sign(input);
            signature.to_bytes().to_vec()
        },
    );
    assert_eq!(
        error(&harness.present(&wrong_key, &[])),
        (400, "invalid_grant".to_owned())
    );
    *enterprise.down.lock().unwrap() = true;
    let assertion = rs256(&json!({}), &claims(&json!({"jti": "jag-7"})));
    assert_eq!(harness.present(&assertion, &[]).status, 503);
}

#[test]
fn identity_assertions_follow_client_and_consumer_policy() {
    let (harness, enterprise) = enterprise();
    let assertion = |changes: &Value| rs256(&json!({}), &claims(changes));

    // A public client cannot authenticate; a confidential client without the
    // option is not allowed.
    let response = harness.token(
        &[
            ("grant_type", JWT_BEARER_GRANT),
            ("assertion", &assertion(&json!({"client_id": PUBLIC}))),
            ("client_id", PUBLIC),
        ],
        &[],
    );
    assert_eq!(error(&response), (401, "invalid_client".to_owned()));
    let response = harness.token(
        &[
            ("grant_type", JWT_BEARER_GRANT),
            ("assertion", &assertion(&json!({"client_id": CONFIDENTIAL}))),
            ("client_id", CONFIDENTIAL),
            ("client_secret", SECRET),
        ],
        &[],
    );
    assert_eq!(error(&response), (400, "unauthorized_client".to_owned()));

    // Scopes: the assertion's, limited to the client's, narrowed by the form.
    let wide = assertion(&json!({"jti": "jag-2", "scope": "acme:read acme:write acme:admin"}));
    let response = harness.present(&wide, &[("scope", "acme:write")]);
    assert_eq!(json_body(&response)["scope"], "acme:write");
    let all = assertion(&json!({"jti": "jag-3", "scope": "acme:read acme:write acme:admin"}));
    assert_eq!(
        json_body(&harness.present(&all, &[]))["scope"],
        "acme:read acme:write"
    );
    assert_eq!(
        enterprise.seen.lock().unwrap()[1].2,
        ["acme:read", "acme:write"]
    );
    let widened = assertion(&json!({"jti": "jag-4"}));
    assert_eq!(
        error(&harness.present(&widened, &[("scope", "acme:write")])),
        (400, "invalid_scope".to_owned())
    );

    // The consumer denies, or answers outside the offered scopes.
    *enterprise.decision.lock().unwrap() = Some(AssertionDecision::Deny);
    assert_eq!(
        error(&harness.present(&assertion(&json!({"jti": "jag-5"})), &[])),
        (400, "access_denied".to_owned())
    );
    *enterprise.decision.lock().unwrap() = Some(AssertionDecision::Approve {
        subject: "alice".to_owned(),
        scopes: vec!["acme:admin".to_owned()],
    });
    assert_eq!(
        error(&harness.present(&assertion(&json!({"jti": "jag-6"})), &[])),
        (500, "server_error".to_owned())
    );

    // Without trusted issuers the grant is neither listed nor served.
    let mut options = options();
    options.clients.push(agent());
    let plain = harness_with(options, MemoryStore::new());
    let metadata = plain.metadata();
    assert!(
        metadata
            .get("authorization_grant_profiles_supported")
            .is_none()
    );
    assert!(
        !metadata["grant_types_supported"]
            .as_array()
            .unwrap()
            .contains(&json!(JWT_BEARER_GRANT))
    );
    assert_eq!(
        error(&plain.present(&assertion(&json!({"jti": "jag-7"})), &[])),
        (400, "unsupported_grant_type".to_owned())
    );
}

#[test]
fn identity_assertion_grants_are_bounded_per_user() {
    let mut options = options();
    options.client_grants = 2;
    let (harness, _) = enterprise_with(options);
    let token = |sub: &str, jti: &str| {
        let assertion = rs256(&json!({}), &claims(&json!({"sub": sub, "jti": jti})));
        json_body(&harness.present(&assertion, &[]))["access_token"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let bob = token("bob", "jag-b");
    let first = token("alice", "jag-1");
    let second = token("alice", "jag-2");
    let third = token("alice", "jag-3");
    assert!(
        harness.grant(&first).is_err(),
        "the oldest of alice's grants ends"
    );
    for live in [&second, &third, &bob] {
        assert!(harness.grant(live).is_ok());
    }
}

#[test]
fn identity_assertion_configuration_is_validated() {
    let build = |issuers: Vec<TrustedIssuer>| {
        harness_with(options(), MemoryStore::new())
            .server
            .with_identity_assertions(issuers, Arc::new(Enterprise::default()))
            .map(|_| ())
    };
    let refused = |index| Err(AuthorizationServerError::TrustedIssuer(index));
    assert_eq!(build(Vec::new()), refused(0));
    assert_eq!(build(vec![TrustedIssuer::new(ISSUER)]), refused(0));
    assert_eq!(
        build(vec![TrustedIssuer::new(IDP), TrustedIssuer::new(IDP)]),
        refused(1)
    );
    assert_eq!(
        build(vec![TrustedIssuer::new("http://idp.example.com")]),
        refused(0)
    );
    assert_eq!(
        build(vec![TrustedIssuer {
            algorithms: Vec::new(),
            ..TrustedIssuer::new(IDP)
        }]),
        refused(0)
    );
    assert_eq!(
        build(vec![TrustedIssuer {
            tenant: Some(String::new()),
            ..TrustedIssuer::new(IDP)
        }]),
        refused(0)
    );
    assert_eq!(
        build(vec![TrustedIssuer::new("http://127.0.0.1:9000/idp")]),
        Ok(())
    );

    // Only confidential clients may present assertions.
    let mut options = options();
    options.clients.push(ClientOptions {
        auth: ClientAuth::None,
        secret: None,
        ..agent()
    });
    let index = options.clients.len() - 1;
    let public_agent = AuthorizationServer::new(
        options,
        &resource(),
        Arc::new(MemoryStore::new()),
        Arc::new(SignIn::default()),
    );
    assert_eq!(
        public_agent.err(),
        Some(AuthorizationServerError::Client(index))
    );
}

#[test]
fn key_sets_keep_only_usable_signing_keys() {
    let usable = |keys: &[Value]| jose::key_set(&key_set(keys)).unwrap().len();
    let mut weak = rsa_jwk("weak");
    let modulus = rsa_private().n_bytes();
    weak["n"] = json!(wire::base64url(&modulus[..128]));
    assert_eq!(
        usable(&[
            rsa_jwk("idp-1"),
            ec_jwk("partner-1"),
            merged(rsa_jwk("encryption"), &json!({"use": "enc"})),
            merged(rsa_jwk("verify-less"), &json!({"key_ops": ["sign"]})),
            merged(rsa_jwk("mislabeled"), &json!({"alg": "ES256"})),
            merged(ec_jwk("p384"), &json!({"crv": "P-384"})),
            merged(ec_jwk("short"), &json!({"x": "AAAA"})),
            json!({"kty": "oct", "k": "AAAA"}),
            weak,
        ]),
        2
    );
    let many = vec![ec_jwk("partner-1"); MAX_KEYS + 1];
    assert_eq!(
        jose::key_set(&key_set(&many)).err(),
        Some(jose::KeySetError::TooLarge)
    );
    assert_eq!(
        jose::key_set(b"[]").err(),
        Some(jose::KeySetError::Malformed)
    );
}
