//! Resource identity, metadata, token extraction, grant checks, scope policies
//! and challenges.
use super::*;
use crate::http::{Endpoint, EndpointError, EndpointOptions, Exchange, RequestHead};
use std::time::Duration;

const RESOURCE: &str = "https://mcp.example.com/mcp";
const ISSUER: &str = "https://auth.example.com";
const METADATA: &str = "/.well-known/oauth-protected-resource/mcp";

fn options() -> ResourceOptions {
    ResourceOptions::new(RESOURCE, vec![ISSUER.to_owned()])
}

fn server() -> ResourceServer {
    ResourceServer::new(options()).unwrap()
}

fn endpoint(server: ResourceServer) -> Endpoint {
    Endpoint::new(EndpointOptions {
        authorization: Some(server),
        ..EndpointOptions::default()
    })
    .unwrap()
}

fn grant() -> Grant {
    Grant::new(vec![RESOURCE.to_owned()])
}

fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(field, _)| field.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_ref())
}

fn challenge(response: &Response) -> &str {
    header(response, "WWW-Authenticate").expect("a challenge")
}

/// The content fields of a POST, plus `extra`.
fn fields<'a>(extra: &[(&'a str, &'a [u8])]) -> Vec<(&'a str, &'a [u8])> {
    let mut fields = vec![
        ("Content-Type", b"application/json".as_slice()),
        ("Accept", b"application/json, text/event-stream".as_slice()),
    ];
    fields.extend_from_slice(extra);
    fields
}

fn check(
    endpoint: &Endpoint,
    method: &str,
    target: &str,
    fields: &[(&str, &[u8])],
) -> Result<(), Response> {
    endpoint.check_head(&RequestHead {
        method,
        target,
        headers: fields,
    })
}

/// Builds a resource server from the default options after `change`.
fn with(change: &dyn Fn(&mut ResourceOptions)) -> Result<(), AuthorizationError> {
    let mut options = options();
    change(&mut options);
    ResourceServer::new(options).map(|_| ())
}

#[test]
fn construction_refuses_invalid_resources_and_issuers() {
    for resource in [
        "mcp.example.com",
        "http://mcp.example.com/mcp",
        "ftp://mcp.example.com",
        "https://mcp.example.com/mcp?x=1",
        "https://mcp.example.com/mcp#x",
        "https://user@mcp.example.com/mcp",
        "https://mcp.example.com/mcp/",
        "https://",
        "https://mcp.example.com:99999/mcp",
    ] {
        assert_eq!(
            with(&|options| options.resource = resource.to_owned()),
            Err(AuthorizationError::Resource),
            "{resource}"
        );
    }
    let long = format!("https://mcp.example.com/{}", "a".repeat(MAX_URL_BYTES));
    assert_eq!(
        with(&|options| options.resource.clone_from(&long)),
        Err(AuthorizationError::Resource)
    );
    for resource in [
        "http://localhost:8080/mcp",
        "http://127.0.0.1/mcp",
        "http://[::1]:8080",
        "https://mcp.example.com/",
    ] {
        assert_eq!(
            with(&|options| options.resource = resource.to_owned()),
            Ok(()),
            "{resource}"
        );
    }
    assert_eq!(
        with(&|options| options.authorization_servers.clear()),
        Err(AuthorizationError::AuthorizationServers)
    );
    assert_eq!(
        with(&|options| options.authorization_servers =
            vec![ISSUER.to_owned(); MAX_AUTHORIZATION_SERVERS + 1]),
        Err(AuthorizationError::AuthorizationServers)
    );
    for issuers in [
        vec![ISSUER, ISSUER],
        vec![ISSUER, "http://auth.example.com"],
        vec![ISSUER, "https://auth.example.com?tenant=1"],
    ] {
        assert_eq!(
            with(&|options| {
                options.authorization_servers =
                    issuers.iter().map(|&issuer| issuer.to_owned()).collect();
            }),
            Err(AuthorizationError::Issuer(1)),
            "{issuers:?}"
        );
    }
    assert_eq!(
        Endpoint::new(EndpointOptions {
            path: METADATA.to_owned(),
            authorization: Some(server()),
            ..EndpointOptions::default()
        }),
        Err(EndpointError::MetadataPath)
    );
}

