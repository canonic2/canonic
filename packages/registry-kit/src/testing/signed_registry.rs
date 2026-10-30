//! A signed registry, with releases and catalogs, served by an owned [`Server`].

use super::{Response, Server, SigningKey};
use crate::{catalog::Entry, digest, names::PackId};
use semver::Version;
use serde_json::json;

/// A signed registry served by an owned [`Server`].
pub struct SignedRegistry {
    server: Server,
    key: SigningKey,
    registry_id: String,
    entries: Vec<Entry>,
}

impl SignedRegistry {
    /// Starts a server publishing `catalog-key.pub` for a new key.
    #[must_use]
    pub fn new(registry_id: &str) -> Self {
        let server = Server::new();
        let key = SigningKey::generate();
        server.set("catalog-key.pub", Response::ok(key.public_hex()));
        Self {
            server,
            key,
            registry_id: registry_id.to_owned(),
            entries: Vec::new(),
        }
    }

    /// Serves `bytes` as the artifact of `id@version` and lists it in the next catalog.
    ///
    /// # Panics
    /// Panics for an invalid pack ID or version.
    pub fn release(&mut self, id: &str, version: &str, bytes: &[u8]) -> Entry {
        let id: PackId = id.parse().expect("fixture pack ID");
        let version: Version = version.parse().expect("fixture version");
        let entry = Entry {
            path: format!("packs/{}-{}-{version}", id.publisher(), id.name()),
            sha256: digest::sha256_hex(bytes),
            id,
            version,
        };
        self.server.set(&entry.path, Response::ok(bytes.to_vec()));
        self.entries.push(entry.clone());
        entry
    }

    /// Publishes a signed catalog of every release with this sequence and expiry.
    pub fn publish(&self, sequence: u64, expires_at: &str) {
        let bytes = json!({
            "schema_version": 1,
            "registry_id": self.registry_id,
            "sequence": sequence,
            "expires_at": expires_at,
            "packs": self.entries,
        })
        .to_string();
        self.publish_bytes(bytes.as_bytes());
    }

    /// Publishes exact catalog bytes with a valid signature.
    pub fn publish_bytes(&self, bytes: &[u8]) {
        self.server
            .set("catalog.json", Response::ok(bytes.to_vec()));
        self.server
            .set("catalog.sig", Response::ok(self.key.sign_hex(bytes)));
    }

    /// The underlying server, for raw responses and request inspection.
    #[must_use]
    pub fn server(&self) -> &Server {
        &self.server
    }

    /// The signing key.
    #[must_use]
    pub fn key(&self) -> &SigningKey {
        &self.key
    }
}
