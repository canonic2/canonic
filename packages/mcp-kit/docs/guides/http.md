# Serve over Streamable HTTP

Serve your tools to remote-capable hosts over Streamable HTTP for protocol
revision `2026-07-28`. Every request is an independent POST: there are no
sessions, no GET stream and no initialization. The same `stdio::Handler` serves
both transports, so lanes, keys, deadlines, progress and cancellation behave as
in [stdio](stdio.md).

Without [bearer token protection](authorization.md), the server authenticates
no one: bind a loopback address. The server has no TLS; terminate it in front
of the server before exposing it further.

## Try the example

From `packages/mcp-kit`, run:

```sh
cargo run --locked --example http --features http-server
```

It serves `acme_status` at `http://127.0.0.1:8080/mcp`. In another terminal,
call it:

```sh
curl -sS http://127.0.0.1:8080/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2026-07-28' \
  -H 'Mcp-Method: tools/call' \
  -H 'Mcp-Name: acme_status' \
  -H 'Mcp-Param-Region: eu-west' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"acme_status","arguments":{"region":"eu-west"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}'
```

The reply is one `application/json` JSON-RPC response with ID `1` whose text
content is `Acme eu-west is healthy.`. Remove the `Mcp-Param-Region` header and
the server answers `400` with error `-32020` instead: the header must mirror
the argument.

## Run the server

Enable the `http-server` feature, set modern options and call `http::serve` with
a bound listener:

```rust,ignore
let mut options = stdio::Options::default();
options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
let listener = TcpListener::bind("127.0.0.1:8080")?;
let shutdown = Shutdown::new();
http::serve(handler, catalog, listener, &HttpOptions::default(), &options, &shutdown)?;
```

`serve` runs on the calling thread until another thread calls
`shutdown.shutdown()`. `Handler::initialize` is never called. When
subscriptions are enabled, `Handler::connected` receives one change sender for
the whole server. A session message limit (`SessionOptions::max_messages`) is
refused, because one session serves every POST.

| `HttpOptions` field | Default | Range | Purpose |
| --- | --- | --- | --- |
| `path` | `/mcp` | Absolute path | The only path served; others get `404` |
| `allowed_origins` | None | Up to 64 `http`/`https` origins | Browser origins allowed to call; any other `Origin` gets `403` |
| `max_connections` | 64 | 1–1,024 | Connections served at once; more get `503` |
| `read_timeout` | 10 s | 1–60 s | Time to receive the whole request; a slow client is closed |
| `write_timeout` | 30 s | 1–300 s | Time for one write; a stalled client counts as closed |
| `keep_alive` | 15 s | 1–300 s | Comment interval on a quiet listen stream |
| `authorization` | None | — | Bearer token protection; see [authorization](authorization.md) |

The request body limit is `SessionOptions::max_message_bytes`.

## Send requests

Each POST carries one JSON-RPC request or notification with
`Content-Type: application/json` and an `Accept` that allows both
`application/json` and `text/event-stream`. A request also needs:

| Header | Value |
| --- | --- |
| `MCP-Protocol-Version` | `2026-07-28`, matching the body's `_meta` version |
| `Mcp-Method` | The body's `method` |
| `Mcp-Name` | For `tools/call` and `prompts/get`, `params.name`; for `resources/read`, `params.uri`; absent otherwise |

Write a value that is not plain visible ASCII as `=?base64?{data}?=`, the
padded standard base64 of its UTF-8 bytes. Request IDs belong to their POST:
concurrent POSTs may reuse an ID.

A notification gets `202` with no body and needs no MCP headers. Only product
notifications listed in `SessionOptions::extension_notifications` whose
`params._meta` names protocol version `2026-07-28` reach your handler;
`notifications/cancelled` has no effect, because closing the connection cancels.

## Mirror arguments into headers

Add `x-mcp-header` to an input schema property so HTTP intermediaries can route
on an argument without reading the body:

```json
{"type":"object","properties":{"region":{"type":"string","x-mcp-header":"Region"}}}
```

Clients then send `Mcp-Param-Region` with the argument's value. The server
refuses the call with `-32020` when the header is missing for a present
argument, present for an absent one, repeated, or different from the body.
Integers compare numerically, so `42.0` and `4.2e1` match `42`. Headers that
match no annotation are ignored.

`Catalog::new` checks each annotation: a nonempty HTTP token, unique ignoring
case, on a property with exactly one type of `string`, `integer` or `boolean`,
reached only through `properties`, and at most 16 per tool. An invalid
annotation fails construction with `CatalogError::Schema` naming the tool.

## Responses, progress and cancellation

Most requests get one `application/json` response. A request whose `_meta`
carries a `progressToken`, and `subscriptions/listen`, get an SSE stream
instead: one `event: message` per notification, then the final reply. Every
response closes its connection.

Closing the connection before the reply cancels the request with the same
effect as stdio cancellation: queued work never runs, and running work sees
`Context::check` fail. Nothing more is written for it. Shutting down only the
sending side after the request counts as closing, so keep the connection fully
open until the reply arrives.

## Listen for changes

With subscriptions enabled, a `subscriptions/listen` POST stays open as an SSE
stream. Its first event is the acknowledgement; changes your handler publishes
follow as notifications, with comment lines at the keep-alive interval while
quiet. The stream ends with its final result at expiry or shutdown, and without
a result when the client closes it or falls 64 notifications behind.
`SubscriptionOptions::max_streams` bounds streams across the whole server. See
[subscriptions](subscriptions.md) for filters and publishing.

## Shut down

`Shutdown::shutdown` stops accepting connections and ends listen streams with
their final results. Admitted work follows the stdio
[disconnection rules](stdio.md): queued mutations get `-32800`, running
mutations are cancelled after `Options::disconnect_grace`, and read-only work
finishes. `serve` returns once every connection has closed.

## Statuses and refusals

| Status | When |
| --- | --- |
| `200` | A reply, including ordinary JSON-RPC errors such as `-32602` and deadlines |
| `202` | A notification |
| `400` | A header mismatch (`-32020`), a malformed message, conflicting framing, or a malformed bearer token |
| `401` | With authorization: a missing, invalid, foreign or expired token |
| `403` | An `Origin` outside the allowlist, or with authorization a missing scope |
| `404` | Another path, or an unknown method (`-32601`) |
| `405` | GET, DELETE or any method other than POST, with `Allow: POST` |
| `406` / `415` | `Accept` lacks JSON or SSE / the body is not `application/json` |
| `411` / `413` | No `Content-Length` / a body over the message limit |
| `417` | An `Expect` other than `100-continue` |
| `431` | A request line over 8 KiB, or headers over 32 KiB or 100 fields |
| `501` / `505` | `Transfer-Encoding` / an HTTP version other than 1.1 |
| `503` | More connections than `max_connections`, or a token verifier that could not decide |

A legacy client that sends `initialize` gets `400`: the server speaks only the
modern revision.

## Use your own HTTP server

Without the feature, `http::Endpoint` checks requests for any HTTP server.
Call `Endpoint::receive` with the method, target, headers and body. It returns
a complete `Response`, a notification, or an admitted request with its message
and response form. Check `Admitted::check_parameters` against your catalog,
give the message a session-unique ID before `Session::receive_value`, and
encode the reply with the client's ID through `RequestContext::encode_success`.
Send it with `Response::reply` or, for the stream form, `Response::stream` and
`http::sse_event`. Your runtime owns scheduling, connection limits, timeouts and
detecting closure. See [Drive the session yourself](session.md).
