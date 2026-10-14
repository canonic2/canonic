# Quick start

This example creates a local registry, reads its catalog, acquires one release
and stores its bytes in a digest cache. It uses an opaque text artifact so you
can try the API without defining a pack format.

Use the dependencies from [Installation](installation.md). Save the following
as your application's `src/main.rs` and run `cargo run`. Each run creates a new
temporary directory and prints its location; remove that directory when done.

```rust
use canonic_registry::{
    acquisition::Fetcher, cache::ArtifactCache, checkpoint::Checkpoints, digest,
};
use serde_json::json;
use std::{error::Error, fs, time::{Duration, SystemTime, UNIX_EPOCH}};

fn main() -> Result<(), Box<dyn Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let work = std::env::temp_dir().join(format!(
        "acme-registry-{}-{nonce}", std::process::id()
    ));
    let registry = work.join("registry");
    fs::create_dir_all(registry.join("packs"))?;

    let artifact = b"acme example pack\n";
    fs::write(registry.join("packs/git-1.0.0.txt"), artifact)?;
    let document = json!({
        "schema_version": 1,
        "packs": [{
            "id": "acme/git",
            "version": "1.0.0",
            "path": "packs/git-1.0.0.txt",
            "sha256": digest::sha256_hex(artifact)
        }]
    });
    fs::write(registry.join("catalog.json"), serde_json::to_vec(&document)?)?;

    let fetcher = Fetcher::new(Duration::from_secs(10));
    let location = registry.to_str().ok_or("registry path is not UTF-8")?;
    let discovery = fetcher.discover(location, SystemTime::now())?;
    // This fixture is owned by the example. A user-supplied source needs review.
    let mut checkpoints = Checkpoints::default();
    checkpoints.admit(&discovery.catalog)?;

    let id = discovery.catalog.lookup("git")?;
    let entry = discovery.catalog.select(&id, None).ok_or("no stable release")?;
    let bytes = fetcher.artifact(&discovery.source, entry, 64 * 1024)?;
    assert_eq!(bytes, artifact);

    let cache = ArtifactCache::new(work.join("cache"));
    cache.put_expected(&entry.sha256, &bytes)?;
    let retained = cache.get(&entry.sha256, 64 * 1024)?.ok_or("cache miss")?;
    assert_eq!(retained, artifact);
    println!("Acquired {}@{}; files are in {}", entry.id, entry.version, work.display());
    Ok(())
}
```

`discover` canonicalizes the directory and validates its catalog. `admit` records
release digests. `artifact` checks bytes against the entry, and `get` verifies
retained bytes again. The 64 KiB artifact limit is chosen by this example.

For a real application, persist checkpoints, review sources before enrollment,
validate the pack format and resolve its dependencies before installation.
The [integration workflow](../guides/integration.md) connects those steps.

[Documentation index](../README.md)
