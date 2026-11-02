# Publish metadata and inspect trace context

Use `Metadata` for bounded JSON extension values and `Icon` for display hints.
The library retains these values without fetching icons or exporting telemetry.
Neither metadata nor a self-reported client identity grants authority.

## Describe implementations and catalogs

`ServerInfo` has `title`, `description`, `website_url` and `icons` fields.
Instructions are separate from the implementation identity. Tools, prompts,
resources and resource templates each have `meta: Option<Metadata>` and
`icons: Vec<Icon>`; their constructors use `None` and an empty vector.
Catalog preparation checks and retains these declarations for discovery.

```rust
use canonic_mcp_kit::{Icon, IconTheme, Metadata, Tool};
use serde_json::json;

let mut tool = Tool::new("acme_status", "Read status", json!({"type":"object"}));
tool.meta = Some(Metadata::new(json!({"com.acme/category":"reports"}))?);
tool.icons.push(Icon::new(
    "https://example.com/icon.png",
    Some("image/png".into()),
    vec!["24x24".into(), "any".into()],
    Some(IconTheme::Light),
)?);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Icon sources are HTTPS URLs or canonical padded base64 `data:image/...` URIs.
Size hints are `any` or positive `WxH` dimensions through 65,535. Each declaration
allows eight icons, 16 size hints per icon and 64 KiB encoded icons in total.
HTTPS sources allow 4,096 bytes; inline image data allows 48 KiB decoded bytes.
Hosts own origin checks, retrieval and safe rendering. Website URLs accept
absolute HTTP/HTTPS addresses; user-info credentials are refused.

Client identities retain all supplied fields in `Client::info` for legacy
initialization and `RequestContext::client_info` for modern requests. Known
display fields are checked; additional fields remain opaque.

## Attach metadata to content and results

`Content::with_metadata` covers text, image, audio, embedded resources and
resource links. `ResourceContents::with_metadata` attaches metadata inside an
embedded or read resource. `ToolResult`, `ReadResourceResult`, `GetPromptResult`
and `CompleteResult` also have `with_metadata` methods; the latter two return
`Result` because metadata counts toward their aggregate byte budget.

```rust
use canonic_mcp_kit::{Content, Metadata, ToolResult};
use serde_json::json;

let meta = Metadata::new(json!({"com.acme/reportId":"example-1","extra":null}))?;
let result = ToolResult::success(vec![Content::text("running")])
    .with_metadata(meta);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Metadata::new` requires an object, at most 64 KiB encoded JSON and 16 nested
containers. Keys follow the MCP prefix/name grammar: an optional dotted prefix
and slash, followed by an alphanumeric name allowing internal `.`, `_` and `-`.
The name may be empty. Null extension values and explicit empty objects are
preserved; `_meta: null` is invalid. Clones share immutable validated storage.
Prompt arguments and messages have no metadata field.

Modern checked encoders merge result metadata and the configured server identity
into one `_meta` object. Supplying protocol-owned identity, subscription or
request-control keys refuses encoding. Legacy replies retain custom metadata
without modern envelopes or cache hints. Incoming metadata is never echoed
automatically. Task snapshots retain metadata on their nested tool result.

## Read incoming propagation strings

For modern requests, `Session::request_context` and
`stdio::Context::request_context` expose the same captured context:

```rust,ignore
if let Some(protocol) = context.request_context() {
    let raw = protocol.metadata().as_map();
    let trace = protocol.trace_context();
    let parent = trace.traceparent();
    let state = trace.tracestate();
    let baggage = trace.baggage();
    // Pass selected values to your own tracing system.
}
```

Malformed propagation strings refuse the request with `-32602` before work runs.
Traceparent validates lowercase hex, nonzero IDs and the versioned prefix,
within 256 bytes. Tracestate allows 32 members and 512 bytes, with unique vendor
keys; it is exposed for tracing only when traceparent exists. Its raw value
remains in metadata otherwise. Baggage allows 180 entries and 8 KiB; percent
escapes, properties, order and duplicate keys are retained without decoding.
Baggage does not require a traceparent. Complete incoming metadata allows 1 MiB
and 32 nested containers, subject also to the runtime's input frame limit.

Context stays with the admitted request through queueing and continuations.
Each new request, including a reused RPC ID, gets its own context. Your consumer
chooses propagation, baggage filtering, spans, sampling and exporters. Avoid
using baggage to make authorization decisions.

The [runnable example](../../examples/metadata.rs) drives the core directly and
uses `RequestContext::encode_success`; stdio uses the same encoding boundary.
Run it with `cargo run --example metadata` from the package directory.