#[test]
fn construction_refuses_invalid_scopes_and_metadata() {
    for scope in ["", "a b", "a\"b", "a\\b", &"a".repeat(MAX_SCOPE_BYTES + 1)] {
        assert_eq!(
            with(&|options| options.required_scopes = vec![scope.to_owned()]),
            Err(AuthorizationError::Scope),
            "{scope:?}"
        );
    }
    assert_eq!(
        with(&|options| options.scopes_supported =
            Some(vec!["a".to_owned(); MAX_CONFIGURED_SCOPES + 1])),
        Err(AuthorizationError::Scope)
    );
    assert_eq!(
        with(&|options| options.scopes_supported =
            Some(vec!["acme:read".to_owned(), "offline_access".to_owned()])),
        Err(AuthorizationError::OfflineAccess)
    );
    for name in [
        String::new(),
        "a".repeat(MAX_RESOURCE_NAME_BYTES + 1),
        "Acme\n".to_owned(),
    ] {
        assert_eq!(
            with(&|options| options.resource_name = Some(name.clone())),
            Err(AuthorizationError::ResourceName)
        );
    }
    assert_eq!(
        with(&|options| options.resource_documentation = Some("http://docs.example.com".to_owned())),
        Err(AuthorizationError::Documentation)
    );
    assert_eq!(
        with(&|options| options.metadata_max_age = MAX_METADATA_MAX_AGE + Duration::from_secs(1)),
        Err(AuthorizationError::MaxAge)
    );
    let many = (0..MAX_CONFIGURED_SCOPES)
        .map(|index| format!("{index}{}", "s".repeat(MAX_SCOPE_BYTES - 3)))
        .collect::<Vec<_>>();
    assert_eq!(
        with(&|options| options.scopes_supported = Some(many.clone())),
        Err(AuthorizationError::Metadata)
    );
}

#[test]
fn resource_identity_is_normalized_once() {
    let server = ResourceServer::new(ResourceOptions::new(
        "HTTPS://MCP.Example.COM:443/Mcp",
        vec![ISSUER.to_owned()],
    ))
    .unwrap();
    assert_eq!(server.resource(), "https://mcp.example.com/Mcp");
    assert_eq!(
        server.metadata_url(),
        "https://mcp.example.com/.well-known/oauth-protected-resource/Mcp"
    );
    let root = ResourceServer::new(ResourceOptions::new(
        "https://mcp.example.com:8443/",
        vec![ISSUER.to_owned()],
    ))
    .unwrap();
    assert_eq!(root.resource(), "https://mcp.example.com:8443");
    assert_eq!(
        root.metadata_url(),
        "https://mcp.example.com:8443/.well-known/oauth-protected-resource"
    );
    assert_eq!(root.metadata_paths().collect::<Vec<_>>(), [METADATA_PREFIX]);
}

#[test]
fn metadata_is_served_without_a_token_at_both_paths() {
    let mut options = options();
    options.scopes_supported = Some(vec!["acme:read".to_owned()]);
    options.resource_name = Some("Acme".to_owned());
    options.resource_documentation = Some("https://docs.example.com/mcp".to_owned());
    options.metadata_max_age = Duration::from_mins(10);
    let endpoint = endpoint(ResourceServer::new(options).unwrap());
    let origin: &[(&str, &[u8])] = &[("Origin", b"https://elsewhere.example.com")];
    for path in [METADATA, METADATA_PREFIX] {
        let response = check(&endpoint, "GET", path, origin).unwrap_err();
        assert_eq!(response.status, 200, "{path}");
        assert_eq!(header(&response, "Content-Type"), Some("application/json"));
        assert_eq!(header(&response, "Cache-Control"), Some("max-age=600"));
        assert_eq!(header(&response, "Access-Control-Allow-Origin"), Some("*"));
        let document: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(
            document,
            json!({
                "resource": RESOURCE,
                "authorization_servers": [ISSUER],
                "bearer_methods_supported": ["header"],
                "scopes_supported": ["acme:read"],
                "resource_name": "Acme",
                "resource_documentation": "https://docs.example.com/mcp",
            })
        );
    }
    let refused = check(&endpoint, "POST", METADATA, &[]).unwrap_err();
    assert_eq!(refused.status, 405);
    assert_eq!(header(&refused, "Allow"), Some("GET"));

    let mut options = self::options();
    options.serve_root_metadata = false;
    let endpoint = self::endpoint(ResourceServer::new(options).unwrap());
    assert_eq!(
        check(&endpoint, "GET", METADATA_PREFIX, &[])
            .unwrap_err()
            .status,
        404
    );
    assert_eq!(
        check(&endpoint, "GET", METADATA, &[]).unwrap_err().status,
        200
    );
}

