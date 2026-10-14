# Security

This topic owns the library's security model: what it protects, from whom,
which defense covers each threat, what it deliberately leaves to products,
the behavior of dependencies it relies on, and the weaknesses that remain.
Each defense is specified by the topic it links to; this topic does not
restate those requirements.

## What must hold

- **Authentic content.** Bytes a product installs are exactly the bytes the
  registry's publisher listed, for a catalog signed by the enrolled key.
- **Stable identity.** A source is the same source however its location and
  key are spelled, and no registration can pass itself off as another source,
  including the product's built-in registry.
- **No silent change.** A catalog never goes back in time, and a release,
  once seen, never changes its bytes.
- **Bounded work.** No registry, file or stored value can make the library
  exhaust memory, hang past a deadline, or panic.
- **Safe local state.** Files the library writes are private to the user,
  appear complete or not at all, and never replace something the library did
  not create.

## Attackers

| Attacker | Can | Cannot |
| --- | --- | --- |
| Network attacker | Observe, delay, drop and alter traffic | Present a certificate for the registry's host from a public root |
| Hostile HTTPS host, or a host that turns hostile after enrollment | Serve any bytes, any status, at any speed; replace its key | Sign a catalog with the enrolled key |
| Publisher with a valid key | Publish any release | Change a release already seen, or roll the catalog back |
| Another local user, or a process racing the product | Swap files and links in directories it can write | Write the user's private files |
| A product bug | Pass values the library did not produce, in any combination | Bypass the library's checks on those values |

## Threats and defenses

