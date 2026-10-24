# Verification

## Checks

Run from `packages/mcp`, each selected individually: `make format-check`,
`make lint`, `make types`, `make test`, `make doctest`, `make docs` and
`make deps`. Coverage is a report from `make coverage`.

## Coverage

| Area | Evidence |
| --- | --- |
| Strict parsing and bounded encoding | `json` unit tests: duplicate keys at any depth, trailing data, oversized input, the output limit and interrupted encoding |
| Message classification | `rpc` unit tests: requests, notifications, responses, malformed members, IDs and parameters |
| Schema subset | `schema` unit tests: paths in nested objects and arrays, ranges, unions, references, conditionals, formats, additional-property schemas and refused keywords |
| Catalog and results | `tool` unit tests: names, duplicates, schema refusal, listing shape, strict call parameters, base64 and result encoding |
| Lifecycle and routing | `session` unit tests: phases, version negotiation, initialization refusals, unknown tools, invalid arguments, duplicate IDs, cancellation, extension routing and frame limits |
| Confirmation requests | `session` unit tests: capability detection, one pending request, reply matching for each decision, withdrawal, closing and the older protocol's request shape |
| Frame reading | `stdio::reader` unit tests: line breaks, the size limit, unterminated input and empty input |
| Driver behavior | The integration binary drives `serve` through in-memory pipes: finite input, refused and retried initialization, tool results and argument errors, panics, extension methods and notifications, oversized frames and results, duplicate IDs, option refusal, per-key ordering, capacity, queued and running deadlines, cancellation, progress, each confirmation path, and each end-of-input rule |

Timing-dependent driver tests wait on recorded events with a bounded wait and
use short deadlines only to make expiry happen. A stress run of the
integration binary repeats them to expose races.

## Acceptance

- A host that initializes, lists tools and closes its input receives both
  replies.
- No reply is sent for a cancelled request, and no outcome is reported for a
  request after its deadline reply.
- A continuation never runs after cancellation, its confirmation deadline or
  the end of input, and never receives `Accepted` unless the host sent accept.
- After the input ends, no queued mutation runs, and queued and running
  read-only work completes.
- Requests with one key in one lane never overlap.
- Every reply fits the output limit.

## Gaps

- Argument validation has a depth limit but no total-work budget or memoization.
  Recursive `$ref` schemas with repeated `oneOf` or failing `anyOf` branches
  can revisit the same schema and value exponentially. The server author must
  publish such a schema for a client call to trigger this behavior. Validation
  runs in `Session::tool_call` before stdio admission, so request deadlines do
  not bound it and it can delay protocol handling on the dispatcher. Evidence:
  `schema::validate_with` recursively evaluates union branches without sharing
  results; `schema::check_at` checks that references resolve without refusing
  cycles. Runtime cost has not been benchmarked.
- The library's tests use in-memory hosts. Acceptance with real hosts, such
  as their confirmation forms, belongs to each server.
