//! The embedded authorization server through the driver: metadata, sign-in,
//! the code grant and an MCP call with the issued token, all on one listener.
use super::*;
use crate::{
    http::{ResourceOptions, ResourceServer, TokenVerifier},
    oauth::{
        AuthorizationServer, AuthorizationServerOptions, ClientAuth, ClientOptions, Interaction,
        InteractionOutcome, InteractionRequest, MemoryStore,
    },
};

const ISSUER: &str = "http://127.0.0.1";
const RESOURCE: &str = "http://127.0.0.1/mcp";
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
    let mut resource = ResourceOptions::new(RESOURCE, vec![ISSUER.to_owned()]);
    resource.serve_root_metadata = root_metadata;
    let resource = ResourceServer::new(resource).unwrap();
    let mut options = AuthorizationServerOptions::new(ISSUER, interaction);
    options.default_scopes = vec!["acme:read".to_owned()];
    options.clients = vec![ClientOptions {
        client_id: "acme-desktop".to_owned(),
        client_name: None,
        redirect_uris: vec![REDIRECT.to_owned()],
        scopes: vec!["acme:read".to_owned()],
        auth: ClientAuth::None,
        secret: None,
        client_credentials: false,
    }];
    let server = Arc::new(
        AuthorizationServer::new(
            options,
            &resource,
            Arc::new(MemoryStore::new()),
            Arc::new(Approve),
        )
        .unwrap(),
    );
    HttpOptions {
        authorization: Some(HttpAuthorization::new(
            resource,
            Arc::clone(&server) as Arc<dyn TokenVerifier>,
        )),
        authorization_server: Some(server),
        ..http()
    }
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

    let whoami = server.exchange(&call(
        &json!(1),
        "acme_whoami",
        &json!({}),
        &format!("Authorization: Bearer {access}\r\n"),
    ));
    assert_eq!(whoami.json()["result"]["content"][0]["text"], "user-1");
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
