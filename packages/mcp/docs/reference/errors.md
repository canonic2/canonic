# Errors

## Replies the host can receive

| Code | Message | Cause | What to do |
| --- | --- | --- | --- |
| `-32700` | Invalid JSON message | The frame is not strict JSON, for example it repeats a key | The connection continues; fix the client |
| `-32700` | Message exceeds size limit, Invalid or oversized message frame | A frame is too long or the input ended mid-frame | The connection ends; raise `SessionOptions::max_message_bytes` only if larger messages are expected |
| `-32600` | Invalid request | A malformed JSON-RPC message | Fix the client |
| `-32600` | Duplicate outstanding request ID | A request reused an ID still being answered | The connection ends |
| `-32600` | Server already initialized | A second `initialize` | Reconnect instead |
| `-32601` | Method not found | An unknown method | List extension methods in `SessionOptions` |
| `-32602` | Unknown tool, Invalid tool call parameters | A name outside the catalog or malformed parameters | Call `tools/list` |
| `-32603` | Response exceeds output limit | A result larger than `Options::max_output_bytes` | Page or shorten the result |
| `-32603` | Request failed | The handler panicked | Fix the handler; the worker continues |
| `-32603` | Request asked for confirmation twice | A continuation returned `Outcome::Confirm` | Ask once per request |
| `-32000` | Server not initialized | A request before initialization completed | Initialize first |
| `-32000` | Server request capacity reached | The lane is full | Retry later or raise `Lane::capacity` |
| `-32000` | Session message limit reached | `SessionOptions::max_messages` was exceeded | The connection ends |
| `-32001` | Request deadline exceeded (before dispatch) | The deadline passed in the queue or while running | Inspect state before retrying a mutation |
| `-32001` | Confirmation deadline exceeded | The user did not answer in time | Ask again if still needed |
| `-32800` | Connection closed before dispatch or confirmation | The host disconnected | None; queued work and confirmation continuations did not run |

Arguments that do not match the input schema are not an error reply: the host
receives a failed tool result naming the field.

## `ServeError`

| Variant | Cause |
| --- | --- |
| `Options` | No lanes, a lane without workers, capacity below workers, or limits too small |
| `Spawn` | A reader or worker thread could not start |
| `Input` | Reading stdin failed; admitted work settled first |
| `Output` | Writing stdout failed |
| `Protocol` | One of the connection-ending replies above |
