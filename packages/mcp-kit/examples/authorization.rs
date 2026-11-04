//! Protect an Acme tool with bearer tokens over Streamable HTTP.
//!
//! Run with `cargo run --example authorization --features http-server`. The
//! stub verifier accepts the token `acme-demo-reader`; a real server checks a
//! signed token, calls its authorization server's introspection endpoint or
//! looks the token up in its own store. Clients discover the authorization
//! server at `http://127.0.0.1:8080/.well-known/oauth-protected-resource/mcp`.
use canonic_mcp_kit::{
    Catalog, Client, Content, ModernOptions, RpcError, ServerInfo, Tool, ToolResult,
    http::{
        self, BearerToken, Grant, HttpAuthorization, HttpOptions, Operation, Rejection,
        ResourceOptions, ResourceServer, ScopePolicy, Shutdown, TokenVerifier,
    },
    stdio::{self, Context, Handler, Outcome, Request},
};
use serde_json::json;
use std::{
    net::TcpListener,
    sync::Arc,
    time::{Duration, SystemTime},
};

/// The address clients use, which tokens must be issued for.
const RESOURCE: &str = "http://127.0.0.1:8080/mcp";

struct Acme;

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1"))
    }

    fn call(&self, request: Request, context: &Context) -> Outcome {
        let Request::Tool(_) = request else {
            return Outcome::Done;
        };
        // The grant says who was authorized; the server decides what it permits.
        let user = context
            .authorization()
            .and_then(Grant::subject)
            .unwrap_or("unknown");
        Outcome::Tool(ToolResult::success(vec![Content::text(format!(
            "Acme reports are ready for {user}."
        ))]))
    }
}

/// Stands in for signature checking or introspection.
struct DemoVerifier;

impl TokenVerifier for DemoVerifier {
    fn verify(&self, token: BearerToken<'_>, resource: &str) -> Result<Grant, Rejection> {
        if token.as_str() != "acme-demo-reader" {
            return Err(Rejection::invalid());
        }
        // Report the audience the token was issued for; the library refuses
        // tokens issued for any other resource.
        Ok(Grant::new(vec![resource.to_owned()])
            .with_subject("demo-user")
            .with_scopes(["reports:read"])
            .with_expiry(SystemTime::now() + Duration::from_hours(1)))
    }
}

/// Reading reports needs `reports:read`.
struct Scopes;

impl ScopePolicy for Scopes {
    fn required(&self, operation: &Operation<'_>) -> Vec<String> {
        match operation.name {
            Some("acme_reports") => vec!["reports:read".to_owned()],
            _ => Vec::new(),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::new(vec![Tool::new(
        "acme_reports",
        "List the Acme reports the caller may read.",
        json!({"type": "object", "additionalProperties": false}),
    )])?;
    let mut resource = ResourceOptions::new(RESOURCE, vec!["https://auth.example.com".to_owned()]);
    resource.scopes_supported = Some(vec!["reports:read".to_owned()]);
    let http = HttpOptions {
        authorization: Some(
            HttpAuthorization::new(ResourceServer::new(resource)?, Arc::new(DemoVerifier))
                .with_policy(Arc::new(Scopes)),
        ),
        ..HttpOptions::default()
    };
    let mut options = stdio::Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    let listener = TcpListener::bind("127.0.0.1:8080")?;
    http::serve(
        Arc::new(Acme),
        Arc::new(catalog),
        listener,
        &http,
        &options,
        &Shutdown::new(),
    )?;
    Ok(())
}
