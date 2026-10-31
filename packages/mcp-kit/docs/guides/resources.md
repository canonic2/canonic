# Publish resources and templates

Resources let a host retrieve guides, reports and captures by URI. Use a
resource for a known artifact and a template for addresses selected by an
identifier, such as `acme://reports/{id}`. Your server supplies the bytes and
decides which addresses the caller may read. The library never opens a URI.

## Serve a guide and reports

This complete stdio server publishes a guide and a report template. Add it as
an executable target in a Rust package depending on `canonic-mcp-kit` and
`serde_json`, using the dependency setup in the [quick start](../getting-started.md).

```rust,ignore
use canonic_mcp_kit::{
    Catalog, Client, ReadResourceResult, Resource, ResourceCatalog,
    ResourceContents, ResourceTemplate, RpcError, ServerInfo, rpc,
    stdio::{self, Context, Handler, Outcome, Request},
};
use std::{io, sync::Arc};

struct Acme;

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1.0"))
    }

    fn call(&self, request: Request, context: &Context) -> Outcome {
        let Request::Method { method, params } = request else {
            return Outcome::Method(Err(RpcError::new(rpc::METHOD_NOT_FOUND, "Unknown method")));
        };
        if method != "resources/read" {
            return Outcome::Method(Err(RpcError::new(rpc::METHOD_NOT_FOUND, "Unknown method")));
        }
        // Session already checked that uri is a concrete URI string.
        let uri = params["uri"].as_str().expect("validated URI");
        let text = match uri {
            "acme://guide" => "# Acme\nRead acme://reports/latest for the current report.",
            "acme://reports/latest" => "All checks completed.",
            _ => return Outcome::ResourceNotFound(uri.to_owned()),
        };
        context.progress(1.0, Some(1.0), Some("Read artifact"));
        let result = ResourceContents::text(uri, Some("text/markdown".into()), text)
            .and_then(|content| ReadResourceResult::new(vec![content]));
        match result {
            Ok(result) => Outcome::Resource(result),
            Err(error) => Outcome::Method(Err(RpcError::new(rpc::INTERNAL_ERROR, error.to_string()))),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut guide = Resource::new("acme://guide", "Guide");
    guide.mime_type = Some("text/markdown".into());
    let resources = ResourceCatalog::new(
        vec![guide],
        vec![ResourceTemplate::new("acme://reports/{id}", "Reports")],
    )?;
    let mut options = stdio::Options::default();
    options.session.resources = Some(Arc::new(resources));
    stdio::serve(
        Arc::new(Acme), Arc::new(Catalog::new(vec![])?),
        io::BufReader::new(io::stdin()), io::stdout(), &options,
    )?;
    Ok(())
}
```

After initialization, `resources/list` returns the guide and
`resources/templates/list` returns the report template. A host expands the
template and calls `resources/read` with `{"uri":"acme://reports/latest"}`.
The reply contains `contents` with the report URI, MIME type and text.
Unknown or inaccessible addresses return `-32002` with `data.uri`.

Replace the match with your own resolver to serve bundled or generated content.
Check authorization in that resolver. Templates publish address syntax; they
do not grant access. URIs may name unlisted resources. Exact spelling, including
percent escapes, reaches the handler unchanged; decoding and path confinement
belong to your resolver.

## Return artifacts from tools

Use a resource link when a report or capture should be retrieved separately.
Embed resource contents when the host needs the bytes with the tool result.
Both work alongside text, images and structured data in all three supported
protocol revisions.

Inside a tool handler, build the result with validated resource types:

```rust
use canonic_mcp_kit::{Content, Resource, ResourceContents, ResourceError, ToolResult};

fn report_result() -> Result<ToolResult, ResourceError> {
    let mut report = Resource::new("acme://reports/latest", "Report");
    report.mime_type = Some("text/plain".into());
    Ok(ToolResult::success(vec![
        Content::text("Checks passed. Read the report for details."),
        Content::resource_link(report)?,
        Content::resource(ResourceContents::text(
            "acme://summary", Some("text/plain".into()), "Passed",
        )?),
        Content::resource(ResourceContents::blob(
            "acme://evidence", Some("application/octet-stream".into()), &[0, 255, 42],
        )?),
    ]))
}
```

Return this through `Outcome::Tool`. Invalid server-generated metadata or
content is an internal error; build and validate the result before publishing
success. The resource blocks serialize as:

```json
[
  {"type":"resource_link","uri":"acme://reports/latest","name":"Report","mimeType":"text/plain"},
  {"type":"resource","resource":{"uri":"acme://summary","mimeType":"text/plain","text":"Passed"}},
  {"type":"resource","resource":{"uri":"acme://evidence","mimeType":"application/octet-stream","blob":"AP8q"}}
]
```

