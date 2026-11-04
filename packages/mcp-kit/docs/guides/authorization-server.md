# Issue tokens from your server

Run an OAuth 2.1 authorization server inside your MCP server, so clients can
sign in without a separate identity provider. It answers on the same listener
as your MCP endpoint and plugs into [bearer token protection](authorization.md)
like any other issuer.

Use it when you own the user accounts and want one process. If you already
have an identity provider, such as Auth0, Entra ID or Keycloak, skip this
guide: point `ResourceOptions::authorization_servers` at it and write a
[token verifier](authorization.md#verify-tokens) instead.

The server is behind the `authorization-server` Cargo feature, which adds
operating-system randomness (`getrandom`) for the codes and tokens it issues.

## What the library does and what you do

| The library | You |
| --- | --- |
| Serves metadata, `/authorize`, `/token`, `/revoke` and optionally `/register` | Sign users in and show consent |
| Checks clients, exact redirect URIs, S256 PKCE and the `resource` | Decide which clients exist and which scopes a user may grant |
| Generates every code, token and client secret and stores only their SHA-256 hashes | Choose where records are stored |
| Ties each sign-in to the browser that started it, with a cookie your code never sees | Protect your own pages: sessions, passwords, CSRF, rate limits |
| Rotates refresh tokens and ends a sign-in when an old token is reused | |

## Try the example

From `packages/mcp-kit`, run:

```sh
cargo run --locked --example authorization_server --features http-server,authorization-server
```

It serves the MCP endpoint at `http://127.0.0.1:8080/mcp`, the issuer at
`http://127.0.0.1:8080` and a one-button consent page at `/oauth/sign-in`. An
MCP client that supports OAuth discovers everything from
`http://127.0.0.1:8080/.well-known/oauth-protected-resource/mcp`, signs in as
the pre-registered `acme-desktop` client and calls `acme_reports`, which
answers with the user's subject, `demo-user`. Read the authorization server's
metadata yourself:

```sh
curl -sS http://127.0.0.1:8080/.well-known/oauth-authorization-server
```

The example's page approves anyone who presses the button. Your page must sign
the user in first.

## Set it up

```rust,ignore
let resource = ResourceServer::new(ResourceOptions::new(
    "https://mcp.example.com/mcp",
    vec!["https://mcp.example.com".to_owned()], // the embedded issuer
))?;
let mut issuer = AuthorizationServerOptions::new("https://mcp.example.com", "/oauth/sign-in");
issuer.default_scopes = vec!["reports:read".to_owned()];
issuer.clients = vec![/* pre-registered clients */];
let server = Arc::new(AuthorizationServer::new(
    issuer,
    &resource,
    Arc::new(MemoryStore::new()),
    Arc::new(MySignIn),
)?);
let http = HttpOptions {
    authorization: Some(HttpAuthorization::new(resource, server.clone())),
    authorization_server: Some(server),
    ..HttpOptions::default()
};
```

The resource must list the issuer, and `serve` refuses the configuration if it
doesn't or if any path collides with the MCP endpoint or its metadata.

| `AuthorizationServerOptions` field | Default | Purpose |
| --- | --- | --- |
| `issuer` | Required | `https`, or `http` on a loopback host; endpoints sit under its path |
| `interaction_path` | Required | Your sign-in path; it and paths under it go to your `Interaction` |
| `clients` | None | Pre-registered clients, at most 256 |
| `default_scopes` | None | Granted when a client asks for none |
| `scopes_supported` | None | Listed in metadata; what document and registered clients may request |
| `lifetimes` | See below | Record lifetimes |
| `metadata_max_age` | 1 hour | How long clients may cache the metadata |

| Lifetime | Default | Range |
| --- | --- | --- |
| `pending` (a sign-in in progress) | 10 minutes | 1–30 minutes |
| `code` | 60 seconds | 10 seconds–10 minutes |
| `access` | 1 hour | 1 minute–24 hours |
| `refresh_idle` | 14 days | 1 hour–90 days |
| `family` (one sign-in, whatever its tokens say) | 90 days | 1 hour–365 days |
| `refresh_grace` | 10 seconds | 0–60 seconds |

The issuer has no TLS of its own. Outside loopback, terminate TLS in front of
the server and use `https` URLs.

## Sign users in

`/authorize` checks the request, stores a pending sign-in and redirects the
browser to `interaction_path?pending=<reference>`. Your `Interaction` gets
every request to that path and the paths under it:

```rust,ignore
impl Interaction for MySignIn {
    fn interact(&self, request: &InteractionRequest<'_>) -> InteractionOutcome {
        let Some(pending) = request.pending else {
            return InteractionOutcome::Respond(expired_page());
        };
        match self.signed_in_user(request.headers) {
            None => InteractionOutcome::Respond(login_form(&pending.reference)),
            Some(user) if request.method == "POST" && consented(request.body) => {
                InteractionOutcome::Approve { subject: user.id, scopes: pending.scopes.clone() }
            }
            Some(_) => InteractionOutcome::Respond(consent_page(pending)),
        }
    }
}
```

- `request.pending` is present only when the request names a pending sign-in
  in its `pending` query or form field and comes from the browser that started
  it. Post the reference back as a hidden `pending` field.
- Show `pending.client_name` (or `client_id`) and `pending.redirect_host` on
  consent, and warn when `localhost_redirect` is true. Escape them: client
  names can come from untrusted documents.
- `Approve` and `Deny` are accepted only on a `POST`, so a link cannot approve
  anything, and each sign-in completes once. Approved scopes may narrow the
  request but never add to it.
- `request.headers` never contains the library's binding cookie. Your own
  cookies, such as a session, pass through.
- Your pages own their security: password storage, sessions, CSRF tokens on
  forms and rate limits on sign-in attempts.

Approval sends the browser back to the client with a single-use code; `Deny`
sends `access_denied`.

## Clients

**Pre-registered.** List them in `clients`. A public client (`ClientAuth::None`)
sends only its `client_id`; a confidential one uses `SecretBasic` or
`SecretPost` with a secret of at least 32 bytes, which the library keeps only
as a hash. Redirect URIs must be `https` or loopback `http` and match exactly,
except that a loopback IP address such as `127.0.0.1` may use any port.

**Client ID Metadata Documents.** Call `with_resolver` with a `ClientResolver`
to accept clients whose `client_id` is an `https` URL. The library never opens
a network connection: your resolver fetches the document and must refuse
private and loopback addresses, redirects and large bodies. The library checks
that the document names the same `client_id`, has a `client_name`, valid
redirect URIs and is a public client.

**Dynamic registration.** Some clients still register themselves (RFC 7591),
though the protocol deprecates it. Call `with_registrar` with a
`ClientRegistrar` to enable `/register`. The library validates the request and
generates the client ID and secret; your registrar approves it, stores the
`RegisteredClient` (with only the secret's hash) and applies its own policy and
rate limits. Without a registrar, `/register` does not exist and the metadata
does not advertise it.

## Tokens

Codes, access tokens and refresh tokens are 256 random bits. Access tokens are
opaque and checked against the store on every request, so revoking one takes
effect at once. Each refresh issues a new refresh token and a new access token
and revokes the previous access token. Presenting an older refresh token ends
the whole sign-in, except that the token just replaced may be used once more
within `refresh_grace`, for a client whose response was lost. Redeeming a code
twice also ends the sign-in it produced.

Refreshing never drops the token's resource and may narrow its scopes but
never widen them. `/revoke` ends the whole sign-in for either token type and
answers `200` for unknown tokens.

## Storage

`MemoryStore` keeps records in memory, up to 4,096 of each kind by default;
restarting the process signs everyone out. For durable storage, implement
`GrantStore` over your database. Every method is one atomic operation, and the
records contain hashes, never usable secrets, so a copied database cannot be
replayed. Your backend still needs integrity: losing a write may only end a
sign-in early, never extend one.

The library removes expired records during token requests. A full
`MemoryStore` drops the oldest pending sign-in, because anyone can start one,
and otherwise refuses with `503` only when every record is still live.

## Endpoints and errors

| Endpoint | Method | Errors |
| --- | --- | --- |
| `/.well-known/oauth-authorization-server` + issuer path | GET | — |
| `/authorize` | GET | A `400` page for an unknown client or redirect URI; otherwise a redirect with `error`, `state` and `iss` |
| `/token` | POST form | RFC 6749 JSON errors: `invalid_client` (`401`), `invalid_grant`, `invalid_scope`, `invalid_target`, `unsupported_grant_type` |
| `/revoke` | POST form | `invalid_client` only |
| `/register` | POST JSON | `invalid_client_metadata` |

Metadata, token, revocation and registration responses allow any origin and
answer CORS preflights, so browser-based clients can use them; none of them
read cookies. Authorization requests are never redirected until the client and
redirect URI check out.

## Use your own HTTP server

`AuthorizationServer::handle` takes the method, target, headers and body and
returns the complete response; `routes` says whether a target belongs to it.
Read bodies up to `MAX_INTERACTION_BODY_BYTES` and call it from a blocking
thread if your runtime is asynchronous, because the store and interaction
traits are synchronous.
