//! The embedded authorization server through the driver: metadata, sign-in,
//! token grants, revocation and MCP calls with the issued tokens.
use super::*;
use crate::{
    http::{ResourceOptions, ResourceServer, TokenVerifier, auth::MAX_GRANT_IDENTITY_BYTES},
    oauth::{
        AssertionDecision, AuthorizationServer, AuthorizationServerOptions, ClientAuth,
        ClientOptions, ClientResolver, DocumentClientGrants, IdentityAssertion, IdentityAssertions,
        Interaction, InteractionOutcome, InteractionRequest, JWT_BEARER_GRANT,
        JWT_CLIENT_ASSERTION_TYPE, MemoryStore, TrustedIssuer, test_client_assertion,
        test_client_jwks, test_identity_assertion, test_identity_jwks,
    },
};
use std::time::SystemTime;

const ISSUER: &str = "http://127.0.0.1";
const RESOURCE: &str = "http://127.0.0.1/mcp";
const CLIENT_ID: &str = "acme-desktop";
const REDIRECT: &str = "http://127.0.0.1:7777/callback";
const VERIFIER: &str = "acme-verifier-0123456789-0123456789-0123456789";

/// Approves every POST that names a pending authorization.
struct Approve;

impl Interaction for Approve {
    fn interact(&self, request: &InteractionRequest<'_>) -> InteractionOutcome {
        match request.pending {
            Some(pending) if request.method == "POST" => InteractionOutcome::Approve {
                subject: "user-1".to_owned(),
                scopes: pending.scopes.clone(),
            },
            _ => InteractionOutcome::Respond(Response::json(200, b"{}".to_vec())),
        }
    }
}

fn embedded() -> HttpOptions {
    embedded_at("/oauth/sign-in", true)
}

/// The embedded server with sign-in at `interaction`, serving the root
/// protected-resource metadata when `root_metadata` is set.
fn embedded_at(interaction: &str, root_metadata: bool) -> HttpOptions {
    embedded_with(interaction, root_metadata, |_| {}, std::convert::identity)
}

fn embedded_with(
    interaction: &str,
    root_metadata: bool,
    configure: impl FnOnce(&mut AuthorizationServerOptions),
    install_hooks: impl FnOnce(AuthorizationServer) -> AuthorizationServer,
) -> HttpOptions {
    let mut resource = ResourceOptions::new(RESOURCE, vec![ISSUER.to_owned()]);
    resource.serve_root_metadata = root_metadata;
    let resource = ResourceServer::new(resource).unwrap();
    let mut options = AuthorizationServerOptions::new(ISSUER, interaction);
    options.default_scopes = vec!["acme:read".to_owned()];
    options.clients = vec![ClientOptions {
        client_id: CLIENT_ID.to_owned(),
        client_name: None,
        redirect_uris: vec![REDIRECT.to_owned()],
        scopes: vec!["acme:read".to_owned()],
        auth: ClientAuth::None,
        secret: None,
        client_credentials: false,
        identity_assertions: false,
    }];
    configure(&mut options);
    let server = AuthorizationServer::new(
        options,
        &resource,
        Arc::new(MemoryStore::new()),
        Arc::new(Approve),
    )
    .unwrap();
    let server = Arc::new(install_hooks(server));
    HttpOptions {
        authorization: Some(HttpAuthorization::new(
            resource,
            Arc::clone(&server) as Arc<dyn TokenVerifier>,
        )),
        authorization_server: Some(server),
        ..http()
    }
}

fn private_key_client(options: &mut AuthorizationServerOptions) -> &mut ClientOptions {
    let client = &mut options.clients[0];
    client.auth = ClientAuth::PrivateKeyJwt;
    options
        .client_jwks
        .insert(client.client_id.clone(), test_client_jwks());
    client
}

/// The value after `name=` in a header or query.
fn value_of<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text.find(&format!("{name}=")).unwrap() + name.len() + 1;
    text[start..].split(['&', ';']).next().unwrap()
}

