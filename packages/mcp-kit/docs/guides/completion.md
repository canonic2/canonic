# Suggest argument values

Completion helps a host offer choices as a user fills in a prompt argument or
resource-template variable. For example, after selecting Python, a user typing
`fl` in a framework field can receive `flask`. The host owns the dropdown and
selection; your handler supplies the choices and controls access to their data.

Completion is optional. It does not execute a prompt, read a resource, expand a
URI or select a value for the user. It works with all supported revisions.

## Try contextual suggestions

From `packages/mcp-kit`, run:

```sh
cargo run --locked --example completion
```

Paste this modern request as one line:

```json
{"jsonrpc":"2.0","id":1,"method":"completion/complete","params":{"ref":{"type":"ref/prompt","name":"acme_review"},"argument":{"name":"framework","value":"fl"},"context":{"arguments":{"language":"python"}},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
```

The reply has `resultType: "complete"` and
`completion: {"values":["flask"],"total":1,"hasMore":false}`. To complete a
resource-template variable, replace the reference with
`{"type":"ref/resource","uri":"acme://reports/{id}"}`, use
`{"name":"id","value":"report"}` as the argument, and omit context. The
example suggests `report-1` and `report-2`. Close stdin to stop the server.
The [example source](../../examples/completion.rs) contains the full setup.

## Enable and handle completion

Publish a [prompt catalog](prompts.md) or [resource-template catalog](resources.md),
then set `options.session.completions = true`. The default is false. Enabling
it advertises `completions: {}` in legacy initialization and modern discovery.
Legacy clients initialize normally; modern calls use per-request metadata.

Your handler receives `Request::Completion(CompleteRequest)` containing:

- `reference`: a `CompletionReference::Prompt { name, title }` or
  `CompletionReference::Resource { uri }` from the published catalogs.
- `argument`: the declared field's `name` and exact partial string `value`.
- `context`: a `BTreeMap<String, String>` of supplied resolved fields.
- `progress_token`: the optional validated correlation token.

Prompt references may carry an optional display title; it grants no authority.
Resource references must contain the exact published template, including its
expressions. Use variable names without operators/modifiers: `path` for
`{/path*}` and `query` for `{?query:4}`. Percent-encoded variable names retain
their original spelling. Concrete resource addresses are not completion targets.

Missing required prompt arguments are allowed during completion. Supplied
context must refer only to fields of the same target; unknown keys and
non-string values fail before dispatch. Empty partial strings are valid.
The active field may also occur in context; inspect the partial value and
resolved context separately. The library never interpolates supplied strings.

## Return bounded suggestions

Create `CompleteResult::new(values, total, has_more)` and return it through
`Outcome::Completion`. Handle constructor failures by returning an RPC error.
Suggestions retain their order and spelling, including duplicates. Your
provider owns filtering, relevance ranking and authorization.

Both `total` and `has_more` are optional. If you know the total, it must be at
least the returned count. When both are supplied, `has_more` must agree with
whether total exceeds that count. When the total is unknown, `has_more: true`
can signal additional matches. Empty arrays are valid. There is no completion
cursor or automatic retrieval of further suggestions.

| Bound | Limit |
| --- | --- |
| Suggestions | 100 |
| Each suggestion | 4,096 UTF-8 bytes |
| Each partial/context value | 65,536 UTF-8 bytes |
| Context arguments | 64 |
| Encoded request fields excluding metadata / encoded result | 262,144 bytes each |
| Prompt reference title | 4,096 UTF-8 bytes, without NUL |
| Supplied total | Exact nonnegative integer, at most 2^53 − 1 |

Overflows are refused rather than truncated. The transport's frame and output
limits also apply. Use fewer/shorter suggestions when a result exceeds its
budget. Unknown references/fields and malformed input return `-32602`;
disabled completion returns `-32601` even if registered as a custom method.

## Scheduling and other runtimes

Completion uses read-only admission, lanes, keys, capacity, deadlines,
cancellation and progress. Queued and running calls can finish after EOF.
Use a suitable admission lane and timeout for your provider; the library does
not add a separate rate limiter. Hosts own debounce and client caching.

Return typed suggestions or an RPC failure. Raw method successes, prompt/tool
results, Tasks, confirmation steps and input-required rounds cannot substitute
for completion and fail with `-32603`. No confirmation continuation runs.

Independent runtimes receive `Inbound::Completion`, supply suggestions, encode
modern results with the captured `RequestContext::encode_success` or legacy
results with `CompleteResult::encode_legacy`, then finish the ID normally.
Both encoders honor their caller's byte limit and interruption checkpoint.
Modern results carry server identity and no cache hints. See
[driving a session](session.md) and [scheduling over stdio](stdio.md).
