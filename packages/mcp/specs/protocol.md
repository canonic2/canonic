# Protocol

The `Session` state machine owns these rules. It performs no I/O; the caller
writes every reply it returns.

## Framing and parsing

- One frame is one JSON value followed by a line break. The session accepts a
  frame with or without its trailing line break.
- Parsing is strict: a repeated object key or data after the value is invalid.
  An invalid frame is answered with `-32700` and the connection continues.
- A frame longer than the message limit ends the connection after a `-32700`
  reply. The stdio reader applies the same limit before parsing and also ends
  the connection on input that stops mid-frame.
- An optional session message limit ends the connection after a `-32000`
  reply once exceeded. It is off by default.

## Message classification

- A request has exactly the members `jsonrpc`, `id`, `method` and optionally
  `params`; a notification omits `id`. `jsonrpc` must be `"2.0"`, `params` an
  object and `id` a string of at most the ID limit or an integer. Anything else
  is answered with `-32600` and a null ID.
- A message with `id` and no `method` is a response to a server request. A
  well-formed response has exactly one of `result` or `error`. Responses that
  match no pending server request are ignored.

## Lifecycle

| Phase | Accepted |
| --- | --- |
| New | `ping`, `initialize` |
| Initializing | `ping`; the caller accepts or rejects the pending initialization |
| Negotiated | `ping`, `notifications/initialized` |
| Ready | Everything below |

- `initialize` parameters contain only `protocolVersion`, `capabilities`,
  `clientInfo` and `_meta`. The version is a non-empty string without NUL
  within the version limit; `capabilities` and `clientInfo` are objects;
  `clientInfo.name` and `clientInfo.version` are strings within the client
  field limit. Invalid parameters are answered with `-32602`.
- A supported requested version is used as requested; any other version gets
  the latest supported version. Supported versions are `2025-06-18` and
  `2025-11-25`.
- The server identity comes from the caller after `Inbound::Initialize`.
  `Session::accept_initialize` accepts only the pending initialization's ID;
  any other call returns `-32600` without changing the session state.
  Rejection returns the session to New, so the client may initialize again.
  A second `initialize` after acceptance is answered with `-32600`.
- The initialization result advertises `tools` with `listChanged: false`, the
  server name, version and optional title, and optional instructions.
- Before Ready, every request other than `ping` and `initialize` is answered
  with `-32000` "Server not initialized".
- `ping` is answered at any phase; its parameters may contain only `_meta`.

## Requests

| Method | Handling |
| --- | --- |
| `tools/list` | The whole catalog in publication order; parameters may contain only `_meta`. |
| `tools/call` | See [tools](tools.md#calls). |
| Listed extension method | `Inbound::Method`, tracked as outstanding. |
| Anything else | `-32601`. |

Listed extension notifications become `Inbound::Notification` once Ready;
unlisted notifications are ignored.

## Outstanding requests

- Admitted tool calls and extension requests are outstanding until the caller
  calls `Session::finish`. The caller writes a reply only when `finish`
  returns `true`.
- A request reusing an outstanding ID ends the connection after a `-32600`
  reply naming that ID: two replies with one ID would be ambiguous to the host.
  IDs are tracked only while outstanding.

## Cancellation

- `notifications/cancelled` with a valid `requestId`, an optional string
  `reason` and optional `_meta` cancels that request if it is outstanding. The
  session releases the ID and returns `Inbound::Cancel`; no reply is sent for
  a cancelled request.
- Cancelling an unknown or finished request has no effect.

## Error codes

| Code | Meaning |
| --- | --- |
| `-32700` | Invalid JSON, or an oversized or unterminated frame |
| `-32600` | Invalid request, repeated initialization or a duplicate outstanding ID |
| `-32601` | Unknown method |
| `-32602` | Invalid parameters, including an unknown tool |
| `-32603` | The server failed to produce a reply: handler panic, oversized or unencodable result, a request answered with no reply, a continuation asking twice, an unconfigured lane or an unavailable worker |
| `-32000` | Refused: not initialized, lane full, message limit |
| `-32001` | The request's deadline, or its confirmation's, passed |
| `-32800` | The connection closed before the request could run or finish confirmation |

Tool failures are tool results, not error codes; see [tools](tools.md#results).
