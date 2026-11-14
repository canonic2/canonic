# Sources and trust

## Discover a source

`Fetcher::discover` accepts a local directory or an HTTPS base URL and returns a
`Discovery` containing a `Source` and its verified `Catalog`. It performs no
registration write and asks no questions.

```rust
use canonic_registry_kit::{acquisition::{Discovery, Fetcher}, checkpoint::Checkpoints};
use std::{error::Error, time::{Duration, SystemTime}};

fn discover_for_review(location: &str, now: SystemTime) -> Result<Discovery, Box<dyn Error>> {
    let fetcher = Fetcher::new(Duration::from_secs(10));
    Ok(fetcher.discover(location, now)?)
}

fn admit_approved(discovery: &Discovery, checkpoints: &mut Checkpoints)
    -> Result<(), Box<dyn Error>>
{
    checkpoints.admit(&discovery.catalog)?;
    // Persist checkpoints and the approved source in application-owned state.
    Ok(())
}
```

For HTTPS, discovery reads `catalog-key.pub`, checks its signature over
`catalog.json` and derives the source using that key and the authenticated
`registry_id`. This is trust on first use: it proves consistency with the key
served at that location, not that the publisher is the one the operator intended.
Show `location()`, `key()`, `registry_id()`, `identity()` and the catalog before
enrolling the source. When needed, compare the public key through a separately
trusted channel.

Subsequent `Fetcher::catalog` calls use the stored key. They do not rediscover or
silently replace it with the server's current `catalog-key.pub`.

## Construct a known source

When a key and registry ID are already trusted, construct a source directly:

```rust
use canonic_registry_kit::source::{Source, SourceError};

fn known_source(public_key: &str) -> Result<Source, SourceError> {
    Source::signed("https://registry.example.com/team/", public_key, "acme-team")
}

fn builtin_source(public_key: &str) -> Result<Source, SourceError> {
    Source::pinned("https://registry.example.com/", "main", public_key, "acme-main")
}
```

`signed` derives an identity. `pinned` uses a fixed application identity and is
intended for your reserved built-in source. A source with a non-derived identity
cannot be added to global or project registrations.

Constructors require a trailing `/`. Discovery adds one if absent. Supported
URLs use `https://`, contain a host and no credentials, query, fragment,
percent encoding or dot segments. Host case and default port 443 are normalized;
path case remains significant. Keys are exactly 64 hex characters with no
surrounding whitespace and are stored lowercase.

For a local source, prefer discovery, which canonicalizes the directory.
`Source::local` expects an already canonical absolute UTF-8 path and performs no
filesystem check itself. Local catalogs are unsigned. Their integrity rests on
local filesystem control and retained release digests, rather than a publisher
signature. Moving the root or replacing it with a link causes acquisition to
refuse; the identity is path-based, not an inode identity.

## Authenticate and admit every catalog

Call `fetcher.catalog(&source, now)`, then `checkpoints.admit(&catalog)` before
using its entries. If you already have bytes from another transport, use
`Catalog::verify(&source, &bytes, signature.as_deref(), now)` and the same
admission step. Keep that transport bounded too.

Signed catalogs require a valid Ed25519 signature, the enrolled registry ID, a
sequence and an unexpired whole-second UTC expiry. The caller supplies `now`;
the library does not read the clock for this decision. Expiry is inclusive:
at `expires_at` the catalog is expired.

Checkpoints refuse a lower sequence, different bytes at the same sequence, and
a changed digest for any retained release. Use `admit_retaining(&catalog,
&protected)` with a map of `publisher/name@version` to exact digest for every
installed or pinned release. Successful admission retains these plus current
discovery entries, pruning older observations after checking incoming entries.
`admit` alone retains discovery entries. Uninstalled historical observations
are not protected indefinitely. An admission failure leaves checkpoints unchanged. Local catalogs also
retain release digests and may start without sequencing. After admitting a
sequenced catalog, the checkpoint refuses every later catalog that omits its
sequence. Local expiry remains optional and is not enforced.

