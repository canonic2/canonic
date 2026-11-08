# Registry Kit

A Rust library for installing packs from registries. It authenticates signed
catalogs, enrolls sources, reads artifacts with bounds, refuses rollback,
resolves dependency closures, caches verified bytes and validates lockfile
entries. Products depend on it through a local path dependency; it is not
published.

## Add the library

Consumers require Rust 1.97 or newer; the checkout pins its own development
toolchain. From a sibling Rust package, add a local dependency in `Cargo.toml`
(adjust paths for your application):

```toml
[dependencies]
canonic-registry-kit = { path = "../registry-kit" }
```

The optional `test-support` feature provides signed registry and HTTPS fixtures
for integration tests; see [Testing](docs/guides/testing.md).

## Acquire your first pack

The [quick start](docs/getting-started/quick-start.md) creates a local registry,
validates its catalog, acquires an example artifact and reads it back from the
digest cache. It prints the acquired release and the temporary files' location.

A registry is a local directory or an HTTPS origin serving `catalog.json` and
the artifacts it lists. See [Catalog format](docs/publishing/catalog-format.md)
for complete metadata and signing requirements.

HTTPS registries also serve `catalog-key.pub` and a detached Ed25519
`catalog.sig`. The library checks catalogs, identities and dependency graphs.
What a pack contains, how changes are reviewed and where state is stored
belong to the product.

Catalog v3 keeps current pack heads in discovery and authenticates per-pack
version trees. `Fetcher::history` resolves old pins through the selected branch
without fetching unrelated history. Flat v1/v2 catalogs remain supported.

## Documentation

- [Documentation](docs/README.md): developer guides and reference, organized by task.
- [Quick start](docs/getting-started/quick-start.md): acquire and cache your first pack.
- [Integration workflow](docs/guides/integration.md): enrollment, resolution and installation.
- [Publish a registry](docs/publishing/hosting-and-signing.md): serve signed catalogs and releases.
- [Development](docs/contributing/development.md): checks, tests and tools.

Select checks individually from this directory: `make format-check`, `make lint`,
`make types`, `make test`, `make doctest`, `make docs` or `make deps`.
See [development](docs/contributing/development.md) for their purposes and costs.

## License

Registry Kit is subject to the [Canonic Proprietary License](LICENSE).
Third-party components retain their own licenses and notices.
