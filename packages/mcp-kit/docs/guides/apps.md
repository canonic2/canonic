# Publish an interactive MCP App

Enable an Apps catalog to pair a tool with a bundled HTML dashboard. Compatible
hosts render the document in their own sandbox and deliver tool results to it.
Your server still returns useful text for hosts that do not render Apps.

The complete [Acme example](../../examples/apps.rs) ships an
[inline dashboard](../../examples/apps/status.html), a status tool and an
app-only refresh tool. From `packages/mcp-kit`, run:

```sh
cargo run --locked --example apps
```

Configure an Apps-capable host to launch that command over stdio. The example
prints protocol frames on stdout. Its HTML is compiled into the executable;
no development server, build pipeline or remote assets are needed.

## Bundle and link the document

```rust
use canonic_mcp_kit::{AppsCatalog, Catalog, SessionOptions, Tool, ToolUi, UiResource};
use serde_json::json;
use std::sync::Arc;

let uri = "ui://acme/status-dashboard";
let apps = AppsCatalog::new(vec![UiResource::new(
    uri,
    "Acme status",
    "<!doctype html><html lang=\"en\"><title>Acme status</title><p>Healthy</p></html>",
)])?;
let tools = Arc::new(Catalog::new(vec![
    Tool::new("acme_show_status", "Show the Acme status.", json!({"type":"object"}))
        .with_ui(ToolUi::new(uri)),
])?);
let options = SessionOptions {
    apps: Some(Arc::new(apps)),
    ..SessionOptions::default()
};
canonic_mcp_kit::Session::try_new(tools, options)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `include_bytes!` with your built HTML in a real package. `UiResource` keeps
the exact UTF-8 bytes, fixes the MIME type to `text/html;profile=mcp-app`, and
publishes identical UI metadata on the resource listing and read contents.
Set `listed = false` to omit a document from listings while retaining reads and
tool links. `Some(empty_catalog)` enables the extension with no documents;
`None` disables it.

Construction refuses duplicate or invalid URIs, empty or invalid UTF-8 HTML,
conflicting MIME types and invalid metadata. A document URI needs `ui://`, an
authority and a nonempty path, without a query or fragment. Session construction
also refuses links to missing documents. With Apps enabled, ordinary resources,
templates, skill roots and resource-update allowlists cannot use the `ui`
scheme. Catalog replacements enforce the same checks atomically.
With subscriptions enabled, Apps-only sessions can publish ordinary resource
catalog replacements through `ChangeSender` or `Session::publish`; the bundled
UI documents remain intact. An explicit ordinary resource catalog is needed
to configure a nonempty resource-update allowlist at construction.

## Negotiate support and choose visibility

Apps is opt-in on `2025-11-25` and on modern `2026-07-28` requests. Modern
support still requires `SessionOptions::modern`; the HTTP driver serves only
that revision. The library declares `io.modelcontextprotocol/ui: {}` when
enabled. Do not add it to `SessionOptions::extensions` yourself.

Clients declare support in `capabilities.extensions` during legacy
initialization, or in the modern request's client-capabilities metadata:

```json
{"extensions":{"io.modelcontextprotocol/ui":{"mimeTypes":["text/html;profile=mcp-app"]}}}
```

With Apps enabled on a negotiating revision, malformed declarations return
`-32602`. Servers without Apps leave its settings opaque under the ordinary
capability-envelope checks. `2025-06-18` never validates or negotiates Apps.
An absent declaration, empty MIME list
or another MIME type means no Apps support. `2025-06-18` always uses that view.
Inspect `Context::supports_apps()` in a shared-driver handler. Modern context is
captured for each request, including its continuations; support never comes
from a client name.

`ToolUi::visibility` defaults to both model and app. To expose a refresh action
only to the frame, set `Some(vec![UiVisibility::App])`. Explicit visibility
arrays must be nonempty and contain distinct values. Hosts without support
never list an app-only tool and receive `-32602` if they call it. Filtering
precedes pagination, and cursors are bound to the selected view. Supported hosts
receive all tools and enforce which are shown to the model or frame.

