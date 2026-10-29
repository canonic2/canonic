# Publish bundled skills

Skills publish workflows alongside your server's tools. A host discovers the
workflow's metadata and file manifest, then reads `SKILL.md` and supporting
files through resource URIs. Your program chooses and bundles the bytes; the
library validates and serves them without opening files or executing scripts.

Use this for product usage workflows. Host activation and user approval remain
host responsibilities. Reading a file does not activate its instructions or
grant permission to run code.

## Start with the offline example

The complete [skills example](../../examples/skills.rs) exposes `acme_status`
and a workflow at `skill://acme/status/SKILL.md`, with a format reference and
an SVG served as binary content. All three files are embedded at compilation.
From the library package, run:

```sh
cargo run --locked --example skills
```

The process reads newline-delimited JSON-RPC from stdin. A client can discover
the skill before initialization:

```json
{"jsonrpc":"2.0","id":1,"method":"skills/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
```

The result contains `resultType: "complete"`, `skills`, and cache hints. Each
entry contains its root-document URI, parsed frontmatter and complete file
manifest, with SHA-256 digests and raw byte lengths. `skills/get` accepts
`{"uri":"skill://acme/status/SKILL.md"}` with the same `_meta` and returns that
same entry. `resources/read` accepts any published file URI with the same
metadata and returns its exact content.

The example is offline after dependencies are acquired. It does not require
the source tree when the compiled executable runs. Its packaging test checks
that its manifest enumerates the complete source directory.

## Construct a catalog

In a consuming package, prepare the catalog before starting a session:

```rust,ignore
use canonic_mcp::{
    ModernOptions, ServerInfo, SessionOptions, SkillCatalog, SkillDefinition, SkillFile,
};
use std::sync::Arc;

let skills = SkillCatalog::new(vec![SkillDefinition::new(
    "skill://acme/status",
    vec![
        SkillFile::text("SKILL.md", include_str!("skills/status/SKILL.md")),
        SkillFile::text("references/FORMAT.md", include_str!("skills/status/references/FORMAT.md")),
    ],
)])?;
let session = SessionOptions {
    modern: Some(ModernOptions::new(ServerInfo::new("acme", "1.0"))),
    skills: Some(Arc::new(skills)),
    ..SessionOptions::default()
};
// Supply session in stdio::Options, or use Session::try_new in your own runtime.
```

`SkillDefinition::new` enumerates the skill by default. Set its public `listed`
field to `false` for URI-only discovery; this is not access control. The files
are still served and `skills/get` still returns the complete entry. Equal
names at different roots are allowed. The final root segment must match the
document's `name`; a custom hierarchical URI scheme is also accepted.

`SkillFile::text` requires UTF-8. `SkillFile::binary` accepts raw bytes as
`Arc<[u8]>`; the library base64-encodes them once. Supporting MIME types default
to `text/plain` or `application/octet-stream`; set `mime_type` for a more
specific type. Root and nested `SKILL.md` files are Markdown text.

Construction derives frontmatter and digests from the supplied bytes. It does
not accept independently declared digests. Newlines and exact file contents
are preserved. Root URI construction normalizes percent escapes; clients use
the published spelling unchanged. Relative path names are escaped when their
URIs are built. Supplied paths use `/`, without absolute
paths, drive prefixes, backslashes, empty components, `.` or `..`.

Catalog construction is atomic and returns `SkillError`. Duplicate paths,
conflicting nested content, invalid text/frontmatter and exceeded budgets
refuse the catalog. Nested skills are separate definitions; an enclosing
manifest must also include their published files. One immutable byte source
backs shared files. File-specific failures retain their URI and typed cause.

The catalog reserves its root namespaces. Ordinary resource definitions inside
them refuse composition, and unknown files there do not fall through to your
resolver. Other URIs retain normal consumer-owned resource resolution.

## Source profile and bounds

`SKILL.md` begins with `---` on its own line, a YAML mapping, another `---`
line and the Markdown body. LF and CRLF are accepted. Required `name` and
`description` fields follow the supported Agent Skills format; unknown author
fields are retained as JSON values. The parser refuses duplicate keys,
non-string mapping keys, tags, anchors, aliases, merge keys, NUL, non-finite
numbers and numbers that lose precision in the JSON representation.