fn form_post(path: &str, body: &str, extra: &str) -> Vec<u8> {
    format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/x-www-form-urlencoded\r\n\
         {extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn whoami(server: &Server, access: &str) -> Reply {
    server.exchange(&call(
        &json!(1),
        "acme_whoami",
        &json!({}),
        &format!("Authorization: Bearer {access}\r\n"),
    ))
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn client_auth(client_id: &str, id: &str) -> String {
    let jwt = test_client_assertion(client_id, ISSUER, id, unix_seconds() + 300);
    format!("client_assertion_type={JWT_CLIENT_ASSERTION_TYPE}&client_assertion={jwt}")
}

#[test]
fn a_client_signs_in_and_calls_a_tool_on_one_listener() {
    let server = Server::start(embedded(), None);
    let metadata = server.exchange(
        b"GET /.well-known/oauth-authorization-server HTTP/1.1\r\nHost: localhost\r\n\r\n",
    );
    assert_eq!(metadata.json()["token_endpoint"], "http://127.0.0.1/token");

    let encoded = {
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(VERIFIER.as_bytes());
        crate::content::base64(&digest)
            .trim_end_matches('=')
            .replace('+', "-")
            .replace('/', "_")
    };
    let started = server.exchange(
        format!(
            "GET /authorize?response_type=code&client_id=acme-desktop&redirect_uri={}&\
             code_challenge={encoded}&code_challenge_method=S256&state=s1 HTTP/1.1\r\n\
             Host: localhost\r\n\r\n",
            "http%3A%2F%2F127.0.0.1%3A7777%2Fcallback"
        )
        .as_bytes(),
    );
    assert_eq!(started.status, 303);
    let reference = value_of(started.header("location").unwrap(), "pending").to_owned();
    let cookie = started.header("set-cookie").unwrap();
    // A loopback http issuer gets a cookie without `__Host-` and `Secure`.
    assert!(cookie.starts_with("mcp-pending-pr_") && !cookie.contains("Secure"));
    let cookie = cookie.split(';').next().unwrap().to_owned();

    let approved = server.exchange(&form_post(
        "/oauth/sign-in",
        &format!("pending={reference}"),
        &format!("Cookie: {cookie}\r\n"),
    ));
    assert_eq!(approved.status, 303);
    let location = approved.header("location").unwrap();
    assert!(location.starts_with(REDIRECT));
    let code = value_of(location, "code").to_owned();

    let tokens = server.exchange(&form_post(
        "/token",
        &format!(
            "grant_type=authorization_code&client_id=acme-desktop&code={code}&\
             redirect_uri=http%3A%2F%2F127.0.0.1%3A7777%2Fcallback&code_verifier={VERIFIER}"
        ),
        "",
    ));
    assert_eq!(tokens.status, 200);
    assert_eq!(tokens.header("access-control-allow-origin"), Some("*"));
    let access = tokens.json()["access_token"].as_str().unwrap().to_owned();

    let reply = whoami(&server, &access);
    assert_eq!(reply.json()["result"]["content"][0]["text"], "user-1");
}

#[test]
fn a_private_key_client_uses_token_and_revocation_on_one_listener() {
    let server = Server::start(
        embedded_with(
            "/oauth/sign-in",
            true,
            |options| {
                private_key_client(options).client_credentials = true;
            },
            std::convert::identity,
        ),
        None,
    );
    let metadata = server
        .exchange(
            b"GET /.well-known/oauth-authorization-server HTTP/1.1\r\nHost: localhost\r\n\r\n",
        )
        .json();
    assert_eq!(
        metadata["token_endpoint_auth_methods_supported"],
        json!(["private_key_jwt"])
    );
    assert_eq!(
        metadata["token_endpoint_auth_signing_alg_values_supported"],
        json!(["RS256", "ES256"])
    );
    let issued = server.exchange(&form_post(
        "/token",
        &format!(
            "grant_type=client_credentials&{}",
            client_auth(CLIENT_ID, "issue")
        ),
        "",
    ));
    assert_eq!(issued.status, 200);
    let body = issued.json();
    let access = body["access_token"].as_str().unwrap();
    assert_eq!(
        whoami(&server, access).json()["result"]["content"][0]["text"],
        CLIENT_ID
    );

    let revoke = |id| {
        server.exchange(&form_post(
            "/revoke",
            &format!("token={access}&{}", client_auth(CLIENT_ID, id)),
            "",
        ))
    };
    assert_eq!(revoke("issue").json()["error"], "invalid_client");
    assert_eq!(revoke("revoke").status, 200);
    assert_eq!(whoami(&server, access).status, 401);
}

#[test]
fn an_enterprise_assertion_authorizes_a_tool_on_one_listener() {
    const IDP: &str = "https://idp.example.com";
    const SUBJECT: &str = "acme-employee-42";

    struct Directory(Vec<u8>);

    impl IdentityAssertions for Directory {
        fn key_set(&self, issuer: &str, _: bool) -> Result<Vec<u8>, String> {
            assert_eq!(issuer, IDP);
            Ok(self.0.clone())
        }

        fn admit(&self, assertion: &IdentityAssertion<'_>) -> AssertionDecision {
            assert_eq!(assertion.subject, "alice");
            assert_eq!(assertion.scopes, ["acme:read", "acme:write"]);
            AssertionDecision::Approve {
                subject: SUBJECT.to_owned(),
                scopes: vec!["acme:read".to_owned()],
            }
        }
    }

    let server = Server::start(
        embedded_with(
            "/oauth/sign-in",
            true,
            |options| {
                let client = private_key_client(options);
                client.identity_assertions = true;
                client.scopes.push("acme:write".to_owned());
            },
            |server| {
                server
                    .with_identity_assertions(
                        vec![TrustedIssuer::new(IDP)],
                        Arc::new(Directory(test_identity_jwks())),
                    )
                    .unwrap()
            },
        ),
        None,
    );

    let now = unix_seconds();
    let assertion = test_identity_assertion(&json!({
        "iss": IDP, "sub": "alice", "aud": ISSUER, "resource": RESOURCE,
        "client_id": CLIENT_ID, "scope": "acme:read acme:write",
        "iat": now, "exp": now + 300, "jti": "acme-enterprise-sign-in",
    }));
    let exchange = |id| {
        server.exchange(&form_post(
            "/token",
            &format!(
                "grant_type={JWT_BEARER_GRANT}&assertion={assertion}&{}",
                client_auth(CLIENT_ID, id)
            ),
            "",
        ))
    };
    let issued = exchange("issue");
    let tokens = issued.json();
    assert_eq!(issued.status, 200, "{tokens}");
    assert_eq!(tokens["scope"], "acme:read");
    assert!(tokens.get("refresh_token").is_none());
    let access = tokens["access_token"].as_str().unwrap();

    let tool = whoami(&server, access);
    assert_eq!(tool.status, 200);
    assert_eq!(tool.json()["result"]["content"][0]["text"], SUBJECT);
    let grant = server.handler.whoami_grant.lock().unwrap().take().unwrap();
    assert!(grant.scopes().eq(["acme:read"]));

    let replay = exchange("replay");
    assert_eq!(replay.status, 400);
    assert_eq!(replay.json()["error"], "invalid_grant");
}

#[test]
fn a_document_machine_client_at_the_identity_bound_can_call_a_tool() {
    struct Document {
        id: String,
        metadata: Vec<u8>,
    }

    impl ClientResolver for Document {
        fn resolve(&self, client_id: &str) -> Result<Vec<u8>, String> {
            assert_eq!(client_id, self.id);
            Ok(self.metadata.clone())
        }
    }

    let base = "https://app.example.com/";
    let client_id = format!(
        "{base}{}",
        "a".repeat(MAX_GRANT_IDENTITY_BYTES - base.len())
    );
    let server = Server::start(
        embedded_with(
            "/oauth/sign-in",
            true,
            |options| {
                options.document_client_grants.insert(
                    client_id.clone(),
                    DocumentClientGrants {
                        client_credentials: true,
                        identity_assertions: false,
                    },
                );
            },
            |server| {
                let metadata = json!({
                    "client_id": client_id, "client_name": "Acme Machine", "redirect_uris": [REDIRECT],
                    "token_endpoint_auth_method": "private_key_jwt",
                    "jwks": serde_json::from_slice::<serde_json::Value>(&test_client_jwks()).unwrap(),
                });
                server.with_resolver(Arc::new(Document {
                    id: client_id.clone(),
                    metadata: metadata.to_string().into_bytes(),
                }))
            },
        ),
        None,
    );
    let request = |id| {
        server.exchange(&form_post(
            "/token",
            &format!("grant_type=client_credentials&{}", client_auth(id, "issue")),
            "",
        ))
    };
    let issued = request(&client_id);
    assert_eq!(issued.status, 200);
    let body = issued.json();
    let access = body["access_token"].as_str().unwrap();
    let tool = whoami(&server, access);
    assert_eq!(tool.json()["result"]["content"][0]["text"], client_id);

    let too_long = format!("{client_id}a");
    let refused = request(&too_long);
    assert_eq!(refused.status, 401);
    assert_eq!(refused.json()["error"], "invalid_client");
}

#[test]
fn serve_refuses_an_unlisted_issuer_or_colliding_paths() {
    let refused = |http: HttpOptions| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut options = Options::default();
        options.session.modern = Some(crate::ModernOptions::new(ServerInfo::new("acme", "1")));
        matches!(
            serve(
                Arc::new(Conflict(Mutex::new(None))),
                catalog(),
                listener,
                &http,
                &options,
                &Shutdown::new(),
            ),
            Err(ServeError::Options(_))
        )
    };
    let unprotected = HttpOptions {
        authorization: None,
        ..embedded()
    };
    assert!(refused(unprotected));
    let colliding = HttpOptions {
        path: "/token".to_owned(),
        ..embedded()
    };
    assert!(refused(colliding));
    // A sign-in path above the metadata would route discovery to the consumer.
    assert!(refused(embedded_at(
        "/.well-known/oauth-protected-resource",
        false
    )));
    let other = ResourceServer::new(ResourceOptions::new(
        RESOURCE,
        vec!["https://auth.example.com".to_owned()],
    ))
    .unwrap();
    let mut unlisted = embedded();
    unlisted.authorization = Some(HttpAuthorization::new(
        other,
        Arc::new(RefuseAll) as Arc<dyn TokenVerifier>,
    ));
    assert!(refused(unlisted));
}

/// Refuses every token.
struct RefuseAll;

impl TokenVerifier for RefuseAll {
    fn verify(
        &self,
        _: crate::http::BearerToken<'_>,
        _: &str,
    ) -> Result<Grant, crate::http::Rejection> {
        Err(crate::http::Rejection::invalid())
    }
}