Visibility and Apps support grant no authority. A frame calls tools through its
host using the same server-side permission and HTTP authorization checks as
other callers.

## Keep useful tool results

Return a human-readable summary alongside structured data for the dashboard:

```rust
use canonic_mcp_kit::{Content, ToolResult};
use serde_json::json;

let mut result = ToolResult::success(vec![Content::text("Acme is healthy.")]);
result.structured_content = Some(json!({"service":"Acme","status":"healthy"}));
```

A successful result from a UI-linked tool must contain at least one content
block. The checked encoders and `ResultSchema::validate_snapshot` enforce this,
including durable results before persistence and after restore. Error results
may have empty content. The library preserves structured data and text for
every host; choose useful fallback text in your handler.

## Request security and presentation metadata

Set `UiResource::ui` using `UiMetadata`, `UiCsp` and `UiPermissions`:

- `csp` declares `connectDomains`, `resourceDomains`, `frameDomains` and
  `baseUriDomains`. Use HTTPS origins; connections may also use WSS. HTTP is
  allowed only for `localhost`, `127.0.0.1` or `[::1]`; WS is also allowed for
  loopback connections. Origins cannot contain paths, user info, queries or
  fragments. A first-label wildcard such as `https://*.example.com` is accepted;
  bare `*` and CSP keywords are refused.
- `permissions` can request camera, microphone, geolocation or clipboard writes
  through typed empty `UiPermission` objects.
- `domain` is an opaque dedicated sandbox-domain request; `prefers_border` is
  a presentation hint.

Hosts decide and enforce CSP, permissions, sandbox origin and presentation.
An omitted CSP requests no external loads. Use inline scripts/styles or bundle
assets into the HTML, as the example does. Product metadata can coexist with
typed UI metadata through `resource.meta` and `tool.meta`, but those custom
maps must not define `ui` or the deprecated `ui/resourceUri` key.

## Reads, limits and another runtime

UI reads use the existing bounded scheduler and skip your resource handler.
Unknown UI URIs return `-32002` on legacy revisions and `-32602` on modern
requests, with `data.uri`. Modern reads use `SessionOptions::apps_cache`
(private, zero TTL by default) and refuse retry inputs. Cancellation, deadlines,
EOF policy and the output limit apply as they do to other resource reads.

Catalog limits are 64 documents including unlisted ones, 1 MiB raw HTML per
document and 16 MiB total. Each CSP list accepts 64 unique origins of at most
1024 bytes; `domain` accepts 1–1024 bytes without whitespace or NUL. Custom
metadata and typed `ui` together must fit 64 KiB. The combined resource,
template, listed UI and skill-root catalog remains limited to 1024 entries.
Client MIME lists accept at most 32 strings of 1–256 bytes. JSON escaping and
envelope metadata can make a valid document exceed a smaller output budget;
that produces a bounded internal error without partial HTML.

An independent runtime receives `Inbound::BundledResource`; encode its
`read.result()` through the captured modern `RequestContext::encode_success`
or the bounded legacy RPC encoder. Capture `Session::supports_apps(Some(&id))`
at tool admission. Direct catalog callers use `Catalog::list_page_for_client`
with validated request-local support; `list_page` selects the non-Apps view.
Use the producing `ResultSchema` for tool output checks. See
[driving the session](session.md).

## Host boundary

The example implements the `2026-01-26` frame handshake, receives tool results
and makes a refresh call through the host. Its script has an in-process host
fixture (`make test-app-ui`); this verifies messages and document packaging,
without certifying rendering in a specific chat host. Hosts own the sandbox,
bridge, consent, resizing and model context. Consumer frontend builds and
compatible-host acceptance belong to each consumer.

The selected profile follows the [stable MCP Apps specification](https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/2026-01-26/apps.mdx).
Generated UI documents and additional host/frame APIs are outside this catalog.
