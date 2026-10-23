# Limits

Every input the library accepts is bounded, so no registry, file or operator
input can exhaust memory, time or storage. Exceeding a bound is a refusal,
never a truncated or partial result. Each bound below is a public constant of
the named module, unless it is marked as chosen by the caller, internal, or
the HTTP client's own.

Bounds are consistent with each other: two full registration scopes and the
built-in source enroll at most 129 sources, below both source bounds, and a
lockfile can hold every pack a resolution can select.

## Text and identifiers

| Bound | Value |
| --- | --- |
| Publisher, pack name and alias | 1–64 characters (`names::MAX_IDENTIFIER`) |
| Source identity, registry ID, HTTPS location, reference, artifact path, dependency requirement, and version in resolution | 4096 bytes (`names::MAX_TEXT`) |
| Comparators in one version requirement | 64 (internal) |

## Catalogs

| Bound | Value |
| --- | --- |
| Releases in one catalog | 64 (`catalog::MAX_ENTRIES`) |
| Packs with command associations | 64 (`catalog::MAX_ENTRIES`) |
| Command basenames per pack | 16 (`catalog::MAX_COMMANDS`) |
| Bytes per command basename | 64 (ASCII, internal) |

## Acquisition

| Bound | Value |
| --- | --- |
| `catalog.json`, however it is supplied | 1 MiB (`catalog::MAX_CATALOG_BYTES`, also `acquisition::MAX_CATALOG_BYTES`) |
| `catalog-key.pub` and `catalog.sig` | 4 KiB each (`acquisition::MAX_TRUST_BYTES`) |
| Artifacts | Chosen by the caller for each read |
| HTTPS connection setup, from the start of the TCP connection attempt to completing the TLS handshake, as one budget | 3 seconds (internal) |
| HTTPS overall deadline per request, from its start to its last byte | Chosen by the caller for each fetcher, capped at 24 hours (`acquisition::MAX_DEADLINE`) |
| HTTPS response headers | 16 KiB (internal) and 128 fields (the HTTP client's); chunked trailers are discarded, each must fit the HTTP client's 128 KiB input buffer, and their number is bounded only by the overall deadline |

## Checkpoints

| Bound | Value |
| --- | --- |
| Sources with checkpoints | 256 (`checkpoint::MAX_SOURCES`) |
| Retained releases per source | 4096 (`checkpoint::MAX_RELEASES`) |

The current command index has at most 64 × 16 associations per source; it does
not accumulate with retained releases.

## Registrations

| Bound | Value |
| --- | --- |
| Aliases per scope | 64 (`registrations::MAX_REGISTRATIONS`) |
| Global registrations file | 1 MiB (`registrations::MAX_FILE_BYTES`) |

## Resolution

| Bound | Value |
| --- | --- |
| Roots, and reachable packs | 64 each (`resolution::MAX_INSTANCES`) |
| Supplied releases, and dependencies in total | 4096 each (`resolution::MAX_RELEASES`) |
| Enrolled source identities | 256 (`resolution::MAX_SOURCES`) |
| Search states | 10,000 (`resolution::MAX_STATES`) |

Optional resolution diagnostics retain at most one rejected-candidate record
per attempted state. They share the same search budget; reporting neither adds
search work nor permits a larger closure.

## Installed state

| Bound | Value |
| --- | --- |
| Lockfile entries | 64 (`lock::MAX_PACKS`, the reachable-pack bound) |
| Cached artifact read | Chosen by the caller for each read |
