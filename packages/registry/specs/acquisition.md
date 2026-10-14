# Acquisition

This topic owns how the library reads from sources: files, catalogs and
artifacts. Starting trust in a new source is owned by
[trust](trust.md#discovery).

## Reads

Every read names a path inside a source and a byte limit, and returns the
file's exact bytes or a refusal. The path must be a valid
[artifact path](registry-format.md#catalog). Content over the limit is refused,
never truncated. Bytes are returned as-is; artifacts need not be text.

**Local sources:**

- The source's directory must still canonicalize to its enrolled location.
  If it has moved, been deleted, been replaced by a file or become a link,
  every read is refused as an identity change, not as a filesystem failure.
- The requested file must canonicalize to a regular file inside that
  directory, so links pointing outside are refused. A file that does not
  exist is a filesystem failure.
- On Unix, files are opened without blocking, so a FIFO or device in place of
  a file is refused rather than waited on, and without following a link at
  the canonical path, so a link swapped in after canonicalization is refused.

**HTTPS sources:** the request is for the source's location followed by the
path, and:

- uses HTTPS only, public TLS roots, and ordinary certificate and host name
  validation, with no option to add roots or skip verification; only the
  test-support feature replaces the roots, with its fixture CA;
- uses no proxy and follows no redirects, so a 3xx response is refused;
- respects the connection-setup and overall deadlines and bounded response headers
  ([limits](limits.md#acquisition)). The overall deadline is absolute, fixed
  when the request starts, and covers name resolution, connection, TLS,
  headers and body as one budget. It is enforced on every socket read and
  write beneath TLS, during the handshake and after it, and no single read or
  write may wait past it. Connection setup also has its own deadline, which
  never extends the overall one: the TCP connection and the TLS handshake
  share one budget, measured from the start of the connection attempt until
  the first write above TLS succeeds, which completes the handshake, and
  enforced the same way. No host can
  keep a request alive past the deadline, whatever it sends: a slow trickle,
  a TLS record dripped a byte at a time, or trailer fields after a chunked
  body, which are discarded without being stored. Connections are not reused,
  so each request's deadline is its own;
- accepts only HTTP 200 without `Content-Encoding`. Every other status,
  including redirects and errors, is a response refusal, distinct from a
  failed request.

## Catalogs

Reading a source's catalog reads `catalog.json`, plus `catalog.sig` for a
signed source, each within its [bound](limits.md#acquisition), and returns the
catalog only if it is [authenticated](trust.md#catalog-authentication) for the
given time. Admission to [checkpoints](trust.md#checkpoints) is a separate step
that must follow.

## Artifacts

Reading a release's artifact reads the path its catalog entry names, within a
limit the product chooses, and refuses bytes whose SHA-256 differs from the
entry's digest. Artifact limits are the product's because pack sizes differ
between products.

## Refusals

| Refusal (`AcquisitionError`) | Cause |
| --- | --- |
| `Location` | A discovered location is neither a directory nor a valid HTTPS URL |
| `Path` | An invalid path, or a local path that is not a regular file |
| `LocalIdentityChanged` | A local source's directory is missing, is not a directory, or no longer canonicalizes to its location |
| `Escapes` | A local file resolves outside its source |
| `Transport` | The HTTPS request or its body failed: connection, TLS, host name or deadline failure, response headers over their bound or malformed, or a body cut short |
| `Response` | A response other than an unencoded HTTP 200, whatever its status |
| `TooLarge` | A response body or local file exceeds its bound, and only then |
| `Key` | A discovered `catalog-key.pub` is not a valid key |
| `RegistryId` | A discovered signed catalog has no `registry_id` |
| `Digest` | Artifact bytes differ from the catalog digest |
| `Catalog(CatalogError)` | The catalog was refused ([trust](trust.md#catalog-authentication)) |
| `Source(SourceError)` | The discovered source is invalid ([trust](trust.md#source-identity)) |
| `Io` | A local filesystem failure, including a requested file that does not exist |

## Gaps

- The HTTP client does not count trailer fields after a chunked body against
  any total byte bound. A host can make a request process trailers until the
  overall deadline, as a slow host can already make it wait. Memory does not
  grow, since trailers are discarded, and a single trailer longer than the
  client's input buffer ends the request as a transport failure. Bounding
  them in total needs a fix in the HTTP client. Refusing chunked responses
  instead would break hosts that send them, so the library does not.
- Name resolution runs in a helper thread that the HTTP client abandons at
  the deadline. The request ends on time, but the thread lives until the
  system resolver returns. Ending the lookup itself needs a resolver that
  can be cancelled.

## Decisions

- **No redirects and no proxies.** A signed registry is identified by its
  location; following a redirect would read from somewhere the operator never
  reviewed.
- **Bytes, not text.** Packs are opaque to the library, so it never assumes an
  encoding.
- **The deadline is enforced on the socket, beneath TLS.** The HTTP client
  checks its overall deadline only before sending; once a body is being read,
  an elapsed deadline becomes a fresh one-second wait for every read, and TLS
  reuses one relative timeout for every socket read while assembling a
  record. Measured 2026-10-13 with the HTTP client's own timeouts only: a
  gapless trailer flood outlived a 10-second deadline by over 110 seconds,
  and a TLS record dripped one byte every 100 ms outlived a 500 ms deadline
  by over 119 seconds. A layer between TCP and TLS checks the absolute
  deadline on every socket read and write and shortens each wait to the time
  that remains; checking only above TLS missed record assembly. The deadline
  is taken before the request starts rather than when the connection opens,
  so time spent resolving and connecting is never granted a second time.
- **Connection setup is bounded separately.** TLS handshakes lazily, inside
  the first write, and reuses the client's relative connection timeout for
  each socket read, so a dripped handshake lasted until the overall deadline
  (measured 2026-10-13: 10 seconds against a 3-second connection timeout).
  The socket layer applies the setup deadline until a layer above TLS sees
  its first successful write, which marks the end of the handshake. The
  setup deadline starts from the time the HTTP client records before the TCP
  connect, not when the socket layer is created after it, so a slow connect
  and a slow handshake cannot each take a full budget.
- **Caller-chosen deadlines and artifact limits.** Products differ in pack
  sizes and in how long an operator waits; catalog and key bounds are fixed
  because their formats are.
