# Registry documentation

`canonic-registry` provides the registry operations for a Rust application that
installs packs: source enrollment, catalog authentication, verified downloads,
dependency resolution, retained artifacts and lockfile validation. Your
application defines the pack format and controls review and installation.

Start with [Installation](getting-started/installation.md), then follow the
[Quick start](getting-started/quick-start.md). For an application integration,
read the [integration workflow](guides/integration.md).

## Getting started

- [Installation](getting-started/installation.md) — dependencies, Rust version and features.
- [Quick start](getting-started/quick-start.md) — a runnable local registry example.
- [Core concepts](getting-started/concepts.md) — packs, sources, identities and retained state.

## Guides

- [Integration workflow](guides/integration.md) — coordinate a complete installation.
- [Sources and trust](guides/sources-and-trust.md) — discover, review and authenticate sources.
- [Registrations](guides/registrations.md) — aliases, defaults, scopes and safe persistence.
- [Find and select packs](guides/selecting-packs.md) — references, catalog queries and installed selectors.
- [Resolve dependencies](guides/dependencies.md) — candidates, constraints, pins and levels.
- [Cache artifacts](guides/cache.md) — verified storage and offline reads.
- [Lock and reproduce installs](guides/lockfiles.md) — portable entries, updates and restoration.
- [Test your integration](guides/testing.md) — signed HTTPS fixtures and failure scenarios.

## Publishing

- [Catalog format](publishing/catalog-format.md) — schema, release entries and paths.
- [Hosting and signing](publishing/hosting-and-signing.md) — publish and renew a registry.

## Reference

- [API map](reference/api.md) — find the right type or function and build rustdoc.
- [Limits](reference/limits.md) — size, count and time budgets.
- [Errors and troubleshooting](reference/errors.md) — refusals and recovery steps.

## Contributing

- [Development](contributing/development.md) — build, checks and test tooling.

These guides describe the supported public API. Generated rustdoc supplies exact
signatures and error variants.
