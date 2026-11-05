//! Issue and check OAuth tokens from the same process as the MCP endpoint.
//!
//! Run with `cargo run --example authorization_server --features
//! http-server,authorization-server`. Clients discover the issuer at
//! `http://127.0.0.1:8080/.well-known/oauth-protected-resource/mcp`, sign in at
//! `/oauth/sign-in` and call `acme_reports` with the token they receive. The
//! sign-in page approves anyone who presses the button: a real server
//! authenticates the user there first.
use canonic_mcp_kit::{
    Catalog, Client, Content, ModernOptions, RpcError, ServerInfo, Tool, ToolResult,
    http::{
        self, Grant, HttpAuthorization, HttpOptions, ResourceOptions, ResourceServer, Response,
        Shutdown, TokenVerifier,
    },
    oauth::{
        AuthorizationServer, AuthorizationServerOptions, ClientAuth, ClientOptions, Interaction,
        InteractionOutcome, InteractionRequest, MemoryStore,
    },
    stdio::{self, Context, Handler, Outcome, Request},
};
use serde_json::json;
use std::{net::TcpListener, sync::Arc};

const ORIGIN: &str = "http://127.0.0.1:8080";

struct Acme;

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1"))
    }

    fn call(&self, request: Request, context: &Context) -> Outcome {
        let Request::Tool(_) = request else {
            return Outcome::Done;
        };
        let user = context
            .authorization()
            .and_then(Grant::subject)
            .unwrap_or("unknown");
        Outcome::Tool(ToolResult::success(vec![Content::text(format!(
            "Acme reports are ready for {user}."
        ))]))
    }
}

/// A one-button consent page. Replace it with real sign-in.
struct ConsentPage;

impl Interaction for ConsentPage {
    fn interact(&self, request: &InteractionRequest<'_>) -> InteractionOutcome {
        let Some(pending) = request.pending else {
            return InteractionOutcome::Respond(page("This sign-in has expired. Start again."));
        };
        if request.method == "POST" {
            return InteractionOutcome::Approve {
                subject: "demo-user".to_owned(),
                scopes: pending.scopes.clone(),
            };
        }
        // Show the client and where it redirects. Client names can come from
        // untrusted metadata documents, so everything is escaped.
        InteractionOutcome::Respond(page(&format!(
            "<p>{} wants {} and returns to {}.</p>\
             <form method=\"post\"><input type=\"hidden\" name=\"pending\" value=\"{}\">\
             <button>Approve</button></form>",
            escape(pending.client_name.as_deref().unwrap_or(&pending.client_id)),
            escape(&pending.scopes.join(", ")),
            escape(&pending.redirect_host),
            escape(&pending.reference),
        )))
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn page(body: &str) -> Response {
    Response {
        status: 200,
        headers: vec![
            ("Content-Type", "text/html; charset=utf-8".into()),
            ("Cache-Control", "no-store".into()),
        ],
        body: format!("<!doctype html><title>Acme sign-in</title>{body}").into_bytes(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::new(vec![Tool::new(
        "acme_reports",
        "List the Acme reports the caller may read.",
        json!({"type": "object", "additionalProperties": false}),
    )])?;
    let mut resource = ResourceOptions::new(format!("{ORIGIN}/mcp"), vec![ORIGIN.to_owned()]);
    resource.scopes_supported = Some(vec!["reports:read".to_owned()]);
    let resource = ResourceServer::new(resource)?;

    let mut issuer = AuthorizationServerOptions::new(ORIGIN, "/oauth/sign-in");
    issuer.default_scopes = vec!["reports:read".to_owned()];
    issuer.clients = vec![ClientOptions {
        client_id: "acme-desktop".to_owned(),
        client_name: Some("Acme Desktop".to_owned()),
        redirect_uris: vec!["http://127.0.0.1:7777/callback".to_owned()],
        scopes: vec!["reports:read".to_owned()],
        auth: ClientAuth::None,
        secret: None,
        client_credentials: false,
    }];
    let server = Arc::new(AuthorizationServer::new(
        issuer,
        &resource,
        Arc::new(MemoryStore::new()),
        Arc::new(ConsentPage),
    )?);

    let http = HttpOptions {
        // The embedded server verifies its own tokens like any other issuer.
        authorization: Some(HttpAuthorization::new(
            resource,
            Arc::clone(&server) as Arc<dyn TokenVerifier>,
        )),
        authorization_server: Some(server),
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
