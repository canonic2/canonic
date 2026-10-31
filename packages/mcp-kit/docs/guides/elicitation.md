# Collect choices, fields and external consent

Use richer elicitation when a tool needs the host user's selections or missing
information. Your server defines the question and validates the answer; the
host renders it. For a simple yes/no decision, keep using
[confirmation](confirmation.md). Rich input works without Tasks or an async runtime.

## Try the example

From `packages/mcp-kit`, run:

```sh
cargo run --locked --example elicitation
```

Send this single line on stdin:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"acme_report","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"elicitation":{"form":{}}}}}}
```

The reply has `resultType: "input_required"`, opaque `requestState` and a
`details` question. Copy the returned state into a new-ID retry with the same
tool and arguments:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"acme_report","arguments":{},"requestState":"RETURNED_STATE","inputResponses":{"details":{"action":"accept","content":{"format":"md","sections":["summary","details"]}}},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"elicitation":{"form":{}}}}}}
```

The example returns the selected preview without changing configuration. Its
private single-use state expires after five minutes, allows eight rounds and
is revoked on restart. Only one interaction can be pending. The `acme_connect`
tool demonstrates URL consent with `elicitation.url`; its `example.com` page
is illustrative and no external connection is established. See the
[complete source](../../examples/elicitation.rs).

## Construct reusable questions

Validate a flat object schema with `elicitation::Form::new`, then wrap it in
`elicitation::Request::form`. Reuse the immutable definition for publication
and answer checking. Its root has `type`, `properties` and optional `required`.
Fields may have `title`, `description` and `default`.

| Field | Constraints |
| --- | --- |
| String | Length bounds and formats `email`, `uri`, `date`, `date-time` |
| Number/integer | `minimum`, `maximum`; integers within the safe JSON range |
| Boolean | True/false values |
| Single choice | String `enum` or titled `oneOf` alternatives with `const` values |
| Multiple choices | Array `items` with string `enum` or titled `anyOf` alternatives; `minItems`, `maxItems` |

Nested objects, arbitrary arrays, patterns and general composition are refused.
Defaults are display hints and never fill missing answers. Accepted values
retain their spelling and selection order. Unknown fields, wrong types,
duplicates, invalid choices and coercions are refused. Dates use Gregorian
validation; date-times accept offsets/fractions but refuse leap seconds. Email
accepts ASCII mailboxes, quoted local parts and address literals. URI checks
syntax without resolving it.

## Modern input rounds

Build `elicitation::Requests` from stable names and questions. Call
`RequestContext::elicitation(requests, state)` and return `Outcome::InputRequired`.
Every required mode must be in this RPC's client capabilities. Missing mode
support returns `-32021` before dependent effects.

On retry, authenticate or privately look up the state first. Bind it to the
exact operation, arguments, questions, scope, expiry and round. Then call
`RequestContext::elicitation_answers` with those retained questions. Never
construct a schema from submitted input. All known answers are validated before
any are returned. Missing answers remain absent; bounded unknown keys are
ignored. You may request missing answers again within your finite round/expiry
policy. Atomically consume single-use state before effects or retain an
idempotent outcome: validation itself does not consume anything or prevent replay.

An interim reply releases its RPC's worker and key. Cancellation of that
settled RPC does not revoke separately retained state. Do not hold authority
locks while a user decides. Only tool calls and consumer-resolved resource
reads currently support interim rounds; catalogs and bundled reads do not.
Independent runtimes use `RequestContext::encode_interim`, then write only
when `Session::finish` succeeds.

## Legacy waits

Return `Outcome::Elicit(Elicit::new(question, continuation))`. It receives an
`elicitation::Answer`: validated `Response`, `Invalid` or `Unavailable`.
The response's `Action` distinguishes accept, decline and cancel. Refusal and
dismissal expose no accepted content. The worker is free during the user wait.

Successive rich questions obey `Options::elicitation`: eight rounds and five
minutes total by default. The total deadline begins at admission and does not
reset on resume; execution/per-wait deadlines also apply. One pending question
shares the legacy confirmation slot. Competing or oversized unsent questions
resolve unavailable. Cancellation, expiry and EOF destroy the continuation;
late answers cannot resume it. For a final confirmation inside a rich workflow,
use a rich empty form rather than switching into the single-round `Confirm` API.

`2025-06-18` supports compatible primitive forms and enum labels, without `mode`.
Multi-select and non-boolean defaults resolve unavailable rather than being
weakened. `2025-11-25` supports forms and URLs. Legacy URL issuance needs a
consumer-supplied `elicitationId`; modern encoding omits it. Optional legacy
URL completion notifications and URL-required errors are not provided.

## External interaction and durable input

`Request::url` accepts absolute HTTPS without embedded credentials or fragments.
Authorize destinations and redirects in your application. The core performs
no fetch, browser launch or OAuth flow. Do not collect credentials in forms.
An accepted URL answer is consent, not proof of completion: query your trusted
service before dependent effects. You may ask again, return a pending product
result or explicitly use a Task. The library starts no poller.

For durable input use `State::RichInputRequired(requests)` in a persisted task
snapshot. `Snapshot::input_answers` checks the full update using its task
request, captured context and your clock. Commit accepted answers and
continuation intent atomically, then acknowledge. Partial valid input may leave
other questions pending. Keep every `pending_keys()` value in lifetime-unique
history; changed questions need new keys. Count definitions, answers and
history in storage quotas. See [Tasks](tasks.md) for access, expiry and recovery.

Form submission does not authorize a protected mutation. If your application
needs final approval, prepare the exact change from validated fields and ask
for that separate decision before committing it.

## Limits

Rich forms allow 64 fields, 128 choices per field and 512 choices combined.
Messages allow 16 KiB UTF-8; titles/descriptions allow 4096 bytes each.
Definitions allow 64 KiB encoded JSON and 16 container levels. Up to eight
questions share a 64 KiB/32-level request map. Answer maps allow 16 KiB/32 levels;
keys allow 128 UTF-8 bytes without controls. URLs allow 4096 bytes. Frame/output
limits also count escaping and enclosing envelopes; overflow is never truncated.

Modern round count, expiry and retained storage limits are consumer policy.
Legacy `ElicitationPolicy` allows 1–64 rounds and a positive timeout up to
24 hours. Protocol fixtures do not establish a particular host's rendering;
verify its declared modes and intended UI separately.
