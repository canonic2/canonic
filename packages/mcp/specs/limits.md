# Limits

Every bound the library enforces. The library refuses input that exceeds a
bound rather than truncating it.

## Sizes

| Bound | Value | Configurable | Exceeded |
| --- | --- | --- | --- |
| Message | 1 MiB, excluding the line break | `SessionOptions::max_message_bytes` | `-32700`, connection ends |
| Encoded output frame | 4 MiB | `Options::max_output_bytes`, at least 1 KiB | Replies: `-32603` for that request; progress: dropped; confirmation: `Unavailable` without sending the form |
| String request ID | 128 bytes | No | `-32600` |
| Tool name | 128 bytes | No | Catalog refused |
| Requested protocol version | 64 bytes | No | `-32602` |
| Client name and version | 4096 bytes each | No | `-32602` |
| Schema nesting, including `$ref` hops | 64 levels | No | Catalog refused, or a mismatch |

## Counts

| Bound | Value | Configurable | Exceeded |
| --- | --- | --- | --- |
| Messages per connection | None | `SessionOptions::max_messages` | `-32000`, connection ends |
| Pending confirmations | 1 | No | `Decision::Unavailable` |
| Requests per lane: queued, running and confirming | 8 in the default lane | `Lane::capacity`, at least its workers | `-32000` |
| Workers per lane | 1 in the default lane | `Lane::workers`, at least 1 | — |
| Frames buffered from the reader | 8 | No | The reader waits |
| Buffered progress notifications | 16 | No | The newest is dropped |

## Time

| Bound | Value | Configurable |
| --- | --- | --- |
| Request deadline, from admission | 30 seconds | `Options::timeout`, or per request with `Admission::timeout` |
| Confirmation deadline, from the request | 300 seconds | `Options::confirmation_timeout` |
| Running mutations after the input ends | 250 milliseconds | `Options::disconnect_grace` |
| Longest deadline | 24 hours; longer values are clamped | No |

Deadlines use a monotonic clock.
