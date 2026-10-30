//! Fixtures shared by unit tests: example keys, sources, releases and catalogs.

use crate::{
    catalog::{Catalog, CatalogError},
    source::Source,
    testing::SigningKey,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, SystemTime},
};

/// A syntactically valid Ed25519 public key for sources that never verify.
pub(crate) const EXAMPLE_KEY: &str =
    "acacacacacacacacacacacacacacacacacacacacacacacacacacacacacacacac";

/// A fixed time, so expiry checks do not depend on the clock.
pub(crate) fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_hours(500_000)
}

/// A local source at a fixed absolute path that is never read.
pub(crate) fn local(path: &str) -> Source {
    Source::local(Path::new(path)).expect("absolute fixture path")
}

/// A pinned built-in source named `main`.
pub(crate) fn builtin() -> Source {
    Source::pinned(
        "https://registry.example.com/",
        "main",
        EXAMPLE_KEY,
        "acme-main",
    )
    .expect("fixture built-in source")
}

/// A signed source enrolled with `key`.
pub(crate) fn signed(key: &SigningKey) -> Source {
    Source::signed("https://registry.example.com/", &key.public_hex(), "acme")
        .expect("fixture signed source")
}

/// A catalog release whose digest repeats `digest` 64 times.
pub(crate) fn release(id: &str, version: &str, digest: char) -> Value {
    json!({
        "id": id,
        "version": version,
        "path": format!("packs/{}-{version}.yml", id.replace('/', "-")),
        "sha256": digest.to_string().repeat(64),
    })
}

/// Authenticates `document` as the catalog of a local source.
pub(crate) fn local_catalog(document: &Value) -> Result<Catalog, CatalogError> {
    Catalog::verify(
        &local("/srv/acme-registry"),
        document.to_string().as_bytes(),
        None,
        now(),
    )
}
