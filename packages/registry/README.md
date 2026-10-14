# canonic-registry

A Rust library for installing packs from registries. It authenticates signed
catalogs, enrolls sources, reads artifacts with bounds, refuses rollback,
resolves dependency closures, caches verified bytes and validates lockfile
entries. Products depend on it through a local path dependency; it is not
published.

```toml
[dependencies]
canonic-registry = { path = "../registry" }

[dev-dependencies]
canonic-registry = { path = "../registry", features = ["test-support"] }
```

A registry is a local directory or an HTTPS origin serving `catalog.json` and
the artifacts it lists:

```json
{"schema_version":1,"registry_id":"acme","sequence":4,"expires_at":"2027-01-01T00:00:00Z",
 "packs":[{"id":"acme/git","version":"1.2.0","path":"packs/git-1.2.0.yml","sha256":"<64 hex>"}]}
```

HTTPS registries also serve `catalog-key.pub` and a detached Ed25519
`catalog.sig`. The library checks catalogs, identities and dependency graphs.
What a pack contains, how changes are reviewed and where state is stored
belong to the product.

- [Documentation](docs/README.md): developer guides and reference, organized by task.
- [Quick start](docs/getting-started/quick-start.md): acquire and cache your first pack.
- [Integration workflow](docs/guides/integration.md): enrollment, resolution and installation.
- [Publish a registry](docs/publishing/hosting-and-signing.md): serve signed catalogs and releases.
- [Development](docs/contributing/development.md): checks, tests and tools.
- [Specifications](specs/README.md): the contracts behind each module.

Run the checks from this directory with `make check`.
