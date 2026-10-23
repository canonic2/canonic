# Hosting and signing

Publish a directory containing immutable artifacts and a version 1 or 2 catalog.
Registry supplies a consuming library, not a production publishing or key
management CLI. Use your release tooling to generate an Ed25519 key, sign the
catalog and deploy the files.

## Local registries

For development or application-controlled local content, create a directory:

```text
acme-registry/
  catalog.json
  packs/
    git-1.2.0.yml
```

A minimal local catalog needs only `schema_version` and `packs`; compute each
digest from its artifact bytes. The [Quick start](../getting-started/quick-start.md)
creates a complete working fixture.

Clients discover the directory by path. Local catalogs are unsigned, but clients
still validate the schema, admit release digests and verify artifact bytes.
Paths must resolve inside the canonical root. A symlink that resolves outside
the root is refused. Treat local filesystem ownership as part of your trust model.

## HTTPS registries

Serve this directory under a stable URL such as
`https://registry.example.com/team/`:

```text
catalog.json
catalog.sig
catalog-key.pub
packs/git-1.2.0.yml
```

1. Keep the private Ed25519 key in your release system. Publish only its raw
   32-byte public key as hex in `catalog-key.pub`.
2. Write catalog metadata with a stable `registry_id`, an increasing `sequence`
   and an expiry that leaves time for your next renewal.
3. Compute artifact digests and finalize `catalog.json` once.
4. Sign those exact file bytes with Ed25519. Hex-encode the raw 64-byte signature
   into `catalog.sig`.
5. Deploy artifacts before making the catalog that references them available.
   Publish the catalog and signature as a consistent pair, using your host's
   atomic deployment mechanism if it has one.
6. Validate discovery and artifact acquisition with a normal `Fetcher::new`
   against the deployed URL. Also test an already enrolled source with retained
   checkpoints, because fresh discovery cannot detect historical rollback.

Private-key formats, signing commands and deployment transactions belong to
your tooling. `testing::SigningKey` is for generated fixtures, not durable
production key storage.

## Configure the host

Catalog v2 can advertise command basenames without adding anything to artifacts:
`"commands":{"acme/runtime":["node","npm","npx"]}`. Every ID must have a
stable release in `packs`. Lists contain 1–16 unique names, at most 64 ASCII
characters each, starting alphanumeric and otherwise using letters, digits or
`._+-`. These authenticated hints establish relevance only. Checkpoint admission
prepares the current index; consumers need no artifact reads for discovery.
Omit the metadata for v1, and upgrade v1-only consumers before publishing v2.
See [registry format](../../specs/registry-format.md) for the complete contract.

Every response must be HTTP 200 with no `Content-Encoding`. Disable compression
and redirects for these paths, including automatic redirects to a CDN or login
page. A normal publicly trusted TLS certificate is required. Registry does not
offer production custom trust roots, client certificate configuration, proxy
support or authentication headers.

Keep `catalog.json` and `catalog.sig` from being reformatted or transformed by
your host. Set cache behavior so renewal reaches clients before expiry and so
the two files do not remain at different deployment versions. If a client sees
an inconsistent pair, it refuses verification rather than accepting a partial
catalog.

Artifact responses must fit each application's selected size limit. The catalog
can contain at most 64 releases and be at most 1 MiB, so this format suits small
pack catalogs. It has no pagination or remote index query protocol.

## Publish updates and renewals

A release ID/version is immutable. Publish changed artifact bytes under a new
version, never by changing the digest of an existing release. Clients remember
digests even after a release disappears from the catalog.

Every catalog byte change requires a higher sequence, including formatting,
removals and expiry renewal. Republishing identical bytes at the same sequence
is allowed. Reverting to an older catalog sequence will fail for clients that
have already admitted a newer one.

Local registries can start without sequences. After a client admits a sequenced
catalog, every later local catalog must keep a sequence and follow these same
monotonicity rules. Removing the field cannot reset the client's checkpoint.

Renew before `expires_at`. At that second, signed acquisition is refused until
an unexpired catalog is published. Cached installed bytes remain available
without a new catalog. Keep old artifacts available if you want clients with
older lockfiles to recover cache misses; a release absent from the catalog cannot
be recovered through the normal locked catalog lookup.

## Keys, moves and mirrors

A derived source identity binds URL, public key and registry ID. Changing any
of them creates a different source that clients must explicitly enroll.
Mirroring identical files at a different URL also creates a different derived
identity. There is no automatic mirror selection or signing-key rotation
protocol. Coordinate a change with consumers instead of replacing the enrolled
key silently. Built-in pinned sources are configured by the consuming application.

Related: [Sources and trust](../guides/sources-and-trust.md),
[Errors and troubleshooting](../reference/errors.md).

[Documentation index](../README.md)
