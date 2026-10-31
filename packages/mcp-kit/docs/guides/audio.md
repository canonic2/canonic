# Return audio and content hints

Use an audio block when a tool returns a recording or generated sound that the
host can present directly. Your server supplies bytes and their MIME type;
MCP Kit packages them into the tool result. Recording, codec validation and
playback belong to your application and host.

## Build an audio result

In a Rust handler using the [quick-start dependency setup](../getting-started.md),
accept the WAV bytes from your own recorder or generator:

```rust
use canonic_mcp_kit::{Content, ContentAnnotations, ContentError, Role, ToolResult};

fn recording_result(wav: &[u8]) -> Result<ToolResult, ContentError> {
    let hints = ContentAnnotations::new(
        Some(vec![Role::User]),
        Some(0.5),
        Some("2026-10-31T12:00:00Z".into()),
    )?;
    Ok(ToolResult::success(vec![
        Content::text("Recording completed."),
        Content::audio(wav, "audio/wav")?.with_annotations(hints),
    ]))
}
```

Return the result through `stdio::Outcome::Tool`. Audio serializes with
`type: "audio"`, base64 `data`, `mimeType` and optional `annotations`.
Text, images, resources and structured data can accompany it in display order.
Use the real encoding's MIME type, such as `audio/wav` or `audio/mpeg`.
The library checks metadata and byte bounds; it does not inspect the container
or negotiate a host's codec support. Invalid local content returns `ContentError`;
report that as an internal error rather than invalid client parameters.

The [complete Acme server](../../examples/audio.rs) generates a one-second silent
WAV without accessing a microphone or file. From the package directory, build
it with `make dev CARGO_FLAGS='--example audio'`. Configure an MCP host to run
`target/debug/examples/audio`; a call to `acme_audio` with `{}` returns a text
block and the WAV with user-audience and priority hints. Both legacy revisions
and explicitly enabled modern requests use the same audio block. Host playback
and rendering depend on the host.

## Annotate content and resources

`ContentAnnotations::new(audience, priority, last_modified)` creates immutable
presentation hints. Each argument is optional; `ContentAnnotations::default()`
creates an explicitly empty hint object.

| Field | Meaning and accepted values |
| --- | --- |
| `audience` | Up to two `Role::User` / `Role::Assistant` entries; `None` omits the field and `Some(vec![])` retains an empty array |
| `priority` | Finite number from 0 through 1, with 1 most important |
| `last_modified` | ISO 8601 timestamp supplied by your server, at most 64 UTF-8 bytes without NUL; retained without calendar/timezone parsing or normalization |

Use `with_annotations` on text, image, audio, resource-link or embedded-resource
blocks. It replaces existing hints. Use `annotations()` to inspect them;
the annotation value offers `audience()`, `priority()` and `last_modified()`.
For embedded resources, hints are on the outer block, beside `resource`.
For links, they share the flattened URI metadata. Set `Resource::annotations`
or `ResourceTemplate::annotations` before catalog construction to publish the
same hints through discovery. Nested `ResourceContents` have no annotation field.

These hints guide presentation and relevance. They do not change access,
scheduling, cache expiry or approvals, and they do not initiate playback.
Tool-level `Annotations` still describe tool behavior, including `read_only_hint`;
they are a separate type.

## Bounds, persistence and other runtimes

Each audio block permits at most 1 MiB of raw data. Its MIME string must contain
1–256 UTF-8 bytes without NUL. Construction checks raw size before base64
encoding. Deserialization checks canonical padded standard base64 and its
decoded size, then retains the encoded bytes without decoding or transcoding.
Known-invalid annotation fields, nulls and unknown audience roles are refused.
Empty audio bytes are accepted; whether they form playable media is a consumer
concern.

The entire result must fit the configured output frame, including base64
expansion and annotations. A task result must also fit its 1 MiB snapshot budget,
so the maximum single audio block cannot fit in a task snapshot. On output
overflow the driver returns an internal error; it never truncates audio.
To keep audio out of a task snapshot, return a
[resource link](resources.md#return-artifacts-from-tools) and let your resolver
serve it under its own access and lifetime policy. Ordinary resource reads also
have a 1 MiB raw-content limit. Larger recordings need a consumer-selected
delivery route, such as a separately served HTTPS download supported by the host.

Task save/restore retains audio, hints, priority number representation and
timestamp spelling. General payload deserialization ignores extra object fields;
task restoration refuses fields it would discard. No annotation grants access
to an underlying recording.

Other runtimes use the same `ToolResult` through `rpc::encode_success` for legacy
calls or the admitted request context's `encode_success` for modern calls.
Keep your output limit and cancellation checkpoint, and send only when
`Session::finish` returns true; see [Drive the session yourself](session.md).
Prompt methods are not implemented yet; this guide describes tool results and
resource discovery.

The `Content::Text`, `Image`, `Audio` and `Resource` variants carry an optional
`annotations` field; their builders initialize it to `None`. `Resource` and
`ResourceTemplate` struct literals must also set `annotations`; their `new`
constructors use `None`.
