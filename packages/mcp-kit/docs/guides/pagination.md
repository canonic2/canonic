# List tools in pages

Tool discovery returns one page at a time so a large catalog does not have to
fit into a single response. The library handles paging automatically; your
handler supplies tools and implements their calls. No extra capability flag
or handler branch is needed.

## Try a multi-page catalog

From `packages/mcp-kit`, run:

```sh
cargo run --locked --example pagination
```

The [example](../../examples/pagination.rs) publishes 130 status tools. Paste
this modern request as one line:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
```

The result contains the first 64 tools and a `nextCursor` string. Copy that
string unchanged into the next request:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"cursor":"<nextCursor from the previous result>","_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
```

Replace the placeholder with the actual cursor. Repeat while `nextCursor` is
present. This example returns pages of 64, 64 and 2 tools. The final page has
no `nextCursor`. An empty catalog returns `tools: []` without a cursor.
Close stdin to stop the server.

Both legacy revisions support the same paging flow after their initialization
handshake. Legacy requests can omit modern metadata, and their results omit
modern fields. Modern pages retain `resultType: "complete"`, server identity,
and the `ModernOptions::tools_cache` hints, defaulting to zero TTL/private scope.

## Follow the cursor contract

- Begin by omitting `cursor`. Only `cursor` and `_meta` are accepted parameters.
- Forward returned cursors unchanged. Do not parse them, build offsets or assume
  that another server uses the same page size.
- Stop when `nextCursor` is absent; clients must support opaque strings,
  including empty strings from servers that issue them.
- Keep the protocol revision consistent throughout traversal. A cursor from
  another revision, catalog or listing kind is invalid.

MCP Kit emits nonempty cursors of at most 128 bytes. Malformed, foreign and
out-of-range tool cursors return `-32602`. Restart listing without a cursor
after a catalog change or an invalid-cursor error. Cursors are stable across
reconnects when catalog content/order and protocol revision are identical.
They carry no authority or retained server-side session state.

Each page preserves the declaration order in `Catalog::new`. Legacy pages
exclude tools whose output schema is incompatible with legacy clients before
applying the page boundary. Hidden tools therefore do not create empty pages.
Modern pages include the entire published catalog over the full traversal.

## Runtime and output limits

Listing is a core reply; it never runs a consumer handler or waits for a worker
lane. It remains responsive while tools are busy. Catalog preparation derives
cursor identity and legacy visibility once; later requests touch only the
selected page entries without re-preparing schemas or hashing the catalog.

MCP Kit returns at most 64 tools per page. That count does not guarantee a
page fits a smaller configured output frame or unusually large descriptions
and schemas. Existing output limits still apply: an oversized page fails with
an internal error, without a partial list, skipped entries or truncated data.
Reduce tool metadata or increase the server's output budget when needed.

Independent runtimes driving `Session` receive the same paginated
`Inbound::Reply` messages. For direct catalog access, use
`Catalog::list_page(&params, protocol_version)` for a revision-aware domain
page. The supported version strings are `2025-06-18`, `2025-11-25` and
`2026-07-28`; unknown revisions fail. `Catalog::listing()` retains the full
unfiltered catalog payload, and `Catalog::tools()` returns the full slice.
See [driving a session](session.md) for transport and ID ownership.

[Resources](resources.md), [prompts](prompts.md) and [Skills](skills.md) have
their own cursor contracts. Enable [subscriptions](subscriptions.md) to publish
prepared catalog replacements and notify opted-in modern hosts to refresh.