| Budget | Limit |
| --- | --- |
| Name | 1–64 lowercase ASCII letters/digits/single hyphens; no edge hyphens |
| Description | 1–1024 Unicode scalar values, nonblank |
| Optional compatibility | 1–500 Unicode scalar values |
| Frontmatter source and encoded JSON | 64 KiB each, 32 container levels |
| Files per skill | 512, including root and nested content |
| Raw bytes per skill | 16 MiB |
| Raw bytes per file | 1 MiB |
| Skills per catalog | 128, including unlisted entries |
| Distinct files / stored raw bytes | 4096 / 64 MiB |
| Encoded entry / combined entries | 256 KiB / 8 MiB |
| List page | At most 32 whole entries and 1 MiB including modern result metadata |
| File path / derived URI | 4096 bytes each |

Resource descriptors for enumerated root documents join your ordinary resource
catalog; supporting files need not be listed. The composed discovery catalog
retains its existing 1024-entry and 1 MiB metadata limits. A smaller configured
output-frame limit can still refuse an otherwise valid page or read with an
internal error. Manifests are never split or truncated.

Pagination uses `nextCursor` unchanged on the next `skills/list` call. Cursors
bind to prepared catalog content; invalid, cross-kind or stale cursors return
`-32602`. Unknown skills and unknown modern resource reads also return `-32602`.
Directory reads and dynamic manifests are not implemented.

## Protocols, context and caching

Skills requires `SessionOptions::modern` and explicit server identity. Invalid
configuration is refused before stdio threads start; another runtime should use
`Session::try_new`. Modern requests carry their own revision and client
capabilities and work without a handshake or prior discovery call.
`server/discover` publishes the enabled protocol revisions, resources and
`io.modelcontextprotocol/skills` extension. Its declaration does not establish
that a particular host supports skill activation.

Both legacy revisions remain supported. They use initialization, omit Skills
from capabilities and refuse `skills/*`; the bundled files remain ordinary
resources. Existing consumers stay legacy by default until they explicitly
adapt and opt in.

Cache policy defaults to `ttlMs: 0` and private scope. Configure the policy for
discovery, tools, resources, skills and consumer reads in `ModernOptions`.
For example, `CachePolicy::new(300_000, CacheScope::Public)?` explicitly permits
sharing those results publicly. Select that only for content you intend to
publish to every caller. Merged resource listings use the most restrictive
scope and shortest TTL. Cache freshness does not authenticate workflow content.

`Handler::admit_context` receives the request and its optional immutable modern
context; the default delegates to `admit`. Bundled reads reach admission as
`Request::Resource`, then the driver serves their validated bytes internally.
Your `Handler::call` never receives that variant from the shared driver.
Normal tools and consumer-resolved reads use `Context::request_context()` to
inspect their own metadata and retry inputs. Return `Outcome::ResourceNotFound`
for revision-correct lookup errors, or `ResourceWithCache` to select a read's
cache policy explicitly.

Modern `Outcome::Method` results must not supply protocol-owned `resultType`,
`_meta`, `ttlMs` or `cacheScope`; the reply boundary supplies them. The legacy
`resource::not_found` helper returns a legacy error. The shared driver recognizes
its exact shape on modern `resources/read` calls and maps it to `-32602`,
preserving `data.uri`. Other runtimes must select the appropriate code themselves.

For another runtime, use `Session::request_context(id)` before finishing an
admitted ID and retain that `Arc` with your work. Encode modern results with
`RequestContext::encode_success`; use ordinary RPC encoding for legacy work.
For a modern `InputRequiredResult`, use `RequestContext::encode_interim` with
the request ID, output byte limit and cancellation checkpoint. It validates
the operation and current form capability, adds configured server identity,
and omits cache hints. It returns `EncodeError::Invalid` for an ineligible
operation or capability, or the encoder's overflow/interruption error.
`Inbound::BundledResource` carries a shared `BundledRead::result()`. Schedule it
as read-only work, propagate cancellation and emit its bounded reply only if
`Session::finish` succeeds. Immediate `Inbound::Reply` values are already
revision-correct. See [Drive the session yourself](session.md).

## Modern confirmation retries

A modern handler can construct a yes/no interim result with
`RequestContext::confirmation(message, state)` and return `Outcome::InputRequired`.
The host retries with a new request ID, echoed `requestState` and
`inputResponses`. The consumer must authenticate that state, bind it to the
operation and authorization scope, enforce expiry and any single-use rule,
then inspect `confirmation_decision()`. The library checks the envelope and
the retry's current form capability; it never grants authority from echoed data.

The interim result releases its RPC ID and worker/key. Cancelling that finished
ID cannot revoke a future retry: the consumer owns workflow revocation,
including connection-bound cleanup. Legacy `Outcome::Confirm` closures remain
legacy; on a modern request they resolve unavailable. Larger forms and URL
interactions are not part of this API. See [Confirmation](confirmation.md).
