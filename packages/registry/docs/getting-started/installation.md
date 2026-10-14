# Installation

Registry is an unpublished Rust library. Add it through a local path dependency;
the path is relative to your application's `Cargo.toml`.

```toml
[dependencies]
canonic-registry = { path = "../registry" }
semver = "1.0"
serde_json = "1.0"
```

Adjust `../registry` to your checkout. The crate is imported as
`canonic_registry`. It requires Rust 1.99 or newer and uses the Rust 2024 edition.
The library checkout pins Rust 1.99.0 in `rust-toolchain.toml`.

Add `semver` when constructing version requirements or releases. Add `serde` and
`serde_json` when embedding registrations, checkpoints or locked entries in your
own documents. These types are not re-exported by Registry.

## Features

The default feature set is empty. Normal catalog verification, local and HTTPS
acquisition, resolution and caching need no opt-in feature.

Enable `test-support` for integration tests that need an owned HTTPS registry:

```toml
[dev-dependencies]
canonic-registry = { path = "../registry", features = ["test-support"] }
```

This exposes `testing::SignedRegistry`, `testing::Server` and
`testing::SigningKey`. Its TLS credentials and signing helpers are test fixtures.
See [Test your integration](../guides/testing.md).

## Execution model

`Fetcher`, `ArtifactCache` and `GlobalFile` have synchronous, blocking methods.
An async application should run their I/O using its runtime's blocking-work
facility. The resolver and domain types do not perform I/O.

Registry provides no CLI, installation service or global configuration. You
construct its types inside your application and supply paths, time, byte limits
and enrolled sources explicitly.

Next: [Quick start](quick-start.md).

[Documentation index](../README.md)
