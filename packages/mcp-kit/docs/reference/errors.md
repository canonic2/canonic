# Errors

## Replies the host can receive

| Code | Message | Cause | What to do |
| --- | --- | --- | --- |
| `-32700` | Invalid JSON message | The frame is not strict JSON, for example it repeats a key | The connection continues; fix the client |
| `-32700` | Message exceeds size limit, Invalid or oversized message frame | A frame is too long or the input ended mid-frame | The connection ends; raise `SessionOptions::max_message_bytes` only if larger messages are expected |
| `-32600` | Invalid request | A malformed JSON-RPC message | Fix the client |
| `-32600` | Duplicate outstanding request ID | A request reused an ID still being answered | The connection ends |
| `-32600` | Server already initialized | A second `initialize` | Reconnect instead |
| `-32601` | Method not found | An unknown or disabled method | Enable the resource catalog for resource methods; register product methods in `SessionOptions::extension_methods` |
| `-32602` | Invalid resource parameters | An invalid URI, cursor, metadata or parameter shape | Use a concrete URI and pass the previous page's cursor unchanged to the same list method |
| `-32602` | Resource not found | A modern read named an unknown or inaccessible URI | Check the address and authorization; use `Outcome::ResourceNotFound` for revision-aware replies |
| `-32602` | Unknown tool, Invalid tool call parameters | A name outside the catalog or malformed parameters | Call `tools/list` |
| `-32602` | Invalid tool listing parameters or cursor | Unsupported fields/revision or a malformed, foreign or out-of-range tool cursor | Restart listing without a cursor and forward each returned `nextCursor` unchanged |
| `-32603` | Response exceeds output limit | A result larger than `Options::max_output_bytes` | Page or shorten the result |
| `-32602` | Invalid subscription filter | Malformed, unknown or over-budget subscription parameters | Send only supported filters and bounded absolute resource URIs |
| `-32000` | Subscription capacity exceeded | The connection already holds its configured number of streams | Cancel an unused stream or wait for expiry |
| `-32603` | Subscription acknowledgement/notification/result exceeds output limit | A stream frame cannot fit the output budget | Reduce selected URIs or server metadata, or increase the frame budget; the affected stream ID is released |
| `-32603` | Tool result does not match its output schema | Missing or invalid structured data, or incompatible legacy data | Fix the handler's result; validation cannot undo effects |
| `-32603` | Tool result exceeds schema validation limits | Structured data exceeds validation bytes, depth or work limits | Reduce the data or its validation cost; effects may already have happened |
| `-32603` | Tool call requires a typed tool result | A tool call returned a raw method success or resource outcome | Return `Outcome::Tool(ToolResult)` even without an output schema |
| `-32602` | Unknown prompt / Missing required prompt argument / Unknown prompt argument | Retrieval failed catalog validation | Use a listed name and supply only its declared string arguments |
| `-32602` | Invalid prompt cursor | A list cursor is malformed or belongs to another catalog | Restart listing and forward the returned cursor unchanged |
| `-32603` | Prompt retrieval requires a typed prompt result | A renderer or continuation returned a raw or wrong-domain success | Return `Outcome::Prompt(GetPromptResult)` |
| `-32602` | Unknown completion target / Unknown or excessive completion argument / Unknown completion context argument | Completion references unpublished targets or fields, or exceeds an input budget | Use a published prompt/template and its declared string fields |
| `-32603` | Completion requires a typed complete result | A provider returned raw data, another result type or an input step | Return `Outcome::Completion(CompleteResult)` or an RPC failure |
| `-32603` | Cannot encode prompt result for legacy request | Resource links are incompatible or the result exceeds its frame budget | Use legacy-supported content and reduce the result size |
| `-32603` | Cannot encode response | A modern payload fails its operation's type or result contract | Return the matching typed tool/resource result and fix its data |
| `-32603` | Invalid protocol error for modern request | An outcome uses a retired, unallocated or HTTP-only code, or omits required protocol error data | Use an application code outside the JSON-RPC reserved range, or supply the specified protocol data |
| `-32603` | Cannot encode error response within its output limit | A modern error exceeds the response budget | Shorten the error message or data |
| `-32603` | Request failed | The handler panicked | Fix the handler; the worker continues |
| `-32603` | Request asked for confirmation twice | A continuation returned `Outcome::Confirm` | Ask once per request |
| `-32000` | Server not initialized | A request before initialization completed | Initialize first |
| `-32000` | Server request capacity reached | The lane is full | Retry later or raise `Lane::capacity` |
| `-32000` | Session message limit reached | `SessionOptions::max_messages` was exceeded | The connection ends |
| `-32001` | Request deadline exceeded (before dispatch) | The deadline passed in the queue or while running | Inspect state before retrying a mutation |
| `-32001` | Confirmation deadline exceeded | The user did not answer in time | Ask again if still needed |
| `-32002` | Resource not found | The consumer refused an unknown or inaccessible URI | Check the address and authorization; `data.uri` identifies the resource |
| `-32021` | Client lacks form elicitation | A modern operation needs unsupported user input | Provide form capability on that request or handle the unavailable path |
| `-32021` | Client lacks Tasks support | A task operation requires the current request's Tasks declaration | Declare the extension or use the server's ordinary fallback before starting work |
| `-32602` | Task not found or unavailable | The record is unknown, expired or inaccessible | Check the retained handle and access scope; do not infer that work stopped |
| `-32603` | Task outcome does not match its request | The consumer returned the wrong typed outcome or record | Fix the adapter; preserve any already committed task |
| `-32022` | Unsupported protocol version | The requested version is unsupported or modern support is disabled | Select a supported version from the reply |
| `-32800` | Connection closed before dispatch or confirmation | The host disconnected | None; queued work and confirmation continuations did not run |
| `-32800` | Server is shutting down | An HTTP request arrived after `Shutdown::shutdown` | Retry against a running server |
| `-32020` | Header mismatch: … | An HTTP `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` or `Mcp-Param-*` header is missing, repeated, malformed or differs from the body | Send each header once with the body's exact value; encode non-ASCII values as `=?base64?…?=` |
| `-32000` | Origin is not allowed | An HTTP request carried an `Origin` outside `HttpOptions::allowed_origins` | Add the exact browser origin, or call from a non-browser client |

Over HTTP, a reply's status follows its code: `-32020`, `-32021`, `-32022`,
`-32600` and `-32700` are `400`, `-32601` is `404`, and every other code is
`200`. See [HTTP](../guides/http.md#statuses-and-refusals) for refusals that have
no JSON-RPC body.

Arguments that do not match the input schema are not an error reply: the host
receives a failed tool result naming the field.
Schema resource refusals also dispatch no work. Catalog construction refuses
invalid schemas, unsupported dialects, unresolved references, exceeded
preparation limits and invalid `x-mcp-header` annotations; see
[schemas](../guides/schemas.md) and [HTTP](../guides/http.md#mirror-arguments-into-headers).

## `ServeError`

| Variant | Cause |
| --- | --- |
| `Options` | No lanes, a lane without workers, capacity below workers, or limits too small; over HTTP, also missing modern options, a session message limit, or HTTP connection and time limits out of range |
| `Endpoint` | An invalid HTTP endpoint path, allowed origin or body limit |
| `Configuration` | Invalid modern identity, Skills or Tasks without modern opt-in, invalid task policy/reserved method registration, or conflicting resource namespace ownership |
| `Spawn` | A reader or worker thread could not start |
| `Input` | Reading stdin failed, admitted work settled first; over HTTP, the listener has no local address |
| `Output` | Writing stdout failed |
| `Protocol` | One of the connection-ending replies above |