#[test]
fn token_extraction_refuses_each_malformed_form_before_the_body() {
    let mut options = options();
    options.required_scopes = vec!["acme:read".to_owned()];
    let endpoint = endpoint(ResourceServer::new(options).unwrap());
    let missing = check(&endpoint, "POST", "/mcp", &fields(&[])).unwrap_err();
    assert_eq!(missing.status, 401);
    assert_eq!(
        challenge(&missing),
        "Bearer scope=\"acme:read\", resource_metadata=\"https://mcp.example.com/.well-known/oauth-protected-resource/mcp\""
    );
    assert_eq!(header(&missing, "Cache-Control"), Some("no-store"));
    assert_eq!(missing.body, Vec::<u8>::new());

    let oversized = format!("Bearer {}", "a".repeat(MAX_TOKEN_BYTES + 1));
    for value in [
        b"Basic YWNtZTpzZWNyZXQ=".as_slice(),
        b"Bearer",
        b"Bearer ",
        b"Bearer a b",
        b"Bearer a=b",
        b"Bearer \xff",
        oversized.as_bytes(),
    ] {
        let refused = check(
            &endpoint,
            "POST",
            "/mcp",
            &fields(&[("Authorization", value)]),
        )
        .unwrap_err();
        assert_eq!(refused.status, 400, "{:?}", String::from_utf8_lossy(value));
        assert!(challenge(&refused).starts_with("Bearer error=\"invalid_request\""));
    }
    let repeated = fields(&[
        ("Authorization", b"Bearer a"),
        ("authorization", b"Bearer a"),
    ]);
    assert_eq!(
        check(&endpoint, "POST", "/mcp", &repeated)
            .unwrap_err()
            .status,
        400
    );
    let valid = fields(&[("Authorization", b"bearer  acme-token_1.~+/==")]);
    assert_eq!(check(&endpoint, "POST", "/mcp", &valid), Ok(()));
    let in_query = check(&endpoint, "POST", "/mcp?access_token=a", &valid).unwrap_err();
    assert_eq!(in_query.status, 400);
    let server = endpoint.authorization().unwrap();
    let token = server
        .bearer(&RequestHead {
            method: "POST",
            target: "/mcp",
            headers: &valid,
        })
        .unwrap();
    assert_eq!(token.as_str(), "acme-token_1.~+/==");
    assert_eq!(format!("{token:?}"), "BearerToken(..)");

    // Notifications need a token too, and a missing one stops before the body.
    let notification = br#"{"jsonrpc":"2.0","method":"notifications/acme"}"#;
    let refused = endpoint.receive(
        &RequestHead {
            method: "POST",
            target: "/mcp",
            headers: &fields(&[]),
        },
        notification,
    );
    assert!(matches!(refused, Exchange::Respond(response) if response.status == 401));
}

