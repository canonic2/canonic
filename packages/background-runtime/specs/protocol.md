# Framing and versions

## Byte framing

Frames carry bytes with an unsigned four-byte big-endian length prefix. The
payload budget excludes the prefix ([limits](limits.md#sizes-and-identifiers)).
Reads reject an oversized advertised length before allocating its body;
writes reject before writing a prefix. Neither
truncates. JSON helpers are provided because multiple consumers use JSON.

The byte helpers do not choose deadlines; their caller owns the transport and
clock. The synchronous driver handles one request and its acknowledgement and
result per connection. A product that retains a cloned stream owns its
separate owner-channel protocol, capacity and lifetime hold.

## Request envelopes

The synchronous driver uses JSON envelopes. Product requests carry `protocol`,
`request_id`, `requires` and `payload`. Control requests carry independently
versioned `control_version: 1`, stable `request_id`, `action`, `requires`,
`manager_id` and `arguments`. Control decoding precedes ordinary decoding on
either endpoint. It never depends on product protocol or build identity.

| Envelope | Required fields | Optional fields and defaults |
| --- | --- | --- |
| Product | `protocol`, `request_id` | `requires`: empty list; `payload`: null |
| Control | `control_version`, `request_id`, `action` | `requires`: empty list; `manager_id`: absent/null; `arguments`: null |

Both request envelopes reject unknown fields. Product payloads and extension
arguments are opaque JSON values interpreted by the product. Built-in control
arguments follow the library's action-specific contract. Request IDs use
the syntax and size bound in [limits](limits.md#sizes-and-identifiers).
The client matches reply IDs and requires one incarnation across the
acknowledgement and result. Request identity is correlation, not authorization.

## Controls and compatibility

Built-ins are identity, status, retire_if_empty, retire_if_older, drain and the
opt-in force_stop. Identity includes a
fresh manager incarnation, build, protocol, control version and capabilities.
Products can use `Identity::new` for 128-bit OS randomness. Retirement/drain
target the exact incarnation. Extensions are explicitly declared action names;
required capabilities are checked before acceptance. Unknown versions/actions
and stale targets refuse without effects. Optional additive reply fields may
be ignored; changed required semantics require a new control version.

| Action | Result and effects |
| --- | --- |
| `identity` | Identity metadata and lifecycle; no product mutation. |
| `status` | Identity, lifetime counts, connection counts and a product blocker summary. |
| `retire_if_empty` | Atomically attempt closure; return `retired` and blockers if busy. |
| `retire_if_older` | Exact incarnation and `arguments: {"version":"2.0.0"}`; compare release precedence, retire now if empty or schedule retirement when work settles. |
| `drain` | Close mutation admission and start product cleanup on the first drain transition. |
| `force_stop` | Opt-in exact-incarnation process exit, bypassing graceful cleanup; see [failure recovery](recovery.md#explicit-force-stop). |
| Declared extension | Invoke the product control callback under mutation admission. |

Product compatibility is exact protocol equality plus support for every
required capability; release ordering does not establish compatibility.
`client::upgrade` is an explicit immediate retirement attempt and reports
`Retired` or `Busy` with compatibility and blockers.
If its identity observation races replacement, the exact-incarnation refusal
returns typed `Error::StaleManager`, without re-targeting the replacement or
treating the refusal as a malformed envelope. The caller must inspect again
before deciding whether another incarnation should retire. Low-level `control`
continues to return the complete `stale_manager` refusal reply.

The [update contract](updates.md) owns `client::connect_versioned`, release
ordering, the `retire-if-older-v1` capability and its wire arguments/results,
pending retirement and successor initialization. Release ordering does not
authorize application replay or establish stored-state compatibility. An
unsupported action or capability must not reach a product callback.

## Replies and refusal

Replies first acknowledge admission (`accepted: true`), then carry the result
(`accepted: null`). Definite refusal is `accepted: false` with a typed code.
Both frames preserve the request ID and manager incarnation. Capacity refusals
before decoding may have an empty request ID. Products own durable mutation
deduplication and content-conflict detection; the transport never stores a
journal or claims exactly-once delivery.

All reply envelopes carry `control_version`, `manager_id`, `request_id`,
`accepted`, `code` and `data`. The result's optional code can report a product
error or response overflow after admission; it does not turn the result into
a non-admission refusal.

| Code | Meaning |
| --- | --- |
| `starting`, `draining` | The manager is not accepting the requested work in this lifecycle state. |
| `capacity` | A bounded lane cannot admit the connection or queued frame. |
| `incompatible` | Unsupported version or required capability. |
| `unknown_action` | Undeclared control action or an ordinary envelope on the control endpoint. |
| `stale_manager` | Retirement or drain targeted another incarnation. |
| `invalid` | Invalid request identity or framing input that can be refused. |
| `frame_too_large` | Input or encoded response exceeds the frame budget. |
| `product` | An admitted product callback returned structured failure data. |
| `invalid_version` | The client or manager build cannot be ordered as a bounded semantic version. |

The client maps lifecycle, capacity and compatibility refusals to typed errors;
other complete refusal codes remain in the returned `Reply`. Malformed JSON
or a selected envelope that cannot be decoded receives a bounded `invalid`
refusal attempt before the connection closes, without acknowledgement or
product dispatch. This includes unknown fields, missing required fields and
wrong field types. A valid bounded request ID is preserved when the JSON value
contains one; otherwise the refusal ID is empty. No parser error text or input
payload is reflected. Ordinary envelopes on the control endpoint retain their
`unknown_action` refusal. A lost refusal still leaves the client uncertain;
only receipt of the complete refusal establishes non-admission.

## Ambiguity and recovery

A missing acknowledgement is ambiguous because the acknowledgement may itself
have been lost. Only an explicit complete refusal establishes non-admission.
After a partial send or lost result the client returns OutcomeUnknown with the
original ID, without automatically replaying application requests. The
[update handshake](updates.md#races-failures-and-older-control-implementations)
can retry its read-only probes and idempotent incarnation-targeted retirement.
An idle older manager can be asked
to retire; a busy compatible one remains usable. Busy incompatible managers
retain controls without ordinary version decoding.

The product allocates stable mutation IDs and owns durable ID/content conflict
checks and outcome lookup. A new connection or replacement incarnation does not
make replay safe. Even after an acknowledgement, a lost final result can mean
the effect completed. Transport failure must not be described as rollback.

The opt-in [recovery API](recovery.md#request-recovery) can return a durable
result or replay an identical request only under product reconciliation authority.

## Bounds and acceptance

Bounds are owned by [Limits](limits.md). Incomplete,
oversized, timed-out and full-lane requests have no product dispatch. Response
size overflow produces an explicit bounded final error rather than truncation.

- Controls remain decodable when ordinary protocol versions disagree.
- Stale incarnation targets and unsupported versions, actions or capabilities
  produce no product effects.
- Admission acknowledgement precedes callback dispatch; the reservation lasts
  through the final reply attempt.
- A lost acknowledgement or result never authorizes application replay by itself.
- Both requests and encoded replies respect the frame budget.

One-shot client admission and result frames each have their own absolute receive deadline
covering both the prefix and body. Partial reads do not renew it. Expiry after
sending an application request preserves the request ID in OutcomeUnknown
and never retries that request.
