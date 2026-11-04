# Protect an HTTP server with bearer tokens

Require OAuth access tokens on your [Streamable HTTP](http.md) server so it can
be reached beyond a loopback address. Your server acts as an OAuth *resource
server*: clients get tokens from an authorization server you name, such as
Auth0, Entra ID or Keycloak, and send them on every request. You decide whether
a token is genuine; the library handles everything around that decision.

Authorization is opt-in. Without it, the HTTP server authenticates no one and
should bind loopback only. Stdio never uses tokens.

## What the library does and what you do

| The library | You |
| --- | --- |
| Serves the protected-resource metadata clients use to find your authorization server | Choose the authorization server and the scopes |
| Reads the `Authorization: Bearer` header before the request body | Check the token's signature or ask your authorization server about it |
| Refuses tokens issued for another server, expired tokens and missing scopes | Report what the token grants: its audiences, scopes, expiry and subject |
| Answers `401`, `403` and `503` with standard `WWW-Authenticate` challenges | Decide which scopes each tool needs |
| Gives your handler the grant, never the token | Decide what the grant permits |

## Try the example

From `packages/mcp-kit`, run:

```sh
cargo run --locked --example authorization --features http-server
```

It serves `acme_reports` at `http://127.0.0.1:8080/mcp` and accepts the demo
token `acme-demo-reader`. Discover the authorization server:

```sh
curl -sS http://127.0.0.1:8080/.well-known/oauth-protected-resource/mcp
```

The reply names the resource, `https://auth.example.com` as its authorization
server and `reports:read` as a supported scope. A request without a token gets
`401` with a challenge pointing at that document:

```sh
curl -sS -i http://127.0.0.1:8080/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{}'
```

With the token, call `acme_reports`:

```sh
curl -sS http://127.0.0.1:8080/mcp \
  -H 'Authorization: Bearer acme-demo-reader' \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2026-07-28' \
  -H 'Mcp-Method: tools/call' \
  -H 'Mcp-Name: acme_reports' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"acme_reports","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}'
```

The reply's text is `Acme reports are ready for demo-user.`, naming the
token's subject.

## Configure the resource

```rust,ignore
let mut resource = ResourceOptions::new(
    "https://mcp.example.com/mcp",
    vec!["https://auth.example.com".to_owned()],
);
resource.scopes_supported = Some(vec!["reports:read".to_owned()]);
let http = HttpOptions {
    authorization: Some(
        HttpAuthorization::new(ResourceServer::new(resource)?, Arc::new(MyVerifier))
            .with_policy(Arc::new(MyScopes)),
    ),
    ..HttpOptions::default()
};
```

The resource is the address clients use, not the address you bind: behind a
proxy that terminates TLS, use the public `https` URL. The library never reads
it from `Host` or forwarding headers. Plain `http` is accepted only for
loopback hosts, for development.

| `ResourceOptions` field | Default | Purpose |
| --- | --- | --- |
| `resource` | Required | The canonical URI tokens must be issued for; no query, fragment or trailing slash |
| `authorization_servers` | Required, 1–8 | Issuer URLs clients may get tokens from |
| `scopes_supported` | None | Scopes listed in the metadata; never `offline_access` |
| `required_scopes` | None | Scopes every request needs, checked before the body is read |
| `resource_name` | None | A display name for the metadata |
| `resource_documentation` | None | An `https` link to your documentation |
| `metadata_max_age` | 1 hour | How long clients may cache the metadata, up to 24 hours |
| `serve_root_metadata` | `true` | Also serve the metadata at `/.well-known/oauth-protected-resource` |

`ResourceServer::new` returns an `AuthorizationError` naming the invalid field.

## Verify tokens

Implement `TokenVerifier`. Return a `Grant` for a genuine token, and report the
audiences it was issued for: the library refuses the grant unless one of them
is your resource URI, so a token for another service never works here.

```rust,ignore
impl TokenVerifier for MyVerifier {
    fn verify(&self, token: BearerToken<'_>, resource: &str) -> Result<Grant, Rejection> {
        let claims = self.keys.check(token.as_str()).map_err(|_| Rejection::invalid())?;
        Ok(Grant::new(claims.audiences)
            .with_scopes(claims.scopes)
            .with_expiry(claims.expires_at)
            .with_subject(claims.subject))
    }
}
```

