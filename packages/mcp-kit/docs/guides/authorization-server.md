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
operating-system randomness (`getrandom`) for the codes and tokens it issues,
and the pure-Rust RustCrypto `rsa` and `p256` crates that check
[enterprise users'](#enterprise-users) identity assertions.

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
| `default_scopes` | None | Granted when a client asks for none, within what the client may request |
| `scopes_supported` | None | Listed in metadata; what document and registered clients may request |
| `lifetimes` | See below | Record lifetimes |
| `metadata_max_age` | 1 hour | How long clients may cache the metadata |
| `client_grants` | 16 | Live [machine client](#machine-clients) tokens per client, 1–1,024; a new one replaces the oldest |

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
sends only its `client_id`; a confidential one (`SecretBasic` or `SecretPost`)
has a secret of at least 32 bytes, which the library keeps only as a hash, and
may send it either by HTTP Basic or in the form. Redirect URIs must be `https`
or loopback `http` and match exactly, except that a loopback IP address such
as `127.0.0.1` may use any port.

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

## Machine clients

Background services, CI jobs and other servers have no user to sign in. Give
them a confidential pre-registered client with `client_credentials: true`
(the OAuth Client Credentials extension); it may have no redirect URIs. It
asks the token endpoint directly:

```sh
curl -sS https://mcp.example.com/token \
  -u acme-nightly:$ACME_NIGHTLY_SECRET \
  -d grant_type=client_credentials \
  -d scope=reports:read \
  -d resource=https://mcp.example.com/mcp
```

The reply has an `access_token` and no `refresh_token`: the client asks again
when the token expires. The token's grant has the client ID as its subject,
so `context.authorization()` tells your handler which service called. A
requested `scope` must be within the client's `scopes`. Without one, the token
gets the `default_scopes` that the client may request, as at sign-in; when you
set no default scopes, that is none, and only tools that require no scope
accept the token. Give machine clients a default, or have them always send
`scope`. Public clients, registered clients and clients without the flag get
`unauthorized_client`.

Each client holds at most `client_grants` live tokens (16 unless you set it,
up to 1,024). A new token replaces the client's oldest, so one busy service
cannot fill the store, but a replaced token stops working before it expires.
When many instances share one client ID, have each reuse its token until it
expires, give each instance its own client, or raise `client_grants` above the
number of instances.

To tell modern clients that machine clients are supported, advertise the
extension in `server/discover` (see [advertised extensions](stdio.md#advertised-extensions)):

```rust
session_options.extensions.insert(
    canonic_mcp_kit::oauth::CLIENT_CREDENTIALS_EXTENSION.into(),
    serde_json::json!({}),
);
```

Keep machine secrets in a secrets manager and rotate them. Signed JWT client
assertions (`private_key_jwt`), which the extension recommends, are not
supported; use an external authorization server for them.

## Enterprise users

In a company that manages access centrally, users sign in to the company's
identity provider (Okta, Entra ID and others), not to your server, and the
provider decides who may use which AI client. The Enterprise-Managed
Authorization extension supports this. The client signs the user in at the
provider, asks it for an *identity assertion* (a signed ID-JAG) addressed to
your authorization server, and trades it at your token endpoint for an access
token. The user never sees your sign-in page.

| The library | You |
| --- | --- |
| Verifies the assertion's RS256 or ES256 signature with the provider's keys | Choose which providers to trust |
| Checks its type, audience, client, resource, lifetime and `tenant`, and refuses a replay | Fetch and cache each provider's key set |
| Issues an access token, without a refresh token | Map each enterprise user to your own subject and scopes |

Register the AI client as a confidential client with `identity_assertions:
true`, using the client ID and secret the provider's administrator configured
for your server. It may have no redirect URIs. Then trust the providers and
supply an `IdentityAssertions` implementation:

```rust,ignore
use canonic_mcp_kit::oauth::{
    AssertionDecision, IdentityAssertion, IdentityAssertions, TrustedIssuer,
};

struct Enterprise {
    keys: KeyCache, // your HTTP client and cache, keyed by issuer
}

impl IdentityAssertions for Enterprise {
    fn key_set(&self, issuer: &str, refresh: bool) -> Result<Vec<u8>, String> {
        // The JSON Web Key Set at the `jwks_uri` of the issuer's metadata.
        // `refresh` means the cached set lacks the assertion's key: fetch
        // again, no more often than your rate limit allows.
        self.keys.get(issuer, refresh).map_err(|error| error.to_string())
    }

    fn admit(&self, assertion: &IdentityAssertion<'_>) -> AssertionDecision {
        // Identify users by issuer and `sub`, never by email alone.
        match accounts::find(assertion.issuer, assertion.subject) {
            Some(account) => AssertionDecision::Approve {
                subject: account.id,
                scopes: assertion.scopes.to_vec(),
            },
            None => AssertionDecision::Deny,
        }
    }
}

let server = AuthorizationServer::new(issuer, &resource, store, Arc::new(SignInPages))?
    .with_identity_assertions(
        vec![TrustedIssuer::new("https://acme.okta.com")],
        Arc::new(Enterprise { keys }),
    )?;
```

The client presents the assertion with its own credentials:

```sh
curl -sS https://mcp.example.com/token \
  -u acme-agent:$ACME_AGENT_SECRET \
  -d grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer \
  -d assertion=$ID_JAG
```

The reply has an `access_token` for the subject you approved and the granted
`scope`, and no `refresh_token`: when the token expires, the client gets a new
assertion from the provider. The scopes offered to `admit` are the
assertion's `scope` (or your default scopes without one), limited to the
client's `scopes` and narrowed by a `scope` the client sends; you may narrow
them further. `Deny` gives the client `access_denied`. An assertion that was
already used is refused before `admit` runs, but two simultaneous requests with
the same assertion can both reach it while only one gets a token, so keep
side effects in `admit`, such as creating an account on first sign-in, safe to
repeat.

- **Algorithms and tenants.** `TrustedIssuer::new` accepts RS256 and ES256.
  Set `algorithms` to what the provider actually uses. For a provider that
  serves many companies from one issuer, set `tenant` to your company's
  tenant, so other tenants' assertions are refused.
- **Bounds.** An assertion may be at most 8 KiB and valid for at most an
  hour from when it arrives, with 60 seconds of clock skew. A key set may be
  at most 64 KiB with 32 keys; RSA keys must be 2,048 to 4,096 bits.
- **Failures.** Anything wrong with the assertion gets `invalid_grant`. A
  client without a secret gets `invalid_client`, and one without
  `identity_assertions` gets `unauthorized_client`. When `key_set` fails, or
  returns something that is not a key set, the client gets `503`.
- **Not supported.** Sender-constrained assertions (`cnf`), encrypted
  assertions and clients that authenticate with `private_key_jwt`, including
  Client ID Metadata Document clients.

Each user holds at most `client_grants` live tokens per client, as machine
clients do, so one user's tokens never replace another's. Advertise the
extension in `server/discover` the same way as for machine clients, with
`canonic_mcp_kit::oauth::ENTERPRISE_MANAGED_AUTHORIZATION_EXTENSION`.

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
replayed. The library answers the client as soon as a method returns, so
return success only after the change survives a restart. This matters most
for `take_pending`, `take_code`, `rotate`, `revoke_family` and
`create_assertion_grant`: if one of them is lost, a used code works again, a
replaced refresh token becomes current again, a revoked sign-in comes back,
or a used identity assertion works again until it expires. A lost insertion only signs someone
out early, and a lost `purge_expired` is harmless. If your backend can lose
acknowledged writes, tell your operators about that risk.

The library removes expired records during token requests. A full
`MemoryStore` drops the oldest pending sign-in, because anyone can start one,
and otherwise refuses with `503` only when every record is still live.

## Endpoints and errors

| Endpoint | Method | Errors |
| --- | --- | --- |
| `/.well-known/oauth-authorization-server` + issuer path | GET | — |
| `/authorize` | GET | A `400` page for an unknown client or redirect URI; otherwise a redirect with `error`, `state` and `iss` |
| `/token` | POST form | RFC 6749 JSON errors: `invalid_client` (`401`), `invalid_grant`, `invalid_scope`, `invalid_target`, `unauthorized_client`, `unsupported_grant_type`; for identity assertions also `access_denied` and `server_error` (`500`) when `admit` widens the scopes or returns an invalid subject; `503` without a body when the store or a key set is unavailable |
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
