# Notify hosts about changes

Enable subscriptions when your server changes its tool, prompt or resource
catalog, or serves resource bytes that can change. Hosts choose what to follow.
The library keeps each stream open without reserving a worker. Your server
decides when to publish a change and which resource addresses are eligible.

## Try a catalog change

From `packages/mcp-kit`, run:

```sh
cargo run --locked --example subscriptions
```

Open a stream with this single-line modern request:

```json
{"jsonrpc":"2.0","id":"changes","method":"subscriptions/listen","params":{"notifications":{"toolsListChanged":true},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
```

The first message is `notifications/subscriptions/acknowledged`. Its
`notifications` reports the accepted filters, and
`params._meta["io.modelcontextprotocol/subscriptionId"]` is `"changes"`.
This acknowledgement keeps the request open. Then publish the example's
expanded catalog:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"acme_refresh","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
```

You receive the tool result and `notifications/tools/list_changed`; their
relative order is unspecified. The notification carries the stream ID.
Listing tools now includes `acme_status`. Follow [pagination](pagination.md)
when refreshing a larger catalog. Cancel the stream with:

```json
{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"changes"}}
```

Cancellation sends no result. Closing stdin completes any remaining streams
and ends the example.

## Enable and publish

Set `options.session.modern` and
`options.session.subscriptions = Some(SubscriptionOptions::default())`.
Implement `Handler::connected` to retain its `ChangeSender`; the callback runs
once per enabled connection and must return quickly. Send prepared changes:

| Change | Effect |
| --- | --- |
| `Change::Tools(Arc<Catalog>)` | Replace tools before notifying tool-list subscribers |
| `Change::Prompts(Arc<PromptCatalog>)` | Replace an enabled prompt catalog before notifying |
| `Change::Resources(Arc<ResourceCatalog>)` | Replace enabled resources, preserving bundled Skills, before notifying |
| `Change::ResourceUpdated(uri)` | Notify subscribers after your resource bytes change |

The sender's `send` is nonblocking. `SendChangeError::Full`, `Closed` and
`Invalid` retain the unsent change. Handle pressure explicitly: retain or
coalesce a change in bounded consumer state, or report publication failure.
Successful enqueueing does not acknowledge delivery. A retained sender does
not keep a closed connection alive. The queue holds at most 16 changes, shared
by its cloned senders. Disabled prompt/resource catalogs and invalid resource
addresses return `Invalid` before enqueueing. Resource composition with bundled
Skills is checked by the dispatcher; a namespace collision or combined-catalog
limit failure ends serving with a configuration error without installing the
rejected catalog.

Resource updates require an enabled resource catalog and an explicit list in
`SubscriptionOptions::resource_uris`. Populate that list for the connection's
access scope. Matching is exact: neither parent URIs nor wildcards include
other addresses. Subscriptions do not grant read or write permission.

## Filters and lifetime

The request's required `notifications` object accepts optional boolean
`toolsListChanged`, `promptsListChanged`, `resourcesListChanged`, and
`resourceSubscriptions` containing an array of absolute URI strings. Empty
filters are valid. Unsupported or unapproved selections are omitted from the
acknowledgement; false flags subscribe to nothing. Unknown fields and malformed
filters return `-32602`. Listen requests do not accept retry inputs.

`SubscriptionOptions` defaults to 16 simultaneous streams and a five-minute
lifetime. `max_streams` accepts 1–64; `timeout` accepts one millisecond through
24 hours. Filters and allowlists accept at most 64 resource URIs and 64 KiB of
encoded JSON; individual URIs follow the resource URI budget. Duplicate
requested URIs become one selection; duplicate allowlist entries are invalid.
Capacity exhaustion returns `-32000` without retaining a stream.

Expiry and graceful EOF complete the original RPC with `resultType: "complete"`,
the subscription ID in result metadata and server identity. A host can renew by
opening another stream; no events are replayed across streams or reconnects.
The listen ID stays reserved until completion or cancellation, so use another
ID for ordinary calls. Progress notifications belong to their original calls
and never carry a subscription ID.

Output limits apply to every frame. An oversized acknowledgement returns an
error and releases its ID. An oversized stream notification ends that stream
with an error instead of silently losing an event.
Final results also obey the output budget. Their server identity can exceed a
budget that admitted the acknowledgement; the stream then ends with an internal
error and releases its ID.

Only the modern protocol supports this API. Legacy subscribe/unsubscribe
methods are unavailable, and legacy initialization does not advertise change
notifications.

## Drive the core directly

Send `Inbound::Subscription::acknowledged` before publishing any stream data.
Use `Session::publish` for consumer changes and send all returned frames in
order. Drive `Session::expire_subscriptions` from a monotonic timer using
`subscription_wait`; use `end_subscription` or `end_subscriptions` for graceful
closure. `Session::close` releases streams on abrupt closure. Independently
driven runtimes own output limits, backpressure and acknowledgement delivery.
See [session ownership](session.md).