`Checkpoints` supports Serde serialization. Load it before fetching and persist
successful admissions before relying on them across runs. Retain historical
checkpoints when removing an alias; otherwise re-enrollment loses that history.
`get` and `release_digest` expose retained information for diagnostics. The
application owns validation and protection of its persisted state.

## Opt in to versioned trust

Versioned trust lets a publisher rotate its signing keys and revoke exact
releases without changing the source your users enrolled. It is opt-in: once your
application adopts it for a source, use only the versioned methods for that
source and never fall back to `catalog` or `artifact`.

```rust
use canonic_registry_kit::{
    acquisition::{Fetcher, VerifiedUpdate, VersionedRequest},
    checkpoint::Checkpoints,
    source::Source,
};
use std::{collections::BTreeMap, time::SystemTime};

fn update(
    fetcher: &Fetcher,
    source: &Source,
    previous: Option<&VerifiedUpdate>,
    checkpoints: &Checkpoints,
) -> Result<VerifiedUpdate, Box<dyn std::error::Error>> {
    let protected = BTreeMap::new(); // installed or pinned release digests
    Ok(fetcher.versioned_catalog(
        VersionedRequest {
            source,
            previous: previous.map(|u| (u.root(), u.revocations())),
            checkpoints: previous.map_or(checkpoints, VerifiedUpdate::checkpoints),
            protected: &protected,
            now: SystemTime::now(),
        },
        &|| Ok(()), // return Err(AcquisitionError::Cancelled) to stop
    )?)
}
```

`versioned_catalog` follows `trust/roots/N.json` successors from the last root
you admitted (or version 1 on adoption), then reads `trust/revocations.json` and
`catalog.json`, each beside a `.sig` file: JSON
`{"schema_version":1,"kind":"registry-signatures","signatures":[{"key_id","signature"}]}`
over the exact payload bytes. Version 1 must be signed by the enrolled key and
its own root keys; each later root by the threshold of both the previous and new
root keys. All reads share one deadline and a 4 MiB budget, and a missing or
invalid file refuses the whole update.

The returned `VerifiedUpdate` changes nothing on disk. Persist its root
(`TrustedRoot::checkpoint`), revocations (`Revocations::signed_bytes`) and
`checkpoints()` yourself, and only if your stored head still equals
`expected_head_revision()`; store `head_revision()` with them. Reload them with
`TrustedRoot::restore_checkpoint(&source, &bytes)` and
`Revocations::restore_checkpoint(&root, &payload, &signatures)`, which keep known
revocations even after they expire.

Before using a selected entry, call `update.check_release(&entry)`: it refuses
entries the catalog does not list and exactly revoked releases, with no clock or
network, so apply it to cached bytes too. Fetch bytes with
`fetcher.versioned_artifact(&update, &entry, max_bytes, now)` and load pack
history with `versioned_history`. Both refuse before any request once the root,
revocations or catalog has expired at `now`; run `versioned_catalog` again to
refresh. `RootSummary` (from `TrustedRoot::summary`) describes the current keys
and roles for display; it grants nothing.

## Transport behavior

HTTPS acquisition uses public TLS roots, no proxies, no redirects and no content
encoding. A response must be HTTP 200. Each request has its own overall timeout;
discovery makes several requests, so that timeout is not a whole-discovery budget.
Connection setup has a separate 3-second cap. The timeout is capped at 24 hours.
See [Limits](../reference/limits.md).

`Fetcher::read` returns bounded bytes without a digest check. Use `artifact` for
release content: it reads and checks against the supplied entry's SHA-256. Pass
an entry from a verified, admitted catalog for that source; the method does not
prove the provenance of an arbitrary `Entry` you construct yourself.

Signatures and hashes do not validate the meaning or safety of pack contents.
Your application still owns pack validation, approval and execution policy.

Related: [Registrations](registrations.md), [Hosting and signing](../publishing/hosting-and-signing.md).

[Documentation index](../README.md)