| Threat | Defense |
| --- | --- |
| Catalog or artifact altered in transit or on the host | The signature covers the exact catalog bytes, and the catalog covers every artifact digest ([catalog authentication](trust.md#catalog-authentication), [artifacts](acquisition.md#artifacts)) |
| Rollback, replay or a stale catalog | Sequence checks and expiry ([checkpoints](trust.md#checkpoints)) |
| Different bytes for a release already seen | Retained release digests ([checkpoints](trust.md#checkpoints)) |
| A host replacing its key | The key is part of the identity and is never replaced from the network ([source identity](trust.md#source-identity)) |
| One source posing as another | Identities are derived from what a source is, in canonical form; stored registrations are checked again when read and when scopes are combined ([source identity](trust.md#source-identity), [effective registrations](registrations.md#effective-registrations)) |
| A catalog admitted under the wrong source | A catalog carries the identity it was authenticated against and is admitted only under it ([checkpoints](trust.md#checkpoints)) |
| Requests sent elsewhere | HTTPS only, public roots, no redirects, no proxy ([reads](acquisition.md#reads)) |
| Paths escaping a source | Artifact paths are validated before any request or read; local reads must canonicalize inside the source ([format](registry-format.md#catalog), [reads](acquisition.md#reads)) |
| A local source that moved, was deleted or was replaced | Refused as an identity change ([reads](acquisition.md#reads)) |
| A link swapped in after a check | On Unix, final path components are opened without following links ([reads](acquisition.md#reads), [artifact cache](installed-state.md#artifact-cache), [global file](registrations.md#global-file)) |
| Oversized documents, responses and graphs | Every input is bounded and refused, never truncated ([limits](limits.md)) |
| A slow, dripping or never-ending host | One absolute deadline enforced on every socket read and write beneath TLS, and a separate connection-setup deadline ([reads](acquisition.md#reads)) |
| FIFOs and devices in place of files | On Unix, files are opened without blocking ([reads](acquisition.md#reads)) |
| Concurrent or interrupted writes | Locked, conditional, atomic replacement of the global file; atomic cache writes that never replace a link or non-file ([global file](registrations.md#global-file), [artifact cache](installed-state.md#artifact-cache)) |
| A corrupted cache | Every cached read verifies its digest ([artifact cache](installed-state.md#artifact-cache)) |

## Not protected

These are outside the library's protection by design. Products own the
mitigations named.

- **First contact.** Discovery trusts the key an HTTPS origin serves at that
  moment; TLS and the operator's review are the only checks. Built-in
  registries pin their key instead.
- **A compromised key, or a malicious release under a valid key.** Judging
  what a pack does is the product's review.
- **Withholding.** A host can keep serving an older catalog until it
  expires, as long as its sequence does not go below the retained one.
- **Writable local registries.** Local registries are unsigned: anyone who
  can write to one can add releases. Releases already seen still keep their
  digests, and a source that has started sequencing must keep doing so.
  These checks do not authenticate the author of a new local release.
- **Revocation.** There is no revocation list; a release stays installable
  until its catalog stops listing it, and retained bytes stay usable offline.
- **Tampered local state.** Checkpoints, registrations and the cache are as
  trustworthy as the storage the product keeps them in. Validation still
  refuses tampering that would let one source impersonate another, and the
  cache verifies every read.
- **Compromised public roots or DNS.** A certificate wrongly issued for a
  registry's host defeats TLS; a signed registry's key still protects its
  catalogs after enrollment.
- **The test-support feature.** It adds a fetcher that trusts a fixture CA
  whose leaf key is public. Products enable it only in dev-dependencies
  ([test support](architecture.md#test-support)).

## Dependency behavior relied on

The library's bounds depend on behavior of its HTTP and TLS dependencies, not
only on its own code. On 2026-10-13 these were read in the locked ureq 3.3.0
and ureq-proto 0.6.4 sources; TLS is rustls 0.23.45 with webpki-roots 1.0.9.
Any upgrade of these crates must re-check them.

- The body limit errors on the read after `limit` bytes. Reading to the end
  always makes that read, so the library sets the limit one byte above the
  caller's and checks the length itself. Bodies are not preallocated from
  `Content-Length`.
- `Content-Length` must be digits that fit a `u64`; repeated values must
  agree. Chunk-size lines over 20 bytes are refused, and sizes are parsed
  with overflow checks.
- Chunked trailers are discarded. A trailer that does not fit the 128 KiB
  input buffer fails the request instead of spinning.
- Responses carry at most 128 header fields; their total size is the
  library's own 16 KiB setting, below the client's 64 KiB default.
- TCP connect tries at most 16 resolved addresses, sharing one budget, with a
  10 ms minimum per address, so many unreachable addresses cannot stretch it.
- Name resolution runs in a helper thread bounded by the request's timeout.
- The client checks its overall deadline only before sending, and TLS reuses
  one relative timeout per socket read; the library's socket layer exists
  because of this ([acquisition decisions](acquisition.md#decisions)).
- TLS uses the WebPKI roots with verification on unless roots are replaced,
  which only the test-support feature can do.

## Reviews

On 2026-10-13 the library had an adversarial review of its code and of the
dependency behavior above. It covered input bounds before parsing, deadline
arithmetic and enforcement against hosts that never pause or drip TLS
records, connection setup, response error mapping, identity derivation and
spelling, checkpoint pairing, registration validation, the global file's
locking, naming and commit reporting, cache links and repair, local source
moves, and races between checks and opens. Each finding is a requirement
in its owning topic, with acceptance in [verification](verification.md), or
a gap listed below.

## Gaps

- **Directory components can still be swapped.** Only the final path
  component is opened without following links. A process that can write a
  parent directory can still redirect a read between the check and the open.
  Closing this needs directory-relative opens.
- **No link or non-blocking protection at open on Windows.** It relies on the
  check made before the open.
- **Chunked trailers have no total byte bound**
  ([acquisition gaps](acquisition.md#gaps)).
- **A timed-out name lookup outlives its request**
  ([acquisition gaps](acquisition.md#gaps)).
- **Refused writes can leave an empty directory.** Creating a missing parent
  of the global file, or the cache directory, happens before later refusals.
  The directory is private to the user, and no file is changed.
- **Interrupted cache writes leave temporary files**
  ([installed state gaps](installed-state.md#gaps)).
- **Checkpoints are never removed** ([trust gaps](trust.md#gaps)).
- **Some spellings of one host stay distinct.** A trailing dot or a
  differently written IPv6 address gives another identity; normalizing them
  would mean interpreting DNS and address syntax.