#[test]
fn grants_are_refused_for_rejections_bounds_audience_and_expiry() {
    let server = server();
    let refused = |outcome| server.check_grant(outcome, None).unwrap_err();
    let rejected = refused(Err(Rejection::Invalid {
        description: Some("Token revoked".to_owned()),
    }));
    assert_eq!(rejected.status, 401);
    assert!(challenge(&rejected).starts_with(
        "Bearer error=\"invalid_token\", error_description=\"Token revoked\", resource_metadata="
    ));
    let unsafe_description = refused(Err(Rejection::Invalid {
        description: Some("bad \"quote\"".to_owned()),
    }));
    assert!(!challenge(&unsafe_description).contains("error_description"));
    let unavailable = refused(Err(Rejection::Unavailable));
    assert_eq!(unavailable.status, 503);
    assert_eq!(header(&unavailable, "WWW-Authenticate"), None);

    // With scopes configured, every 401 names them, not only a missing token's.
    let scoped = |required: &[&str], supported: Option<&[&str]>| {
        let mut options = options();
        options.required_scopes = required.iter().map(|&scope| scope.to_owned()).collect();
        options.scopes_supported =
            supported.map(|scopes| scopes.iter().map(|&scope| scope.to_owned()).collect());
        ResourceServer::new(options).unwrap()
    };
    let expired = grant().with_expiry(SystemTime::now() - Duration::from_secs(1));
    for (server, scope) in [
        (
            scoped(&["acme:read"], Some(&["acme:read", "acme:write"])),
            "acme:read",
        ),
        (
            scoped(&[], Some(&["acme:read", "acme:write"])),
            "acme:read acme:write",
        ),
    ] {
        for outcome in [Err(Rejection::invalid()), Ok(expired.clone())] {
            let response = server.check_grant(outcome, None).unwrap_err();
            assert_eq!(response.status, 401);
            assert!(
                challenge(&response).contains(&format!("scope=\"{scope}\"")),
                "{}",
                challenge(&response)
            );
        }
    }
    assert!(!challenge(&rejected).contains("scope="));

    for grant in [
        Grant::new(Vec::new()),
        Grant::new(vec!["https://other.example.com/mcp".to_owned()]),
        Grant::new(vec!["https://mcp.example.com/mcp/".to_owned()]),
        Grant::new(vec!["https://mcp.example.com/MCP".to_owned()]),
        Grant::new(vec!["https://mcp.example.com:8443/mcp".to_owned()]),
        Grant::new(vec!["not a uri".to_owned()]),
        grant().with_expiry(SystemTime::now() - Duration::from_secs(1)),
        grant().with_subject("a".repeat(MAX_GRANT_IDENTITY_BYTES + 1)),
        grant().with_client_id(""),
        grant().with_scopes(["a b"]),
        Grant::new(vec![RESOURCE.to_owned(); MAX_GRANT_AUDIENCES + 1]),
    ] {
        let response = refused(Ok(grant.clone()));
        assert_eq!(response.status, 401, "{grant:?}");
        assert!(challenge(&response).starts_with("Bearer error=\"invalid_token\""));
    }
    for audiences in [
        vec![RESOURCE],
        vec![
            "https://other.example.com",
            "HTTPS://MCP.EXAMPLE.COM:443/mcp",
        ],
    ] {
        let grant = Grant::new(
            audiences
                .iter()
                .map(|&audience| audience.to_owned())
                .collect(),
        );
        assert!(server.check_grant(Ok(grant), None).is_ok(), "{audiences:?}");
    }
    let unexpiring = grant().with_subject("user-1");
    assert!(server.check_grant(Ok(unexpiring), None).is_ok());
    let live = grant().with_expiry(SystemTime::now() + Duration::from_mins(1));
    assert!(server.check_grant(Ok(live), None).is_ok());

    let root = ResourceServer::new(ResourceOptions::new(
        "https://mcp.example.com",
        vec![ISSUER.to_owned()],
    ))
    .unwrap();
    let slash = Grant::new(vec!["https://mcp.example.com/".to_owned()]);
    assert!(root.check_grant(Ok(slash), None).is_ok());
}

/// Requires `acme:write` for `acme_deploy`, plus `acme:prod` when it targets
/// production; `acme:admin` implies every scope.
struct Policy;

