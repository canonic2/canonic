# Validate tool arguments and results

Tool schemas describe the arguments your handler accepts and the structured
results it returns. `Catalog::new` checks and prepares both schemas once.
The session validates inputs before dispatch, and the stdio driver validates
typed tool results before publishing them.

## Declare a result contract

With the [quick-start dependencies](../getting-started.md), define a tool whose
input uses a shared local definition and whose result has a required field:

```rust
use canonic_mcp_kit::{Catalog, CatalogError, Tool};
use serde_json::json;

fn catalog() -> Result<Catalog, CatalogError> {
    Catalog::new(vec![Tool::new(
        "acme_greet", "Return a greeting for an Acme name.",
        json!({
            "type":"object",
            "$defs":{"name":{"type":"string","pattern":"^Acme"}},
            "properties":{"name":{"$ref":"#/$defs/name","maxLength":64}},
            "required":["name"], "additionalProperties":false
        }),
    ).with_output_schema(json!({
        "type":"object", "properties":{"greeting":{"type":"string"}},
        "required":["greeting"], "additionalProperties":false
    }))])
}
```

In the handler, return `Outcome::Tool(ToolResult::structured(
json!({"greeting":"Hello Acme"}), false))`. The helper also returns the JSON
as a text block for hosts that display text. Missing or malformed structured
data for a successful call produces an internal error (`-32603`); the invalid
payload is not published. A tool error (`is_error: true`) may omit structured
data. If it supplies structured data, that data must match the output schema.
Without an output schema, structured data is optional.

The [complete server](../../examples/schemas.rs) exposes `acme_greet` and a
modern array-returning `acme_names`. Build it from the package directory with
`make dev CARGO_FLAGS='--example schemas'` and run `target/debug/examples/schemas`
as an MCP server. Calling `acme_greet` with `{"name":"Acme"}` returns
`{"greeting":"Hello Acme"}` as structured data; a name without the `Acme`
prefix fails validation before the handler runs.

## Schema and protocol profiles

The default and explicit supported dialect is JSON Schema 2020-12:
`https://json-schema.org/draft/2020-12/schema` (an optional trailing `#` is
accepted). Unsupported dialects are refused during catalog construction.
Input schemas must be JSON objects declaring `type: "object"`.
Output schemas must be JSON objects; nested subschemas may be boolean schemas.

The validator supports the 2020-12 validation and applicator vocabularies,
including `prefixItems`, `contains`, `dependentRequired`, `dependentSchemas`,
`propertyNames`, `patternProperties`, `unevaluatedProperties`,
`unevaluatedItems`, composition and conditionals. `$ref` and its siblings are
both enforced. Numeric equality and integer checks follow JSON Schema semantics,
so a number written as `1.0` can satisfy `type: "integer"`.

Known built-in formats are asserted, including UUID, date/time, email, hostname,
IP, URI and JSON pointer formats supported by the engine. Unknown formats remain
annotations; internationalized email/hostname formats are not enabled.
UUID uses the library's hyphenated 8-4-4-4-12 hexadecimal spelling check.
Content encoding/media-type/schema keywords are annotations: the core does not
decode embedded documents. Unknown schema keywords and required unsupported
vocabularies are refused. `x-mcp-header` is accepted as metadata; header
extraction belongs to an HTTP adapter.

Patterns use the engine's linear regex implementation with bounded program
and DFA cache sizes. Lookaround and backreferences are refused at construction.
All schemas must also pass the resource limits below; the library does not
promise unrestricted acceptance of every valid schema.

For `2026-07-28`, structured results may be objects, arrays, strings, numbers,
booleans or explicit `null`. Absence and explicit `null` are distinct.
For both legacy revisions, structured results remain objects. A tool whose
output schema does not explicitly declare `type: "object"` is omitted from
legacy discovery and rejected before legacy dispatch. It is available through
modern discovery and calls. This prevents advertising an incompatible schema
or running work that cannot produce a valid legacy result.

