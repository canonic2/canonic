# Limits

Registry refuses oversized input rather than truncating it or returning a
partial catalog or resolution. Choose your own artifact limits and outer
document bounds in addition to these library limits.

## Names and metadata

| Input | Limit |
| --- | --- |
| Publisher, name or alias | 1–64 lowercase ASCII letters, digits or hyphens (`names::MAX_IDENTIFIER`) |
| Source identity | Non-blank, no NUL, at most 4096 bytes (`names::MAX_TEXT`) |
| Reference, artifact path, dependency requirement, resolver version text | 4096 bytes (`names::MAX_TEXT`) |
| Comparators per requirement | 64 |

Byte limits measure UTF-8 bytes. Path and identifier character rules can refuse
input even when its length fits. See [Catalog format](../publishing/catalog-format.md).

## Catalogs and transport

| Input or operation | Limit |
| --- | --- |
| Catalog document | 1 MiB (`catalog::MAX_CATALOG_BYTES`, also exported by `acquisition`) |
| Releases in one catalog | 64 (`catalog::MAX_ENTRIES`) |
| Public key and signature files | 4 KiB each (`acquisition::MAX_TRUST_BYTES`) |
| Artifact reads | Caller-supplied `max_bytes` |
| Overall HTTPS request | Caller-supplied timeout, capped at 24 hours (`acquisition::MAX_DEADLINE`) |
| TCP connection through TLS setup | 3 seconds, within the overall request deadline |
| HTTP response headers | 16 KiB and 128 fields |

The request deadline includes response-body reads. Each file request gets a
separate deadline. Chunked trailers are discarded; each must fit the client's
128 KiB input buffer, and the overall deadline bounds continuing trailers.
Timeouts do not apply to local filesystem operations.

## Registrations and retained state

| State | Limit |
| --- | --- |
| Aliases in one scope | 64 (`registrations::MAX_REGISTRATIONS`) |
| Global registrations JSON | 1 MiB (`registrations::MAX_FILE_BYTES`) |
| Checkpoint sources | 256 (`checkpoint::MAX_SOURCES`) |
| Retained releases per checkpoint source | 4096 (`checkpoint::MAX_RELEASES`) |
| Locked packs | 64 (`lock::MAX_PACKS`) |
| Cached artifact read | Caller-supplied `max_bytes` |

The cache has no total capacity or write-size limit. Bound acquired artifacts
before storing them and manage retention in your application. Checkpoints retain
historical release digests even when catalogs stop listing them, so their limit
is cumulative rather than a limit on one current catalog.

## Resolution

| Input or work | Limit |
| --- | --- |
| Roots | 64 (`resolution::MAX_INSTANCES`) |
| Reachable selected packs | 64 (`resolution::MAX_INSTANCES`) |
| Supplied releases | 4096 (`resolution::MAX_RELEASES`) |
| Total supplied dependency edges | 4096 (`resolution::MAX_RELEASES`) |
| Enrolled source identities | 256 (`resolution::MAX_SOURCES`) |
| Search states | 10,000 (`resolution::MAX_STATES`) |

`resolution::Error::Limit` can mean oversized inputs, a closure larger than 64
packs or exhausted search work. Keeping unrelated candidates out of a request
can reduce work, but ensure you still supply candidates needed for backtracking.

`resolve_with_diagnostics` shares these bounds and retains at most one rejection
record per attempted search state. It does not extend the search budget.

Next: [Errors and troubleshooting](errors.md).

[Documentation index](../README.md)
