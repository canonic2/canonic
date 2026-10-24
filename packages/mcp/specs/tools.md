# Tools

## Catalog

A `Catalog` is validated once and is immutable.

- Names are 1 to 128 bytes of `A-Z a-z 0-9 _ - .` and unique.
- Every input schema has `type: "object"` and passes the schema check below.
- An output schema, when present, has `type: "object"`. It is published for
  hosts and not enforced on results.
- Annotations (`title`, `readOnlyHint`, `destructiveHint`, `idempotentHint`,
  `openWorldHint`) are hints. They grant nothing. The stdio driver uses only
  `readOnlyHint` for its [end-of-input rules](scheduling.md#end-of-input).
- `tools/list` returns tools in publication order with protocol field names
  and omits absent optional fields.

## Argument schemas

The validator enforces a subset of JSON Schema. The catalog refuses an input
schema that uses any other keyword, so no published constraint is silently
ignored.

| Kind | Keywords |
| --- | --- |
| Enforced | `$ref` (local `#` pointers), `type` (string or list), `enum`, `const`, `properties`, `required`, `additionalProperties` (boolean or schema), `minProperties`, `maxProperties`, `items` (one schema), `minItems`, `maxItems`, `minLength`, `maxLength` (characters), `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `oneOf`, `anyOf`, `allOf`, `not`, `if`/`then`/`else`, `format: "uuid"` |
| Accepted, not enforced | `$schema`, `$id`, `$comment`, `$defs`, `definitions`, `title`, `description`, `default`, `examples`, `deprecated`, `readOnly`, `writeOnly`, other `format` values |

- A `$ref` is followed to its target. The schema check refuses assertion
  keywords beside `$ref`; annotation siblings are accepted.
- Validation stops at the schema depth limit and reports a mismatch.
- Errors name a path from `arguments`, such as `arguments.point.x is required`
  or `arguments.paths must contain at most 2 items; received 3`. A range error
  names both bounds when the schema has both.

## Calls

- `tools/call` parameters contain only `name`, `arguments` and `_meta`.
  `name` is a string; `arguments` is an object and defaults to `{}`;
  `_meta.progressToken`, when present, is a string or an integer. Other shapes
  are answered with `-32602` and nothing runs.
- A name outside the catalog is answered with `-32602` "Unknown tool".
- Arguments that fail validation produce `Inbound::InvalidArguments`: the
  caller answers with a failed tool result and nothing runs. The stdio
  driver's default result is the validation message as text; a handler may
  replace it.

## Results

- A tool result has content blocks, optional structured content and
  `isError`. A tool's own failure, including invalid arguments, is a result
  with `isError: true`, so the caller's model can read and correct it.
- Content blocks are text or inline images. Image bytes are base64-encoded by
  the library; the server bounds what it reads.
- `ToolResult::structured` puts the structured content's JSON in the first
  text block as well, for hosts that show only text.
- Replies are encoded within the output limit. A result that does not fit is
  answered with `-32603` "Response exceeds output limit".