impl ScopePolicy for Policy {
    fn required(&self, operation: &Operation<'_>) -> Vec<String> {
        match (operation.method, operation.name) {
            ("tools/call", Some("acme_deploy")) => {
                let mut scopes = vec!["acme:write".to_owned(), "acme:write".to_owned()];
                if operation.params["arguments"]["region"] == "prod" {
                    scopes.push("acme:prod".to_owned());
                }
                scopes
            }
            ("resources/read", Some(uri)) if uri.starts_with("acme://secret/") => {
                vec!["acme:secret".to_owned()]
            }
            ("notifications/acme/deployed", None) => vec!["acme:write".to_owned()],
            ("tools/call", Some("acme_flood")) | ("notifications/acme/flood", None) => {
                vec!["a".to_owned(); MAX_REQUIRED_SCOPES + 1]
                    .into_iter()
                    .enumerate()
                    .map(|(index, scope)| format!("{scope}{index}"))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    fn satisfies(&self, grant: &Grant, scope: &str) -> bool {
        grant.has_scope(scope) || grant.has_scope("acme:admin")
    }
}

fn admitted(method: &str, params: Value) -> Admitted {
    Admitted {
        id: json!(7),
        method: method.to_owned(),
        message: crate::rpc::request(json!(7), method, params),
        form: crate::http::Form::Json,
        parameters: Vec::new(),
    }
}

#[test]
fn scope_policies_name_every_required_scope_at_once() {
    let server = server();
    let deploy = |region: &str| {
        admitted(
            "tools/call",
            json!({"name": "acme_deploy", "arguments": {"region": region}}),
        )
    };
    let reader = grant().with_scopes(["acme:read"]);
    let refused = server
        .check_scopes(&deploy("prod"), &reader, &Policy)
        .unwrap_err();
    assert_eq!(refused.status, 403);
    assert_eq!(
        challenge(&refused),
        "Bearer error=\"insufficient_scope\", scope=\"acme:write acme:prod\", resource_metadata=\"https://mcp.example.com/.well-known/oauth-protected-resource/mcp\""
    );
    let writer = grant().with_scopes(["acme:write"]);
    assert!(server.check_scopes(&deploy("eu"), &writer, &Policy).is_ok());
    assert!(
        server
            .check_scopes(&deploy("prod"), &writer, &Policy)
            .is_err()
    );
    let admin = grant().with_scopes(["acme:admin"]);
    assert!(
        server
            .check_scopes(&deploy("prod"), &admin, &Policy)
            .is_ok()
    );
    let secret = admitted("resources/read", json!({"uri": "acme://secret/1"}));
    assert_eq!(
        server
            .check_scopes(&secret, &reader, &Policy)
            .unwrap_err()
            .status,
        403
    );
    let listing = admitted("tools/list", json!({}));
    assert!(server.check_scopes(&listing, &reader, &Policy).is_ok());
    let flood = admitted("tools/call", json!({"name": "acme_flood"}));
    let failed = server.check_scopes(&flood, &admin, &Policy).unwrap_err();
    assert_eq!(failed.status, 200);
    let reply: Value = serde_json::from_slice(&failed.body).unwrap();
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["error"]["code"], crate::rpc::INTERNAL_ERROR);

    // Notifications go through the same policy, with no name and no reply body.
    let params = json!({"region": "eu"});
    let notified =
        |method: &str, grant: &Grant| server.check_notification(method, &params, grant, &Policy);
    let refused = notified("notifications/acme/deployed", &reader).unwrap_err();
    assert_eq!(refused.status, 403);
    assert!(challenge(&refused).contains("scope=\"acme:write\""));
    assert!(notified("notifications/acme/deployed", &writer).is_ok());
    assert!(notified("notifications/acme/other", &reader).is_ok());
    let failed = notified("notifications/acme/flood", &admin).unwrap_err();
    assert_eq!((failed.status, failed.body), (500, Vec::new()));

    let mut options = options();
    options.required_scopes = vec!["acme:read".to_owned()];
    let based = ResourceServer::new(options).unwrap();
    let missing = based.check_grant(Ok(grant()), None).unwrap_err();
    assert_eq!(missing.status, 403);
    assert!(challenge(&missing).contains("scope=\"acme:read\""));
    assert!(
        based
            .check_grant(Ok(admin), Some(&Policy as &dyn ScopePolicy))
            .is_ok()
    );
    assert!(based.check_grant(Ok(reader), None).is_ok());
}

#[test]
fn grants_expose_identity_and_authority_without_a_token() {
    #[derive(Debug, PartialEq)]
    struct Tenant(&'static str);
    let expires = SystemTime::now() + Duration::from_mins(1);
    let grant = grant()
        .with_scopes(["acme:write", "acme:read"])
        .with_expiry(expires)
        .with_subject("user-1")
        .with_client_id("https://client.example.com/metadata.json")
        .with_extension(Tenant("acme"));
    assert_eq!(grant.audiences(), [RESOURCE]);
    assert_eq!(
        grant.scopes().collect::<Vec<_>>(),
        ["acme:read", "acme:write"]
    );
    assert_eq!(grant.expires_at(), Some(expires));
    assert_eq!(grant.subject(), Some("user-1"));
    assert_eq!(
        grant.client_id(),
        Some("https://client.example.com/metadata.json")
    );
    assert_eq!(grant.extension::<Tenant>(), Some(&Tenant("acme")));
    assert_eq!(grant.extension::<String>(), None);
    assert!(format!("{grant:?}").contains("extension: true"));
}