| Return | Response |
| --- | --- |
| `Ok(grant)` for this resource, unexpired, with the base scopes | The request continues |
| `Rejection::Invalid` | `401` with `error="invalid_token"` |
| `Ok(grant)` for another resource, or expired | `401` with `error="invalid_token"` |
| `Rejection::Unavailable`, such as when introspection is unreachable | `503`; the client may retry |

The server calls the verifier on the request's own connection thread, so a
slow verifier delays only that request. Your verifier owns its network
timeouts, key caching and rotation. A grant without an expiry is accepted; your
verifier is then the only judge of when the token stops working.

Attach your own authority context with `Grant::with_extension`, such as a
tenant or role set, and read it back with `grant.extension::<T>()`.

## Require scopes per tool

Implement `ScopePolicy` to name the scopes an operation needs. It sees the
method, the tool or prompt name or resource URI, and the parameters, so
requirements can depend on arguments:

```rust,ignore
impl ScopePolicy for MyScopes {
    fn required(&self, operation: &Operation<'_>) -> Vec<String> {
        match operation.name {
            Some("acme_deploy") => vec!["deploy:write".to_owned()],
            Some("acme_reports") => vec!["reports:read".to_owned()],
            _ => Vec::new(),
        }
    }

    // Optional: a broader scope implies narrower ones.
    fn satisfies(&self, grant: &Grant, scope: &str) -> bool {
        grant.has_scope(scope) || grant.has_scope("admin")
    }
}
```

A grant missing any required scope gets `403` with `error="insufficient_scope"`
and a `scope` listing every scope the operation needs, so the client can ask
for them in one step. Without a policy, any verified token may call anything.
Tool lists are not filtered: a listed tool can still answer `403`.

The policy also sees every notification, with its method and parameters and
no name. A notification missing a scope gets the same `403` instead of `202`
and never reaches your handler.

## Use the grant in your handler

`Context::authorization()` returns the grant on an authorized HTTP request and
`None` on stdio or unprotected HTTP:

```rust,ignore
let user = context.authorization().and_then(Grant::subject);
```

The grant never contains the token, so your handler cannot forward it. When
your tools call other APIs, get separate tokens for them.

If you use [tasks](tasks.md) or [input rounds](elicitation.md), any authorized
client can present a task ID or `requestState`. Record the grant's subject with
them and check it on every later request.

## Listen streams

A `subscriptions/listen` stream ends with its final result when its token
expires, or earlier at its normal lifetime. The scope policy sees the listen
request's filter, including each resource URI, so you can limit which updates a
grant may watch. Revoking a token does not end a stream it already opened.

## Challenges and statuses

| Status | When | Challenge |
| --- | --- | --- |
| `401` | No `Authorization` header | `Bearer` with `scope` (base or supported scopes) and `resource_metadata` |
| `400` | Another scheme, malformed or repeated header, oversized token, or `access_token` in the URL | `error="invalid_request"` |
| `401` | Invalid, foreign or expired token | `error="invalid_token"`, with the same `scope` as a missing token |
| `403` | A missing base or operation scope | `error="insufficient_scope"` with every required `scope` |
| `503` | The verifier could not decide | None |
| `500` | Your scope policy returned an invalid scope or more than 32 for a notification (a request gets a `-32603` reply instead) | None |

These responses have no body and carry `Cache-Control: no-store`. Every
challenge names the metadata URL in `resource_metadata`. Notifications need a
token too. Metadata requests need none and skip the `Origin` allowlist.

## Use your own HTTP server

Without the driver, set `EndpointOptions::authorization` and drive the checks
yourself, verifying the token however your runtime prefers, including
asynchronously:

1. `Endpoint::check_head` answers metadata requests and refuses a missing or
   malformed token, before you read the body.
2. `ResourceServer::bearer` returns the token; verify it.
3. `ResourceServer::check_grant` applies the audience, expiry and base scope
   checks to your verifier's outcome.
4. Read the body and call `Endpoint::receive`, then
   `Admitted::check_parameters` and `ResourceServer::check_scopes` for a
   request, or `ResourceServer::check_notification` for a notification before
   answering `202`.
5. Pass the grant to your own handler code.

See [Drive the session yourself](session.md) for the rest of the request.
