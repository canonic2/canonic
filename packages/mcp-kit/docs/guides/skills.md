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
It also publishes a generated `skill://acme/queue` workflow and serves
directory reads; see [Publish generated skills](#publish-generated-skills).
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
use canonic_mcp_kit::{
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
| Skills per catalog | 128, including unlisted and dynamic entries |
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

## Serve directory reads

Directory reads let a host browse a skill's files one level at a time, without
reading the manifest first. Enable them on the finished catalog:

```rust,ignore
let skills = SkillCatalog::new(definitions)?.with_directory_reads()?;
```

`server/discover` then declares
`{"io.modelcontextprotocol/skills":{"directoryRead":true}}`, and
`resources/directory/read` accepts any directory in a skill namespace:

```json
{"jsonrpc":"2.0","id":2,"method":"resources/directory/read","params":{"uri":"skill://acme/status/references","_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
```

The result lists the directory's direct children in URI order: files with their
name, MIME type and size, and subdirectories with `mimeType: "inode/directory"`.
Directory URIs have no trailing slash; the skill root is a directory too. Pages
hold up to 64 children; pass `nextCursor` back as `cursor` for the next page.
Static pages carry the Skills cache policy (`skills_cache`). Listings are prepared once, when you call
`with_directory_reads`, so requests do no path work.

`with_directory_reads` returns `SkillError::Limit` above 4096 directories and
`SkillError::Conflict` when one path is both a file and a directory (for
example, files named `references` and `references/GUIDE.md`). A file URI, an
unknown path, an address outside every skill namespace or a cursor from another
directory returns `-32602`. Without `with_directory_reads` the method returns
`-32601`; legacy revisions never serve it. A listing does not extend the
manifest; hosts take digests only from `skills/list` and `skills/get`.

## Publish generated skills

Use a dynamic skill when its content is generated on request, such as a
workflow that embeds live queue state, so stable digests cannot be published.
Publish anything with stable bytes as a static skill instead: hosts can verify
static files, and some hosts decline dynamic skills.

Supply the root and a `SKILL.md` whose frontmatter the entry publishes; its
body is never served:

```rust,ignore
use canonic_mcp_kit::DynamicSkill;

const QUEUE: &str = "---\nname: queue\ndescription: Review the items waiting in the Acme queue.\n---\n";
let skills = SkillCatalog::with_dynamic(static_definitions, &[
    DynamicSkill::new("skill://acme/queue", QUEUE),
])?
.with_directory_reads()?;
```

The entry is `{"uri":"skill://acme/queue/SKILL.md","frontmatter":{...},"resources":"dynamic"}`
in both `skills/list` and `skills/get`. Static entries come first in the
listing. Set `listed` to `false` for URI-only discovery, as for static skills.
A dynamic root cannot contain or sit inside another skill root; construction
returns `SkillError::Conflict`.

Your handler serves everything under the dynamic root. Reads arrive as
`Request::Method` with `resources/read`, and directory reads as
`Request::Method` with `resources/directory/read`:

```rust,ignore
Request::Method { method, params } => {
    let uri = params["uri"].as_str().unwrap_or_default();
    let answer = if method == "resources/directory/read" {
        queue_directory(uri).map(Outcome::Directory)
    } else {
        queue_file(uri).map(Outcome::Resource)
    };
    answer.unwrap_or_else(|| Outcome::ResourceNotFound(uri.into()))
}
```

Build listings with `ReadDirectoryResult::new(children, next_cursor)`. It
validates each child like a resource link and refuses more than 1024 children,
an encoding above 1 MiB, or an empty or oversized cursor. Mark child
directories with `resource::DIRECTORY_MIME_TYPE`. The cursor is yours to define;
the host sends it back unchanged.

The driver checks your replies before sending them, because hosts compare the
served `SKILL.md` with the entry and refuse the skill on any difference:

| Reply | Requirement | Otherwise |
| --- | --- | --- |
| `SKILL.md` read | One text content at that URI; its frontmatter equals the entry's. The body, key order and YAML spelling may change | Internal error: `SKILL.md frontmatter does not match its entry` |
| Directory listing | `Outcome::Directory` whose children are distinct direct children of the requested URI | Internal error: `listing entries must be distinct direct children` |
| Unknown path | `Outcome::ResourceNotFound` | — (returns `-32602`) |
| `Outcome::Directory` for any other request | Not allowed | Internal error |

A dynamic file read may ask the host for input first, like any consumer read:
return `Outcome::InputRequired` (see [Modern confirmation retries](#modern-confirmation-retries))
or a legacy `Outcome::Confirm` or `Outcome::Elicit`. The check applies to the
reply that ends the request. Bundled files never ask for input, so a bundled
read that carries `requestState` or `inputResponses` returns `-32602`.

Generated files and listings are consumer reads, so they carry the
`reads_cache` policy rather than `skills_cache`. Return
`Outcome::ResourceWithCache` to choose a file's policy explicitly.

Other files under the root are returned as you supply them. Directory reads
of a dynamic skill need `with_directory_reads`, like static ones. Hosts get no
integrity guarantee for generated content and must not treat an earlier
approval as covering it; say so in your server's documentation.

## Protocols, context and caching

Skills requires `SessionOptions::modern` and explicit server identity. Invalid
configuration is refused before stdio threads start; another runtime should use
`Session::try_new`. Modern requests carry their own revision and client
capabilities and work without a handshake or prior discovery call.
`server/discover` publishes the enabled protocol revisions, resources and
`io.modelcontextprotocol/skills` extension, with `directoryRead` when enabled.
Its declaration does not establish that a particular host supports skill
activation.

Both legacy revisions remain supported. They use initialization, omit Skills
from capabilities and refuse `skills/*`; the bundled files remain ordinary
resources. A server stays legacy-only until it explicitly enables the modern
path.

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
revision-correct, including static directory pages. Dynamic reads and
directory reads arrive as `Inbound::Method`; check your result with
`SkillCatalog::check_read(uri, &result)` or `check_directory(uri, &listing)`
before encoding it, as the shared driver does. See [Drive the session yourself](session.md).

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