The [complete Acme server](../../examples/artifacts.rs) returns these blocks
and serves the full report through `resources/read`. Build it from the package
directory with `make dev CARGO_FLAGS='--example artifacts'`; the executable is
`target/debug/examples/artifacts`. A call to `acme_report` with `{}` returns a
text block, the report link and embedded summary and evidence; reading
`acme://reports/latest` returns the full report.

`Content::resource_link` validates a `Resource` and stores an immutable
`ResourceLink`. Its `resource()` accessor exposes the URI, name and optional
title, description, MIME type and raw size hint. A link need not appear in
`resources/list`. Returning it does not enable resource methods, add a catalog
entry, fetch bytes or grant access. Configure `SessionOptions::resources` and
a read resolver when serving the URI through this server; an empty catalog is
enough to enable reads. Servers embedding resources should also enable the
resources capability. Consumers own artifact retention, authorization and
the reply when an artifact expires or becomes inaccessible.

`Content::resource` takes an already validated `ResourceContents`; it adds no
read or worker job. Each embedded block permits at most 1 MiB of raw text or
binary data. The combined tool result must fit the output frame limit, including
JSON escaping and base64 expansion; Task results must also fit their snapshot
budget. No content is truncated. Deserialization validates resource URIs,
metadata and content bounds, requires exactly one of `text` or `blob`, and
requires canonical padded standard base64 for binary resources. Task restoration
preserves the blocks and refuses data it would discard. Host rendering and
retrieval support depend on the host.

## Catalog and content reference

Resources, templates and tool-result blocks support optional audience, priority
and modification hints through `ContentAnnotations`. See
[Annotate content and resources](audio.md#annotate-content-and-resources) for
construction, bounds and how hints differ from access and cache policy.

`SessionOptions::resources` defaults to `None`: resource methods return `-32601`.
`Some` enables the capability, including when the catalog is empty. Catalogs
are immutable and list in insertion order. Subscriptions, change notifications
and completion are not implemented. The default protocol versions are
`2025-06-18` and `2025-11-25`; these paths do not publish cache hints.
Explicit [modern configuration](skills.md#protocols-context-and-caching) also
supports `2026-07-28`, including per-request context and cache policies.
Use `Outcome::ResourceNotFound` for revision-aware errors: `-32002` for legacy
calls and `-32602` for modern calls.

Resource definitions have public `uri`, `name`, `title`, `description`,
`mime_type` and `size` fields. Templates have `uri_template`, `name`, `title`,
`description` and `mime_type`. Constructors set optional fields to `None`.
`ResourceCatalog::new` validates metadata, concrete RFC 3986 URIs, RFC 6570
templates and duplicate addresses. Relative templates are allowed. Invalid
catalogs return `ResourceError` before a session starts.

| Bound | Value |
| --- | --- |
| Resources and templates combined | 1024 entries |
| Combined encoded catalog metadata | 1 MiB |
| URI or template | 4096 bytes |
| Name or title | 4096 bytes each |
| Description | 16384 bytes |
| MIME type | 256 bytes |
| List page | 64 entries |
| Contents per read | 1–64 |
| Aggregate raw text/blob data per read | 1 MiB |

List calls accept an optional string `cursor`. Pass `nextCursor` from the
previous result unchanged to the same list method; stop when it is absent.
Invalid, cross-kind and out-of-range cursors return `-32602`. Read calls
require a string `uri`. All three methods accept optional object `_meta` and
refuse other parameters. A read's `_meta.progressToken` may be a string or
integer.

`ResourceContents::text` accepts UTF-8 text. `ResourceContents::blob` accepts
raw bytes and encodes them as base64. Both accept an optional MIME string.
Use `ReadResourceResult::new` to combine them; it refuses empty results and
excessive counts or bytes. These constructors return `ResourceError` for
invalid server output; report that as an internal error, rather than blaming
the client's parameters. The driver also enforces its configured output-frame
limit, returning `-32603` for replies that exceed it.

## Scheduling and other runtimes

Read requests arrive as `Request::Method` with method `resources/read`.
`Handler::admit` selects their lane, key and deadline. They receive the usual
context and progress token. Queued and running reads finish after EOF, up to
their deadlines. Cancellation suppresses late replies and progress. Discovery
is answered directly by the session, without taking a worker lane.

For another runtime, set the same catalog in `SessionOptions`, handle
`Inbound::Method` reads, build a `ReadResourceResult`, and encode it with
`rpc::encode_success` for legacy calls or the captured request context's
`encode_success` for modern calls. Send that reply only if `Session::finish` returns
`true`. Your runtime owns scheduling and propagating cancellation to its work;
see [Drive the session yourself](session.md).