## Bundle references without acquisition

Local JSON pointers, anchors, embedded `$id` resources and bounded dynamic
references are resolved from the supplied schema document. For example, a
definition with `$id: "https://example.com/schema/name"` can be referenced by
that URI from elsewhere in the same bundled document. The URI is an identifier;
the library does not fetch it.

Unresolved references are refused, including `file:` and network references
that have no bundled resource. Fetching schemas, host allowlists and acquisition
policy remain outside the core. If your application acquires schemas, supply a
bounded compound schema document before catalog construction. Recursive
references must descend into child properties, items or property names.
Same-value cycles and expansion beyond the fixed budget are refused.

## Work limits and errors

| Bound | Value |
| --- | --- |
| One encoded schema document | 256 KiB |
| Raw schema nodes / graph nodes / graph edges | 32768 each |
| Same-value expanded subschema visits | 4096 |
| JSON/schema/reference depth | 64 |
| Instance-aware evaluation depth / queued visits | 256 / 32768 |
| Encoded instance being validated | 4 MiB |
| Conservative work estimate | At most 8,388,608 units |
| Regex program / DFA cache | 32 KiB each |

Before evaluation, the prepared graph is walked against the actual instance.
Every possible branch and repeated reference is charged, including immediate
sizes, literal comparisons, name-pattern tests and quadratic unique-item work.
Definitions and possible dynamic targets are checked during preparation even
if a particular value does not visit them.
Objects/arrays and each contained value count as nodes. These are selected
operating limits, not performance guarantees or upstream protocol ceilings.
Frame, output and task snapshot limits also apply.
Instance nodes have no separate restrictive cap: their ceiling follows the
encoded byte bound. The schema-aware work walk can still refuse expensive
validation within that byte budget.

Budget refusal is distinct from a schema mismatch and cannot become a successful
`not`, `anyOf` or conditional branch. Input refusals produce failed tool results
without handler effects. Output refusals produce internal errors without partial
content. Validation does not roll back work your handler has already performed.

For ad-hoc validation, `schema::check`, `schema::validate` and
`schema::validate_at` prepare their supplied schema each time. For repeated
validation, retain `schema::PreparedSchema` and call `validate`, `validate_at` or
`validate_checked`. The checked form observes cancellation before, during
preflight and after bounded synchronous evaluation. `ArgumentError::kind`
distinguishes mismatch, resource limit, interruption and invalid ad-hoc schema;
paths identify the affected argument or `structuredContent` field.

## Other runtimes and durable tasks

Capture `Catalog::result_schema(&call.name)` when admitting a legacy call, and
use its `encode_legacy` method to validate and encode before publishing. For
modern calls, the admitted `RequestContext::encode_success` applies the captured
contract automatically and requires a typed `ToolResult` for a tool call.
Raw `rpc::encode_success` is an envelope builder and cannot infer a tool's schema.
Send only if `Session::finish` returns true; see [Drive the session](session.md).

For tasks, retain the producing tool's contract with the task's immutable
producer identity/schema version. `Context::result_schema()` and
`RequestContext::result_schema()` expose that captured contract. Validate a
completed snapshot before committing it:

```rust
use canonic_mcp_kit::{ResultSchema, ToolResult, task::{Metadata, Snapshot, State}};

fn completed_snapshot(
    output: &ResultSchema, metadata: Metadata, result: ToolResult,
) -> Result<Snapshot, Box<dyn std::error::Error>> {
    let snapshot = Snapshot::new(metadata, State::Completed(result))?;
    output.validate_snapshot(&snapshot)?;
    Ok(snapshot) // The consumer can now commit this validated snapshot.
}
```

After restore, validate against the same producer contract before serving a
completed result. Snapshot restoration checks structural validity and storage
bounds; a `tasks/get` request alone does not identify the tool or schema that
produced its record. Immediate completed snapshots returned during task creation
are also checked against the creating call's contract. See
[Tasks](tasks.md) for persistence, uncertain delivery and failure settlement.
