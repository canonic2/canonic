# Test your integration

Enable `test-support` in dev-dependencies to use a real signed HTTPS registry on
an ephemeral localhost port. The fixture owns its server thread and stops and
joins it on drop.

## Acquire a signed release

Place this in an integration test file in your application:

```rust
use canonic_registry::{
    checkpoint::Checkpoints, testing::{Server, SignedRegistry},
};
use std::{error::Error, time::{Duration, SystemTime}};

#[test]
fn acquires_signed_bytes() -> Result<(), Box<dyn Error>> {
    let mut registry = SignedRegistry::new("acme");
    registry.release("acme/git", "1.0.0", b"example pack");
    registry.publish(1, "9999-12-31T00:00:00Z");
    let fetcher = Server::fetcher(Duration::from_secs(2));
    let discovery = fetcher.discover(registry.server().url(), SystemTime::now())?;
    Checkpoints::default().admit(&discovery.catalog)?;
    let id = discovery.catalog.lookup("git")?;
    let entry = discovery.catalog.select(&id, None).ok_or("release missing")?;
    assert_eq!(fetcher.artifact(&discovery.source, entry, 1024)?, b"example pack");
    Ok(())
}
```

`release` adds a release and serves its artifact, returning the `Entry` for
assertions or overrides. `publish` creates and signs a catalog from the current
releases. `publish_bytes` signs your supplied catalog bytes, useful for malformed
schema and authentication tests. `key` returns the fixture signing key.

`Server::fetcher` trusts only the fixture CA. Production `Fetcher::new` uses
public roots and cannot trust this localhost fixture. Fixture roots never change
production TLS behavior.

## Inject transport failures

Use `registry.server().set(path, response)` to replace one served response.
Paths are relative, such as `catalog.json`, `catalog.sig` or an entry's `path`.
For more control, construct `Server::new()` and define the responses yourself.

`Response::ok(body)` builds an ordinary response. Public fields let tests change
the status, add content encoding, delay the response, override declared length,
trickle chunks or keep trailers arriving. `requests()` returns the recorded
requests so you can assert that an offline path did not fetch anything.

Use the fixture to test observable application outcomes:

- A correctly signed catalog with an expired timestamp is refused at your
  supplied `now`; no sleeps are needed for expiry tests.
- A lower sequence or changed bytes at the same sequence fail admission after
  you retain the first checkpoint.
- Changed bytes for an already admitted release fail admission, even at a
  higher catalog sequence.
- Wrong artifact bytes fail acquisition and never reach activation.
- Redirects, non-200 status, encoding, oversized bodies and timeouts fail
  acquisition.
- An unenrolled foreign dependency fails resolution without implicitly
  enrolling its source.
- A retained installation still restores after the fixture server is dropped.

## Test signing without a server

`SigningKey::generate()` creates a fixture Ed25519 key. `public_hex()` returns
the public key and `sign_hex(bytes)` signs exact catalog bytes. Combine these
with `Source::signed` and `Catalog::verify` for deterministic pure verification
tests using a fixed `SystemTime`.

Use temporary directories for cache and registration tests. Run tests that open
the TLS server on a host that permits loopback listeners. Fixtures can panic on
invalid setup inputs or failure to start a listener; they are test conveniences,
not production adapters.

See [Development](../contributing/development.md) for the library's own checks.

[Documentation index](../README.md)
