# Publish reusable prompts

Prompts offer user-selected starting messages for common workflows. A host
lists your prompts, lets its user choose one and collects string arguments.
Your handler returns messages for that conversation. For example, a review
prompt can accept code and ask for correctness findings. Retrieving it does
not call a model, execute tools or grant access to data.

Use resources for content retrieved by URI and skills for bundled workflows
with supporting files. Prompts provide parameterized conversation messages.

## Try the example

From `packages/mcp-kit`, run:

```sh
cargo run --locked --example prompts
```

Paste each request as one line:

```json
{"jsonrpc":"2.0","id":1,"method":"prompts/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"acme_review","arguments":{"code":"fn main() {}"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
```

The list contains `acme_review` and its required `code` argument. Retrieval
returns a complete result with a user message containing that code. Close
stdin to stop the server. The [complete source](../../examples/prompts.rs)
shows the catalog, handler and stdio setup.

## Define and enable a catalog

Create `Prompt::new(name)`, set its optional `title` and `description`, and
populate `arguments` with `PromptArgument::new(name, required)`. Arguments may
also have a title and description. Prepare `PromptCatalog::new(definitions)`
once, wrap it in `Arc`, and set `options.session.prompts = Some(catalog)`.
`None` disables prompt methods; an empty catalog enables an empty list.

The catalog checks duplicate names, metadata and aggregate limits. Declaration
order is preserved. `prompts/list` returns up to 64 entries and `nextCursor`
when more remain. Send that cursor unchanged in the next list request. Cursors
belong to their catalog content; restart listing if the catalog changes.

Legacy clients initialize normally. Both `2025-06-18` and `2025-11-25` advertise
`prompts` with `listChanged: false`. Enable `ModernOptions` to expose the same
catalog through `2026-07-28` discovery and requests. Modern lists include server
identity and `prompts_cache` hints, defaulting to zero TTL and private scope.
Prompt retrieval has no catalog cache hints.

## Render a checked request

Your handler receives `Request::Prompt(PromptCall)` after validation. The call
contains a known `name`, a `BTreeMap<String, String>` of supplied arguments and
an optional progress token. Missing required arguments, unknown arguments,
non-string values and unknown names return `-32602` before your handler runs.
Empty strings satisfy required presence; validate their meaning yourself.
Omitted optional arguments stay absent. Strings are preserved exactly and no
template language is evaluated.

Return `Outcome::Prompt(GetPromptResult::new(description, messages)?)`, handling
the constructor error in your handler. Each `PromptMessage` holds a `Role::User`
or `Role::Assistant` and one shared `Content` block. Text, images, audio,
embedded resources and annotations are supported. Resource links are supported
for modern clients; legacy prompt encoding refuses them. A raw
`Outcome::Method(Ok(...))` cannot stand in for a prompt result. Report access or
application failures with `Outcome::Method(Err(RpcError))`.

Prompt rendering shares lanes, admission keys, capacity, deadlines,
cancellation and progress with other requests. It is admitted as read-only,
so queued and running retrievals can finish after stdin closes. Consumers own
resource access and authority checks, even when a prompt is published.

## Request more input or use your own runtime

Legacy rendering can return `Outcome::Confirm` or `Outcome::Elicit`, then
return a typed prompt result from the continuation. Modern `prompts/get` can
return `Outcome::InputRequired` using `context.request_context()` and its
confirmation/elicitation helpers. On retry, validate your opaque state and
authorization before consuming answers. See [elicitation](elicitation.md).
Listing does not accept retry fields.

Independent runtimes handle `Inbound::Prompt`, render the checked call, then
use the captured `RequestContext::encode_success` for modern results or
`GetPromptResult::encode_legacy` for legacy results. Both encoders enforce the
caller byte limit and interruption checkpoint. Finish the request ID normally;
see [driving a session](session.md).

## Limits and recovery

| Value | Limit |
| --- | --- |
| Catalog entries / encoded catalog | 1,024 / 1 MiB |
| Name / title / description | 128 / 4,096 / 16,384 UTF-8 bytes |
| Arguments per prompt | 64 |
| Supplied argument / encoded argument map | 65,536 / 262,144 bytes |
| Result messages / encoded result | 64 / 1 MiB |

The transport's frame/output limits also apply. Reduce content if a result
exceeds its output budget; data is refused rather than truncated. Use image
builders with a valid image MIME type and base64 data. Prefer resource links
for large modern artifacts, or bounded embedded content for legacy prompts.
Enable [argument completion](completion.md) to suggest values while a host
collects prompt arguments. Catalog changes, subscriptions, icons and arbitrary
metadata are separate capabilities; this catalog publishes no change notifications.
